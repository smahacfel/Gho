use crate::aem::{config::AemConfig, types::*};

#[derive(Debug, Default, Clone)]
pub struct DefaultHardSafetyCheck;

impl HardSafetyCheck for DefaultHardSafetyCheck {
    fn evaluate(
        &self,
        features: &StateFeatures,
        now_unix_ms: UnixMs,
        cfg: &AemConfig,
    ) -> Option<SafetyAction> {
        if !features.entry_price_or_mcap.is_finite()
            || !features.current_price_or_mcap.is_finite()
            || !features.peak_since_entry.is_finite()
            || !features.drawdown_pct.is_finite()
            || !features.unrealized_pnl_pct.is_finite()
            || !features.slope_pct_per_s.is_finite()
        {
            return Some(SafetyAction {
                action: ActionChosen::Partial,
                reason_code: SafetyReasonCode::InvalidFeatureData,
                hard_lock_until_unix_ms: now_unix_ms.saturating_add(3_000),
            });
        }

        if features.oracle_stale_age_ms >= cfg.oracle_stale_hard_ms {
            return Some(SafetyAction {
                action: ActionChosen::Partial,
                reason_code: SafetyReasonCode::OracleStaleHard,
                hard_lock_until_unix_ms: now_unix_ms.saturating_add(5_000),
            });
        }

        if features.stress_bucket == StressBucket::High {
            return Some(SafetyAction {
                action: ActionChosen::Partial,
                reason_code: SafetyReasonCode::StressHigh,
                hard_lock_until_unix_ms: now_unix_ms.saturating_add(5_000),
            });
        }

        if features.drawdown_pct >= 75.0 {
            return Some(SafetyAction {
                action: ActionChosen::Panic,
                reason_code: SafetyReasonCode::HardDrawdownCatastrophic,
                hard_lock_until_unix_ms: now_unix_ms.saturating_add(10_000),
            });
        }

        None
    }
}
