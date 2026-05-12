//! Configuration for Address Lookup Tables (LUT)
//!
//! This module contains static addresses for Pump.fun and Bonk.fun AMM
//! integrations, including program IDs, fee recipients, global configs,
//! mints, and system programs.
//!
//! Additionally provides keypair loading functionality for transaction signing.

use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::Keypair;
use std::fs;
use std::path::Path;
use std::str::FromStr;

/// Load a keypair from a JSON file (solana-keygen format)
///
/// # Arguments
/// * `path` - Path to the keypair JSON file. Supports `~` expansion for home directory.
///
/// # Returns
/// * `Ok(Keypair)` - Successfully loaded keypair
///
/// # Panics
/// * If the file does not exist
/// * If the file cannot be read
/// * If the file content is not valid keypair JSON
///
/// # Example
/// ```ignore
/// let keypair = load_keypair("~/.config/solana/id.json");
/// ```
pub fn load_keypair(path: &str) -> Keypair {
    // Expand ~ to home directory
    let expanded_path = if path.starts_with('~') {
        let home = std::env::var("HOME")
            .expect("HOME environment variable not set - cannot expand ~ in keypair path");
        path.replacen('~', &home, 1)
    } else {
        path.to_string()
    };

    let path = Path::new(&expanded_path);

    // Check if file exists
    if !path.exists() {
        panic!(
            "Keypair file not found at: {}\n\
            Please generate a keypair using: solana-keygen new --outfile {}",
            expanded_path, expanded_path
        );
    }

    // Read the file content
    let file_content = fs::read_to_string(path).unwrap_or_else(|e| {
        panic!(
            "Failed to read keypair file at {}: {}\n\
            Ensure the file is readable and contains valid keypair JSON.",
            expanded_path, e
        )
    });

    // Parse the JSON as a byte array
    let bytes: Vec<u8> = serde_json::from_str(&file_content).unwrap_or_else(|e| {
        panic!(
            "Failed to parse keypair file at {}: {}\n\
            The file should contain a JSON array of 64 bytes (solana-keygen format).",
            expanded_path, e
        )
    });

    // Validate the keypair length
    if bytes.len() != 64 {
        panic!(
            "Invalid keypair file at {}: expected 64 bytes, got {} bytes.\n\
            The file should be generated using: solana-keygen new",
            expanded_path,
            bytes.len()
        );
    }

    // Create the keypair
    Keypair::from_bytes(&bytes).unwrap_or_else(|e| {
        panic!(
            "Failed to create keypair from file at {}: {}\n\
            The keypair file may be corrupted.",
            expanded_path, e
        )
    })
}

/// AMM type enum
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AmmType {
    PumpFun,
    BonkFun,
}

/// Leapfrog strategy configuration for TPU transaction sending
#[derive(Debug, Clone)]
pub struct LeapfrogConfig {
    /// Number of additional future leaders to target (default: 2)
    /// With redundancy=2, sends to: current slot, current+4, current+8
    pub leapfrog_redundancy: usize,
    /// Whether to use QUIC instead of UDP for sending (default: false)
    pub use_quic: bool,
}

impl Default for LeapfrogConfig {
    fn default() -> Self {
        Self {
            leapfrog_redundancy: 2,
            use_quic: false,
        }
    }
}

impl LeapfrogConfig {
    /// Create a new leapfrog configuration
    pub fn new(leapfrog_redundancy: usize, use_quic: bool) -> Self {
        Self {
            leapfrog_redundancy,
            use_quic,
        }
    }

    /// Get the total number of leaders to target (current + redundancy)
    pub fn total_leaders(&self) -> usize {
        1 + self.leapfrog_redundancy
    }

    /// Get the slot offsets for leapfrog strategy
    /// Returns: [0, 4, 8, ...] based on redundancy count
    pub fn slot_offsets(&self) -> Vec<u64> {
        (0..self.total_leaders()).map(|i| (i * 4) as u64).collect()
    }
}

