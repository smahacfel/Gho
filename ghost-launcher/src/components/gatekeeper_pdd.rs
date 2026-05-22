//! Gatekeeper V2.5 Pump & Dump Detector (PDD)
//!
//! First-layer negative pattern detection. Measures observable facts:
//! entry drift, spike volume, ramping, whale concentration, reserve health,
//! and flash crash risk. All signals are shadow-first — live hard veto only
//! after promotion via ADR.

use ghost_brain::config::gatekeeper_v25_config::PumpAndDumpDetectorConfig;

use crate::components::gatekeeper::GatekeeperBuffer;

/// Type of PDD hard fail. Each maps to a specific pump & dump signature.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PddHardFail {
    EntryDrift,
    Spike,
    Ramping,
    Whale,
    Reserve,
    FlashCrash,
}

impl PddHardFail {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EntryDrift => "ENTRY_DRIFT",
            Self::Spike => "SPIKE",
            Self::Ramping => "RAMPING",
            Self::Whale => "WHALE",
            Self::Reserve => "RESERVE",
            Self::FlashCrash => "FLASH_CRASH",
        }
    }
}

/// Full PDD diagnostics produced by `evaluate_pdd()`.
#[derive(Debug, Clone)]
pub struct PddDiagnostics {
    pub enabled: bool,
    pub hard_fail: Option<PddHardFail>,
    pub entry_drift_pct: Option<f64>,
    pub entry_drift_anchor_source: Option<&'static str>,
    pub entry_drift_anchor_quality: Option<&'static str>,
    pub entry_drift_anchor_price: Option<f64>,
    pub entry_drift_current_price: Option<f64>,
    pub entry_drift_anchor_ts_ms: Option<u64>,
    pub entry_drift_current_ts_ms: Option<u64>,
    pub entry_drift_elapsed_ms: Option<u64>,
    pub entry_drift_static_max_pct: Option<f64>,
    pub entry_drift_elapsed_max_pct: Option<f64>,
    pub entry_drift_effective_max_pct: Option<f64>,
    pub entry_drift_threshold_source: Option<&'static str>,
    pub spike_detected: bool,
    pub spike_ratio: Option<f64>,
    pub spike_ratio_quality: Option<&'static str>,
    pub spike_recent_rate: Option<f64>,
    pub spike_earlier_rate: Option<f64>,
    pub ramping_detected: bool,
    pub whale_top3_pct: Option<f64>,
    pub whale_single_max_pct: Option<f64>,
    pub reserve_health_pass: bool,
    pub flash_crash_risk: bool,
    /// 1.0 = completely clean, 0.0 = hard fail
    pub pdd_score: f64,
    pub soft_penalty_points: u8,
}

impl PddDiagnostics {
    pub fn not_run() -> Self {
        Self {
            enabled: false,
            hard_fail: None,
            entry_drift_pct: None,
            entry_drift_anchor_source: None,
            entry_drift_anchor_quality: None,
            entry_drift_anchor_price: None,
            entry_drift_current_price: None,
            entry_drift_anchor_ts_ms: None,
            entry_drift_current_ts_ms: None,
            entry_drift_elapsed_ms: None,
            entry_drift_static_max_pct: None,
            entry_drift_elapsed_max_pct: None,
            entry_drift_effective_max_pct: None,
            entry_drift_threshold_source: None,
            spike_detected: false,
            spike_ratio: None,
            spike_ratio_quality: None,
            spike_recent_rate: None,
            spike_earlier_rate: None,
            ramping_detected: false,
            whale_top3_pct: None,
            whale_single_max_pct: None,
            reserve_health_pass: true,
            flash_crash_risk: false,
            pdd_score: 1.0,
            soft_penalty_points: 0,
        }
    }
}

