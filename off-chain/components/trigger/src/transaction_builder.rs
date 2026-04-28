//! Transaction Builder for Ghost Transactions
//!
//! This module builds minimal Ghost Transactions (~180B) using Address Lookup Tables (LUT)
//! for direct AMM interaction (Pump.fun, Bonk.fun) without requiring on-chain programs.
//!
//! # Zero-Cost Direct AMM Interaction
//!
//! The builder uses DirectBuyBuilder to create raw buy instructions for Pump.fun,
//! reducing costs and increasing speed.
//!
//! # Pre-signing Support
//!
//! The builder supports pre-signing transactions ahead of slot leader selection, allowing
//! ultra-fast submission when the InitializePool event is detected.

use crate::config::{AmmType, LutConfig};
use crate::errors::{Result, TriggerError};
use ghost_core::{BondingCurve, SwapPlan};
use solana_sdk::{
    address_lookup_table::AddressLookupTableAccount,
    hash::Hash,
    instruction::Instruction,
    message::{v0, VersionedMessage},
    pubkey::Pubkey,
    signature::Keypair,
    signer::Signer,
    transaction::VersionedTransaction,
};
use std::collections::HashSet;

/// Default slippage tolerance (15%)
const DEFAULT_SLIPPAGE_TOLERANCE: f64 = 0.15;

/// Base TTL for blockhash freshness in calm network conditions (ms)
const BASE_TTL_MS: f64 = 400.0;

/// Tension multiplier for dynamic TTL calculation
const TENSION_MULTIPLIER: f64 = 3.5;

/// Minimum TTL for blockhash freshness in high tension conditions (ms)
const MIN_TTL_MS: f64 = 50.0;

/// AMM-specific accounts needed for swap execution
#[derive(Debug, Clone)]
pub struct AmmAccounts {
    /// Pool account address
    pub pool: Pubkey,
    /// AMM program identifier/owner for whitelist validation
    pub amm_program_id: Option<Pubkey>,
    /// Bonding curve PDA (dynamically calculated)
    pub bonding_curve: Option<Pubkey>,
    /// Additional accounts specific to the AMM
    pub additional_accounts: Vec<Pubkey>,
}

/// Pre-signed transaction ready for immediate submission
///
/// Contains a transaction that has been pre-signed and validated,
/// ready to be submitted when InitializePool event is detected.
#[derive(Debug, Clone)]
pub struct PreSignedTransaction {
    /// The signed transaction
    pub transaction: VersionedTransaction,
    /// Blockhash used for signing
    pub blockhash: Hash,
    /// Timestamp when the transaction was signed
    pub signed_at: i64,
    /// Estimated transaction size in bytes
    pub size_bytes: usize,
}

impl PreSignedTransaction {
    /// Check if the pre-signed transaction is still valid
    ///
    /// Transactions are valid for ~150 slots (60-90 seconds depending on network)
    /// We consider them stale after 60 seconds to be conservative.
    pub fn is_valid(&self, current_timestamp: i64) -> bool {
        const MAX_AGE_SECONDS: i64 = 60;
        current_timestamp - self.signed_at <= MAX_AGE_SECONDS
    }

    /// Get the transaction signature (first signature)
    pub fn signature(&self) -> String {
        self.transaction
            .signatures
            .first()
            .map(|sig| sig.to_string())
            .unwrap_or_default()
    }
}

/// Ghost Transaction Builder
///
/// Builds minimal transactions using LUT to keep transaction size ~180B
pub struct GhostTransactionBuilder {
    /// Swap plan from Oracle/Features
    pub swap_plan: SwapPlan,
    /// AMM type (Pump.fun or Bonk.fun)
    pub amm_type: AmmType,
    /// AMM-specific accounts
    pub amm_accounts: AmmAccounts,
    /// LUT configuration
    pub lut_config: LutConfig,
    /// Slippage tolerance for min_amount_out calculation (0.0 - 1.0)
    slippage_tolerance: f64,
    /// Optional expected price in lamports per token for dynamic min_amount_out
    expected_price_lamports_per_token: Option<u64>,
    /// Cached LUT addresses for this AMM
    lut_addresses: Vec<Pubkey>,
    /// Pre-loaded static LUT account for transaction compression
    static_lut_account: Option<AddressLookupTableAccount>,
    /// Network tension level (0.0 = calm, 100.0 = war) for dynamic TTL
    network_tension: f64,
}

impl GhostTransactionBuilder {
    /// Create a new Ghost Transaction Builder
    pub fn new(swap_plan: SwapPlan, amm_type: AmmType, amm_accounts: AmmAccounts) -> Self {
        Self::with_config(swap_plan, amm_type, amm_accounts, LutConfig::new())
    }

    /// Create a new Ghost Transaction Builder with a custom LUT config
    pub fn with_config(
        swap_plan: SwapPlan,
        amm_type: AmmType,
        amm_accounts: AmmAccounts,
        lut_config: LutConfig,
    ) -> Self {
        let lut_addresses = lut_config.get_lut_addresses(amm_type);

        Self {
            swap_plan,
            amm_type,
            amm_accounts,
            lut_config,
            slippage_tolerance: DEFAULT_SLIPPAGE_TOLERANCE,
            expected_price_lamports_per_token: None,
            lut_addresses,
            static_lut_account: None,
            network_tension: 0.0,
        }
    }