/// Redundancy policy for bundle submission
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RedundancyPolicy {
    /// N+1 redundancy: submit 2 bundles
    NPlusOne,
    /// N+3 redundancy: submit 4 bundles (default)
    NPlusThree,
    /// N+5 redundancy: submit 6 bundles (maximum safety)
    NPlusFive,
}

impl RedundancyPolicy {
    /// Get the number of bundles to submit
    pub fn bundle_count(&self) -> usize {
        match self {
            RedundancyPolicy::NPlusOne => 2,
            RedundancyPolicy::NPlusThree => 4,
            RedundancyPolicy::NPlusFive => 6,
        }
    }
}

impl Default for RedundancyPolicy {
    fn default() -> Self {
        RedundancyPolicy::NPlusThree
    }
}

/// Tip calculation configuration
#[derive(Debug, Clone)]
pub struct TipConfig {
    /// Base tip percentage (e.g., 0.02 for 2%)
    pub base_tip_percent: f64,
    /// Dynamic tip percentage (e.g., 0.05 for 5%)
    pub dynamic_tip_percent: f64,
    /// Maximum tip percentage cap (safety limit)
    pub max_tip_percent: f64,
    /// Minimum tip amount in lamports
    pub min_tip_lamports: u64,
    /// Maximum tip amount in lamports (safety cap)
    pub max_tip_lamports: u64,
}

impl Default for TipConfig {
    fn default() -> Self {
        Self {
            base_tip_percent: 0.02,        // 2%
            dynamic_tip_percent: 0.05,     // 5%
            max_tip_percent: 0.05,         // 5% cap
            min_tip_lamports: 10_000,      // 0.00001 SOL
            max_tip_lamports: 100_000_000, // 0.1 SOL
        }
    }
}

impl TipConfig {
    /// Create a new tip configuration
    pub fn new(
        base_tip_percent: f64,
        dynamic_tip_percent: f64,
        max_tip_percent: f64,
        min_tip_lamports: u64,
        max_tip_lamports: u64,
    ) -> Self {
        Self {
            base_tip_percent,
            dynamic_tip_percent,
            max_tip_percent,
            min_tip_lamports,
            max_tip_lamports,
        }
    }

    /// Calculate tip amount based on transaction value and priority
    ///
    /// # Arguments
    /// * `transaction_value` - Estimated value of the transaction in lamports
    /// * `priority` - Priority factor (0.0 = base, 1.0 = dynamic max)
    ///
    /// # Returns
    /// Tip amount in lamports, capped by min/max limits
    pub fn calculate_tip(&self, transaction_value: u64, priority: f64) -> u64 {
        let priority = priority.clamp(0.0, 1.0);

        // Interpolate between base and dynamic tip based on priority
        let tip_percent =
            self.base_tip_percent + (self.dynamic_tip_percent - self.base_tip_percent) * priority;

        // Apply safety cap
        let tip_percent = tip_percent.min(self.max_tip_percent);

        // Calculate tip amount
        let tip = (transaction_value as f64 * tip_percent) as u64;

        // Apply min/max limits
        tip.clamp(self.min_tip_lamports, self.max_tip_lamports)
    }

    /// Calculate tip with optional fixed override
    ///
    /// # Arguments
    /// * `transaction_value` - Estimated value of the transaction in lamports
    /// * `priority` - Priority factor (0.0 = base, 1.0 = dynamic max)
    /// * `fixed_tip` - Optional fixed tip amount (overrides auto-calculation)
    ///
    /// # Returns
    /// Tip amount in lamports
    pub fn calculate_tip_with_override(
        &self,
        transaction_value: u64,
        priority: f64,
        fixed_tip: Option<u64>,
    ) -> u64 {
        if let Some(tip) = fixed_tip {
            // Use fixed tip, but still apply safety limits
            tip.clamp(self.min_tip_lamports, self.max_tip_lamports)
        } else {
            // Use auto-calculation
            self.calculate_tip(transaction_value, priority)
        }
    }
}

