use std::collections::HashMap;

const BUNDLE_CLUSTER_THRESHOLD_MS: u64 = 50;

/// Accumulated per-signer statistics.
#[derive(Debug, Clone, Default)]
pub struct SignerStats {
    pub tx_count: usize,
    pub buy_count: usize,
    pub sell_count: usize,
    pub total_volume_sol: f64,
}

/// Phase 2: Velocity profile.
#[derive(Debug, Clone)]
pub struct VelocityProfile {
    pub avg_interval_ms: f64,
    pub interval_std_dev: f64,
    pub interval_cv: f64,
    pub burst_ratio: f64,
    pub timing_entropy: f64,
    pub is_accelerating: bool,
}

/// Phase 3: Signer diversity profile.
#[derive(Debug, Clone)]
pub struct SignerDiversityProfile {
    pub unique_ratio: f64,
    pub hhi: f64,
    pub max_tx_per_signer: usize,
    pub volume_gini: f64,
    pub top3_volume_pct: f64,
    pub same_ms_tx_ratio: f64,
}

/// Phase 4: Volume sanity profile.
#[derive(Debug, Clone)]
pub struct VolumeSanityProfile {
    pub buy_ratio: f64,
    pub avg_tx_sol: f64,
    pub volume_cv: f64,
    pub total_volume_sol: f64,
    pub min_tx_sol: f64,
    pub max_tx_sol: f64,
    pub sol_buy_ratio: f64,
    pub max_consecutive_buys: usize,
}

/// Phase 5: Dev behavior profile.
#[derive(Debug, Clone)]
pub struct DevBehaviorProfile {
    pub dev_wallet_known: bool,
    pub dev_buy_total_sol: f64,
    pub dev_initial_buy_tokens: Option<f64>,
    pub dev_tx_count: usize,
    pub dev_tx_ratio: f64,
    pub dev_volume_ratio: f64,
    pub dev_has_sold: bool,
    pub dev_is_first_buyer: bool,
}

pub fn compute_velocity_profile(timestamps: &[u64], observation_window_ms: u64) -> VelocityProfile {
    if timestamps.len() < 2 {
        return VelocityProfile {
            avg_interval_ms: 0.0,
            interval_std_dev: 0.0,
            interval_cv: 0.0,
            burst_ratio: 1.0,
            timing_entropy: 0.0,
            is_accelerating: false,
        };
    }

    let intervals: Vec<f64> = timestamps
        .windows(2)
        .map(|w| w[1].saturating_sub(w[0]) as f64)
        .filter(|&d| d > 0.0)
        .collect();

    if intervals.is_empty() {
        return VelocityProfile {
            avg_interval_ms: 0.0,
            interval_std_dev: 0.0,
            interval_cv: 0.0,
            burst_ratio: 1.0,
            timing_entropy: 0.0,
            is_accelerating: false,
        };
    }

    let n = intervals.len() as f64;
    let mean = intervals.iter().sum::<f64>() / n;
    let variance = intervals.iter().map(|x| (x - mean).powi(2)).sum::<f64>() / n;
    let std_dev = variance.sqrt();
    let cv = if mean > 0.0 { std_dev / mean } else { 0.0 };

    let first_ts = timestamps[0];
    let window_20pct_ms = observation_window_ms / 5;
    let window_20pct_end = first_ts.saturating_add(window_20pct_ms);
    let burst_count = timestamps
        .iter()
        .filter(|&&t| t <= window_20pct_end)
        .count();
    let burst_ratio = burst_count as f64 / timestamps.len() as f64;

    let num_buckets = 10usize;
    let last_ts = *timestamps.last().unwrap();
    let span = (last_ts.saturating_sub(first_ts)).max(1) as f64;
    let mut buckets = vec![0u32; num_buckets];
    for &t in timestamps {
        let offset = t.saturating_sub(first_ts) as f64;
        let bucket_idx = ((offset / span) * (num_buckets as f64 - 1.0))
            .round()
            .clamp(0.0, (num_buckets - 1) as f64) as usize;
        buckets[bucket_idx] += 1;
    }
    let total = timestamps.len() as f64;
    let entropy: f64 = buckets
        .iter()
        .filter(|&&c| c > 0)
        .map(|&c| {
            let p = c as f64 / total;
            -p * p.ln()
        })
        .sum();

    let is_accelerating = if intervals.len() >= 4 {
        let mid = intervals.len() / 2;
        let first_half_mean = intervals[..mid].iter().sum::<f64>() / mid as f64;
        let second_half_mean =
            intervals[mid..].iter().sum::<f64>() / (intervals.len() - mid) as f64;
        second_half_mean < first_half_mean * 0.8
    } else {
        false
    };

    VelocityProfile {
        avg_interval_ms: mean,
        interval_std_dev: std_dev,
        interval_cv: cv,
        burst_ratio,
        timing_entropy: entropy,
        is_accelerating,
    }
}

pub fn compute_gini(sorted_values: &[f64]) -> f64 {
    let n = sorted_values.len();
    if n == 0 {
        return 0.0;
    }
    let total: f64 = sorted_values.iter().sum();
    if total <= 0.0 {
        return 0.0;
    }
    let mut numerator = 0.0;
    for (i, &val) in sorted_values.iter().enumerate() {
        numerator += (2.0 * (i + 1) as f64 - n as f64 - 1.0) * val;
    }
    numerator / (n as f64 * total)
}

