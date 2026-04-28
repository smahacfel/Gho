use crate::aem::{config::AemConfig, types::*};

pub fn detect_regime(features: &StateFeatures) -> RegimeTag {
    if features.drawdown_pct >= 40.0 && features.slope_pct_per_s <= -0.15 {
        return RegimeTag::Capitulation;
    }
    match features.reclaim_flag {
        ReclaimFlag::Partial | ReclaimFlag::Full => RegimeTag::ReclaimAttempt,
        ReclaimFlag::None => {
            if features.slope_pct_per_s >= 0.15 {
                RegimeTag::DriftUp
            } else if features.slope_pct_per_s <= -0.8 {
                RegimeTag::DeadSlide
            } else {
                RegimeTag::Stabilizing
            }
        }
    }
}

pub fn compute_regime_key_with_config(features: &StateFeatures, cfg: &AemConfig) -> RegimeKey {
    let drawdown_bucket = if features.drawdown_pct < cfg.drawdown_bucket_edges_pct[0] {
        DrawdownBucket::Dd0_20
    } else if features.drawdown_pct < cfg.drawdown_bucket_edges_pct[1] {
        DrawdownBucket::Dd20_40
    } else {
        DrawdownBucket::Dd40Plus
    };

    let time_bucket = if features.time_since_entry_s < cfg.time_bucket_edges_s[0] {
        TimeBucket::T0_30s
    } else if features.time_since_entry_s <= cfg.time_bucket_edges_s[1] {
        TimeBucket::T30_120s
    } else {
        TimeBucket::T120Plus
    };

    let slope_bucket = if features.slope_pct_per_s <= cfg.slope_fast_down_pct_per_s {
        SlopeBucket::FastDown
    } else if features.slope_pct_per_s <= cfg.slope_slow_down_pct_per_s {
        SlopeBucket::SlowDown
    } else if features.slope_pct_per_s < cfg.slope_up_pct_per_s {
        SlopeBucket::Flat
    } else {
        SlopeBucket::Up
    };

    RegimeKey {
        drawdown_bucket,
        time_bucket,
        slope_bucket,
        reclaim_flag: features.reclaim_flag,
        stress_bucket: features.stress_bucket,
    }
}

pub fn compute_regime_key(features: &StateFeatures) -> RegimeKey {
    compute_regime_key_with_config(features, &AemConfig::default())
}
