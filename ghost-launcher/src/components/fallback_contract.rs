//! Shadow fallback classification contract for bounded PR7 truth/readiness boundaries.
//!
//! The remaining shadow-read fallbacks stay explicit so the repo can prove:
//! - which fallbacks are bootstrap-only,
//! - which are degraded/diagnostic and telemetry-visible,
//! - and that no hidden primary truth fallback is silently participating,
//!   without re-introducing RPC-based state repair paths.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShadowFallbackCategory {
    BootstrapOnly,
    DegradedDiagnostic,
    HiddenPrimary,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ShadowFallbackContract {
    pub site: &'static str,
    pub category: ShadowFallbackCategory,
    pub helper: &'static str,
    pub rationale: &'static str,
}

const FALLBACK_CONTRACTS: &[ShadowFallbackContract] = &[
    ShadowFallbackContract {
        site: "resolve_price_context",
        category: ShadowFallbackCategory::DegradedDiagnostic,
        helper: "shadow_ledger_snapshot",
        rationale: "Observation/runtime truth stays canonical-first; snapshot fallback is a bounded degraded diagnostic helper and must never become primary truth.",
    },
    ShadowFallbackContract {
        site: "resolve_gatekeeper_initial_reserves",
        category: ShadowFallbackCategory::DegradedDiagnostic,
        helper: "shadow_ledger_snapshot",
        rationale: "Gatekeeper reserve bootstrap prefers canonical then bootstrap state; ShadowLedger snapshot remains an explicitly bounded degraded helper before genesis safety-net fallback.",
    },
    ShadowFallbackContract {
        site: "tx_curve_enrichment_shadow",
        category: ShadowFallbackCategory::BootstrapOnly,
        helper: "shadow_ledger_curve",
        rationale: "TX enrichment may borrow short-lived curve context before canonical account-state materializes; it must never outrank canonical AccountStateCore.",
    },
];

#[must_use]
pub fn classify_shadow_fallback(site: &str) -> ShadowFallbackCategory {
    FALLBACK_CONTRACTS
        .iter()
        .find(|contract| contract.site == site)
        .map(|contract| contract.category)
        .unwrap_or(ShadowFallbackCategory::HiddenPrimary)
}

#[must_use]
pub fn shadow_fallback_contract(site: &str) -> Option<&'static ShadowFallbackContract> {
    FALLBACK_CONTRACTS
        .iter()
        .find(|contract| contract.site == site)
}

#[must_use]
pub fn declared_shadow_fallback_sites() -> &'static [ShadowFallbackContract] {
    FALLBACK_CONTRACTS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn phase3_shadow_fallback_contract_has_no_hidden_primary_sites() {
        for contract in declared_shadow_fallback_sites() {
            assert_ne!(
                contract.category,
                ShadowFallbackCategory::HiddenPrimary,
                "declared fallback site {} must not masquerade as hidden primary truth",
                contract.site
            );
        }
    }

    #[test]
    fn phase3_known_shadow_truth_sites_are_classified() {
        assert_eq!(
            classify_shadow_fallback("resolve_price_context"),
            ShadowFallbackCategory::DegradedDiagnostic
        );
        assert_eq!(
            classify_shadow_fallback("resolve_gatekeeper_initial_reserves"),
            ShadowFallbackCategory::DegradedDiagnostic
        );
        assert_eq!(
            classify_shadow_fallback("post_buy_price_read"),
            ShadowFallbackCategory::HiddenPrimary
        );
        assert_eq!(
            classify_shadow_fallback("tx_curve_enrichment_shadow"),
            ShadowFallbackCategory::BootstrapOnly
        );
    }

    #[test]
    fn phase3_unknown_shadow_truth_site_is_hidden_primary() {
        assert_eq!(
            classify_shadow_fallback("mystery_site"),
            ShadowFallbackCategory::HiddenPrimary
        );
    }
}