/// Evaluate all PDD detection mechanisms against the current buffer state.
///
/// `regime_drift_max_pct`: optional regime-aware drift threshold from APS.
/// When provided (e.g., 3.0% for HighVolatility), overrides the config default
/// for the entry drift hard veto check.
pub fn evaluate_pdd(
    buffer: &GatekeeperBuffer,
    config: &PumpAndDumpDetectorConfig,
    regime_drift_max_pct: Option<f64>,
) -> PddDiagnostics {
    if !config.enabled {
        return PddDiagnostics::not_run();
    }

    let mut diag = PddDiagnostics {
        enabled: true,
        ..PddDiagnostics::not_run()
    };

    // 1. Entry drift detection
    let drift = detect_entry_drift(buffer, config, regime_drift_max_pct);
    diag.entry_drift_pct = drift.drift_pct;
    diag.entry_drift_anchor_source = drift.anchor_source;
    diag.entry_drift_anchor_quality = drift.anchor_quality;
    diag.entry_drift_anchor_price = drift.anchor_price;
    diag.entry_drift_current_price = drift.current_price;
    diag.entry_drift_anchor_ts_ms = drift.anchor_ts_ms;
    diag.entry_drift_current_ts_ms = drift.current_ts_ms;
    diag.entry_drift_elapsed_ms = drift.elapsed_ms;
    diag.entry_drift_static_max_pct = Some(drift.static_max_pct);
    diag.entry_drift_elapsed_max_pct = drift.elapsed_max_pct;
    diag.entry_drift_effective_max_pct = Some(drift.effective_max_pct);
    diag.entry_drift_threshold_source = Some(drift.threshold_source);
    if let Some(d) = drift.drift_pct {
        if d > drift.effective_max_pct {
            diag.hard_fail = Some(PddHardFail::EntryDrift);
            diag.pdd_score = 0.0;
            return diag;
        }
        if d > config.entry_drift_soft_max_pct {
            diag.soft_penalty_points = diag
                .soft_penalty_points
                .saturating_add(config.entry_drift_soft_weight);
        }
    }

    // 2. Spike pattern detection
    if config.spike_detection_enabled {
        let spike = detect_spike(buffer, config);
        diag.spike_detected = spike.detected;
        diag.spike_ratio = spike.ratio;
        diag.spike_ratio_quality = Some(spike.ratio_quality);
        diag.spike_recent_rate = spike.recent_rate;
        diag.spike_earlier_rate = spike.earlier_rate;
        if spike.detected && config.spike_hard_veto {
            diag.hard_fail = Some(PddHardFail::Spike);
            diag.pdd_score = 0.0;
            return diag;
        }
        if spike.detected {
            diag.soft_penalty_points = diag
                .soft_penalty_points
                .saturating_add(config.spike_soft_penalty);
        }
    }

    // 3. Ramping detection
    if config.ramping_detection_enabled {
        let ramping = detect_ramping(buffer, config);
        diag.ramping_detected = ramping;
        if ramping && config.ramping_hard_veto {
            diag.hard_fail = Some(PddHardFail::Ramping);
            diag.pdd_score = 0.0;
            return diag;
        }
        if ramping {
            // Plan: -0.30 soft penalty → 6 points * 0.05 = 0.30
            diag.soft_penalty_points = diag.soft_penalty_points.saturating_add(6);
        }
    }

    // 4. Whale concentration
    let (top3_pct, single_max_pct) = detect_whale_concentration(buffer);
    diag.whale_top3_pct = top3_pct;
    diag.whale_single_max_pct = single_max_pct;
    if let Some(t3) = top3_pct {
        if t3 > config.whale_top3_max_pct {
            diag.hard_fail = Some(PddHardFail::Whale);
            diag.pdd_score = 0.0;
            return diag;
        }
    }
    if let Some(sm) = single_max_pct {
        if sm > config.whale_single_max_pct {
            diag.hard_fail = Some(PddHardFail::Whale);
            diag.pdd_score = 0.0;
            return diag;
        }
    }

    // 5. Reserve health
    let reserve_pass = check_reserve_health(buffer, config);
    diag.reserve_health_pass = reserve_pass;
    if !reserve_pass {
        diag.hard_fail = Some(PddHardFail::Reserve);
        diag.pdd_score = 0.0;
        return diag;
    }

    // 6. Flash crash protection
    if config.flash_crash_protection_enabled {
        let flash = detect_flash_crash(buffer, config);
        diag.flash_crash_risk = flash;
        if flash {
            diag.hard_fail = Some(PddHardFail::FlashCrash);
            diag.pdd_score = 0.0;
            return diag;
        }
    }

    // Compute final pdd_score from soft penalties (1.0 = clean)
    if diag.hard_fail.is_none() {
        let penalty = (diag.soft_penalty_points as f64 * 0.05).min(0.3);
        diag.pdd_score = 1.0 - penalty;
    }

    diag
}