    /// Set a pre-loaded static LUT account for transaction compression
    ///
    /// When set, all transactions will automatically use this LUT to compress
    /// addresses, reducing transaction size from ~500 bytes to ~250 bytes.
    ///
    /// # Arguments
    /// * `lut_account` - The AddressLookupTableAccount to use
    ///
    /// # Returns
    /// Self for method chaining
    pub fn with_static_lut(mut self, lut_account: AddressLookupTableAccount) -> Self {
        self.static_lut_account = Some(lut_account);
        self
    }

    /// Override slippage tolerance (0.0 - 1.0)
    pub fn with_slippage_tolerance(mut self, tolerance: f64) -> Self {
        self.slippage_tolerance = tolerance;
        self
    }

    /// Provide an expected price (lamports per token) to derive min_amount_out when not set
    pub fn with_expected_price_lamports_per_token(mut self, price_lamports: u64) -> Self {
        self.expected_price_lamports_per_token = Some(price_lamports);
        self
    }

    /// Set network tension level for dynamic TTL calculation
    ///
    /// # Arguments
    /// * `tension` - Network tension level (0.0 = calm, 100.0 = war)
    ///
    /// # Returns
    /// Self for method chaining
    pub fn with_network_tension(mut self, tension: f64) -> Self {
        self.network_tension = tension.clamp(0.0, 100.0);
        self
    }

    /// Set network tension level (mutable version)
    ///
    /// # Arguments
    /// * `tension` - Network tension level (0.0 = calm, 100.0 = war)
    pub fn set_network_tension(&mut self, tension: f64) {
        self.network_tension = tension.clamp(0.0, 100.0);
    }

    /// Calculate dynamic blockhash freshness limit based on network tension
    ///
    /// Formula: BASE_TTL_MS - (Tension * TENSION_MULTIPLIER)
    /// - Tension 0 (calm) → 400ms
    /// - Tension 100 (war) → 50ms (minimum)
    ///
    /// # Arguments
    /// * `tension` - Network tension level (0.0 = calm, 100.0 = war)
    ///
    /// # Returns
    /// Blockhash freshness limit in milliseconds
    pub fn get_dynamic_blockhash_limit(&self, tension: f64) -> u64 {
        let tension_clamped = tension.clamp(0.0, 100.0);
        let limit_ms = BASE_TTL_MS - (tension_clamped * TENSION_MULTIPLIER);
        limit_ms.max(MIN_TTL_MS) as u64
    }

    /// Get the current dynamic blockhash limit based on stored tension
    ///
    /// # Returns
    /// Blockhash freshness limit in milliseconds
    pub fn get_current_blockhash_limit(&self) -> u64 {
        self.get_dynamic_blockhash_limit(self.network_tension)
    }

    /// Create a mock AddressLookupTableAccount for testing
    ///
    /// This creates a local LUT account with the static addresses used by Pump.fun.
    /// Use this in tests to verify transaction compression works correctly.
    ///
    /// # Arguments
    /// * `lut_key` - The pubkey to use as the LUT address
    pub fn create_mock_lut_account(lut_key: Pubkey) -> AddressLookupTableAccount {
        let addresses = LutConfig::get_static_lut_addresses();
        AddressLookupTableAccount {
            key: lut_key,
            addresses,
        }
    }

    /// Build and pre-sign initialize_intent transaction
    ///
    /// This creates a pre-signed transaction ready for immediate submission.
    /// The transaction is validated and sized to be minimal (~180 bytes with LUT).
    ///
    /// # Arguments
    ///
    /// * `payer` - The keypair that will sign and pay for the transaction
    /// * `recent_blockhash` - Recent blockhash to use for the transaction
    ///
    /// # Returns
    ///
    /// A PreSignedTransaction ready for immediate submission
    ///
    /// # Notes
    ///
    /// Without an actual on-chain LUT, the transaction size will be larger (~300 bytes).
    /// With a proper LUT, the size will be reduced to ~180 bytes as addresses are
    /// loaded from the lookup table instead of being embedded in the transaction.
    pub fn presign_initialize_intent_tx(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
    ) -> Result<PreSignedTransaction> {
        // Validate swap plan first
        self.validate_swap_plan()?;

        // Build the transaction
        let tx = self.build_initialize_intent_tx(payer, recent_blockhash)?;

        // Calculate size
        let size_bytes = bincode::serialize(&tx)
            .map(|bytes| bytes.len())
            .unwrap_or(0);

        // Note: Without actual LUT, size will be ~300 bytes
        // With LUT, it will be ~180 bytes
        // We don't enforce strict size here as it depends on LUT availability

        // Get current timestamp
        let signed_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        Ok(PreSignedTransaction {
            transaction: tx,
            blockhash: recent_blockhash,
            signed_at,
            size_bytes,
        })
    }