/// Bundle building configuration
#[derive(Debug, Clone)]
pub struct BundleConfig {
    /// Redundancy policy
    pub redundancy_policy: RedundancyPolicy,
    /// Tip calculation configuration
    pub tip_config: TipConfig,
    /// Whether to stagger slot/nonce for inclusion safety
    pub stagger_nonce: bool,
    /// Enable detailed diagnostics logging
    pub enable_diagnostics: bool,
}

impl Default for BundleConfig {
    fn default() -> Self {
        Self {
            redundancy_policy: RedundancyPolicy::default(),
            tip_config: TipConfig::default(),
            stagger_nonce: true,
            enable_diagnostics: true,
        }
    }
}

impl BundleConfig {
    /// Create a new bundle configuration
    pub fn new(
        redundancy_policy: RedundancyPolicy,
        tip_config: TipConfig,
        stagger_nonce: bool,
        enable_diagnostics: bool,
    ) -> Self {
        Self {
            redundancy_policy,
            tip_config,
            stagger_nonce,
            enable_diagnostics,
        }
    }
}

/// LUT configuration for Ghost Transactions
#[derive(Debug, Clone)]
pub struct LutConfig {
    /// Pump.fun addresses
    pub pump_fun: AmmAddresses,
    /// Bonk.fun addresses
    pub bonk_fun: AmmAddresses,
    /// Common token mints
    pub mints: CommonMints,
    /// System programs
    pub system_programs: SystemPrograms,
    /// Optional on-chain Static LUT address for transaction compression
    /// When set, transactions will use this LUT to reduce size from ~500 to ~250 bytes
    pub static_lut_address: Option<Pubkey>,
}

/// AMM-specific addresses
#[derive(Debug, Clone)]
pub struct AmmAddresses {
    /// AMM Program ID
    pub program_id: Pubkey,
    /// Fee recipient account
    pub fee_recipient: Pubkey,
    /// Global config account
    pub global_config: Pubkey,
}

/// Common token mints
#[derive(Debug, Clone)]
pub struct CommonMints {
    /// SOL wrapped token mint
    pub sol: Pubkey,
    /// USDC token mint
    pub usdc: Pubkey,
    /// BONK token mint
    pub bonk: Pubkey,
}

/// System programs
#[derive(Debug, Clone)]
pub struct SystemPrograms {
    /// SPL Token Program
    pub token_program: Pubkey,
    /// Associated Token Program
    pub associated_token_program: Pubkey,
    /// System Program
    pub system_program: Pubkey,
    /// Rent Sysvar
    pub rent: Pubkey,
}

impl Default for LutConfig {
    fn default() -> Self {
        Self::new()
    }
}

impl LutConfig {
    /// Create a new LUT configuration with all static addresses
    pub fn new() -> Self {
        Self {
            pump_fun: AmmAddresses {
                program_id: Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
                    .expect("Valid Pump.fun program ID"),
                fee_recipient: Pubkey::from_str("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM")
                    .expect("Valid Pump.fun fee recipient"),
                global_config: Pubkey::from_str("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf")
                    .expect("Valid Pump.fun global config"),
            },
            bonk_fun: AmmAddresses {
                program_id: Pubkey::from_str("LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj")
                    .expect("Valid Bonk.fun program ID"),
                fee_recipient: Pubkey::from_str("C8Qf4o5ZwJbSz7Y6srR4gvfXx4Z4qyhW5AsYLSRQA8nc")
                    .expect("Valid Bonk.fun fee recipient"),
                global_config: Pubkey::from_str("FfYek5vEz23cMkWsdJwG2oa6EphsvXSHrGpdALN4g6W1")
                    .expect("Valid Bonk.fun global config"),
            },
            mints: CommonMints {
                sol: Pubkey::from_str("So11111111111111111111111111111111111111112")
                    .expect("Valid SOL mint"),
                usdc: Pubkey::from_str("EPjFWdd5AufqSSqeM2qN1xzybapC8G4wEGGkZwyTDt1v")
                    .expect("Valid USDC mint"),
                bonk: Pubkey::from_str("DezXAZ8z7PnrnRJjz3wXBoRgixCa6xjnB7YaB1pPB263")
                    .expect("Valid BONK mint"),
            },
            system_programs: SystemPrograms {
                token_program: Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                    .expect("Valid Token Program"),
                associated_token_program: Pubkey::from_str(
                    "ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL",
                )
                .expect("Valid Associated Token Program"),
                system_program: Pubkey::from_str("11111111111111111111111111111111")
                    .expect("Valid System Program"),
                rent: Pubkey::from_str("SysvarRent111111111111111111111111111111111")
                    .expect("Valid Rent Sysvar"),
            },
            static_lut_address: None,
        }
    }

