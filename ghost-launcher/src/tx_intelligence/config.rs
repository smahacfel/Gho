use crate::config::TxIntelligenceRuntimeConfig;
use ghost_brain::config::GatekeeperV2Config;
use seer::early_fingerprint::EarlyFingerprintConfig;

pub const DEFAULT_SESSION_TX_RING_CAPACITY: usize = 128;

#[derive(Debug, Clone)]
pub struct TxIntelligenceConfig {
    pub min_sol_threshold: f64,
    pub observation_window_ms: u64,
    pub min_interval_cv: f64,
    pub max_interval_cv: f64,
    pub max_burst_ratio: f64,
    pub min_avg_interval_ms: f64,
    pub max_avg_interval_ms: f64,
    pub min_timing_entropy: f64,
    pub max_timing_entropy: f64,
    pub min_dust_filtered_count: u64,
    pub min_unique_ratio: f64,
    pub max_unique_ratio: f64,
    pub max_hhi: f64,
    pub max_tx_per_signer: usize,
    pub max_volume_gini: f64,
    pub max_top3_volume_pct: f64,
    pub max_same_ms_tx_ratio: f64,
    pub max_dev_buy_sol: f64,
    pub min_dev_buy_sol: f64,
    pub max_dev_tx_ratio: f64,
    pub min_dev_tx_ratio: f64,
    pub max_dev_volume_ratio: f64,
    pub min_dev_volume_ratio: f64,
    pub reject_on_dev_sell: bool,
    pub tx_key_capacity: usize,
    pub burst_window_ms: u64,
    pub fingerprint: EarlyFingerprintConfig,
}

impl TxIntelligenceConfig {
    #[must_use]
    pub fn from_gatekeeper_config(
        gatekeeper: &GatekeeperV2Config,
        fingerprint: EarlyFingerprintConfig,
    ) -> Self {
        Self {
            min_sol_threshold: gatekeeper.min_sol_threshold,
            observation_window_ms: gatekeeper.max_wait_time_ms,
            min_interval_cv: gatekeeper.min_interval_cv,
            max_interval_cv: gatekeeper.max_interval_cv,
            max_burst_ratio: gatekeeper.max_burst_ratio,
            min_avg_interval_ms: gatekeeper.min_avg_interval_ms,
            max_avg_interval_ms: gatekeeper.max_avg_interval_ms,
            min_timing_entropy: gatekeeper.min_timing_entropy,
            max_timing_entropy: gatekeeper.max_timing_entropy,
            min_dust_filtered_count: gatekeeper.min_dust_filtered_count,
            min_unique_ratio: gatekeeper.min_unique_ratio,
            max_unique_ratio: gatekeeper.max_unique_ratio,
            max_hhi: gatekeeper.max_hhi,
            max_tx_per_signer: gatekeeper.max_tx_per_signer,
            max_volume_gini: gatekeeper.max_volume_gini,
            max_top3_volume_pct: gatekeeper.max_top3_volume_pct,
            max_same_ms_tx_ratio: gatekeeper.max_same_ms_tx_ratio,
            max_dev_buy_sol: gatekeeper.max_dev_buy_sol,
            min_dev_buy_sol: gatekeeper.min_dev_buy_sol,
            max_dev_tx_ratio: gatekeeper.max_dev_tx_ratio,
            min_dev_tx_ratio: gatekeeper.min_dev_tx_ratio,
            max_dev_volume_ratio: gatekeeper.max_dev_volume_ratio,
            min_dev_volume_ratio: gatekeeper.min_dev_volume_ratio,
            reject_on_dev_sell: gatekeeper.reject_on_dev_sell,
            tx_key_capacity: gatekeeper.min_tx_count.saturating_mul(8).max(256),
            burst_window_ms: TxIntelligenceRuntimeConfig::default().burst_window_ms,
            fingerprint,
        }
    }

    #[must_use]
    pub fn apply_runtime_defaults(mut self, runtime: &TxIntelligenceRuntimeConfig) -> Self {
        self.burst_window_ms = runtime.burst_window_ms.max(1);
        self
    }
}

impl Default for TxIntelligenceConfig {
    fn default() -> Self {
        Self::from_gatekeeper_config(
            &GatekeeperV2Config::default(),
            EarlyFingerprintConfig::default(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn runtime_defaults_do_not_override_gatekeeper_min_sol_threshold() {
        let mut gatekeeper = GatekeeperV2Config::default();
        gatekeeper.min_sol_threshold = 0.00001;
        gatekeeper.min_tx_count = 4;

        let runtime = TxIntelligenceRuntimeConfig {
            dust_threshold_sol: 0.001,
            burst_window_ms: 750,
        };

        let config = TxIntelligenceConfig::from_gatekeeper_config(
            &gatekeeper,
            EarlyFingerprintConfig::default(),
        )
        .apply_runtime_defaults(&runtime);

        assert!((config.min_sol_threshold - 0.00001).abs() < f64::EPSILON);
        assert_eq!(config.burst_window_ms, 750);
    }
}