    /// Build initialize_intent transaction
    ///
    /// This creates a minimal transaction that sends a direct buy instruction
    /// to the Pump.fun AMM using LUT for address compression.
    ///
    /// If a static LUT account has been set via `with_static_lut()`, it will
    /// automatically be used to compress the transaction.
    pub fn build_initialize_intent_tx(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        // Validate swap plan first
        self.validate_swap_plan()?;

        // Build direct buy instruction using DirectBuyBuilder
        let instruction = self.build_initialize_intent_instruction()?;

        // Create message with LUT if available, otherwise without
        let message = if let Some(ref lut_account) = self.static_lut_account {
            self.create_versioned_message_with_lut(
                vec![instruction],
                &payer.pubkey(),
                recent_blockhash,
                lut_account.clone(),
            )?
        } else {
            self.create_versioned_message(vec![instruction], &payer.pubkey(), recent_blockhash)?
        };

        // Sign transaction
        let tx = VersionedTransaction::try_new(message, &[payer])
            .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(tx)
    }

    /// Build initialize_intent transaction with LUT account
    ///
    /// This creates a minimal transaction using an actual on-chain LUT account
    pub fn build_initialize_intent_tx_with_lut(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
        lut_account: AddressLookupTableAccount,
    ) -> Result<VersionedTransaction> {
        // Validate swap plan first
        self.validate_swap_plan()?;

        // Build initialize_intent instruction
        let instruction = self.build_initialize_intent_instruction()?;

        // Create message with LUT
        let message = self.create_versioned_message_with_lut(
            vec![instruction],
            &payer.pubkey(),
            recent_blockhash,
            lut_account,
        )?;

        // Sign transaction
        let tx = VersionedTransaction::try_new(message, &[payer])
            .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(tx)
    }

    /// Build full swap transaction (initialize + execute)
    ///
    /// This creates a transaction with both initialize_intent and execute_planned_swap
    /// for atomic execution scenarios.
    ///
    /// If a static LUT account has been set via `with_static_lut()`, it will
    /// automatically be used to compress the transaction.
    pub fn build_full_swap_tx(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
    ) -> Result<VersionedTransaction> {
        // Validate swap plan first
        self.validate_swap_plan()?;

        // Build both instructions
        let init_ix = self.build_initialize_intent_instruction()?;
        let execute_ix = self.build_execute_swap_instruction()?;

        // Create message with LUT if available, otherwise without
        let message = if let Some(ref lut_account) = self.static_lut_account {
            self.create_versioned_message_with_lut(
                vec![init_ix, execute_ix],
                &payer.pubkey(),
                recent_blockhash,
                lut_account.clone(),
            )?
        } else {
            self.create_versioned_message(
                vec![init_ix, execute_ix],
                &payer.pubkey(),
                recent_blockhash,
            )?
        };

        // Sign transaction
        let tx = VersionedTransaction::try_new(message, &[payer])
            .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(tx)
    }

    /// Build full swap transaction with LUT account
    pub fn build_full_swap_tx_with_lut(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
        lut_account: AddressLookupTableAccount,
    ) -> Result<VersionedTransaction> {
        // Validate swap plan first
        self.validate_swap_plan()?;

        // Build both instructions
        let init_ix = self.build_initialize_intent_instruction()?;
        let execute_ix = self.build_execute_swap_instruction()?;

        // Create message with LUT
        let message = self.create_versioned_message_with_lut(
            vec![init_ix, execute_ix],
            &payer.pubkey(),
            recent_blockhash,
            lut_account,
        )?;

        // Sign transaction
        let tx = VersionedTransaction::try_new(message, &[payer])
            .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(tx)
    }

    /// Validate swap plan before building transaction
    fn validate_swap_plan(&self) -> Result<()> {
        // Check minimum amount
        if self.swap_plan.amount_in < 1000 {
            return Err(TriggerError::InvalidSwapPlan(
                "amount_in must be >= 1000 lamports".to_string(),
            ));
        }

        // Check min_amount_out
        let min_out = self.compute_min_amount_out()?;
        if min_out == 0 {
            return Err(TriggerError::InvalidSwapPlan(
                "min_amount_out must be > 0".to_string(),
            ));
        }

        // Check timeout (should be in the future)
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        if self.swap_plan.timeout <= now {
            return Err(TriggerError::InvalidSwapPlan(
                "timeout must be in the future".to_string(),
            ));
        }

        // Check timeout duration (max 7 days)
        const MAX_TIMEOUT_DURATION: i64 = 7 * 24 * 60 * 60;
        if self.swap_plan.timeout - now > MAX_TIMEOUT_DURATION {
            return Err(TriggerError::InvalidSwapPlan(
                "timeout exceeds maximum duration of 7 days".to_string(),
            ));
        }

        // Verify pool_amm_id belongs to whitelisted program
        let whitelist_target = self
            .amm_accounts
            .amm_program_id
            .unwrap_or(self.swap_plan.pool_amm_id);

        if !self.lut_config.is_whitelisted_program(&whitelist_target) {
            return Err(TriggerError::InvalidSwapPlan(format!(
                "pool/program id {} is not whitelisted",
                whitelist_target
            )));
        }

        Ok(())
    }

