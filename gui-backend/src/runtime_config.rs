//! Runtime configuration bridge for Ghost components
//!
//! This module provides a way to share runtime configuration between
//! GUI backend and other components (Features/Trigger) without restart.

use serde::{Deserialize, Serialize};
use std::sync::{Arc, RwLock};

/// Runtime configuration that can be updated during execution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RuntimeConfig {
    /// Position size in lamports for new trades
    pub position_size_lamports: u64,

    /// Jito tip amount in lamports (override if not in auto mode)
    pub jito_tip_lamports: u64,

    /// Maximum slippage tolerance (0.0 - 1.0)
    pub max_slippage: f64,

    /// Enable Jito bundles
    pub enable_jito: bool,

    /// Auto-calculate Jito tips based on transaction value
    /// If true, jito_tip_lamports is used as minimum tip
    /// If false, jito_tip_lamports is used as fixed tip
    pub auto_jito_tip: bool,
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        Self {
            position_size_lamports: 100_000_000, // 0.1 SOL
            jito_tip_lamports: 10_000,           // 0.00001 SOL
            max_slippage: 0.01,                  // 1%
            enable_jito: false,
            auto_jito_tip: true,
        }
    }
}

/// Thread-safe runtime configuration holder
pub type SharedRuntimeConfig = Arc<RwLock<RuntimeConfig>>;

/// Create a new shared runtime configuration
pub fn create_shared_config() -> SharedRuntimeConfig {
    Arc::new(RwLock::new(RuntimeConfig::default()))
}

/// Create a shared runtime configuration from initial settings
pub fn create_shared_config_from(config: RuntimeConfig) -> SharedRuntimeConfig {
    Arc::new(RwLock::new(config))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_default_config() {
        let config = RuntimeConfig::default();
        assert_eq!(config.position_size_lamports, 100_000_000);
        assert_eq!(config.jito_tip_lamports, 10_000);
        assert_eq!(config.max_slippage, 0.01);
        assert!(!config.enable_jito);
        assert!(config.auto_jito_tip);
    }

    #[test]
    fn test_shared_config() {
        let shared = create_shared_config();

        // Read initial value
        {
            let config = shared.read().unwrap();
            assert_eq!(config.position_size_lamports, 100_000_000);
        }

        // Update value
        {
            let mut config = shared.write().unwrap();
            config.position_size_lamports = 200_000_000;
        }

        // Verify update
        {
            let config = shared.read().unwrap();
            assert_eq!(config.position_size_lamports, 200_000_000);
        }
    }
}