    /// Create a new LUT configuration with a static LUT address
    pub fn with_static_lut(mut self, lut_address: Pubkey) -> Self {
        self.static_lut_address = Some(lut_address);
        self
    }

    /// Set the static LUT address
    pub fn set_static_lut_address(&mut self, lut_address: Option<Pubkey>) {
        self.static_lut_address = lut_address;
    }

    /// Get the static addresses that should be stored in the on-chain LUT
    /// These are the addresses that are common to all Pump.fun transactions
    pub fn get_static_lut_addresses() -> Vec<Pubkey> {
        vec![
            // System Program
            Pubkey::from_str("11111111111111111111111111111111").expect("Valid System Program"),
            // Token Program
            Pubkey::from_str("TokenkegQfeZyiNwAJbNbGKPFXCWuBvf9Ss623VQ5DA")
                .expect("Valid Token Program"),
            // Associated Token Program
            Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
                .expect("Valid Associated Token Program"),
            // Rent Sysvar
            Pubkey::from_str("SysvarRent111111111111111111111111111111111")
                .expect("Valid Rent Sysvar"),
            // Pump.fun Program ID
            Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
                .expect("Valid Pump.fun Program ID"),
            // Pump.fun Fee Recipient
            Pubkey::from_str("CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM")
                .expect("Valid Pump.fun Fee Recipient"),
            // Pump.fun Global PDA (derived from seeds ["global"])
            Self::derive_pump_global_pda(),
        ]
    }

    /// Derive the Pump.fun Global PDA
    fn derive_pump_global_pda() -> Pubkey {
        let pump_program_id = Pubkey::from_str("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P")
            .expect("Valid Pump.fun Program ID");
        let (global_pda, _bump) = Pubkey::find_program_address(&[b"global"], &pump_program_id);
        global_pda
    }

    /// Get AMM addresses for a specific AMM type
    pub fn get_amm_addresses(&self, amm_type: AmmType) -> &AmmAddresses {
        match amm_type {
            AmmType::PumpFun => &self.pump_fun,
            AmmType::BonkFun => &self.bonk_fun,
        }
    }

    /// Get all addresses that should be included in LUT for a specific AMM
    pub fn get_lut_addresses(&self, amm_type: AmmType) -> Vec<Pubkey> {
        let amm_addrs = self.get_amm_addresses(amm_type);

        vec![
            // AMM-specific addresses
            amm_addrs.program_id,
            amm_addrs.fee_recipient,
            amm_addrs.global_config,
            // Common mints
            self.mints.sol,
            self.mints.usdc,
            self.mints.bonk,
            // System programs
            self.system_programs.token_program,
            self.system_programs.associated_token_program,
            self.system_programs.system_program,
            self.system_programs.rent,
        ]
    }

    /// Check if a program ID is whitelisted
    pub fn is_whitelisted_program(&self, program_id: &Pubkey) -> bool {
        *program_id == self.pump_fun.program_id || *program_id == self.bonk_fun.program_id
    }