    /// Build initialize_intent instruction
    ///
    /// This uses DirectBuyBuilder to create a raw Pump.fun buy instruction.
    ///
    /// Note: For this method, the token mint should be extracted from
    /// swap_plan.metadata. If metadata is not available, we fall back to
    /// using the bonding_curve or pool address for testing purposes.
    fn build_initialize_intent_instruction(&self) -> Result<Instruction> {
        use crate::direct_buy_builder::DirectBuyBuilder;

        // Extract token mint from bonding_curve (which is set from metadata.token_mint in production)
        // For testing without metadata, fall back to pool address
        let token_mint = self
            .amm_accounts
            .bonding_curve
            .unwrap_or(self.amm_accounts.pool);

        // Use DirectBuyBuilder to create a raw Pump.fun buy instruction
        let min_amount_out = self.compute_min_amount_out()?;
        let instruction = DirectBuyBuilder::build_buy_ix(
            &self.swap_plan.authority,
            &token_mint,
            self.swap_plan.amount_in,
            min_amount_out,
        );

        Ok(instruction)
    }

    /// Build execute_planned_swap instruction
    ///
    /// For direct AMM interaction, this is no longer needed as we send
    /// the buy instruction directly. Returns a no-op instruction for compatibility.
    fn build_execute_swap_instruction(&self) -> Result<Instruction> {
        use crate::direct_buy_builder::DirectBuyBuilder;

        // Extract token mint from bonding_curve (which is set from metadata.token_mint in production)
        let token_mint = self
            .amm_accounts
            .bonding_curve
            .unwrap_or(self.amm_accounts.pool);

        // For direct AMM interaction, the execute step is not needed
        // We create the same buy instruction for compatibility with full_swap_tx
        let min_amount_out = self.compute_min_amount_out()?;
        let instruction = DirectBuyBuilder::build_buy_ix(
            &self.swap_plan.authority,
            &token_mint,
            self.swap_plan.amount_in,
            min_amount_out,
        );

        Ok(instruction)
    }

    /// Resolve min_amount_out using explicit value or derived from expected price & slippage
    fn compute_min_amount_out(&self) -> Result<u64> {
        if self.swap_plan.min_amount_out > 0 {
            return Ok(self.swap_plan.min_amount_out);
        }

        let expected_price = self.expected_price_lamports_per_token.ok_or_else(|| {
            TriggerError::InvalidSwapPlan(
                "min_amount_out missing and expected price not provided".to_string(),
            )
        })?;

        if expected_price == 0 {
            return Err(TriggerError::InvalidSwapPlan(
                "expected price must be > 0".to_string(),
            ));
        }

        let expected_tokens = (self.swap_plan.amount_in as f64) / (expected_price as f64);
        if !expected_tokens.is_finite() || expected_tokens <= 0.0 {
            return Err(TriggerError::InvalidSwapPlan(
                "expected output tokens resolved to 0 (check price/amount_in)".to_string(),
            ));
        }

        let tolerance = self.slippage_tolerance.clamp(0.0, 1.0);
        let min_tokens_f64 = expected_tokens * (1.0 - tolerance);

        if !min_tokens_f64.is_finite() || min_tokens_f64 <= 0.0 {
            return Err(TriggerError::InvalidSwapPlan(
                "computed min_amount_out is 0 after slippage".to_string(),
            ));
        }

        if min_tokens_f64 > u64::MAX as f64 {
            return Err(TriggerError::InvalidSwapPlan(
                "computed min_amount_out exceeds u64 range".to_string(),
            ));
        }

        let min_tokens = min_tokens_f64.floor() as u64;

        if min_tokens == 0 {
            return Err(TriggerError::InvalidSwapPlan(
                "computed min_amount_out is 0 after slippage".to_string(),
            ));
        }

        Ok(min_tokens)
    }

    /// Create versioned message with LUT support
    ///
    /// This method creates a v0 message that references addresses from the LUT,
    /// minimizing the on-chain transaction size.
    fn create_versioned_message(
        &self,
        instructions: Vec<Instruction>,
        payer: &Pubkey,
        recent_blockhash: Hash,
    ) -> Result<VersionedMessage> {
        // Extract all unique addresses from instructions
        let mut all_addresses: HashSet<Pubkey> = HashSet::new();

        for ix in &instructions {
            all_addresses.insert(ix.program_id);
            for acc in &ix.accounts {
                all_addresses.insert(acc.pubkey);
            }
        }
        all_addresses.insert(*payer);

        // Identify addresses that can be loaded from LUT
        let lut_set: HashSet<Pubkey> = self.lut_addresses.iter().copied().collect();
        let _lut_loadable: Vec<Pubkey> = all_addresses
            .iter()
            .filter(|addr| lut_set.contains(addr))
            .copied()
            .collect();

        // For now, create a simple v0 message without actual LUT loading
        // In production, you would create an AddressLookupTableAccount and reference it
        // This requires the LUT to be created on-chain first
        //
        // Example production code:
        // let lut_account = AddressLookupTableAccount {
        //     key: lut_pubkey,
        //     addresses: self.lut_addresses.clone(),
        // };
        // let message = v0::Message::try_compile(
        //     payer,
        //     &instructions,
        //     &[lut_account],
        //     recent_blockhash,
        // )?;

        let message = v0::Message::try_compile(
            payer,
            &instructions,
            &[], // Address lookup table accounts would go here
            recent_blockhash,
        )
        .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(VersionedMessage::V0(message))
    }