// ═══════════════════════════════════════════════════════════════════════
// Individual detection functions
// ═══════════════════════════════════════════════════════════════════════

/// Entry drift anchor info — tracks the provenance and quality of the price anchor.
#[derive(Debug, Clone)]
pub struct PddAnchorInfo {
    pub source: &'static str,
    pub quality: &'static str, // "strong" / "weak"
}

/// Detect entry drift: how much the price has already moved from initial.
///
/// **4-level anchor hierarchy** — each level uses a DISTINCT condition:
/// 1. InitPoolEvent proxy: curve_data_known AND reserve > 0 → "strong"
///    (both parser-confirmed AND real on-chain SOL reserve = best quality)
/// 2. AccountStateCore proxy: reserve > 0, curve_data_known MAY be false → "strong"
///    (raw on-chain state even before parser confirms; real reserve data)
/// 3. Parser-authoritative: curve_data_known, no reserve requirement → "strong"
///    (parser says this is good data even if reserves aren't populated yet)
/// 4. Fallback: any price point in history → "weak"
///
/// Drift is DIRECTIONAL: ((current / anchor) - 1.0) * 100.
/// Positive drift = price pumped up from baseline = red flag.
#[derive(Debug, Clone, Copy)]
struct EntryDriftDetection {
    drift_pct: Option<f64>,
    anchor_source: Option<&'static str>,
    anchor_quality: Option<&'static str>,
    anchor_price: Option<f64>,
    current_price: Option<f64>,
    anchor_ts_ms: Option<u64>,
    current_ts_ms: Option<u64>,
    elapsed_ms: Option<u64>,
    static_max_pct: f64,
    elapsed_max_pct: Option<f64>,
    effective_max_pct: f64,
    threshold_source: &'static str,
}

fn detect_entry_drift(
    buffer: &GatekeeperBuffer,
    config: &PumpAndDumpDetectorConfig,
    regime_drift_max_pct: Option<f64>,
) -> EntryDriftDetection {
    let history = buffer.price_history();
    let static_max_pct = regime_drift_max_pct.unwrap_or(config.entry_drift_max_pct);
    let mut detection = EntryDriftDetection {
        drift_pct: None,
        anchor_source: None,
        anchor_quality: None,
        anchor_price: None,
        current_price: None,
        anchor_ts_ms: None,
        current_ts_ms: None,
        elapsed_ms: None,
        static_max_pct,
        elapsed_max_pct: None,
        effective_max_pct: static_max_pct,
        threshold_source: if regime_drift_max_pct.is_some() {
            "regime_static"
        } else {
            "static"
        },
    };
    if history.is_empty() {
        detection.threshold_source = "fallback_no_anchor";
        return detection;
    }

    let current = match buffer.last_price_point() {
        Some(p) if p.price_sol_per_token.is_finite() && p.price_sol_per_token > 0.0 => p,
        _ => {
            detection.threshold_source = "fallback_no_anchor";
            return detection;
        }
    };
    detection.current_price = Some(current.price_sol_per_token);
    detection.current_ts_ms = Some(current.timestamp_ms);

    let anchor = history
        .iter()
        .find(|p| {
            p.curve_data_known
                && p.price_sol_per_token.is_finite()
                && p.price_sol_per_token > 0.0
                && p.v_sol_in_curve.is_finite()
                && p.v_sol_in_curve > 0.0
        })
        .map(|p| (*p, "init_pool_authoritative", "strong"))
        .or_else(|| {
            history
                .iter()
                .find(|p| {
                    p.price_sol_per_token.is_finite()
                        && p.price_sol_per_token > 0.0
                        && p.v_sol_in_curve.is_finite()
                        && p.v_sol_in_curve > 0.0
                })
                .map(|p| (*p, "account_state_reserve", "strong"))
        })
        .or_else(|| {
            history
                .iter()
                .find(|p| {
                    p.curve_data_known
                        && p.price_sol_per_token.is_finite()
                        && p.price_sol_per_token > 0.0
                })
                .map(|p| (*p, "curve_known_parser", "strong"))
        })
        .or_else(|| {
            history
                .first()
                .filter(|p| p.price_sol_per_token.is_finite() && p.price_sol_per_token > 0.0)
                .map(|p| (*p, "first_price_point_fallback", "weak"))
        });

    let Some((anchor, source, quality)) = anchor else {
        detection.threshold_source = "fallback_no_anchor";
        return detection;
    };

    detection.anchor_source = Some(source);
    detection.anchor_quality = Some(quality);
    detection.anchor_price = Some(anchor.price_sol_per_token);
    detection.anchor_ts_ms = Some(anchor.timestamp_ms);
    if current.timestamp_ms < anchor.timestamp_ms {
        detection.threshold_source = "invalid_timestamp_order";
        return detection;
    }

    let elapsed_ms = current.timestamp_ms - anchor.timestamp_ms;
    detection.elapsed_ms = Some(elapsed_ms);

    if config.entry_drift_elapsed_scaling_enabled {
        let elapsed_seconds = elapsed_ms as f64 / 1000.0;
        let elapsed_max = (config.entry_drift_elapsed_base_pct
            + config.entry_drift_elapsed_slope_pct_per_second * elapsed_seconds)
            .min(config.entry_drift_elapsed_cap_pct);
        detection.elapsed_max_pct = Some(elapsed_max);
        detection.effective_max_pct = elapsed_max;
        detection.threshold_source = "elapsed_scaled";
    }

    let drift = ((current.price_sol_per_token / anchor.price_sol_per_token) - 1.0) * 100.0;
    if drift > 0.0 {
        detection.drift_pct = Some(drift);
    }

    detection
}

