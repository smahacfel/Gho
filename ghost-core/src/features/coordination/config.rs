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

    #[serde(default = "default_clean_coverage_min_bps")]
    pub clean_coverage_min_bps: u16,
    #[serde(default = "default_economic_spend_coverage_min_bps")]
    pub economic_spend_coverage_min_bps: u16,
    #[serde(default = "default_price_evidence_coverage_min_bps")]
    pub price_evidence_coverage_min_bps: u16,
    #[serde(default = "default_compute_unit_coverage_min_bps")]
    pub compute_unit_coverage_min_bps: u16,
    #[serde(default = "default_same_slot_dominated_bps")]
    pub same_slot_dominated_bps: u16,

    #[serde(default = "default_ftdi_low_threshold_bps")]
    pub ftdi_low_threshold_bps: u16,
    #[serde(default = "default_dbia_high_threshold_bps")]
    pub dbia_high_threshold_bps: u16,
    #[serde(default = "default_sfd_low_threshold_bps")]
    pub sfd_low_threshold_bps: u16,
    #[serde(default = "default_cpv_high_threshold_bps")]
    pub cpv_high_threshold_bps: u16,
    #[serde(default = "default_fsc_high_threshold_bps")]
    pub fsc_high_threshold_bps: u16,
    #[serde(default = "default_des_high_threshold_bps")]
    pub des_high_threshold_bps: u16,
    #[serde(default = "default_bse_high_threshold_bps")]
    pub bse_high_threshold_bps: u16,
    #[serde(default = "default_cucd_low_threshold_bps")]
    pub cucd_low_threshold_bps: u16,

    #[serde(default = "default_sfd_sane_max_bps")]
    pub sfd_sane_max_bps: u16,
    #[serde(default = "default_cpv_intensity_cap_pools")]
    pub cpv_intensity_cap_pools: u8,
    #[serde(default = "default_cucd_bucket_size")]
    pub cucd_bucket_size: u64,
}

impl Default for CoordinationRiskConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            export_only: true,
            min_unique_buyers_for_diagnostics: default_min_unique_buyers_for_diagnostics(),
            min_unique_buyers_for_soft_scoring: default_min_unique_buyers_for_soft_scoring(),
            funding_visibility: FundingVisibility::Unavailable,
            clean_coverage_min_bps: default_clean_coverage_min_bps(),
            economic_spend_coverage_min_bps: default_economic_spend_coverage_min_bps(),
            price_evidence_coverage_min_bps: default_price_evidence_coverage_min_bps(),
            compute_unit_coverage_min_bps: default_compute_unit_coverage_min_bps(),
            same_slot_dominated_bps: default_same_slot_dominated_bps(),
            ftdi_low_threshold_bps: default_ftdi_low_threshold_bps(),
            dbia_high_threshold_bps: default_dbia_high_threshold_bps(),
            sfd_low_threshold_bps: default_sfd_low_threshold_bps(),
            cpv_high_threshold_bps: default_cpv_high_threshold_bps(),
            fsc_high_threshold_bps: default_fsc_high_threshold_bps(),
            des_high_threshold_bps: default_des_high_threshold_bps(),
            bse_high_threshold_bps: default_bse_high_threshold_bps(),
            cucd_low_threshold_bps: default_cucd_low_threshold_bps(),
            sfd_sane_max_bps: default_sfd_sane_max_bps(),
            cpv_intensity_cap_pools: default_cpv_intensity_cap_pools(),
            cucd_bucket_size: default_cucd_bucket_size(),
        }
    }
}

impl CoordinationRiskConfig {
    #[must_use]
    pub fn clean_coverage_min(&self) -> f64 {
        bps_to_unit(self.clean_coverage_min_bps)
    }

    #[must_use]
    pub fn economic_spend_coverage_min(&self) -> f64 {
        bps_to_unit(self.economic_spend_coverage_min_bps)
    }

    #[must_use]
    pub fn price_evidence_coverage_min(&self) -> f64 {
        bps_to_unit(self.price_evidence_coverage_min_bps)
    }

    #[must_use]
    pub fn compute_unit_coverage_min(&self) -> f64 {
        bps_to_unit(self.compute_unit_coverage_min_bps)
    }

    #[must_use]
    pub fn same_slot_dominated_ratio(&self) -> f64 {
        bps_to_unit(self.same_slot_dominated_bps)
    }

    #[must_use]
    pub fn ftdi_low_threshold(&self) -> f64 {
        bps_to_unit(self.ftdi_low_threshold_bps)
    }

    #[must_use]
    pub fn dbia_high_threshold(&self) -> f64 {
        bps_to_unit(self.dbia_high_threshold_bps)
    }

    #[must_use]
    pub fn sfd_low_threshold(&self) -> f64 {
        bps_to_unit(self.sfd_low_threshold_bps)
    }

    #[must_use]
    pub fn cpv_high_threshold(&self) -> f64 {
        bps_to_unit(self.cpv_high_threshold_bps)
    }

    #[must_use]
    pub fn fsc_high_threshold(&self) -> f64 {
        bps_to_unit(self.fsc_high_threshold_bps)
    }

    #[must_use]
    pub fn des_high_threshold(&self) -> f64 {
        bps_to_unit(self.des_high_threshold_bps)
    }

    #[must_use]
    pub fn bse_high_threshold(&self) -> f64 {
        bps_to_unit(self.bse_high_threshold_bps)
    }

    #[must_use]
    pub fn cucd_low_threshold(&self) -> f64 {
        bps_to_unit(self.cucd_low_threshold_bps)
    }

    #[must_use]
    pub fn sfd_sane_max(&self) -> f64 {
        bps_to_unit(self.sfd_sane_max_bps)
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

const fn default_clean_coverage_min_bps() -> u16 {
    8_000
}

const fn default_economic_spend_coverage_min_bps() -> u16 {
    8_000
}

const fn default_price_evidence_coverage_min_bps() -> u16 {
    8_000
}

const fn default_compute_unit_coverage_min_bps() -> u16 {
    8_000
}

const fn default_same_slot_dominated_bps() -> u16 {
    5_000
}

const fn default_ftdi_low_threshold_bps() -> u16 {
    3_000
}

const fn default_dbia_high_threshold_bps() -> u16 {
    7_000
}

const fn default_sfd_low_threshold_bps() -> u16 {
    800
}

const fn default_cpv_high_threshold_bps() -> u16 {
    5_000
}

const fn default_fsc_high_threshold_bps() -> u16 {
    7_000
}

const fn default_des_high_threshold_bps() -> u16 {
    7_000
}

const fn default_bse_high_threshold_bps() -> u16 {
    7_000
}

const fn default_cucd_low_threshold_bps() -> u16 {
    800
}

const fn default_sfd_sane_max_bps() -> u16 {
    10_000
}

const fn default_cpv_intensity_cap_pools() -> u8 {
    3
}

const fn default_cucd_bucket_size() -> u64 {
    2_000
}

fn bps_to_unit(value: u16) -> f64 {
    (f64::from(value) / 10_000.0).clamp(0.0, 1.0)
}