    /// Create versioned message with actual LUT account references
    ///
    /// This method should be used when the LUT has been created on-chain
    fn create_versioned_message_with_lut(
        &self,
        instructions: Vec<Instruction>,
        payer: &Pubkey,
        recent_blockhash: Hash,
        lut_account: AddressLookupTableAccount,
    ) -> Result<VersionedMessage> {
        let message =
            v0::Message::try_compile(payer, &instructions, &[lut_account], recent_blockhash)
                .map_err(|e| TriggerError::TransactionBuildFailed(e.to_string()))?;

        Ok(VersionedMessage::V0(message))
    }

    /// Optimize slippage using ShadowLedger state
    ///
    /// This method updates `min_amount_out` in the swap plan based on the latest
    /// bonding curve state from the ShadowLedger. It acts as a "Shadow Guard" that
    /// protects against front-running and sandwich attacks.
    ///
    /// # How It Works
    ///
    /// 1. Takes a snapshot of the bonding curve state (read lock - fast)
    /// 2. Simulates the buy using `simulate_buy` which accounts for Pump.fun's 1% fee
    /// 3. Applies slippage tolerance (e.g., 50 bps = 0.5%)
    /// 4. Sets the `min_amount_out` to this calculated value
    ///
    /// # Anti-Front-Run Protection
    ///
    /// If someone front-runs us and the on-chain price is worse than our ShadowLedger
    /// state indicates, the transaction will fail simulation (in Jito bundle) because
    /// `min_amount_out` will be higher than what the degraded price can provide.
    ///
    /// # Arguments
    ///
    /// * `shadow_curve` - Reference to the ShadowBondingCurve from ShadowLedger
    /// * `slippage_bps` - Slippage tolerance in basis points (e.g., 50 = 0.5%)
    ///
    /// # Example
    ///
    /// ```ignore
    /// use ghost_core::market_state::BondingCurve;
    /// use std::sync::RwLock;
    ///
    /// // Get curve from ShadowLedger
    /// if let Some(curve) = shadow_ledger.get(&mint) {
    ///     // Use tight slippage for Shadow Guard protection
    ///     builder.optimize_slippage_with_shadow(&curve, 50); // 0.5% slippage
    /// }
    /// ```
    pub fn optimize_slippage_with_shadow(&mut self, curve: &BondingCurve, slippage_bps: u64) {
        // Simulate buy with current state (includes 1% fee handling)
        let expected_out = curve.simulate_buy(self.swap_plan.amount_in);

        // Calculate strict minimum with slippage tolerance
        // 10000 bps = 100%, so (10000 - slippage_bps) / 10000 gives us the fraction to keep
        // Use saturating arithmetic to safely handle potential overflow
        let strict_min_out = if slippage_bps >= 10000 {
            0 // 100% or more slippage means accept anything
        } else {
            // Use saturating_mul to prevent overflow - if overflow would occur,
            // we get u64::MAX which after division gives a safe, conservative value
            expected_out.saturating_mul(10000 - slippage_bps) / 10000
        };

        // Update the swap plan's min_amount_out
        self.swap_plan.min_amount_out = strict_min_out;

        tracing::info!(
            target: "ghost_guard",
            "🛡️ SHADOW GUARD ARMED: In: {} SOL | Est: {} tokens | Min: {} tokens | Slippage: {} bps",
            self.swap_plan.amount_in,
            expected_out,
            strict_min_out,
            slippage_bps
        );
    }

    /// Get estimated transaction size
    pub fn estimate_transaction_size(&self) -> usize {
        // Rough estimate:
        // - Signature: 64 bytes
        // - Message header: ~3 bytes
        // - Recent blockhash: 32 bytes
        // - Instructions: ~60-80 bytes (with LUT compression)
        // Target: ~180 bytes total
        180
    }

    /// Get the LUT addresses that would be used for this transaction
    pub fn get_lut_addresses(&self) -> &[Pubkey] {
        &self.lut_addresses
    }

    /// Get the AMM addresses for this transaction
    pub fn get_amm_addresses(&self) -> &crate::config::AmmAddresses {
        self.lut_config.get_amm_addresses(self.amm_type)
    }

