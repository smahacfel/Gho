//! SSOT configuration knobs.

use serde::{Deserialize, Serialize};

/// Configuration for the Pool State SSOT layer.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SsotConfig {
    /// Enable Yellowstone gRPC as primary source.
    pub enable_yellowstone: bool,
    /// Yellowstone gRPC endpoint URL.
    pub yellowstone_endpoint: String,
    /// Optional authentication token for Yellowstone.
    pub yellowstone_auth_token: Option<String>,

    /// Maximum snapshot age before ORACLE_STALE is emitted (ms).
    /// Recommended: 1000–1500ms.
    pub stale_ms: u64,
    /// Default slippage tolerance in basis points for `min_out` calculation.
    pub slippage_bps_default: u16,

    /// Fee bps for bonding curve phase if not known from on-chain data.
    /// Pump.fun default is 100 (1%).
    pub bonding_fee_bps_default: u16,
    /// Fee bps for AMM phase if not known from pool state.
    /// Raydium default is 25 (0.25%).
    pub amm_fee_bps_default: u16,

    /// Bonding progress percentage threshold for phase switch consideration.
    pub bonding_progress_threshold_pct: f64,
}

impl Default for SsotConfig {
    fn default() -> Self {
        Self {
            enable_yellowstone: true,
            yellowstone_endpoint: String::new(),
            yellowstone_auth_token: None,
            stale_ms: 1500,
            slippage_bps_default: 100,
            bonding_fee_bps_default: 100,
            amm_fee_bps_default: 25,
            bonding_progress_threshold_pct: 95.0,
        }
    }
}
