//! Gatekeeper V2.5 — Typed reason code taxonomy.
//!
//! Every verdict (BUY, REJECT, TIMEOUT) emits a `GatekeeperReasonCode` variant
//! serialized as `SCREAMING_SNAKE_CASE`. This replaces the legacy free-form
//! `decision_reason` with a machine-auditable taxonomy.
//!
//! Version history:
//! - v1: NoEmit-only (legacy)
//! - v2: All verdict types (P4 workstream)

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum GatekeeperReasonCode {
    // ── BUY ──
    BuyNormal,
    BuyEarly,
    BuyExtended,

    // ── HARD FAIL ──
    HardFailDevSold,
    HardFailMarketCap,
    HardFailExtremeHhi,
    HardFailExtremeBundling,
    HardFailExtremeTop3,
    HardFailExtremeBotTiming,
    HardFailFailedTxRatio,
    HardFailSlowPool,
    HardFailSellImpact,
    HardFailTxPriceImpact,
    HardFailPriceChange,

    // ── PDD ──
    RejectPddEntryDrift,
    RejectPddSpike,
    RejectPddRamping,
    RejectPddWhale,
    RejectPddReserve,
    RejectPddFlashCrash,

    // ── CORE / SYBIL / ALPHA / PROSPERITY ──
    RejectCoreFail,
    RejectSybilCombo,
    RejectSybilInterference,
    RejectSybilSoftExcess,
    RejectLegacySoftExcess,
    RejectLowAlpha,
    RejectLowProsperity,

    // ── IWIM (post-Gatekeeper veto gate) ──
    RejectIwimVeto,
    RejectIwimLowConf,
    RejectIwimUnknownStrict,

    // ── TAS / TIMING ──
    RejectLowTrajectory,
    RejectInsufficientConfidence,

    // ── TIMEOUT (terminal subtypes) ──
    TimeoutPhase1NoData,
    TimeoutPhase1Insufficient,
    TimeoutDeadlineLowPhases,

    // ── INVARIANT ──
    /// Verdict was never produced (invariant break).
    InvariantTimeoutNoVerdict,
    /// PDD hard-fail contradicted by BUY verdict (V25-I1).
    InvariantPddBuyContradiction,
    /// Zero-confidence BUY verdict (V25-I2).
    InvariantZeroConfidenceBuy,

    // ── SHADOW ──
    ShadowInsufficientData,
    ShadowEvalSkipped,
}

impl GatekeeperReasonCode {
    /// Current reason code taxonomy version.
    pub fn version() -> u32 {
        2
    }

    /// Map a `HardFailReason` variant to the corresponding reason code.
    pub fn from_hard_fail_reason(reason: &str) -> Option<Self> {
        match reason {
            "DevSold" => Some(Self::HardFailDevSold),
            "MarketCapTooLow" => Some(Self::HardFailMarketCap),
            "ExtremeHhi" => Some(Self::HardFailExtremeHhi),
            "ExtremeBundling" => Some(Self::HardFailExtremeBundling),
            "ExtremeTop3Dominance" => Some(Self::HardFailExtremeTop3),
            "ExtremeBotTiming" => Some(Self::HardFailExtremeBotTiming),
            "FailedTxRatio" => Some(Self::HardFailFailedTxRatio),
            "SlowPool" => Some(Self::HardFailSlowPool),
            "SellImpact" => Some(Self::HardFailSellImpact),
            "TxPriceImpact" => Some(Self::HardFailTxPriceImpact),
            "PriceChange" => Some(Self::HardFailPriceChange),
            _ => None,
        }
    }

    /// Derive a reason code string from a `GatekeeperVerdictType` tag.
    /// Only 1:1 unambiguous mappings are included. Generic/aggregate tags
    /// (REJECT_HARD_FAIL, REJECT_PUMP_AND_DUMP) return None because they
    /// cannot be resolved to a single reason_code subtype.
    /// Used by `expand_gatekeeper_plane_logs` for per-plane recomputation.
    pub fn derive_from_verdict_type_str(tag: &str) -> Option<String> {
        let code = match tag {
            "BUY" => Self::BuyNormal,
            "EARLY_BUY" => Self::BuyEarly,
            "REJECT_CORE_FAIL" => Self::RejectCoreFail,
            "REJECT_SOFT_EXCESS" => Self::RejectLegacySoftExcess,
            "REJECT_SYBIL_SOFT_EXCESS" => Self::RejectSybilSoftExcess,
            "REJECT_SYBIL_INTERFERENCE" => Self::RejectSybilInterference,
            "REJECT_LOW_ALPHA" => Self::RejectLowAlpha,
            "REJECT_LOW_PROSPERITY" => Self::RejectLowProsperity,
            "REJECT_IWIM_VETO" => Self::RejectIwimVeto,
            "REJECT_IWIM_LOW_CONF" => Self::RejectIwimLowConf,
            "REJECT_IWIM_UNKNOWN_STRICT" => Self::RejectIwimUnknownStrict,
            "REJECT_ENTRY_DRIFT" => Self::RejectPddEntryDrift,
            "REJECT_FLASH_CRASH" => Self::RejectPddFlashCrash,
            "REJECT_RAMPING" => Self::RejectPddRamping,
            "REJECT_LOW_TRAJECTORY" => Self::RejectLowTrajectory,
            "TIMEOUT_PHASE1" => Self::TimeoutPhase1Insufficient,
            "TIMEOUT_NO_DATA" => Self::TimeoutPhase1NoData,
            "TIMEOUT_PHASE1_NO_DATA" => Self::TimeoutPhase1NoData,
            "TIMEOUT_PHASE1_INSUFFICIENT" => Self::TimeoutPhase1Insufficient,
            "TIMEOUT_DEADLINE_LOW_PHASES" => Self::TimeoutDeadlineLowPhases,
            // Generic tags (REJECT_HARD_FAIL, REJECT_PUMP_AND_DUMP) → None
            _ => return None,
        };
        serde_json::to_string(&code)
            .ok()
            .map(|s| s.trim_matches('"').to_string())
    }