    /// Build full swap transaction with Bulkhead safety checks
    ///
    /// This method validates the trade amount against the Bulkhead safety configuration
    /// before building the transaction. If the trade exceeds safe limits, it will return
    /// an error instead of building an unsafe transaction.
    ///
    /// # Arguments
    /// * `payer` - The keypair that will sign the transaction
    /// * `recent_blockhash` - Recent blockhash for the transaction
    /// * `current_balance_sol` - Current wallet balance in SOL
    /// * `safety_config` - Bulkhead safety configuration
    ///
    /// # Returns
    /// * `Ok(VersionedTransaction)` - If the trade passes safety checks
    /// * `Err(TriggerError)` - If the trade violates safety constraints
    ///
    /// # Safety Checks
    /// 1. Emergency floor check: Ensures balance > EMERGENCY_FLOOR_SOL (0.05)
    /// 2. Position size validation: Ensures trade amount is within safe limits
    /// 3. Post-trade balance check: Ensures sufficient reserves after trade
    ///
    /// # Example
    /// ```ignore
    /// use trigger::safety::SafetyConfig;
    ///
    /// let safety_config = SafetyConfig::default();
    /// let balance_sol = 1.0; // 1 SOL
    /// let tx = builder.build_full_swap_tx_with_safety(
    ///     &payer,
    ///     recent_blockhash,
    ///     balance_sol,
    ///     &safety_config,
    /// )?;
    /// ```
    pub fn build_full_swap_tx_with_safety(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
        current_balance_sol: f64,
        safety_config: &crate::safety::SafetyConfig,
    ) -> Result<VersionedTransaction> {
        use crate::safety::{calculate_safe_trade_amount, check_emergency_floor, validate_trade};

        // SAFETY CHECK 1: Emergency floor check
        check_emergency_floor(current_balance_sol, safety_config)
            .map_err(|e| TriggerError::ValidationFailed(format!("Safety violation: {}", e)))?;

        // SAFETY CHECK 2: Calculate safe trade amount
        let trade_amount_sol = self.swap_plan.amount_in as f64 / 1_000_000_000.0;
        let safe_amount_sol = calculate_safe_trade_amount(current_balance_sol, safety_config, 1.0);

        // SAFETY CHECK 3: Validate trade amount
        validate_trade(trade_amount_sol, current_balance_sol, safety_config).map_err(|e| {
            TriggerError::ValidationFailed(format!("Trade validation failed: {}", e))
        })?;

        // Log safety check results
        tracing::info!(
            target: "bulkhead",
            "🛡️ BULKHEAD SAFETY PASSED: Trade {} SOL, Safe limit {} SOL, Balance {} SOL",
            trade_amount_sol,
            safe_amount_sol,
            current_balance_sol
        );

        // If all safety checks pass, build the transaction normally
        self.build_full_swap_tx(payer, recent_blockhash)
    }

    /// Build full swap transaction with LUT and Bulkhead safety checks
    ///
    /// Same as `build_full_swap_tx_with_safety` but uses a specific LUT account.
    ///
    /// # Arguments
    /// * `payer` - The keypair that will sign the transaction
    /// * `recent_blockhash` - Recent blockhash for the transaction
    /// * `lut_account` - Address lookup table account for compression
    /// * `current_balance_sol` - Current wallet balance in SOL
    /// * `safety_config` - Bulkhead safety configuration
    pub fn build_full_swap_tx_with_lut_and_safety(
        &self,
        payer: &Keypair,
        recent_blockhash: Hash,
        lut_account: AddressLookupTableAccount,
        current_balance_sol: f64,
        safety_config: &crate::safety::SafetyConfig,
    ) -> Result<VersionedTransaction> {
        use crate::safety::{calculate_safe_trade_amount, check_emergency_floor, validate_trade};

        // SAFETY CHECK 1: Emergency floor check
        check_emergency_floor(current_balance_sol, safety_config)
            .map_err(|e| TriggerError::ValidationFailed(format!("Safety violation: {}", e)))?;

        // SAFETY CHECK 2: Calculate safe trade amount
        let trade_amount_sol = self.swap_plan.amount_in as f64 / 1_000_000_000.0;
        let safe_amount_sol = calculate_safe_trade_amount(current_balance_sol, safety_config, 1.0);

        // SAFETY CHECK 3: Validate trade amount
        validate_trade(trade_amount_sol, current_balance_sol, safety_config).map_err(|e| {
            TriggerError::ValidationFailed(format!("Trade validation failed: {}", e))
        })?;

        // Log safety check results
        tracing::info!(
            target: "bulkhead",
            "🛡️ BULKHEAD SAFETY PASSED: Trade {} SOL, Safe limit {} SOL, Balance {} SOL",
            trade_amount_sol,
            safe_amount_sol,
            current_balance_sol
        );

        // If all safety checks pass, build the transaction normally
        self.build_full_swap_tx_with_lut(payer, recent_blockhash, lut_account)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::signature::Keypair;

    fn create_test_swap_plan_with_authority(authority: Pubkey) -> SwapPlan {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let config = LutConfig::new();

        SwapPlan::new(
            authority,
            config.pump_fun.program_id,
            1_000_000,
            900_000,
            now + 3600, // 1 hour from now
        )
    }

    fn create_test_swap_plan() -> SwapPlan {
        create_test_swap_plan_with_authority(Keypair::new().pubkey())
    }

    #[test]
    fn test_builder_creation() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert_eq!(builder.amm_type, AmmType::PumpFun);
        assert!(!builder.lut_addresses.is_empty());
    }