/// Detect volume spike: compare recent volume rate vs earlier period.
#[derive(Debug, Clone, Copy, Default)]
struct SpikeDetection {
    detected: bool,
    ratio: Option<f64>,
    ratio_quality: &'static str,
    recent_rate: Option<f64>,
    earlier_rate: Option<f64>,
}

fn detect_spike(buffer: &GatekeeperBuffer, config: &PumpAndDumpDetectorConfig) -> SpikeDetection {
    let last_ts = buffer.highest_seen_ts_ms();
    let spike_start = last_ts.saturating_sub(config.spike_observation_window_ms);

    let mut recent_vol = 0.0f64;
    let mut earlier_vol = 0.0f64;

    for btx in buffer.buffered_txs_slice() {
        let ts = btx.tx.timestamp_ms;
        if ts >= spike_start {
            recent_vol += btx.tx.volume_sol;
        } else {
            earlier_vol += btx.tx.volume_sol;
        }
    }

    if buffer.buffered_txs_slice().is_empty() {
        return SpikeDetection {
            ratio_quality: "unavailable",
            ..SpikeDetection::default()
        };
    }
    if recent_vol <= 0.0 {
        return SpikeDetection {
            ratio_quality: "insufficient_recent_window",
            ..SpikeDetection::default()
        };
    }

    let recent_dur = config.spike_observation_window_ms as f64;
    let earlier_dur = (buffer
        .highest_seen_ts_ms()
        .saturating_sub(buffer.first_tx_ts_ms().unwrap_or(0)))
    .max(spike_start.saturating_sub(buffer.first_tx_ts_ms().unwrap_or(0)))
    .saturating_sub(config.spike_observation_window_ms) as f64;

    if earlier_dur <= 0.0 {
        return SpikeDetection {
            ratio_quality: "insufficient_earlier_window",
            ..SpikeDetection::default()
        };
    }

    let recent_rate = recent_vol / recent_dur;
    if earlier_vol <= 0.0 {
        return SpikeDetection {
            ratio_quality: "earlier_rate_zero",
            recent_rate: Some(recent_rate),
            earlier_rate: Some(0.0),
            ..SpikeDetection::default()
        };
    }
    let earlier_rate = earlier_vol / earlier_dur;
    if !earlier_rate.is_finite() || earlier_rate <= 0.0 {
        return SpikeDetection {
            ratio_quality: "earlier_rate_zero",
            recent_rate: Some(recent_rate),
            earlier_rate: Some(earlier_rate),
            ..SpikeDetection::default()
        };
    }
    let ratio = recent_rate / earlier_rate;
    SpikeDetection {
        detected: ratio > config.spike_ratio_threshold,
        ratio: Some(ratio),
        ratio_quality: "ok",
        recent_rate: Some(recent_rate),
        earlier_rate: Some(earlier_rate),
    }
}