pub fn compute_signer_diversity(
    signer_stats: &HashMap<String, SignerStats>,
    total_tx: usize,
    total_volume: f64,
    sorted_timestamps: &[u64],
) -> SignerDiversityProfile {
    if signer_stats.is_empty() {
        return SignerDiversityProfile {
            unique_ratio: 0.0,
            hhi: 0.0,
            max_tx_per_signer: 0,
            volume_gini: 0.0,
            top3_volume_pct: 0.0,
            same_ms_tx_ratio: 0.0,
        };
    }

    let unique_signers = signer_stats.len();
    let unique_ratio = unique_signers as f64 / total_tx.max(1) as f64;

    let hhi: f64 = signer_stats
        .values()
        .map(|s| {
            let share = s.tx_count as f64 / total_tx.max(1) as f64;
            share * share
        })
        .sum();

    let max_tx_per_signer = signer_stats.values().map(|s| s.tx_count).max().unwrap_or(0);

    let mut volumes: Vec<f64> = signer_stats.values().map(|s| s.total_volume_sol).collect();
    volumes.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    let volume_gini = compute_gini(&volumes);

    let mut volumes_desc = volumes.clone();
    volumes_desc.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
    let top3_vol: f64 = volumes_desc.iter().take(3).sum();
    let top3_volume_pct = if total_volume > 0.0 {
        top3_vol / total_volume
    } else {
        0.0
    };

    let same_ms_tx_ratio = if sorted_timestamps.len() >= 2 {
        let mut clustered_count = 0usize;
        for window in sorted_timestamps.windows(2) {
            if window[1].saturating_sub(window[0]) < BUNDLE_CLUSTER_THRESHOLD_MS {
                clustered_count += 1;
            }
        }
        clustered_count as f64 / sorted_timestamps.len() as f64
    } else {
        0.0
    };

    SignerDiversityProfile {
        unique_ratio,
        hhi,
        max_tx_per_signer,
        volume_gini,
        top3_volume_pct,
        same_ms_tx_ratio,
    }
}

pub fn compute_volume_sanity(
    tx_volumes: &[f64],
    buy_count: usize,
    sell_count: usize,
    total_volume: f64,
    buy_volume_sol: f64,
    max_consecutive_buys: usize,
) -> VolumeSanityProfile {
    if tx_volumes.is_empty() {
        return VolumeSanityProfile {
            buy_ratio: 0.0,
            avg_tx_sol: 0.0,
            volume_cv: 0.0,
            total_volume_sol: 0.0,
            min_tx_sol: 0.0,
            max_tx_sol: 0.0,
            sol_buy_ratio: 0.0,
            max_consecutive_buys: 0,
        };
    }

    let total_count = buy_count + sell_count;
    let buy_ratio = if total_count > 0 {
        buy_count as f64 / total_count as f64
    } else {
        0.0
    };

    let n = tx_volumes.len() as f64;
    let avg_tx_sol = if n > 0.0 { total_volume / n } else { 0.0 };

    let min_tx_sol = tx_volumes.iter().copied().fold(f64::INFINITY, f64::min);
    let max_tx_sol = tx_volumes.iter().copied().fold(0.0_f64, f64::max);

    let variance = if n > 1.0 {
        tx_volumes
            .iter()
            .map(|v| (v - avg_tx_sol).powi(2))
            .sum::<f64>()
            / n
    } else {
        0.0
    };
    let std_dev = variance.sqrt();
    let volume_cv = if avg_tx_sol > 0.0 {
        std_dev / avg_tx_sol
    } else {
        0.0
    };

    let sol_buy_ratio = if total_volume > 0.0 {
        buy_volume_sol / total_volume
    } else {
        0.0
    };

    VolumeSanityProfile {
        buy_ratio,
        avg_tx_sol,
        volume_cv,
        total_volume_sol: total_volume,
        min_tx_sol: if min_tx_sol.is_finite() {
            min_tx_sol
        } else {
            0.0
        },
        max_tx_sol,
        sol_buy_ratio,
        max_consecutive_buys,
    }
}

pub fn compute_dev_behavior(
    dev_wallet: &Option<String>,
    first_signer: &Option<String>,
    dev_buy_total_sol: f64,
    dev_buy_volume_total_sol: f64,
    dev_sell_total_sol: f64,
    dev_tx_count: usize,
    dev_has_sold: bool,
    dev_initial_buy_tokens: Option<f64>,
    total_tx: usize,
    total_volume: f64,
) -> DevBehaviorProfile {
    let dev_wallet_known = dev_wallet.is_some();

    let dev_tx_ratio = if total_tx > 0 {
        dev_tx_count as f64 / total_tx as f64
    } else {
        0.0
    };

    let dev_total_volume = dev_buy_volume_total_sol + dev_sell_total_sol;
    let dev_volume_ratio = if total_volume > 0.0 {
        dev_total_volume / total_volume
    } else {
        0.0
    };

    let dev_is_first_buyer = match (dev_wallet, first_signer) {
        (Some(dev), Some(first)) => dev == first,
        _ => false,
    };

    DevBehaviorProfile {
        dev_wallet_known,
        dev_buy_total_sol,
        dev_initial_buy_tokens,
        dev_tx_count,
        dev_tx_ratio,
        dev_volume_ratio,
        dev_has_sold,
        dev_is_first_buyer,
    }
}