    /// Determine AMM type from program ID
    pub fn get_amm_type(&self, program_id: &Pubkey) -> Option<AmmType> {
        if *program_id == self.pump_fun.program_id {
            Some(AmmType::PumpFun)
        } else if *program_id == self.bonk_fun.program_id {
            Some(AmmType::BonkFun)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lut_config_creation() {
        let config = LutConfig::new();

        // Verify Pump.fun addresses
        assert_eq!(
            config.pump_fun.program_id.to_string(),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
        );
        assert_eq!(
            config.pump_fun.fee_recipient.to_string(),
            "CebN5WGQ4jvEPvsVU4EoHEpgzq1VV7AbicfhtW4xC9iM"
        );
        assert_eq!(
            config.pump_fun.global_config.to_string(),
            "4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf"
        );

        // Verify Bonk.fun addresses
        assert_eq!(
            config.bonk_fun.program_id.to_string(),
            "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj"
        );
    }

    #[test]
    fn test_is_whitelisted_program() {
        let config = LutConfig::new();

        assert!(config.is_whitelisted_program(&config.pump_fun.program_id));
        assert!(config.is_whitelisted_program(&config.bonk_fun.program_id));
        assert!(!config.is_whitelisted_program(&Pubkey::new_unique()));
    }

    #[test]
    fn test_get_lut_addresses() {
        let config = LutConfig::new();
        let addresses = config.get_lut_addresses(AmmType::PumpFun);

        // Should contain at least 10 addresses (3 AMM + 3 mints + 4 system programs)
        assert!(addresses.len() >= 10);
        assert!(addresses.contains(&config.pump_fun.program_id));
        assert!(addresses.contains(&config.mints.sol));
        assert!(addresses.contains(&config.system_programs.token_program));
    }

    #[test]
    fn test_get_amm_type() {
        let config = LutConfig::new();

        assert_eq!(
            config.get_amm_type(&config.pump_fun.program_id),
            Some(AmmType::PumpFun)
        );
        assert_eq!(
            config.get_amm_type(&config.bonk_fun.program_id),
            Some(AmmType::BonkFun)
        );
        assert_eq!(config.get_amm_type(&Pubkey::new_unique()), None);
    }

    #[test]
    fn test_redundancy_policy() {
        assert_eq!(RedundancyPolicy::NPlusOne.bundle_count(), 2);
        assert_eq!(RedundancyPolicy::NPlusThree.bundle_count(), 4);
        assert_eq!(RedundancyPolicy::NPlusFive.bundle_count(), 6);
        assert_eq!(RedundancyPolicy::default(), RedundancyPolicy::NPlusThree);
    }

    #[test]
    fn test_tip_config_calculate_tip() {
        let config = TipConfig::default();

        // Test base tip (priority = 0.0)
        let tx_value = 1_000_000_000; // 1 SOL
        let base_tip = config.calculate_tip(tx_value, 0.0);
        assert_eq!(base_tip, 20_000_000); // 2% of 1 SOL

        // Test dynamic tip (priority = 1.0)
        let dynamic_tip = config.calculate_tip(tx_value, 1.0);
        assert_eq!(dynamic_tip, 50_000_000); // 5% of 1 SOL

        // Test medium priority (priority = 0.5)
        let medium_tip = config.calculate_tip(tx_value, 0.5);
        assert_eq!(medium_tip, 35_000_000); // 3.5% of 1 SOL

        // Test minimum cap
        let small_tip = config.calculate_tip(100, 0.0);
        assert_eq!(small_tip, 10_000); // Should be minimum

        // Test maximum cap
        let large_tx = 10_000_000_000; // 10 SOL
        let large_tip = config.calculate_tip(large_tx, 1.0);
        assert_eq!(large_tip, 100_000_000); // Capped at max
    }

    #[test]
    fn test_bundle_config_default() {
        let config = BundleConfig::default();
        assert_eq!(config.redundancy_policy, RedundancyPolicy::NPlusThree);
        assert!(config.stagger_nonce);
        assert!(config.enable_diagnostics);
        assert_eq!(config.tip_config.base_tip_percent, 0.02);
    }

    #[test]
    fn test_leapfrog_config_default() {
        let config = LeapfrogConfig::default();
        assert_eq!(config.leapfrog_redundancy, 2);
        assert_eq!(config.use_quic, false);
        assert_eq!(config.total_leaders(), 3);
    }

    #[test]
    fn test_leapfrog_config_custom() {
        let config = LeapfrogConfig::new(3, true);
        assert_eq!(config.leapfrog_redundancy, 3);
        assert_eq!(config.use_quic, true);
        assert_eq!(config.total_leaders(), 4);
    }

    #[test]
    fn test_leapfrog_slot_offsets() {
        let config = LeapfrogConfig::default();
        let offsets = config.slot_offsets();
        assert_eq!(offsets, vec![0, 4, 8]);
    }

    #[test]
    fn test_leapfrog_slot_offsets_custom() {
        let config = LeapfrogConfig::new(4, false);
        let offsets = config.slot_offsets();
        assert_eq!(offsets, vec![0, 4, 8, 12, 16]);
    }

    #[test]
    fn test_load_keypair_valid_file() {
        use solana_sdk::signer::Signer;
        use std::io::Write;

        // Create a temporary keypair file
        let temp_dir = std::env::temp_dir();
        let keypair_path = temp_dir.join("test_keypair.json");

        // Generate a keypair and save it
        let original_keypair = Keypair::new();
        let keypair_bytes: Vec<u8> = original_keypair.to_bytes().to_vec();
        let json = serde_json::to_string(&keypair_bytes).unwrap();

        let mut file = std::fs::File::create(&keypair_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        // Load the keypair
        let loaded_keypair = load_keypair(keypair_path.to_str().unwrap());

        // Verify it matches
        assert_eq!(loaded_keypair.pubkey(), original_keypair.pubkey());

        // Clean up
        std::fs::remove_file(&keypair_path).ok();
    }

    #[test]
    #[should_panic(expected = "Keypair file not found")]
    fn test_load_keypair_file_not_found() {
        load_keypair("/nonexistent/path/keypair.json");
    }

    #[test]
    #[should_panic(expected = "Failed to parse keypair file")]
    fn test_load_keypair_invalid_json() {
        use std::io::Write;

        // Create a temporary file with invalid JSON
        let temp_dir = std::env::temp_dir();
        let keypair_path = temp_dir.join("invalid_keypair.json");

        let mut file = std::fs::File::create(&keypair_path).unwrap();
        file.write_all(b"not valid json").unwrap();

        // This should panic
        let result = std::panic::catch_unwind(|| {
            load_keypair(keypair_path.to_str().unwrap());
        });

        // Clean up
        std::fs::remove_file(&keypair_path).ok();

        // Re-panic if it did panic
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
        panic!("Expected panic but didn't get one");
    }

    #[test]
    #[should_panic(expected = "Invalid keypair file")]
    fn test_load_keypair_wrong_length() {
        use std::io::Write;

        // Create a temporary file with wrong length bytes
        let temp_dir = std::env::temp_dir();
        let keypair_path = temp_dir.join("wrong_length_keypair.json");

        let wrong_bytes: Vec<u8> = vec![1, 2, 3, 4, 5]; // Only 5 bytes instead of 64
        let json = serde_json::to_string(&wrong_bytes).unwrap();

        let mut file = std::fs::File::create(&keypair_path).unwrap();
        file.write_all(json.as_bytes()).unwrap();

        // This should panic
        let result = std::panic::catch_unwind(|| {
            load_keypair(keypair_path.to_str().unwrap());
        });

        // Clean up
        std::fs::remove_file(&keypair_path).ok();

        // Re-panic if it did panic
        if let Err(e) = result {
            std::panic::resume_unwind(e);
        }
        panic!("Expected panic but didn't get one");
    }

    #[test]
    fn test_load_keypair_home_expansion() {
        // Test that path expansion works (without actually creating file)
        // This test just verifies the expansion logic doesn't crash
        let home = std::env::var("HOME").unwrap_or_default();
        if !home.is_empty() {
            // The path won't exist, but we can test the expansion happens
            let result = std::panic::catch_unwind(|| {
                load_keypair("~/.config/solana/nonexistent_test_keypair.json");
            });
            // We expect it to panic with "file not found" not "HOME not set"
            assert!(result.is_err());
        }
    }
}
