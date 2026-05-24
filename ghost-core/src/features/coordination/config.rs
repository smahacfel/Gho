use crate::features::coordination::evidence::FundingVisibility;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CoordinationRiskConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_export_only")]
    pub export_only: bool,

    #[serde(default = "default_min_unique_buyers_for_diagnostics")]
    pub min_unique_buyers_for_diagnostics: u8,
    #[serde(default = "default_min_unique_buyers_for_soft_scoring")]
    pub min_unique_buyers_for_soft_scoring: u8,

    #[serde(default)]
    pub funding_visibility: FundingVisibility,
}

impl Default for CoordinationRiskConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            export_only: true,
            min_unique_buyers_for_diagnostics: default_min_unique_buyers_for_diagnostics(),
            min_unique_buyers_for_soft_scoring: default_min_unique_buyers_for_soft_scoring(),
            funding_visibility: FundingVisibility::Unavailable,
        }
    }
}

const fn default_export_only() -> bool {
    true
}

const fn default_min_unique_buyers_for_diagnostics() -> u8 {
    3
}

const fn default_min_unique_buyers_for_soft_scoring() -> u8 {
    5
}