/// Detect ramping: N consecutive buys of similar size.
fn detect_ramping(buffer: &GatekeeperBuffer, config: &PumpAndDumpDetectorConfig) -> bool {
    if buffer.max_consecutive_buys_count() < config.ramping_min_consecutive_buys {
        return false;
    }

    // Find the longest consecutive buy run and check size uniformity
    let mut current_streak = 0u32;
    let mut streak_sizes: Vec<f64> = Vec::new();

    for btx in buffer.buffered_txs_slice() {
        if btx.tx.is_buy {
            current_streak += 1;
            streak_sizes.push(btx.tx.volume_sol);
        } else {
            if current_streak >= config.ramping_min_consecutive_buys as u32 {
                if check_size_uniformity(&streak_sizes, config.ramping_size_tolerance_pct) {
                    return true;
                }
            }
            current_streak = 0;
            streak_sizes.clear();
        }
    }
    // Check final streak
    if current_streak >= config.ramping_min_consecutive_buys as u32 {
        if check_size_uniformity(&streak_sizes, config.ramping_size_tolerance_pct) {
            return true;
        }
    }

    false
}

fn check_size_uniformity(sizes: &[f64], tolerance_pct: f64) -> bool {
    if sizes.len() < 2 {
        return false;
    }
    let mean = sizes.iter().sum::<f64>() / sizes.len() as f64;
    if mean <= 0.0 {
        return false;
    }
    sizes
        .iter()
        .all(|&s| ((s - mean).abs() / mean) * 100.0 <= tolerance_pct)
}

/// Detect whale concentration from per-signer volume stats.
/// Returns (top3_pct, single_max_pct).
fn detect_whale_concentration(buffer: &GatekeeperBuffer) -> (Option<f64>, Option<f64>) {
    let total_vol = buffer.total_volume_sol();
    if total_vol <= 0.0 {
        return (None, None);
    }

    let stats = buffer.signer_stats();
    let mut signer_vols: Vec<f64> = stats.values().map(|s| s.total_volume_sol).collect();
    signer_vols.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));

    let single_max = signer_vols.first().copied();
    let top3_sum: f64 = signer_vols.iter().take(3).sum();

    let single_max_pct = single_max.map(|v| (v / total_vol) * 100.0);
    let top3_pct = Some((top3_sum / total_vol) * 100.0);

    (top3_pct, single_max_pct)
}

/// Check bonding curve reserve health.
fn check_reserve_health(buffer: &GatekeeperBuffer, config: &PumpAndDumpDetectorConfig) -> bool {
    // Use the last price point to estimate current reserve
    let last_price = match buffer.last_price_point() {
        Some(p) => p,
        None => return true, // no curve data → can't check → pass
    };

    let reserve_sol = last_price.v_sol_in_curve;
    if reserve_sol <= 0.0 {
        return true; // no reserve data → pass
    }

    if reserve_sol < config.reserve_min_sol {
        return false;
    }

    let market_cap = last_price.market_cap_sol;
    if market_cap > 0.0 && (reserve_sol / market_cap) < config.reserve_min_ratio {
        return false;
    }

    true
}

/// Detect flash crash: single large sell impact (>15%) OR 2+ sells within cluster_max_ms.
fn detect_flash_crash(buffer: &GatekeeperBuffer, config: &PumpAndDumpDetectorConfig) -> bool {
    // Check for single sell with outsized price impact
    if let Some(single_impact) = detect_single_sell_impact(buffer) {
        if single_impact > config.flash_crash_max_price_impact_pct {
            return true;
        }
    }

    // Check for sell clusters within flash_crash_sell_cluster_max_ms
    let mut sell_ts: Vec<u64> = buffer
        .buffered_txs_slice()
        .iter()
        .filter(|btx| !btx.tx.is_buy)
        .map(|btx| btx.tx.timestamp_ms)
        .collect();
    sell_ts.sort_unstable();

    if sell_ts.len() < 2 {
        return false;
    }

    for window in sell_ts.windows(2) {
        let gap = window[1].saturating_sub(window[0]);
        if gap <= config.flash_crash_sell_cluster_max_ms {
            // Check if there was price impact — use price_history if available
            if let Some(cluster_price_impact) =
                check_cluster_price_impact(buffer, window[0], window[1], config)
            {
                if cluster_price_impact {
                    return true;
                }
            } else {
                // No price data but sell cluster found — flag as risk
                return true;
            }
        }
    }

    false
}