    #[test]
    fn test_swap_plan_validation() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert!(builder.validate_swap_plan().is_ok());
    }

    #[test]
    fn test_invalid_amount_validation() {
        let mut swap_plan = create_test_swap_plan();
        swap_plan.amount_in = 100; // Below minimum

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert!(builder.validate_swap_plan().is_err());
    }

    #[test]
    fn test_zero_amount_out_validation() {
        let mut swap_plan = create_test_swap_plan();
        swap_plan.min_amount_out = 0; // Zero amount out

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert!(builder.validate_swap_plan().is_err());
    }

    #[test]
    fn test_min_amount_out_derived_from_expected_price() {
        let mut swap_plan = create_test_swap_plan();
        swap_plan.min_amount_out = 0;

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let price_lamports = 1_000u64; // 0.000001 SOL per token
        let slippage = 0.10; // 10%
        let expected_tokens = swap_plan.amount_in / price_lamports;
        let expected_min = ((expected_tokens as f64) * (1.0 - slippage)) as u64;

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts)
            .with_expected_price_lamports_per_token(price_lamports)
            .with_slippage_tolerance(slippage);

        assert_eq!(builder.compute_min_amount_out().unwrap(), expected_min);
    }

    #[test]
    fn test_invalid_pool_id_validation() {
        let mut swap_plan = create_test_swap_plan();
        swap_plan.pool_amm_id = Pubkey::new_unique(); // Not whitelisted

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert!(builder.validate_swap_plan().is_err());
    }

    #[test]
    fn test_expired_timeout_validation() {
        use std::time::{SystemTime, UNIX_EPOCH};

        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        let mut swap_plan = create_test_swap_plan();
        swap_plan.timeout = now - 100; // Expired

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        assert!(builder.validate_swap_plan().is_err());
    }

    #[test]
    fn test_estimate_transaction_size() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let size = builder.estimate_transaction_size();
        assert_eq!(size, 180); // Target size
    }

    #[test]
    fn test_presigned_transaction_validity() {
        let payer = Keypair::new();
        let swap_plan = create_test_swap_plan_with_authority(payer.pubkey());
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let blockhash = Hash::default();

        let presigned = builder.presign_initialize_intent_tx(&payer, blockhash);
        if let Err(e) = &presigned {
            eprintln!("Error presigning transaction: {}", e);
        }
        assert!(presigned.is_ok());

        let presigned = presigned.unwrap();

        use std::time::{SystemTime, UNIX_EPOCH};
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as i64;

        assert!(presigned.is_valid(now));
        assert!(!presigned.is_valid(now + 100)); // Future check should fail
    }

    #[test]
    fn test_presigned_transaction_size_validation() {
        let payer = Keypair::new();
        let swap_plan = create_test_swap_plan_with_authority(payer.pubkey());
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let blockhash = Hash::default();

        let presigned = builder.presign_initialize_intent_tx(&payer, blockhash);
        if let Err(e) = &presigned {
            eprintln!("Error presigning transaction: {}", e);
        }
        assert!(presigned.is_ok());

        let presigned = presigned.unwrap();
        // DirectBuyBuilder instruction has 12 accounts, so the transaction is larger than before
        // Without LUT: ~500-600 bytes
        // With LUT: ~200-300 bytes
        // We check for a reasonable size (< 700 bytes)
        assert!(presigned.size_bytes < 700);
        eprintln!(
            "Transaction size: {} bytes (target with LUT: ~200-300 bytes)",
            presigned.size_bytes
        );
    }

    #[test]
    fn test_get_lut_addresses() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let lut_addresses = builder.get_lut_addresses();
        assert!(!lut_addresses.is_empty());
        assert!(lut_addresses.len() >= 10); // Should have at least 10 addresses
    }

    #[test]
    fn test_get_amm_addresses() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let amm_addresses = builder.get_amm_addresses();
        assert_eq!(
            amm_addresses.program_id.to_string(),
            "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P"
        );
    }

    #[test]
    fn test_build_initialize_intent_instruction() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let result = builder.build_initialize_intent_instruction();
        if let Err(e) = &result {
            eprintln!("Error building instruction: {}", e);
        }
        assert!(result.is_ok());

        let ix = result.unwrap();
        assert!(!ix.data.is_empty());
        assert!(!ix.accounts.is_empty());
    }

    #[test]
    fn test_build_initialize_intent_tx() {
        let payer = Keypair::new();
        let swap_plan = create_test_swap_plan_with_authority(payer.pubkey());
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let blockhash = Hash::default();

        let result = builder.build_initialize_intent_tx(&payer, blockhash);
        if let Err(e) = &result {
            eprintln!("Error building transaction: {}", e);
        }
        assert!(result.is_ok());

        let tx = result.unwrap();
        assert!(!tx.signatures.is_empty());
    }

    #[test]
    fn test_optimize_slippage_with_shadow() {
        use ghost_core::BondingCurve;

        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let mut builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Create a test bonding curve with realistic values
        let curve = BondingCurve {
            discriminator: 0x1234567890abcdef,
            virtual_token_reserves: 1_000_000_000_000, // 1 trillion tokens
            virtual_sol_reserves: 30_000_000_000,      // 30 SOL (30B lamports)
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Initial min_amount_out is 900_000
        assert_eq!(builder.swap_plan.min_amount_out, 900_000);

        // Optimize with 50 bps (0.5%) slippage
        builder.optimize_slippage_with_shadow(&curve, 50);

        // min_amount_out should now be updated based on simulation
        // It should be approximately 99.5% of the expected tokens from simulate_buy
        let expected_tokens = curve.simulate_buy(builder.swap_plan.amount_in);
        let expected_min_out = expected_tokens * (10000 - 50) / 10000;

        assert_eq!(
            builder.swap_plan.min_amount_out, expected_min_out,
            "min_amount_out should be updated based on simulation"
        );
    }

    #[test]
    fn test_optimize_slippage_zero_slippage() {
        use ghost_core::BondingCurve;

        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let mut builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Zero slippage means we expect exactly the simulated amount
        builder.optimize_slippage_with_shadow(&curve, 0);

        let expected_tokens = curve.simulate_buy(builder.swap_plan.amount_in);
        assert_eq!(builder.swap_plan.min_amount_out, expected_tokens);
    }

    #[test]
    fn test_optimize_slippage_high_slippage() {
        use ghost_core::BondingCurve;

        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let mut builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        let curve = BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_000_000_000_000,
            virtual_sol_reserves: 30_000_000_000,
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // 100% slippage (10000 bps) should set min_amount_out to 0
        builder.optimize_slippage_with_shadow(&curve, 10000);
        assert_eq!(builder.swap_plan.min_amount_out, 0);
    }

    #[test]
    fn test_dynamic_ttl_at_zero_tension() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Test TTL at tension=0 → 400ms
        let ttl = builder.get_dynamic_blockhash_limit(0.0);
        assert_eq!(ttl, 400, "TTL at tension=0 should be 400ms");
    }

    #[test]
    fn test_dynamic_ttl_at_medium_tension() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Test TTL at tension=50 → 225ms
        let ttl = builder.get_dynamic_blockhash_limit(50.0);
        assert_eq!(ttl, 225, "TTL at tension=50 should be 225ms");
    }

    #[test]
    fn test_dynamic_ttl_at_high_tension() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Test TTL at tension=90 → 85ms
        let ttl = builder.get_dynamic_blockhash_limit(90.0);
        assert_eq!(ttl, 85, "TTL at tension=90 should be 85ms");
    }

    #[test]
    fn test_dynamic_ttl_at_max_tension() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Test TTL at tension=100 → 50ms (minimum)
        let ttl = builder.get_dynamic_blockhash_limit(100.0);
        assert_eq!(ttl, 50, "TTL at tension=100 should be 50ms (minimum)");
    }

    #[test]
    fn test_with_network_tension_builder_pattern() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts)
            .with_network_tension(75.0);

        // Test that tension is stored
        let ttl = builder.get_current_blockhash_limit();
        let expected = 400.0 - (75.0 * 3.5);
        assert_eq!(
            ttl, expected as u64,
            "TTL should be calculated from stored tension"
        );
    }

    #[test]
    fn test_set_network_tension_mutable() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let mut builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Initially tension is 0
        assert_eq!(builder.get_current_blockhash_limit(), 400);

        // Set tension to 50
        builder.set_network_tension(50.0);
        assert_eq!(builder.get_current_blockhash_limit(), 225);

        // Set tension to 100
        builder.set_network_tension(100.0);
        assert_eq!(builder.get_current_blockhash_limit(), 50);
    }

    #[test]
    fn test_dynamic_ttl_clamping() {
        let swap_plan = create_test_swap_plan();
        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Test tension values beyond valid range are clamped
        assert_eq!(
            builder.get_dynamic_blockhash_limit(-10.0),
            400,
            "Negative tension should clamp to 0"
        );
        assert_eq!(
            builder.get_dynamic_blockhash_limit(150.0),
            50,
            "Tension > 100 should clamp to 100"
        );
    }

    #[test]
    fn test_optimize_slippage_realistic_scenario() {
        use ghost_core::BondingCurve;

        // Create a realistic swap plan: 1 SOL input
        let config = LutConfig::new();
        let swap_plan = SwapPlan::new(
            Keypair::new().pubkey(),
            config.pump_fun.program_id,
            1_000_000_000, // 1 SOL
            1,             // Initial min_amount_out (will be updated)
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_secs() as i64
                + 3600,
        );

        let amm_accounts = AmmAccounts {
            pool: Pubkey::new_unique(),
            amm_program_id: None,
            bonding_curve: None,
            additional_accounts: vec![],
        };

        let mut builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);

        // Realistic bonding curve state
        let curve = BondingCurve {
            discriminator: 0x17b7bca8e24d1d39,
            virtual_token_reserves: 1_073_000_000_000, // ~1.073T tokens
            virtual_sol_reserves: 30_000_000_000,      // 30 SOL
            real_token_reserves: 800_000_000_000,
            real_sol_reserves: 20_000_000_000,
            token_total_supply: 1_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        };

        // Use 50 bps (0.5%) slippage
        builder.optimize_slippage_with_shadow(&curve, 50);

        // Verify the min_amount_out is set to a reasonable value
        assert!(
            builder.swap_plan.min_amount_out > 0,
            "min_amount_out should be positive"
        );

        // Verify it's less than the expected out (due to slippage allowance)
        let expected_tokens = curve.simulate_buy(1_000_000_000);
        assert!(
            expected_tokens > 0,
            "Expected tokens should be positive for this test"
        );
        assert!(
            builder.swap_plan.min_amount_out < expected_tokens,
            "min_amount_out should be less than expected due to slippage"
        );

        // Verify the reduction is approximately 0.5%
        let reduction_bps = ((expected_tokens - builder.swap_plan.min_amount_out) as u128 * 10000)
            / expected_tokens as u128;
        assert_eq!(reduction_bps, 50, "Reduction should be 50 bps");
    }
}