    /// Map an IWIM veto variant tag to the corresponding reason code.
    pub fn from_iwim_verdict(tag: &str) -> Option<Self> {
        match tag {
            "REJECT_IWIM_VETO" => Some(Self::RejectIwimVeto),
            "REJECT_IWIM_LOW_CONF" => Some(Self::RejectIwimLowConf),
            "REJECT_IWIM_UNKNOWN_STRICT" => Some(Self::RejectIwimUnknownStrict),
            _ => None,
        }
    }

    /// Map a `PddHardFail` string tag to the corresponding reason code.
    pub fn from_pdd_hard_fail(tag: &str) -> Option<Self> {
        match tag {
            "ENTRY_DRIFT" => Some(Self::RejectPddEntryDrift),
            "SPIKE" => Some(Self::RejectPddSpike),
            "RAMPING" => Some(Self::RejectPddRamping),
            "WHALE" => Some(Self::RejectPddWhale),
            "RESERVE" => Some(Self::RejectPddReserve),
            "FLASH_CRASH" => Some(Self::RejectPddFlashCrash),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_reason_code_serialization_roundtrip() {
        let codes = [
            GatekeeperReasonCode::BuyNormal,
            GatekeeperReasonCode::RejectPddEntryDrift,
            GatekeeperReasonCode::TimeoutPhase1NoData,
            GatekeeperReasonCode::ShadowInsufficientData,
        ];
        for code in codes {
            let json = serde_json::to_string(&code).expect("serialize");
            let back: GatekeeperReasonCode = serde_json::from_str(&json).expect("deserialize");
            assert_eq!(code, back, "roundtrip failed for {:?}", code);
        }
    }

    #[test]
    fn test_reason_code_screaming_snake_case_format() {
        let json = serde_json::to_string(&GatekeeperReasonCode::TimeoutPhase1NoData).unwrap();
        assert_eq!(json, "\"TIMEOUT_PHASE1_NO_DATA\"");

        let json = serde_json::to_string(&GatekeeperReasonCode::RejectPddEntryDrift).unwrap();
        assert_eq!(json, "\"REJECT_PDD_ENTRY_DRIFT\"");
    }

    #[test]
    fn test_version_is_2() {
        assert_eq!(GatekeeperReasonCode::version(), 2);
    }

    #[test]
    fn test_from_pdd_hard_fail() {
        assert_eq!(
            GatekeeperReasonCode::from_pdd_hard_fail("ENTRY_DRIFT"),
            Some(GatekeeperReasonCode::RejectPddEntryDrift)
        );
        assert_eq!(
            GatekeeperReasonCode::from_pdd_hard_fail("SPIKE"),
            Some(GatekeeperReasonCode::RejectPddSpike)
        );
        assert_eq!(GatekeeperReasonCode::from_pdd_hard_fail("UNKNOWN"), None);
    }

    #[test]
    fn test_from_iwim_verdict() {
        assert_eq!(
            GatekeeperReasonCode::from_iwim_verdict("REJECT_IWIM_VETO"),
            Some(GatekeeperReasonCode::RejectIwimVeto)
        );
        assert_eq!(
            GatekeeperReasonCode::from_iwim_verdict("REJECT_IWIM_LOW_CONF"),
            Some(GatekeeperReasonCode::RejectIwimLowConf)
        );
        assert_eq!(
            GatekeeperReasonCode::from_iwim_verdict("REJECT_IWIM_UNKNOWN_STRICT"),
            Some(GatekeeperReasonCode::RejectIwimUnknownStrict)
        );
        assert_eq!(GatekeeperReasonCode::from_iwim_verdict("BUY"), None);
    }

    #[test]
    fn test_invariant_timeout_no_verdict_format() {
        let json =
            serde_json::to_string(&GatekeeperReasonCode::InvariantTimeoutNoVerdict).unwrap();
        assert_eq!(json, "\"INVARIANT_TIMEOUT_NO_VERDICT\"");
    }
}