/// Check if price dropped significantly during the sell cluster window.
fn check_cluster_price_impact(
    buffer: &GatekeeperBuffer,
    cluster_start: u64,
    cluster_end: u64,
    config: &PumpAndDumpDetectorConfig,
) -> Option<bool> {
    let history = buffer.price_history();
    if history.len() < 2 {
        return None; // insufficient price data
    }

    let mut prices_in_window: Vec<f64> = history
        .iter()
        .filter(|p| p.timestamp_ms >= cluster_start && p.timestamp_ms <= cluster_end)
        .map(|p| p.price_sol_per_token)
        .collect();

    if prices_in_window.len() < 2 {
        // Try broader window
        prices_in_window = history
            .iter()
            .filter(|p| {
                p.timestamp_ms >= cluster_start.saturating_sub(1000)
                    && p.timestamp_ms <= cluster_end.saturating_add(1000)
            })
            .map(|p| p.price_sol_per_token)
            .collect();
    }

    if prices_in_window.len() < 2 {
        return None;
    }

    let first = prices_in_window.first()?;
    let last = prices_in_window.last()?;
    if *first <= 0.0 {
        return None;
    }

    let impact_pct = ((last - first) / first).abs() * 100.0;
    Some(impact_pct > config.flash_crash_max_price_impact_pct)
}

/// Detect single sell with outsized price impact from price_history.
fn detect_single_sell_impact(buffer: &GatekeeperBuffer) -> Option<f64> {
    let history = buffer.price_history();
    if history.len() < 2 {
        return None;
    }
    // Find consecutive price points where price dropped
    let mut max_drop = 0.0f64;
    for w in history.windows(2) {
        if !w[1].is_buy || !w[0].is_buy {
            // At least one sell involved
            if w[0].price_sol_per_token > 0.0 {
                let impact = ((w[1].price_sol_per_token - w[0].price_sol_per_token)
                    / w[0].price_sol_per_token)
                    .abs()
                    * 100.0;
                max_drop = max_drop.max(impact);
            }
        }
    }
    if max_drop > 0.0 {
        Some(max_drop)
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_brain::config::GatekeeperV2Config;
    use solana_sdk::pubkey::Pubkey;

    fn pdd_test_config() -> PumpAndDumpDetectorConfig {
        let cfg = GatekeeperV2Config::default();
        let mut pdd = cfg.pdd;
        pdd.enabled = true;
        pdd
    }

    #[test]
    fn test_pdd_disabled_returns_not_run() {
        let mut config = pdd_test_config();
        config.enabled = false;
        let buffer = GatekeeperBuffer::new(Pubkey::new_unique(), &GatekeeperV2Config::default());
        let result = evaluate_pdd(&buffer, &config, None);
        assert!(!result.enabled);
        assert!(result.hard_fail.is_none());
        assert!((result.pdd_score - 1.0).abs() < f64::EPSILON);
    }

    #[test]
    fn test_pdd_hard_fail_as_str() {
        assert_eq!(PddHardFail::EntryDrift.as_str(), "ENTRY_DRIFT");
        assert_eq!(PddHardFail::Spike.as_str(), "SPIKE");
        assert_eq!(PddHardFail::Ramping.as_str(), "RAMPING");
        assert_eq!(PddHardFail::Whale.as_str(), "WHALE");
        assert_eq!(PddHardFail::Reserve.as_str(), "RESERVE");
        assert_eq!(PddHardFail::FlashCrash.as_str(), "FLASH_CRASH");
    }

    #[test]
    fn test_check_size_uniformity() {
        // Uniform sizes within 15%
        assert!(check_size_uniformity(&[1.0, 1.05, 0.98, 1.02], 15.0));
        // Non-uniform
        assert!(!check_size_uniformity(&[1.0, 2.0, 1.0], 15.0));
        // Single element
        assert!(!check_size_uniformity(&[1.0], 15.0));
    }

    #[test]
    fn test_detect_whale_concentration_empty() {
        let buffer = GatekeeperBuffer::new(Pubkey::new_unique(), &GatekeeperV2Config::default());
        let (top3, single) = detect_whale_concentration(&buffer);
        assert!(top3.is_none());
        assert!(single.is_none());
    }

    #[test]
    fn test_flash_crash_no_sells() {
        let buffer = GatekeeperBuffer::new(Pubkey::new_unique(), &GatekeeperV2Config::default());
        let config = pdd_test_config();
        assert!(!detect_flash_crash(&buffer, &config));
    }
}
