//! Prospective, shadow-only RUG SCALP V2 signal reducer.
//!
//! This module owns only the bounded, decision-time signal state.  It cannot
//! submit, close, reconcile, or otherwise own a position lifecycle.  A caller
//! may consume [`RugScalpEntryAssessmentV2::is_candidate`] and hand the one
//! accepted intent to the existing isolated probe lane / Position Manager.

use std::collections::{HashMap, HashSet, VecDeque};

use anyhow::{anyhow, bail, Context, Result};
pub use ghost_brain::guardian::post_buy::RUG_SCALP_V2_STRATEGY_ID;
use ghost_brain::guardian::post_buy::{
    RugScalpDataCompletenessV1, RugScalpEntryWatermarkV1, RugScalpExitProfileConfigV1,
    RugScalpMarketFactKindV1, RugScalpMarketFactV1,
};
use ghost_core::market_state::BondingCurve;
use ghost_core::{
    FeeRounding, ProgramFeeRule, ProgramFeeSchedule, ProgramFeeScheduleEvidenceV1, PumpQuoteError,
    PumpQuoteV1, PumpReserveState, PumpRouteVariant, RuntimeProgramFeeScheduleRegistryV1,
    TransactionCosts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_client::nonblocking::rpc_client::RpcClient;
use solana_sdk::{account::Account, commitment_config::CommitmentConfig, pubkey::Pubkey};

use crate::events::{DetectedPool, PoolTransaction};
use crate::rug_scalp_validation_tape::RugScalpValidationTapeConfigV1;

pub const RUG_SCALP_EXIT_PROFILE_ID: &str = "rug_scalp_exit_v1";
const LAMPORTS_PER_SOL: u64 = 1_000_000_000;
const MAX_TRACKED_TRADES_PER_MINT: usize = 64;
const MAX_QTP_SEARCH_LAMPORTS: u64 = 1_000 * LAMPORTS_PER_SOL;
pub const RUG_SCALP_ENTRY_ROUTE: PumpRouteVariant = PumpRouteVariant::BuyV2;
pub const RUG_SCALP_EXIT_ROUTE: PumpRouteVariant = PumpRouteVariant::LegacySell;

/// Pinned by the same Pump IDL commit used by the typed BuyV2 builder. These
/// are the only two config accounts from which the RUG runtime materialises
/// fee authority; a serialized schedule is never its source of truth.
pub const RUG_SCALP_PUMP_GLOBAL_CONFIG: Pubkey =
    solana_sdk::pubkey!("4wTV1YmiEkRvAtNtsSGPtUrqRYQMe5SKy2uB4Jjaxnjf");
pub const RUG_SCALP_PUMP_FEE_CONFIG: Pubkey =
    solana_sdk::pubkey!("8Wf5TiAheLUqBrKXeYg2JtAFFMWtKdG2BSFgqUcPVwTt");
pub const RUG_SCALP_PUMP_PROGRAM: Pubkey =
    solana_sdk::pubkey!("6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P");
pub const RUG_SCALP_PUMP_FEE_PROGRAM: Pubkey =
    solana_sdk::pubkey!("pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ");

const PUMP_GLOBAL_DISCRIMINATOR: [u8; 8] = [167, 232, 232, 177, 200, 108, 114, 127];
const PUMP_FEE_CONFIG_DISCRIMINATOR: [u8; 8] = [143, 52, 146, 187, 219, 123, 76, 155];
const PUMP_GLOBAL_ACCOUNT_LEN: usize = 1_045;
const PUMP_FEE_CONFIG_ACCOUNT_LEN: usize = 4_073;
const PUMP_FEE_CONFIG_KNOWN_PREFIX_LEN: usize = 153;
const BPS_DENOMINATOR: u64 = 10_000;

/// Stable diagnostic class for a refresh of the immutable two-account fee
/// authority.  Transport failures are intentionally distinct from an
/// on-chain semantic contradiction: the former may be retried/advisory while
/// the latter remains evidence about the optional RUG lane itself.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RugScalpFeeAuthorityRefreshErrorClassV1 {
    Timeout,
    RateLimited,
    HttpStatus,
    Transport,
    Decode,
    SemanticValidation,
}

impl RugScalpFeeAuthorityRefreshErrorClassV1 {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Timeout => "timeout",
            Self::RateLimited => "http_429",
            Self::HttpStatus => "http_status",
            Self::Transport => "transport",
            Self::Decode => "decode",
            Self::SemanticValidation => "semantic_validation",
        }
    }
}

/// Classify without losing the raw error text at the callsite.  Solana's RPC
/// client wraps transport implementations, so the diagnostic boundary uses
/// conservative string recognition and defaults to `transport` only for
/// known connection-family messages; all other failures remain semantic.
pub fn classify_rug_scalp_fee_authority_refresh_error(
    error: &anyhow::Error,
) -> RugScalpFeeAuthorityRefreshErrorClassV1 {
    let text = error.to_string().to_ascii_lowercase();
    if text.contains("timeout") || text.contains("timed out") {
        RugScalpFeeAuthorityRefreshErrorClassV1::Timeout
    } else if text.contains("429") || text.contains("too many requests") {
        RugScalpFeeAuthorityRefreshErrorClassV1::RateLimited
    } else if text.contains("http status")
        || text.contains("http error")
        || text.contains("status code")
    {
        RugScalpFeeAuthorityRefreshErrorClassV1::HttpStatus
    } else if text.contains("connection")
        || text.contains("dns")
        || text.contains("reset")
        || text.contains("transport")
        || text.contains("network")
    {
        RugScalpFeeAuthorityRefreshErrorClassV1::Transport
    } else if text.contains("decode")
        || text.contains("discriminator")
        || text.contains("layout")
        || text.contains("account data")
    {
        RugScalpFeeAuthorityRefreshErrorClassV1::Decode
    } else {
        RugScalpFeeAuthorityRefreshErrorClassV1::SemanticValidation
    }
}

/// Frozen runtime authority for the two and only two Pump routes used by the
/// prospective RUG experiment.  The serialized form is deliberately only an
/// input to [`RuntimeProgramFeeScheduleRegistryV1`]: fixture evidence is
/// rejected by that registry before a RUG quote can be materialised.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RugScalpPumpQuoteAuthorityV1 {
    pub schedules: Vec<RugScalpPumpFeeScheduleV1>,
    pub entry_transaction_costs: TransactionCosts,
    pub exit_transaction_costs: TransactionCosts,
}

impl Default for RugScalpPumpQuoteAuthorityV1 {
    fn default() -> Self {
        Self {
            schedules: Vec::new(),
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugScalpPumpFeeScheduleV1 {
    pub route_variant: PumpRouteVariant,
    pub schedule: ProgramFeeSchedule,
}

/// Immutable receipt of the exact two-account authority snapshot used to
/// build the runtime registry. It is persisted into the validation run
/// manifest so schedule identifiers cannot be separated from their concrete
/// owner/address/data evidence later.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugScalpRuntimeFeeAuthorityManifestV1 {
    pub schema_version: u16,
    pub observed_slot: u64,
    pub effective_slot: u64,
    pub global_config_pubkey: String,
    pub global_owner_program: String,
    pub global_account_data_hash: String,
    pub fee_config_pubkey: String,
    pub fee_config_owner_program: String,
    pub fee_config_account_data_hash: String,
    /// SHA-256 over the two fixed pubkeys, their owners, and their complete
    /// data. The common observed slot is stored separately; excluding it here
    /// lets the runtime watch detect an actual account update rather than
    /// treating a newer unchanged RPC context as a fee-config mutation.
    pub evidence_hash: String,
    pub buy_v2_fee_schedule_id: String,
    pub legacy_sell_fee_schedule_id: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RugScalpPumpFeesV1 {
    lp_fee_bps: u64,
    protocol_fee_bps: u64,
    creator_fee_bps: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct RugScalpGlobalFeeConfigV1 {
    initialized: bool,
    create_v2_enabled: bool,
    fee_basis_points: u64,
    buyback_basis_points: u64,
}

/// Fetches the canonical Pump `global` and `fee_config` accounts in one RPC
/// context, validates their owner/address/discriminator/layout, then derives
/// the two route-specific typed fee schedules. Any evolved, partial, future,
/// or conflicting account surface is an error; callers must keep the RUG path
/// fail-closed rather than retain stale economics.
pub async fn materialize_rug_scalp_runtime_fee_authority_v1(
    rpc: &RpcClient,
    entry_transaction_costs: TransactionCosts,
    exit_transaction_costs: TransactionCosts,
) -> Result<(
    RugScalpPumpQuoteAuthorityV1,
    RugScalpRuntimeFeeAuthorityManifestV1,
)> {
    let response = rpc
        .get_multiple_accounts_with_commitment(
            &[RUG_SCALP_PUMP_GLOBAL_CONFIG, RUG_SCALP_PUMP_FEE_CONFIG],
            CommitmentConfig::processed(),
        )
        .await
        .context("fetch canonical Pump global and fee_config accounts")?;
    let observed_slot = response.context.slot;
    let mut accounts = response.value.into_iter();
    let global = accounts
        .next()
        .flatten()
        .ok_or_else(|| anyhow!("canonical Pump global account is missing"))?;
    let fee_config = accounts
        .next()
        .flatten()
        .ok_or_else(|| anyhow!("canonical Pump fee_config account is missing"))?;
    if accounts.next().is_some() {
        bail!("unexpected additional account in Pump fee authority response");
    }

    let global_state = decode_rug_scalp_global_config(&global)?;
    if !global_state.initialized || !global_state.create_v2_enabled {
        bail!(
            "Pump global does not authorize BuyV2: initialized={} create_v2_enabled={}",
            global_state.initialized,
            global_state.create_v2_enabled,
        );
    }
    if global_state.buyback_basis_points > BPS_DENOMINATOR {
        bail!(
            "Pump global buyback_basis_points {} exceeds {}",
            global_state.buyback_basis_points,
            BPS_DENOMINATOR,
        );
    }

    let fee_schedule = decode_rug_scalp_fee_config(&fee_config)?;
    if fee_schedule.protocol_fee_bps != global_state.fee_basis_points {
        bail!(
            "Pump global/fee_config protocol fee conflict: global={} fee_config={}",
            global_state.fee_basis_points,
            fee_schedule.protocol_fee_bps,
        );
    }

    // The account data hash preserves both config accounts because either can
    // change the route's settlement. The Evidence enum has one config pubkey,
    // therefore the fee-config PDA is the primary address and this aggregate
    // content hash binds the validated global alongside it.
    let global_data_hash = sha256_label(&global.data);
    let fee_config_data_hash = sha256_label(&fee_config.data);
    let evidence_hash = rug_scalp_fee_authority_evidence_hash(&global, &fee_config);
    let evidence = ProgramFeeScheduleEvidenceV1::OnChainConfig {
        config_pubkey: RUG_SCALP_PUMP_FEE_CONFIG.to_string(),
        owner_program: RUG_SCALP_PUMP_FEE_PROGRAM.to_string(),
        account_data_hash: evidence_hash.clone(),
        observed_slot,
    };
    let hash_suffix = evidence_hash.trim_start_matches("sha256:");
    let hash_suffix = &hash_suffix[..16.min(hash_suffix.len())];
    let buy_v2_fee_schedule_id = format!("pump-buy-v2@{observed_slot}:{hash_suffix}");
    let legacy_sell_fee_schedule_id = format!("pump-legacy-sell@{observed_slot}:{hash_suffix}");
    let effective_slot = observed_slot;

    let authority = RugScalpPumpQuoteAuthorityV1 {
        schedules: vec![
            RugScalpPumpFeeScheduleV1 {
                route_variant: RUG_SCALP_ENTRY_ROUTE,
                schedule: ProgramFeeSchedule {
                    fee_schedule_id: buy_v2_fee_schedule_id.clone(),
                    effective_slot,
                    evidence: evidence.clone(),
                    rules: runtime_buy_v2_fee_rules(fee_schedule, global_state)?,
                },
            },
            RugScalpPumpFeeScheduleV1 {
                route_variant: RUG_SCALP_EXIT_ROUTE,
                schedule: ProgramFeeSchedule {
                    fee_schedule_id: legacy_sell_fee_schedule_id.clone(),
                    effective_slot,
                    evidence,
                    rules: runtime_legacy_sell_fee_rules(fee_schedule, global_state)?,
                },
            },
        ],
        entry_transaction_costs,
        exit_transaction_costs,
    };
    authority
        .materialize()
        .map_err(|error| anyhow!("materialize runtime Pump fee authority: {error}"))?;

    Ok((
        authority,
        RugScalpRuntimeFeeAuthorityManifestV1 {
            schema_version: 1,
            observed_slot,
            effective_slot,
            global_config_pubkey: RUG_SCALP_PUMP_GLOBAL_CONFIG.to_string(),
            global_owner_program: global.owner.to_string(),
            global_account_data_hash: global_data_hash,
            fee_config_pubkey: RUG_SCALP_PUMP_FEE_CONFIG.to_string(),
            fee_config_owner_program: fee_config.owner.to_string(),
            fee_config_account_data_hash: fee_config_data_hash,
            evidence_hash,
            buy_v2_fee_schedule_id,
            legacy_sell_fee_schedule_id,
        },
    ))
}

fn decode_rug_scalp_global_config(account: &Account) -> Result<RugScalpGlobalFeeConfigV1> {
    validate_rug_scalp_config_account(
        account,
        RUG_SCALP_PUMP_PROGRAM,
        PUMP_GLOBAL_DISCRIMINATOR,
        PUMP_GLOBAL_ACCOUNT_LEN,
        "global",
    )?;
    let mut cursor = RugScalpAccountCursorV1::new(&account.data);
    cursor.take_discriminator(PUMP_GLOBAL_DISCRIMINATOR, "global")?;
    let initialized = cursor.take_bool("global.initialized")?;
    cursor.skip(32, "global.authority")?;
    cursor.skip(32, "global.fee_recipient")?;
    cursor.skip(8 * 4, "global.initial_curve_reserves_and_supply")?;
    let fee_basis_points = cursor.take_u64("global.fee_basis_points")?;
    cursor.skip(32, "global.withdraw_authority")?;
    cursor.take_bool("global.enable_migrate")?;
    cursor.skip(8 * 2, "global.pool_migration_and_creator_fee")?;
    cursor.skip(32 * 7, "global.fee_recipients")?;
    cursor.skip(32 * 2, "global.creator_authorities")?;
    let create_v2_enabled = cursor.take_bool("global.create_v2_enabled")?;
    cursor.skip(32 * 2, "global.whitelist_and_reserved_recipient")?;
    cursor.take_bool("global.mayhem_mode_enabled")?;
    cursor.skip(32 * 7, "global.reserved_fee_recipients")?;
    cursor.take_bool("global.is_cashback_enabled")?;
    cursor.skip(32 * 8, "global.buyback_fee_recipients")?;
    let buyback_basis_points = cursor.take_u64("global.buyback_basis_points")?;
    cursor.skip(8, "global.initial_virtual_quote_reserves")?;
    cursor.skip(32, "global.whitelisted_quote_mints")?;
    cursor.finish("global")?;
    Ok(RugScalpGlobalFeeConfigV1 {
        initialized,
        create_v2_enabled,
        fee_basis_points,
        buyback_basis_points,
    })
}

fn decode_rug_scalp_fee_config(account: &Account) -> Result<RugScalpPumpFeesV1> {
    validate_rug_scalp_config_account(
        account,
        RUG_SCALP_PUMP_FEE_PROGRAM,
        PUMP_FEE_CONFIG_DISCRIMINATOR,
        PUMP_FEE_CONFIG_ACCOUNT_LEN,
        "fee_config",
    )?;
    let mut cursor = RugScalpAccountCursorV1::new(&account.data);
    cursor.take_discriminator(PUMP_FEE_CONFIG_DISCRIMINATOR, "fee_config")?;
    cursor.skip(1 + 32, "fee_config.bump_and_admin")?;
    let flat_fees = cursor.take_fees("fee_config.flat_fees")?;
    let fee_tiers = cursor.take_fee_tiers("fee_config.fee_tiers")?;
    let stable_fee_tiers = cursor.take_fee_tiers("fee_config.stable_fee_tiers")?;
    if cursor.position() != PUMP_FEE_CONFIG_KNOWN_PREFIX_LEN {
        bail!(
            "fee_config parser consumed {} bytes, expected known prefix {}",
            cursor.position(),
            PUMP_FEE_CONFIG_KNOWN_PREFIX_LEN,
        );
    }
    let trailing = cursor.remaining();
    if trailing.iter().any(|byte| *byte != 0) {
        bail!("fee_config has a nonzero unknown trailing surface");
    }
    cursor.skip(trailing.len(), "fee_config.zero_reserved_tail")?;
    cursor.finish("fee_config")?;

    // The RUG quote contract is intentionally not a dynamic market-cap tier
    // selector. Current account evidence has one zero-threshold tier and an
    // identical stable tier; any other shape must be implemented and parity
    // proven separately, so it fails closed here.
    let resolve_uniform =
        |label: &str, tiers: &[(u128, RugScalpPumpFeesV1)]| -> Result<RugScalpPumpFeesV1> {
            let Some((threshold, fees)) = tiers.first().copied() else {
                bail!("fee_config.{label} is empty");
            };
            if tiers.len() != 1 || threshold != 0 {
                bail!("fee_config.{label} is not a single zero-threshold schedule");
            }
            Ok(fees)
        };
    let fee_tier_fees = resolve_uniform("fee_tiers", &fee_tiers)?;
    let stable_tier_fees = resolve_uniform("stable_fee_tiers", &stable_fee_tiers)?;
    if flat_fees != fee_tier_fees || fee_tier_fees != stable_tier_fees {
        bail!("fee_config flat, regular, and stable fees are not identical");
    }
    Ok(fee_tier_fees)
}

fn validate_rug_scalp_config_account(
    account: &Account,
    expected_owner: Pubkey,
    expected_discriminator: [u8; 8],
    expected_len: usize,
    label: &str,
) -> Result<()> {
    if account.executable {
        bail!("Pump {label} account is unexpectedly executable");
    }
    if account.owner != expected_owner {
        bail!(
            "Pump {label} owner mismatch: expected {}, got {}",
            expected_owner,
            account.owner,
        );
    }
    if account.data.len() != expected_len {
        bail!(
            "Pump {label} layout length mismatch: expected {expected_len}, got {}",
            account.data.len(),
        );
    }
    if account.data.get(..8) != Some(expected_discriminator.as_slice()) {
        bail!("Pump {label} discriminator mismatch");
    }
    Ok(())
}

fn runtime_buy_v2_fee_rules(
    fees: RugScalpPumpFeesV1,
    global: RugScalpGlobalFeeConfigV1,
) -> Result<Vec<ProgramFeeRule>> {
    let mut rules = protocol_fee_split_rules(fees.protocol_fee_bps, global.buyback_basis_points)?;
    if fees.creator_fee_bps > 0 {
        rules.push(ProgramFeeRule {
            component_id: "creator_fee".to_string(),
            numerator: fees.creator_fee_bps,
            denominator: BPS_DENOMINATOR,
            rounding: FeeRounding::Ceil,
        });
    }
    if rules.is_empty() {
        bail!("BuyV2 runtime fee schedule has no fee rules");
    }
    Ok(rules)
}

fn runtime_legacy_sell_fee_rules(
    fees: RugScalpPumpFeesV1,
    global: RugScalpGlobalFeeConfigV1,
) -> Result<Vec<ProgramFeeRule>> {
    let mut rules = Vec::new();
    if fees.lp_fee_bps > 0 {
        rules.push(ProgramFeeRule {
            component_id: "lp_fee".to_string(),
            numerator: fees.lp_fee_bps,
            denominator: BPS_DENOMINATOR,
            rounding: FeeRounding::Ceil,
        });
    }
    rules.extend(protocol_fee_split_rules(
        fees.protocol_fee_bps,
        global.buyback_basis_points,
    )?);
    if fees.creator_fee_bps > 0 {
        rules.push(ProgramFeeRule {
            component_id: "creator_fee".to_string(),
            numerator: fees.creator_fee_bps,
            denominator: BPS_DENOMINATOR,
            rounding: FeeRounding::Floor,
        });
    }
    if rules.is_empty() {
        bail!("LegacySell runtime fee schedule has no fee rules");
    }
    Ok(rules)
}

fn protocol_fee_split_rules(
    protocol_fee_bps: u64,
    buyback_basis_points: u64,
) -> Result<Vec<ProgramFeeRule>> {
    if protocol_fee_bps == 0 {
        return Ok(Vec::new());
    }
    if buyback_basis_points > BPS_DENOMINATOR {
        bail!("buyback basis points exceeds denominator");
    }
    let denominator = BPS_DENOMINATOR
        .checked_mul(BPS_DENOMINATOR)
        .ok_or_else(|| anyhow!("basis point denominator overflow"))?;
    let fee_recipient_numerator = protocol_fee_bps
        .checked_mul(BPS_DENOMINATOR.saturating_sub(buyback_basis_points))
        .ok_or_else(|| anyhow!("fee-recipient numerator overflow"))?;
    let buyback_numerator = protocol_fee_bps
        .checked_mul(buyback_basis_points)
        .ok_or_else(|| anyhow!("buyback numerator overflow"))?;
    let mut rules = Vec::with_capacity(2);
    if fee_recipient_numerator > 0 {
        rules.push(ProgramFeeRule {
            component_id: "fee_recipient".to_string(),
            numerator: fee_recipient_numerator,
            denominator,
            rounding: FeeRounding::Ceil,
        });
    }
    if buyback_numerator > 0 {
        rules.push(ProgramFeeRule {
            component_id: "buyback_fee_recipient".to_string(),
            numerator: buyback_numerator,
            denominator,
            rounding: FeeRounding::Floor,
        });
    }
    Ok(rules)
}

fn rug_scalp_fee_authority_evidence_hash(global: &Account, fee_config: &Account) -> String {
    let mut hasher = Sha256::new();
    hasher.update(b"rug_scalp_runtime_fee_authority_v1");
    for (pubkey, account) in [
        (RUG_SCALP_PUMP_GLOBAL_CONFIG, global),
        (RUG_SCALP_PUMP_FEE_CONFIG, fee_config),
    ] {
        hasher.update(pubkey.as_ref());
        hasher.update(account.owner.as_ref());
        hasher.update((account.data.len() as u64).to_le_bytes());
        hasher.update(&account.data);
    }
    format!("sha256:{:x}", hasher.finalize())
}

fn sha256_label(data: &[u8]) -> String {
    format!("sha256:{:x}", Sha256::digest(data))
}

struct RugScalpAccountCursorV1<'a> {
    data: &'a [u8],
    offset: usize,
}

impl<'a> RugScalpAccountCursorV1<'a> {
    fn new(data: &'a [u8]) -> Self {
        Self { data, offset: 0 }
    }

    fn position(&self) -> usize {
        self.offset
    }

    fn remaining(&self) -> &'a [u8] {
        &self.data[self.offset..]
    }

    fn take(&mut self, len: usize, label: &str) -> Result<&'a [u8]> {
        let end = self
            .offset
            .checked_add(len)
            .ok_or_else(|| anyhow!("{label}: cursor overflow"))?;
        let value = self
            .data
            .get(self.offset..end)
            .ok_or_else(|| anyhow!("{label}: truncated account layout"))?;
        self.offset = end;
        Ok(value)
    }

    fn skip(&mut self, len: usize, label: &str) -> Result<()> {
        self.take(len, label).map(|_| ())
    }

    fn take_discriminator(&mut self, expected: [u8; 8], label: &str) -> Result<()> {
        if self.take(8, label)? != expected.as_slice() {
            bail!("{label}: discriminator changed while decoding");
        }
        Ok(())
    }

    fn take_bool(&mut self, label: &str) -> Result<bool> {
        match self.take(1, label)?[0] {
            0 => Ok(false),
            1 => Ok(true),
            value => bail!("{label}: invalid bool discriminant {value}"),
        }
    }

    fn take_u64(&mut self, label: &str) -> Result<u64> {
        let raw: [u8; 8] = self
            .take(8, label)?
            .try_into()
            .map_err(|_| anyhow!("{label}: invalid u64 layout"))?;
        Ok(u64::from_le_bytes(raw))
    }

    fn take_u128(&mut self, label: &str) -> Result<u128> {
        let raw: [u8; 16] = self
            .take(16, label)?
            .try_into()
            .map_err(|_| anyhow!("{label}: invalid u128 layout"))?;
        Ok(u128::from_le_bytes(raw))
    }

    fn take_fees(&mut self, label: &str) -> Result<RugScalpPumpFeesV1> {
        Ok(RugScalpPumpFeesV1 {
            lp_fee_bps: self.take_u64(&format!("{label}.lp_fee_bps"))?,
            protocol_fee_bps: self.take_u64(&format!("{label}.protocol_fee_bps"))?,
            creator_fee_bps: self.take_u64(&format!("{label}.creator_fee_bps"))?,
        })
    }

    fn take_fee_tiers(&mut self, label: &str) -> Result<Vec<(u128, RugScalpPumpFeesV1)>> {
        let count_raw: [u8; 4] = self
            .take(4, label)?
            .try_into()
            .map_err(|_| anyhow!("{label}: invalid vector length"))?;
        let count = u32::from_le_bytes(count_raw) as usize;
        if count > 32 {
            bail!("{label}: fee tier count {count} exceeds bounded layout");
        }
        let mut tiers = Vec::with_capacity(count);
        for index in 0..count {
            let threshold = self.take_u128(&format!("{label}[{index}].threshold"))?;
            let fees = self.take_fees(&format!("{label}[{index}].fees"))?;
            tiers.push((threshold, fees));
        }
        Ok(tiers)
    }

    fn finish(&self, label: &str) -> Result<()> {
        if self.offset != self.data.len() {
            bail!(
                "{label}: {} unconsumed layout bytes remain",
                self.data.len().saturating_sub(self.offset)
            );
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RugScalpPumpQuoteContractV1 {
    registry: RuntimeProgramFeeScheduleRegistryV1,
    entry_transaction_costs: TransactionCosts,
    exit_transaction_costs: TransactionCosts,
}

impl RugScalpPumpQuoteAuthorityV1 {
    pub(crate) fn materialize(&self) -> Result<RugScalpPumpQuoteContractV1, PumpQuoteError> {
        let mut registry = RuntimeProgramFeeScheduleRegistryV1::default();
        for entry in &self.schedules {
            if !matches!(
                entry.route_variant,
                RUG_SCALP_ENTRY_ROUTE | RUG_SCALP_EXIT_ROUTE
            ) {
                return Err(PumpQuoteError::InvalidFeeEvidence {
                    detail: format!(
                        "rug_scalp_v2 route {} is typed but not execution-authorized",
                        entry.route_variant.as_str()
                    ),
                });
            }
            registry.register(entry.route_variant, entry.schedule.clone())?;
        }
        // Resolve neither schedule here: resolution is deliberately bound to
        // every canonical quote slot below.  This only rejects malformed
        // transaction-cost evidence before a runtime adapter is created.
        self.entry_transaction_costs.net_wallet_debit()?;
        self.exit_transaction_costs.net_wallet_debit()?;
        Ok(RugScalpPumpQuoteContractV1 {
            registry,
            entry_transaction_costs: self.entry_transaction_costs,
            exit_transaction_costs: self.exit_transaction_costs,
        })
    }
}

impl RugScalpPumpQuoteContractV1 {
    pub(crate) fn entry_transaction_cost_lamports(&self) -> Result<u64, PumpQuoteError> {
        self.entry_transaction_costs.net_wallet_debit()
    }

    /// Frozen cost of one full-position sell attempt.  Offline ACE-EV V2
    /// records this cost when a typed take-profit instruction misses its
    /// landed min-output protection; it never treats a failed attempt as a
    /// free hypothetical retry.
    pub(crate) fn exit_transaction_cost_lamports(&self) -> Result<u64, PumpQuoteError> {
        self.exit_transaction_costs.net_wallet_debit()
    }

    pub(crate) fn quote_buy_v2_under_wallet_cap(
        &self,
        slot: u64,
        reserves: PumpReserveState,
        max_wallet_debit: u64,
    ) -> Result<PumpQuoteV1, PumpQuoteError> {
        if max_wallet_debit == 0 || reserves.real_base_reserves == 0 {
            return Err(PumpQuoteError::ZeroAmount);
        }
        let mut lower = 1_u64;
        let mut upper = reserves.real_base_reserves;
        let mut best = None;
        while lower <= upper {
            let middle = lower.saturating_add(upper.saturating_sub(lower) / 2);
            let quote = self.registry.quote_exact_base_out(
                RUG_SCALP_ENTRY_ROUTE,
                slot,
                reserves,
                middle,
                max_wallet_debit,
            )?;
            if quote.instruction_limit_check.passed {
                best = Some(quote);
                lower = middle.saturating_add(1);
            } else {
                upper = middle.saturating_sub(1);
            }
        }
        best.ok_or(PumpQuoteError::ZeroAmount)
    }

    /// Re-quote the *already selected* BuyV2 base amount on a landed state.
    /// The original instruction cap remains immutable: callers cannot resize
    /// upward after observing a more favourable landed reserve state.
    pub(crate) fn quote_buy_v2_exact_base_out_with_max_sol_cost(
        &self,
        slot: u64,
        reserves: PumpReserveState,
        token_amount: u64,
        max_sol_cost: u64,
    ) -> Result<PumpQuoteV1, PumpQuoteError> {
        self.registry.quote_exact_base_out(
            RUG_SCALP_ENTRY_ROUTE,
            slot,
            reserves,
            token_amount,
            max_sol_cost,
        )
    }

    fn quote_exit_value(
        &self,
        slot: u64,
        reserves: PumpReserveState,
        token_amount: u64,
        min_program_credit: u64,
    ) -> Result<PumpQuoteV1, PumpQuoteError> {
        self.registry.quote_exact_base_in_sell(
            RUG_SCALP_EXIT_ROUTE,
            slot,
            reserves,
            token_amount,
            min_program_credit,
        )
    }

    /// Typed full-position sell quote with an explicit instruction min-output
    /// floor.  It exposes the program-level credit separately from transaction
    /// costs so an offline state machine can account for failed take-profit
    /// attempts without pretending a min-output rejection filled.
    pub(crate) fn quote_full_position_exit_with_min_program_credit(
        &self,
        slot: u64,
        reserves: PumpReserveState,
        token_amount: u64,
        min_program_credit: u64,
    ) -> Result<PumpQuoteV1, PumpQuoteError> {
        self.quote_exit_value(slot, reserves, token_amount, min_program_credit)
    }

    pub(crate) fn executable_exit_value_lamports(
        &self,
        slot: u64,
        reserves: PumpReserveState,
        token_amount: u64,
    ) -> Result<(PumpQuoteV1, u64), PumpQuoteError> {
        let quote = self.quote_exit_value(slot, reserves, token_amount, 0)?;
        let net = quote
            .program_settlement
            .wallet_debit_or_credit
            .checked_sub(self.exit_transaction_cost_lamports()?)
            .ok_or(PumpQuoteError::TransactionRefundExceedsDebit)?;
        Ok((quote, net))
    }
}

/// Frozen prospective settings for the RUG SCALP V2 experiment.
///
/// All fields have safe defaults: the experiment is disabled, its authority is
/// observe-only and latency has not been frozen.  Startup code must reject an
/// enabled configuration that lacks frozen latency or a complete cost model.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct RugScalpV2Config {
    pub enabled: bool,
    pub mode: RugScalpV2Mode,
    pub max_birth_age_ms: u64,
    pub min_prev_slot_buys: usize,
    pub min_current_slot_buys: usize,
    pub min_two_slot_buys: usize,
    pub min_two_slot_unique_users: usize,
    pub min_two_slot_effective_quote_sol: f64,
    pub min_current_to_previous_quote_ratio: f64,
    pub max_top1_quote_share: f64,
    pub require_zero_sells_before_entry: bool,
    pub primary_position_size_sol: f64,
    pub sensitivity_position_size_sol: f64,
    pub max_position_to_recent_flow_ratio: f64,
    pub max_entry_self_impact_bps: u32,
    pub profit_min_net_bps: i32,
    pub max_required_flow_to_recent_flow_ratio: f64,
    pub material_sell_reserve_drain_bps: u32,
    pub material_sell_position_value_drop_bps: u32,
    pub hard_stop_net_bps: i32,
    pub max_hold_slots: u64,
    pub max_hold_ms: u64,
    pub flow_stop_empty_slots: u8,
    pub primary_entry_latency_slots: Option<u64>,
    pub primary_exit_latency_slots: Option<u64>,
    pub stress_extra_latency_slots_1: u64,
    pub stress_extra_latency_slots_2: u64,
    pub one_signal_per_mint: bool,
    pub reentry_enabled: bool,
    pub position_manager_profile: String,
    /// Dedicated canonical Position Manager lifecycle file for RUG positions.
    /// It is deliberately separate from the generic shadow and P37 probe files.
    pub lifecycle_log_path: String,
    /// Typed pre-entry evidence, one assessment per inspected successful trade.
    pub signal_assessment_log_path: String,
    /// Isolated primary-only entry-attempt evidence.  It is never shared with
    /// P37 and contains no second sensitivity lifecycle.
    pub entry_log_path: String,
    /// Exactly one terminal outcome row per entry attempt or registered
    /// position.  The PM lifecycle file remains the detailed close authority.
    pub outcome_log_path: String,
    /// Backwards-compatible mirror of the typed entry transaction envelope.
    /// Program fees come only from `PumpQuoteV1` and must never be added here.
    pub entry_fixed_cost_lamports: Option<u64>,
    /// Fixed non-program costs applied once per full-position exit attempt.
    pub exit_fixed_cost_lamports: Option<u64>,
    /// Slot-resolved Pump V2 fee authority and the separately accounted
    /// transaction envelope.  This must be materialised from current
    /// on-chain/effective-slot evidence; a canonical fixture is rejected by
    /// the runtime registry and can only exercise offline quote tests.
    pub pump_quote_authority: Option<RugScalpPumpQuoteAuthorityV1>,
    /// Append-only, observe-only trajectory evidence. This is intentionally
    /// separate from the reducer, execution adapter, and Position Manager.
    pub validation_tape: RugScalpValidationTapeConfigV1,
}

impl Default for RugScalpV2Config {
    fn default() -> Self {
        Self {
            enabled: false,
            mode: RugScalpV2Mode::ObserveOnly,
            max_birth_age_ms: 5_000,
            min_prev_slot_buys: 2,
            min_current_slot_buys: 2,
            min_two_slot_buys: 6,
            min_two_slot_unique_users: 4,
            min_two_slot_effective_quote_sol: 0.50,
            min_current_to_previous_quote_ratio: 0.50,
            max_top1_quote_share: 0.40,
            require_zero_sells_before_entry: true,
            primary_position_size_sol: 0.10,
            sensitivity_position_size_sol: 0.20,
            max_position_to_recent_flow_ratio: 0.20,
            max_entry_self_impact_bps: 100,
            profit_min_net_bps: 1_000,
            max_required_flow_to_recent_flow_ratio: 0.50,
            material_sell_reserve_drain_bps: 500,
            material_sell_position_value_drop_bps: 1_500,
            hard_stop_net_bps: -500,
            max_hold_slots: 8,
            max_hold_ms: 5_000,
            flow_stop_empty_slots: 2,
            primary_entry_latency_slots: None,
            primary_exit_latency_slots: None,
            stress_extra_latency_slots_1: 1,
            stress_extra_latency_slots_2: 2,
            one_signal_per_mint: true,
            reentry_enabled: false,
            position_manager_profile: RUG_SCALP_EXIT_PROFILE_ID.to_string(),
            lifecycle_log_path: "logs/rug_scalp_v2/rug_scalp_position_events_v2.jsonl".to_string(),
            signal_assessment_log_path: "logs/rug_scalp_v2/rug_scalp_signal_assessments_v2.jsonl"
                .to_string(),
            entry_log_path: "logs/rug_scalp_v2/rug_scalp_probe_entries_v2.jsonl".to_string(),
            outcome_log_path: "logs/rug_scalp_v2/rug_scalp_outcomes_v2.jsonl".to_string(),
            entry_fixed_cost_lamports: None,
            exit_fixed_cost_lamports: None,
            pump_quote_authority: None,
            validation_tape: RugScalpValidationTapeConfigV1::default(),
        }
    }
}

impl RugScalpV2Config {
    pub fn primary_notional_lamports(&self) -> Option<u64> {
        sol_to_lamports(self.primary_position_size_sol)
    }

    pub fn sensitivity_notional_lamports(&self) -> Option<u64> {
        sol_to_lamports(self.sensitivity_position_size_sol)
    }

    pub fn technical_validation_capture_enabled(&self) -> bool {
        self.validation_tape.enabled && self.validation_tape.technical_capture
    }

    /// Strict execution readiness.  This deliberately does not auto-fill a
    /// latency or cost value: smoke must freeze both before a capture run.
    pub fn validate_enabled_contract(&self) -> Result<(), RugScalpConfigError> {
        if !self.enabled {
            if self.validation_tape.enabled {
                return Err(RugScalpConfigError::ValidationTapeRequiresEnabledExperiment);
            }
            return Ok(());
        }
        if !matches!(self.mode, RugScalpV2Mode::ObserveOnly) {
            return Err(RugScalpConfigError::ModeMustBeObserveOnly);
        }
        let technical_capture = self.technical_validation_capture_enabled();
        if self.validation_tape.enabled && self.validation_tape.log_path.trim().is_empty() {
            return Err(RugScalpConfigError::MissingValidationTapePath);
        }
        if !technical_capture
            && (self.primary_entry_latency_slots.is_none()
                || self.primary_exit_latency_slots.is_none())
        {
            return Err(RugScalpConfigError::LatencyNotFrozen);
        }
        if !technical_capture
            && (self.entry_fixed_cost_lamports.is_none() || self.exit_fixed_cost_lamports.is_none())
        {
            return Err(RugScalpConfigError::CostModelNotFrozen);
        }
        if let Some(quote_authority) = self.pump_quote_authority.as_ref() {
            let quote_contract = quote_authority
                .materialize()
                .map_err(|_| RugScalpConfigError::InvalidPumpQuoteAuthority)?;
            // The historical scalar values remain only as a backwards-compatible
            // config surface.  They must exactly mirror the typed envelope costs,
            // never replace the typed Pump settlement contract.
            if self.entry_fixed_cost_lamports.is_some()
                && (self.entry_fixed_cost_lamports
                    != Some(
                        quote_contract
                            .entry_transaction_cost_lamports()
                            .map_err(|_| RugScalpConfigError::InvalidPumpQuoteAuthority)?,
                    )
                    || self.exit_fixed_cost_lamports
                        != Some(
                            quote_contract
                                .exit_transaction_cost_lamports()
                                .map_err(|_| RugScalpConfigError::InvalidPumpQuoteAuthority)?,
                        ))
            {
                return Err(RugScalpConfigError::CostModelMismatch);
            }
        } else if !technical_capture {
            return Err(RugScalpConfigError::PumpQuoteAuthorityNotFrozen);
        }
        if self.primary_notional_lamports().is_none()
            || self.sensitivity_notional_lamports().is_none()
            || self.primary_position_size_sol <= 0.0
            || self.sensitivity_position_size_sol <= self.primary_position_size_sol
        {
            return Err(RugScalpConfigError::InvalidNotional);
        }
        if self.position_manager_profile != RUG_SCALP_EXIT_PROFILE_ID {
            return Err(RugScalpConfigError::InvalidPositionManagerProfile);
        }
        if self.lifecycle_log_path.trim().is_empty()
            || self.signal_assessment_log_path.trim().is_empty()
            || self.entry_log_path.trim().is_empty()
            || self.outcome_log_path.trim().is_empty()
        {
            return Err(RugScalpConfigError::MissingArtifactPath);
        }
        if self.reentry_enabled || !self.one_signal_per_mint {
            return Err(RugScalpConfigError::SignalIdempotencyRequired);
        }
        if !(0.0..=1.0).contains(&self.max_position_to_recent_flow_ratio)
            || !(0.0..=1.0).contains(&self.max_top1_quote_share)
            || !(0.0..=1.0).contains(&self.max_required_flow_to_recent_flow_ratio)
            || self.profit_min_net_bps <= 0
            || self.hard_stop_net_bps >= 0
        {
            return Err(RugScalpConfigError::InvalidThreshold);
        }
        Ok(())
    }

    /// Explicit one-way projection into the Position Manager profile.  The
    /// reducer never consumes this value; the adapter installs it only for
    /// the isolated shadow/probe manager after full launcher preflight.
    pub fn position_manager_exit_profile(
        &self,
    ) -> Result<RugScalpExitProfileConfigV1, RugScalpConfigError> {
        self.validate_enabled_contract()?;
        Ok(RugScalpExitProfileConfigV1 {
            enabled: self.enabled,
            profile_id: self.position_manager_profile.clone(),
            material_sell_reserve_drain_bps: self.material_sell_reserve_drain_bps,
            material_sell_position_value_drop_bps: self.material_sell_position_value_drop_bps,
            profit_min_net_bps: self.profit_min_net_bps,
            hard_stop_net_bps: self.hard_stop_net_bps,
            max_hold_slots: self.max_hold_slots,
            max_hold_ms: self.max_hold_ms,
            flow_stop_empty_slots: self.flow_stop_empty_slots,
            primary_exit_latency_slots: self.primary_exit_latency_slots.unwrap_or_default(),
            entry_fixed_cost_lamports: self.entry_fixed_cost_lamports.unwrap_or_default(),
            exit_fixed_cost_lamports: self.exit_fixed_cost_lamports.unwrap_or_default(),
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpV2Mode {
    #[default]
    ObserveOnly,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RugScalpConfigError {
    ModeMustBeObserveOnly,
    LatencyNotFrozen,
    CostModelNotFrozen,
    PumpQuoteAuthorityNotFrozen,
    InvalidPumpQuoteAuthority,
    CostModelMismatch,
    InvalidNotional,
    InvalidPositionManagerProfile,
    MissingArtifactPath,
    MissingValidationTapePath,
    ValidationTapeRequiresEnabledExperiment,
    IsolatedProbeLaneRequired,
    SignalIdempotencyRequired,
    InvalidThreshold,
}

impl std::fmt::Display for RugScalpConfigError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::ModeMustBeObserveOnly => "rug_scalp_v2.mode must be observe_only",
            Self::LatencyNotFrozen => "rug_scalp_v2 latency slots must be frozen before enable",
            Self::CostModelNotFrozen => {
                "rug_scalp_v2 fixed cost model must be frozen before enable"
            }
            Self::PumpQuoteAuthorityNotFrozen => {
                "rug_scalp_v2 requires slot-resolved Pump fee authority before enable"
            }
            Self::InvalidPumpQuoteAuthority => {
                "rug_scalp_v2 Pump fee authority is invalid or cannot authorize runtime quotes"
            }
            Self::CostModelMismatch => {
                "rug_scalp_v2 legacy cost totals must equal the typed transaction-cost ledger"
            }
            Self::InvalidNotional => {
                "rug_scalp_v2 requires finite 0.10 SOL primary and larger sensitivity"
            }
            Self::InvalidPositionManagerProfile => "rug_scalp_v2 must use rug_scalp_exit_v1",
            Self::MissingArtifactPath => "rug_scalp_v2 requires dedicated lifecycle and signal artifact paths",
            Self::MissingValidationTapePath => "rug_scalp_v2.validation_tape requires a log_path",
            Self::ValidationTapeRequiresEnabledExperiment => {
                "rug_scalp_v2.validation_tape requires rug_scalp_v2.enabled=true"
            }
            Self::IsolatedProbeLaneRequired => {
                "rug_scalp_v2 cannot share the isolated probe Position Manager with p37_shadow_probe"
            }
            Self::SignalIdempotencyRequired => {
                "rug_scalp_v2 requires one_signal_per_mint and reentry disabled"
            }
            Self::InvalidThreshold => "rug_scalp_v2 contains an invalid threshold",
        })
    }
}

impl std::error::Error for RugScalpConfigError {}

/// Provenance supplied by the canonical ingest/state owner at assessment time.
/// The reducer never repairs an unknown state or ordering relation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct RugScalpCanonicalStateV2 {
    pub state_clean: bool,
    pub ordering_known: bool,
    pub accepted_window_has_gap: bool,
}

impl Default for RugScalpCanonicalStateV2 {
    fn default() -> Self {
        Self {
            state_clean: false,
            ordering_known: false,
            accepted_window_has_gap: true,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpAssessment {
    Rejected,
    RejectedLowCapacity,
    ShadowEdgeCandidate,
    NonEvaluable,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpReasonCodeV2 {
    Disabled,
    UniverseIneligible,
    MissingBirthSlot,
    MissingTradeSlot,
    MissingTradeOrder,
    DuplicateTrade,
    ReorderedTrade,
    AcceptedWindowGap,
    CanonicalStateUnavailable,
    CanonicalStateDirty,
    CanonicalOrderingUnknown,
    SellSeen,
    AgeExceeded,
    MissingPreviousSlot,
    PreviousSlotBuyCount,
    CurrentSlotBuyCount,
    TwoSlotBuyCount,
    UniqueUsers,
    RecentFlow,
    CurrentFlowPersistence,
    Top1Share,
    PrimaryCapacity,
    PrimarySelfImpact,
    PrimaryTargetFlow,
    QuoteMathUnavailable,
    SignalAlreadyEmitted,
    CandidateAccepted,
}

/// Program-math result for one notional on one immutable curve state.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RugScalpNotionalQuoteV2 {
    pub notional_lamports: u64,
    pub entry_token_amount_raw: u64,
    /// Program settlement debit for the exact `buy_v2` instruction.  The
    /// primary notional is a ceiling, never an ambiguous curve input.
    pub entry_wallet_debit_lamports: u64,
    /// Transaction-envelope debit kept outside Pump settlement.
    pub entry_transaction_cost_lamports: u64,
    pub exit_transaction_cost_lamports: u64,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub entry_fee_schedule_id: String,
    pub exit_fee_schedule_id: String,
    pub self_impact_bps: u32,
    pub q_tp_lamports: Option<u64>,
    pub q_tp_status: RugScalpQuoteStatusV2,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpQuoteStatusV2 {
    Resolved,
    Unreachable,
    InvalidCurve,
    RuntimeFeeAuthorityUnavailable,
}

/// Typed assessment written before any execution adapter consumes a signal.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct RugScalpEntryAssessmentV2 {
    pub strategy_id: &'static str,
    pub exit_profile_id: &'static str,
    pub mint: String,
    pub pool_id: String,
    pub birth_slot: Option<u64>,
    pub signal_slot: Option<u64>,
    pub birth_ingress_ms: u64,
    pub signal_ingress_ms: u64,
    pub age_ms: u64,
    pub n_prev: usize,
    pub n_curr: usize,
    pub n_2: usize,
    pub u_2: usize,
    pub v_prev_sol: f64,
    pub v_curr_sol: f64,
    pub v_2_sol: f64,
    pub top1_share: Option<f64>,
    pub sell_seen: bool,
    pub assessment: RugScalpAssessment,
    pub reason: RugScalpReasonCodeV2,
    pub primary_quote: Option<RugScalpNotionalQuoteV2>,
    pub sensitivity_quote: Option<RugScalpNotionalQuoteV2>,
}

impl RugScalpEntryAssessmentV2 {
    pub fn is_candidate(&self) -> bool {
        matches!(self.assessment, RugScalpAssessment::ShadowEdgeCandidate)
    }
}

/// Pure bounded reducer.  It retains only five seconds of pre-entry signal
/// evidence per mint; it has no lifecycle or exit API by construction.
#[derive(Debug)]
pub struct RugScalpSignalReducerV2 {
    config: RugScalpV2Config,
    quote_contract: Option<RugScalpPumpQuoteContractV1>,
    mints: HashMap<String, MintSignalState>,
}

impl RugScalpSignalReducerV2 {
    fn new(config: RugScalpV2Config, quote_contract: Option<RugScalpPumpQuoteContractV1>) -> Self {
        Self {
            config,
            quote_contract,
            mints: HashMap::new(),
        }
    }

    pub fn on_birth(&mut self, pool: &DetectedPool, ingress_ms: u64) {
        if !self.config.enabled {
            return;
        }
        let universe_eligible = is_canonical_rug_scalp_pump_program(&pool.amm_program)
            && is_sol_pair(&pool.quote_mint)
            && !pool.bonding_curve.trim().is_empty()
            && pool.slot.is_some();
        self.mints.insert(
            pool.base_mint.clone(),
            MintSignalState {
                pool_id: pool.pool_amm_id.clone(),
                birth_slot: pool.slot,
                birth_ingress_ms: ingress_ms,
                universe_eligible,
                ..Default::default()
            },
        );
    }

    /// Observe one canonical trade.  `curve` must be the same canonical curve
    /// materialization used by the current authoritative Pump quote path.
    pub fn on_trade(
        &mut self,
        tx: &PoolTransaction,
        ingress_ms: u64,
        state: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> Option<RugScalpEntryAssessmentV2> {
        if !self.config.enabled || !tx.success {
            return None;
        }
        let mint = tx.token_mint.as_ref()?;
        let signal_state = self.mints.get_mut(mint)?;
        let slot = match tx.slot {
            Some(slot) => slot,
            None => {
                signal_state.terminal_reason = Some(RugScalpReasonCodeV2::MissingTradeSlot);
                return Some(signal_state.assessment(
                    mint,
                    ingress_ms,
                    RugScalpAssessment::NonEvaluable,
                    RugScalpReasonCodeV2::MissingTradeSlot,
                    None,
                    None,
                ));
            }
        };
        let order = match (tx.tx_index, tx.event_ordinal) {
            (Some(tx_index), Some(event_ordinal)) => TradeOrder {
                slot,
                tx_index,
                event_ordinal,
            },
            _ => {
                signal_state.terminal_reason = Some(RugScalpReasonCodeV2::MissingTradeOrder);
                return Some(signal_state.assessment(
                    mint,
                    ingress_ms,
                    RugScalpAssessment::NonEvaluable,
                    RugScalpReasonCodeV2::MissingTradeOrder,
                    None,
                    None,
                ));
            }
        };
        if !signal_state.seen_signatures.insert(tx.signature.clone()) {
            return Some(signal_state.assessment(
                mint,
                ingress_ms,
                RugScalpAssessment::Rejected,
                RugScalpReasonCodeV2::DuplicateTrade,
                None,
                None,
            ));
        }
        if signal_state
            .last_order
            .is_some_and(|last_order| order <= last_order)
        {
            signal_state.terminal_reason = Some(RugScalpReasonCodeV2::ReorderedTrade);
        }
        if signal_state
            .last_order
            .is_some_and(|last_order| slot > last_order.slot.saturating_add(1))
        {
            signal_state.accepted_window_has_gap = true;
        }
        signal_state.last_order = Some(order);

        if !tx.is_buy {
            signal_state.sell_seen = true;
            signal_state.terminal_reason = Some(RugScalpReasonCodeV2::SellSeen);
        } else {
            signal_state.trades.push_back(ObservedBuy {
                slot,
                signer: tx.signer.clone(),
                effective_quote_lamports: tx
                    .sol_amount_lamports
                    .unwrap_or_else(|| sol_to_lamports(tx.volume_sol).unwrap_or(0)),
            });
            while signal_state.trades.len() > MAX_TRACKED_TRADES_PER_MINT {
                signal_state.trades.pop_front();
                signal_state.accepted_window_has_gap = true;
            }
        }

        if signal_state.signal_emitted && self.config.one_signal_per_mint {
            return Some(signal_state.assessment(
                mint,
                ingress_ms,
                RugScalpAssessment::Rejected,
                RugScalpReasonCodeV2::SignalAlreadyEmitted,
                None,
                None,
            ));
        }

        let age_ms = ingress_ms.saturating_sub(signal_state.birth_ingress_ms);
        if age_ms > self.config.max_birth_age_ms {
            signal_state.terminal_reason = Some(RugScalpReasonCodeV2::AgeExceeded);
        }
        let (assessment, reason, primary_quote, sensitivity_quote) = signal_state.evaluate(
            &self.config,
            self.quote_contract.as_ref(),
            mint,
            slot,
            ingress_ms,
            state,
            curve,
        );
        if matches!(assessment, RugScalpAssessment::ShadowEdgeCandidate) {
            signal_state.signal_emitted = true;
        }
        Some(signal_state.assessment(
            mint,
            ingress_ms,
            assessment,
            reason,
            primary_quote,
            sensitivity_quote,
        ))
    }

    pub fn mark_stream_gap(&mut self) {
        for state in self.mints.values_mut() {
            if !state.signal_emitted {
                state.accepted_window_has_gap = true;
            }
        }
    }
}

/// A single entry intent emitted by the signal reducer.  It carries the
/// primary `0.10 SOL` notional only; the `0.20 SOL` quote stays solely in the
/// paired assessment as a counterfactual sensitivity measurement.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct RugScalpEntryIntentV2 {
    pub candidate_id: String,
    pub assessment: RugScalpEntryAssessmentV2,
    pub primary_notional_lamports: u64,
    pub expected_entry_token_amount_raw: u64,
    /// Exact typed `BuyV2` economics carried into the one PM lifecycle.  The
    /// sensitivity notional has no corresponding field and no lifecycle.
    pub entry_wallet_debit_lamports: u64,
    pub entry_total_debit_lamports: u64,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub entry_fee_schedule_id: String,
    pub exit_fee_schedule_id: String,
}

/// Bounded, typed hand-off produced by the launcher-side RUG adapter.  The
/// adapter never chooses an exit: market evidence is forwarded to the
/// Position Manager as [`RugScalpMarketFactV1`].
#[derive(Debug, Clone, PartialEq)]
pub enum RugScalpRuntimeActionV2 {
    Assessment(RugScalpEntryAssessmentV2),
    EntryIntent(RugScalpEntryIntentV2),
    MarketFact(RugScalpMarketFactV1),
}

/// One terminal outcome per RUG entry attempt.  The detailed decision and
/// reconciliation chain remains in the Position Manager lifecycle stream;
/// this row is the bounded experiment-level join point.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpTerminalOutcomeV2 {
    NoEntry,
    EntryFailed,
    EntryUnknown,
    PositionClosed,
    ExitUnavailable,
    DataInvalidated,
}

/// Accounting disposition is deliberately separate from terminal outcome.
/// A missing-evidence terminal is neither a zero-PnL trade nor an omitted
/// denominator row: it invalidates the smoke/run evidence and is excluded
/// from EV aggregation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RugScalpEvDispositionV2 {
    Eligible,
    ExcludedDataInvalidated,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpOutcomeRecordV2 {
    pub schema_version: u16,
    pub strategy_id: &'static str,
    pub exit_profile_id: &'static str,
    pub terminal_outcome: RugScalpTerminalOutcomeV2,
    pub ev_disposition: RugScalpEvDispositionV2,
    pub invalidates_smoke_or_run: bool,
    pub candidate_id: String,
    pub position_id: Option<String>,
    pub mint: String,
    pub pool_id: String,
    pub primary_notional_lamports: u64,
    pub sensitivity_notional_lamports: u64,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub entry_fee_schedule_id: String,
    pub exit_fee_schedule_id: String,
    pub entry_status: String,
    pub exit_reason: Option<String>,
    pub failure_reason: Option<String>,
    pub entry_token_amount_raw: Option<u64>,
    pub entry_landed_slot: Option<u64>,
    pub exit_landed_slot: Option<u64>,
    pub net_pnl_lamports: Option<i64>,
}

#[derive(Debug, Clone, Serialize)]
pub struct RugScalpEntryAttemptRecordV2 {
    pub schema_version: u16,
    pub strategy_id: &'static str,
    pub exit_profile_id: &'static str,
    pub candidate_id: String,
    pub mint: String,
    pub pool_id: String,
    pub primary_notional_lamports: u64,
    pub sensitivity_notional_lamports: u64,
    pub entry_route_id: String,
    pub exit_route_id: String,
    pub entry_fee_schedule_id: String,
    pub exit_fee_schedule_id: String,
    pub signal_slot: Option<u64>,
    pub dispatch_status: String,
    pub simulation_rpc_slot: Option<u64>,
    pub entry_token_amount_raw: Option<u64>,
    pub position_id: Option<String>,
    pub failure_reason: Option<String>,
}

impl RugScalpEntryAttemptRecordV2 {
    pub fn from_intent(intent: &RugScalpEntryIntentV2, dispatch_status: impl Into<String>) -> Self {
        Self {
            schema_version: 2,
            strategy_id: RUG_SCALP_V2_STRATEGY_ID,
            exit_profile_id: RUG_SCALP_EXIT_PROFILE_ID,
            candidate_id: intent.candidate_id.clone(),
            mint: intent.assessment.mint.clone(),
            pool_id: intent.assessment.pool_id.clone(),
            primary_notional_lamports: intent.primary_notional_lamports,
            sensitivity_notional_lamports: intent
                .assessment
                .sensitivity_quote
                .as_ref()
                .map(|quote| quote.notional_lamports)
                .unwrap_or_default(),
            entry_route_id: intent.entry_route_id.clone(),
            exit_route_id: intent.exit_route_id.clone(),
            entry_fee_schedule_id: intent.entry_fee_schedule_id.clone(),
            exit_fee_schedule_id: intent.exit_fee_schedule_id.clone(),
            signal_slot: intent.assessment.signal_slot,
            dispatch_status: dispatch_status.into(),
            simulation_rpc_slot: None,
            entry_token_amount_raw: None,
            position_id: None,
            failure_reason: None,
        }
    }
}

impl RugScalpOutcomeRecordV2 {
    fn accounting_disposition(
        outcome: &RugScalpTerminalOutcomeV2,
    ) -> (RugScalpEvDispositionV2, bool) {
        if matches!(outcome, RugScalpTerminalOutcomeV2::DataInvalidated) {
            (RugScalpEvDispositionV2::ExcludedDataInvalidated, true)
        } else {
            (RugScalpEvDispositionV2::Eligible, false)
        }
    }

    pub fn entry_terminal(
        intent: &RugScalpEntryIntentV2,
        outcome: RugScalpTerminalOutcomeV2,
        entry_status: impl Into<String>,
        failure_reason: Option<String>,
    ) -> Self {
        let (ev_disposition, invalidates_smoke_or_run) = Self::accounting_disposition(&outcome);
        Self {
            schema_version: 2,
            strategy_id: RUG_SCALP_V2_STRATEGY_ID,
            exit_profile_id: RUG_SCALP_EXIT_PROFILE_ID,
            terminal_outcome: outcome,
            ev_disposition,
            invalidates_smoke_or_run,
            candidate_id: intent.candidate_id.clone(),
            position_id: None,
            mint: intent.assessment.mint.clone(),
            pool_id: intent.assessment.pool_id.clone(),
            primary_notional_lamports: intent.primary_notional_lamports,
            sensitivity_notional_lamports: intent
                .assessment
                .sensitivity_quote
                .as_ref()
                .map(|quote| quote.notional_lamports)
                .unwrap_or_default(),
            entry_route_id: intent.entry_route_id.clone(),
            exit_route_id: intent.exit_route_id.clone(),
            entry_fee_schedule_id: intent.entry_fee_schedule_id.clone(),
            exit_fee_schedule_id: intent.exit_fee_schedule_id.clone(),
            entry_status: entry_status.into(),
            exit_reason: None,
            failure_reason,
            entry_token_amount_raw: None,
            entry_landed_slot: None,
            exit_landed_slot: None,
            net_pnl_lamports: None,
        }
    }

    pub fn position_terminal(
        candidate_id: String,
        position_id: String,
        mint: String,
        pool_id: String,
        primary_notional_lamports: u64,
        sensitivity_notional_lamports: u64,
        outcome: RugScalpTerminalOutcomeV2,
        exit_reason: String,
        exit_landed_slot: Option<u64>,
        net_pnl_lamports: Option<i64>,
        entry_route_id: String,
        exit_route_id: String,
        entry_fee_schedule_id: String,
        exit_fee_schedule_id: String,
    ) -> Self {
        let (ev_disposition, invalidates_smoke_or_run) = Self::accounting_disposition(&outcome);
        Self {
            schema_version: 2,
            strategy_id: RUG_SCALP_V2_STRATEGY_ID,
            exit_profile_id: RUG_SCALP_EXIT_PROFILE_ID,
            terminal_outcome: outcome,
            ev_disposition,
            invalidates_smoke_or_run,
            candidate_id,
            position_id: Some(position_id),
            mint,
            pool_id,
            primary_notional_lamports,
            sensitivity_notional_lamports,
            entry_route_id,
            exit_route_id,
            entry_fee_schedule_id,
            exit_fee_schedule_id,
            entry_status: "modelled_fill_registered".to_string(),
            exit_reason: Some(exit_reason),
            failure_reason: None,
            entry_token_amount_raw: None,
            entry_landed_slot: None,
            exit_landed_slot,
            net_pnl_lamports,
        }
    }
}

/// Appends a single JSONL record.  The caller owns terminal dedupe; PM's
/// one-shot terminal channel and the adapter's one-intent invariant provide
/// that ownership boundary.  Failure is explicit so smoke may reject writer
/// gaps without changing runtime behaviour.
pub async fn append_rug_scalp_jsonl_record<T: Serialize>(
    path: &std::path::Path,
    record: &T,
) -> Result<(), String> {
    use tokio::io::AsyncWriteExt;

    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|error| format!("create rug_scalp artifact directory: {error}"))?;
    }
    let mut file = tokio::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .await
        .map_err(|error| format!("open rug_scalp artifact: {error}"))?;
    let mut payload = serde_json::to_vec(record)
        .map_err(|error| format!("serialize rug_scalp artifact: {error}"))?;
    payload.push(b'\n');
    file.write_all(&payload)
        .await
        .map_err(|error| format!("write rug_scalp artifact: {error}"))?;
    file.flush()
        .await
        .map_err(|error| format!("flush rug_scalp artifact: {error}"))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RugScalpAdapterBindErrorV2 {
    InvalidMint,
    NoPendingEntry,
    CandidateMismatch,
    ZeroFilledTokens,
}

impl std::fmt::Display for RugScalpAdapterBindErrorV2 {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(match self {
            Self::InvalidMint => "rug_scalp invalid mint identity",
            Self::NoPendingEntry => "rug_scalp position binding without accepted entry intent",
            Self::CandidateMismatch => "rug_scalp position binding candidate mismatch",
            Self::ZeroFilledTokens => "rug_scalp fill evidence has zero token quantity",
        })
    }
}

impl std::error::Error for RugScalpAdapterBindErrorV2 {}

/// Launcher-side runtime bridge.  It owns only the short, bounded interval
/// between a signal and PM registration.  Once bound, it materializes facts
/// from canonical trade/curve evidence; it does not own a position, PnL, or an
/// exit verdict.
#[derive(Debug)]
pub struct RugScalpRuntimeAdapterV2 {
    config: RugScalpV2Config,
    quote_contract: Option<RugScalpPumpQuoteContractV1>,
    signal: RugScalpSignalReducerV2,
    pending_entries: HashMap<String, PendingRugScalpEntryV2>,
    active_positions: HashMap<String, ActiveRugScalpFactStreamV2>,
}

#[derive(Debug)]
struct PendingRugScalpEntryV2 {
    intent: RugScalpEntryIntentV2,
    entry_curve: Option<BondingCurve>,
    deferred_trades: VecDeque<DeferredRugScalpTradeV2>,
}

#[derive(Debug)]
struct DeferredRugScalpTradeV2 {
    tx: PoolTransaction,
    canonical: RugScalpCanonicalStateV2,
    curve: Option<BondingCurve>,
}

#[derive(Debug)]
struct ActiveRugScalpFactStreamV2 {
    position_id: String,
    mint: Pubkey,
    entry_token_amount_raw: u64,
    entry_watermark: RugScalpEntryWatermarkV1,
    current_slot: Option<RugScalpOpenSlotV2>,
    last_curve: Option<BondingCurve>,
    last_order: Option<TradeOrder>,
    seen_signatures: HashSet<String>,
    gap_reported: bool,
    quote_contract: Option<RugScalpPumpQuoteContractV1>,
}

#[derive(Debug, Clone, Copy)]
struct RugScalpOpenSlotV2 {
    slot: u64,
    successful_buy_count: u32,
}

impl RugScalpRuntimeAdapterV2 {
    pub fn new(config: RugScalpV2Config) -> Self {
        let quote_contract = config
            .pump_quote_authority
            .as_ref()
            .and_then(|authority| authority.materialize().ok());
        Self {
            signal: RugScalpSignalReducerV2::new(config.clone(), quote_contract.clone()),
            config,
            quote_contract,
            pending_entries: HashMap::new(),
            active_positions: HashMap::new(),
        }
    }

    pub fn on_birth(&mut self, pool: &DetectedPool, ingress_ms: u64) {
        self.signal.on_birth(pool, ingress_ms);
    }

    /// Consumes one canonical pool transaction and emits only explicit
    /// assessment/entry/fact actions.  A received assessment alone never
    /// creates a PM position; [`Self::bind_confirmed_or_modelled_fill`] is the
    /// mandatory post-submission boundary.
    pub fn on_trade(
        &mut self,
        tx: &PoolTransaction,
        ingress_ms: u64,
        canonical: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> Vec<RugScalpRuntimeActionV2> {
        if !self.config.enabled {
            return Vec::new();
        }
        let Some(mint) = tx.token_mint.as_ref() else {
            return Vec::new();
        };
        let mut actions = Vec::with_capacity(3);
        if let Some(active) = self.active_positions.get_mut(mint) {
            actions.extend(active.observe_trade(tx, canonical, curve));
        } else if let Some(pending) = self.pending_entries.get_mut(mint) {
            if pending.deferred_trades.len() >= MAX_TRACKED_TRADES_PER_MINT {
                pending.deferred_trades.pop_front();
            }
            pending.deferred_trades.push_back(DeferredRugScalpTradeV2 {
                tx: tx.clone(),
                canonical,
                curve,
            });
        }

        let Some(assessment) = self.signal.on_trade(tx, ingress_ms, canonical, curve) else {
            return actions;
        };
        actions.push(RugScalpRuntimeActionV2::Assessment(assessment.clone()));
        if !assessment.is_candidate() || self.pending_entries.contains_key(mint) {
            return actions;
        }
        let Some(primary) = assessment.primary_quote.as_ref() else {
            return actions;
        };
        let primary_notional_lamports = primary.notional_lamports;
        let expected_entry_token_amount_raw = primary.entry_token_amount_raw;
        let entry_wallet_debit_lamports = primary.entry_wallet_debit_lamports;
        let entry_total_debit_lamports = primary
            .entry_wallet_debit_lamports
            .saturating_add(primary.entry_transaction_cost_lamports);
        let entry_route_id = primary.entry_route_id.clone();
        let exit_route_id = primary.exit_route_id.clone();
        let entry_fee_schedule_id = primary.entry_fee_schedule_id.clone();
        let exit_fee_schedule_id = primary.exit_fee_schedule_id.clone();
        let candidate_id =
            crate::events::build_execution_candidate_id(mint, &assessment.pool_id, &tx.signature);
        let intent = RugScalpEntryIntentV2 {
            candidate_id,
            assessment,
            primary_notional_lamports,
            expected_entry_token_amount_raw,
            entry_wallet_debit_lamports,
            entry_total_debit_lamports,
            entry_route_id,
            exit_route_id,
            entry_fee_schedule_id,
            exit_fee_schedule_id,
        };
        self.pending_entries.insert(
            mint.clone(),
            PendingRugScalpEntryV2 {
                intent: intent.clone(),
                entry_curve: curve,
                deferred_trades: VecDeque::new(),
            },
        );
        actions.push(RugScalpRuntimeActionV2::EntryIntent(intent));
        actions
    }

    /// Binds an accepted intent only after isolated shadow submission yielded
    /// confirmed/modelled fill evidence *and* PM registration was acknowledged.
    /// Any trades that arrived during that bounded hand-off are replayed as
    /// facts after registration, never dropped into an unknown PM position.
    pub fn bind_confirmed_or_modelled_fill(
        &mut self,
        mint: &str,
        candidate_id: &str,
        position_id: String,
        entry_token_amount_raw: u64,
        entry_watermark: RugScalpEntryWatermarkV1,
    ) -> Result<Vec<RugScalpRuntimeActionV2>, RugScalpAdapterBindErrorV2> {
        if entry_token_amount_raw == 0 {
            return Err(RugScalpAdapterBindErrorV2::ZeroFilledTokens);
        }
        let mint_pubkey = mint
            .parse::<Pubkey>()
            .map_err(|_| RugScalpAdapterBindErrorV2::InvalidMint)?;
        let pending = self
            .pending_entries
            .remove(mint)
            .ok_or(RugScalpAdapterBindErrorV2::NoPendingEntry)?;
        if pending.intent.candidate_id != candidate_id {
            self.pending_entries.insert(mint.to_string(), pending);
            return Err(RugScalpAdapterBindErrorV2::CandidateMismatch);
        }
        let mut active = ActiveRugScalpFactStreamV2 {
            position_id,
            mint: mint_pubkey,
            entry_token_amount_raw,
            entry_watermark,
            current_slot: None,
            last_curve: pending.entry_curve,
            last_order: None,
            seen_signatures: HashSet::new(),
            gap_reported: false,
            quote_contract: self.quote_contract.clone(),
        };
        let mut actions = Vec::new();
        for deferred in pending.deferred_trades {
            actions.extend(active.observe_trade(&deferred.tx, deferred.canonical, deferred.curve));
        }
        self.active_positions.insert(mint.to_string(), active);
        Ok(actions)
    }

    /// A stream gap invalidates every active RUG fact stream.  It never adds a
    /// synthetic empty slot; PM receives a sticky typed `DATA_GAP` blocker.
    pub fn mark_stream_gap(&mut self) -> Vec<RugScalpRuntimeActionV2> {
        self.signal.mark_stream_gap();
        self.active_positions
            .values_mut()
            .filter_map(ActiveRugScalpFactStreamV2::data_gap)
            .map(RugScalpRuntimeActionV2::MarketFact)
            .collect()
    }

    pub fn pending_intent(&self, mint: &str) -> Option<&RugScalpEntryIntentV2> {
        self.pending_entries
            .get(mint)
            .map(|pending| &pending.intent)
    }
}

impl ActiveRugScalpFactStreamV2 {
    fn observe_trade(
        &mut self,
        tx: &PoolTransaction,
        canonical: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> Vec<RugScalpRuntimeActionV2> {
        let mut facts = Vec::with_capacity(3);
        let Some(slot) = tx.slot else {
            return self
                .data_gap()
                .into_iter()
                .map(RugScalpRuntimeActionV2::MarketFact)
                .collect();
        };
        let Some(order) = (match (tx.tx_index, tx.event_ordinal) {
            (Some(tx_index), Some(event_ordinal)) => Some(TradeOrder {
                slot,
                tx_index,
                event_ordinal,
            }),
            _ => None,
        }) else {
            return self
                .data_gap()
                .into_iter()
                .map(RugScalpRuntimeActionV2::MarketFact)
                .collect();
        };
        match self.classify_against_entry(order) {
            EntryWatermarkOrderingV2::BeforeEntry => return facts,
            EntryWatermarkOrderingV2::AmbiguousSameSlot => {
                return self
                    .data_gap()
                    .into_iter()
                    .map(RugScalpRuntimeActionV2::MarketFact)
                    .collect();
            }
            EntryWatermarkOrderingV2::AfterEntry => {}
        }
        if !canonical.state_clean || !canonical.ordering_known || canonical.accepted_window_has_gap
        {
            return self
                .data_gap()
                .into_iter()
                .map(RugScalpRuntimeActionV2::MarketFact)
                .collect();
        }
        if !self.seen_signatures.insert(tx.signature.clone()) {
            return facts;
        }
        if self.last_order.is_some_and(|previous| order <= previous) {
            return self
                .data_gap()
                .into_iter()
                .map(RugScalpRuntimeActionV2::MarketFact)
                .collect();
        }
        self.last_order = Some(order);

        if let Some(open_slot) = self.current_slot {
            if slot > open_slot.slot {
                if slot > open_slot.slot.saturating_add(1) {
                    if let Some(fact) = self.data_gap() {
                        facts.push(RugScalpRuntimeActionV2::MarketFact(fact));
                    }
                }
                facts.push(RugScalpRuntimeActionV2::MarketFact(
                    self.slot_complete(open_slot),
                ));
                self.current_slot = Some(RugScalpOpenSlotV2 {
                    slot,
                    successful_buy_count: 0,
                });
            } else if slot < open_slot.slot {
                return self
                    .data_gap()
                    .into_iter()
                    .map(RugScalpRuntimeActionV2::MarketFact)
                    .collect();
            }
        } else {
            self.current_slot = Some(RugScalpOpenSlotV2 {
                slot,
                successful_buy_count: 0,
            });
        }

        if tx.success && tx.is_buy {
            let count = self
                .current_slot
                .as_mut()
                .map(|open_slot| {
                    open_slot.successful_buy_count =
                        open_slot.successful_buy_count.saturating_add(1);
                    open_slot.successful_buy_count
                })
                .unwrap_or_default();
            let Some((_exit_quote, executable_after)) =
                curve.and_then(pump_reserves).and_then(|reserves| {
                    self.quote_contract
                        .as_ref()?
                        .executable_exit_value_lamports(slot, reserves, self.entry_token_amount_raw)
                        .ok()
                })
            else {
                return self
                    .data_gap()
                    .into_iter()
                    .map(RugScalpRuntimeActionV2::MarketFact)
                    .collect();
            };
            facts.push(RugScalpRuntimeActionV2::MarketFact(RugScalpMarketFactV1 {
                position_id: self.position_id.clone(),
                mint: self.mint,
                slot,
                tx_index: tx.tx_index,
                event_ordinal: tx.event_ordinal,
                fact_kind: RugScalpMarketFactKindV1::SuccessfulBuy,
                successful_buy_count_in_slot: count,
                sell_quote_lamports: None,
                reserve_before: None,
                reserve_after: None,
                executable_position_value_before: None,
                executable_position_value_after: Some(executable_after),
                data_completeness: RugScalpDataCompletenessV1::Complete,
            }));
        } else if tx.success && !tx.is_buy {
            let values = self.last_curve.zip(curve).and_then(|(before, after)| {
                let contract = self.quote_contract.as_ref()?;
                let before_reserves = pump_reserves(before)?;
                let after_reserves = pump_reserves(after)?;
                let (before_quote, executable_before) = contract
                    .executable_exit_value_lamports(
                        slot,
                        before_reserves,
                        self.entry_token_amount_raw,
                    )
                    .ok()?;
                let (after_quote, executable_after) = contract
                    .executable_exit_value_lamports(
                        slot,
                        after_reserves,
                        self.entry_token_amount_raw,
                    )
                    .ok()?;
                Some((
                    before_quote.reserve_transition.quote_before,
                    after_quote.reserve_transition.quote_before,
                    executable_before,
                    executable_after,
                ))
            });
            let Some((reserve_before, reserve_after, executable_before, executable_after)) = values
            else {
                return self
                    .data_gap()
                    .into_iter()
                    .map(RugScalpRuntimeActionV2::MarketFact)
                    .collect();
            };
            facts.push(RugScalpRuntimeActionV2::MarketFact(RugScalpMarketFactV1 {
                position_id: self.position_id.clone(),
                mint: self.mint,
                slot,
                tx_index: tx.tx_index,
                event_ordinal: tx.event_ordinal,
                fact_kind: RugScalpMarketFactKindV1::SuccessfulSell,
                successful_buy_count_in_slot: self
                    .current_slot
                    .map(|open_slot| open_slot.successful_buy_count)
                    .unwrap_or_default(),
                sell_quote_lamports: Some(executable_after),
                reserve_before: Some(reserve_before),
                reserve_after: Some(reserve_after),
                executable_position_value_before: Some(executable_before),
                executable_position_value_after: Some(executable_after),
                data_completeness: RugScalpDataCompletenessV1::Complete,
            }));
        }
        if curve.is_some() {
            self.last_curve = curve;
        }
        facts
    }

    fn slot_complete(&self, open_slot: RugScalpOpenSlotV2) -> RugScalpMarketFactV1 {
        RugScalpMarketFactV1 {
            position_id: self.position_id.clone(),
            mint: self.mint,
            slot: open_slot.slot,
            tx_index: None,
            event_ordinal: None,
            fact_kind: RugScalpMarketFactKindV1::SlotComplete,
            successful_buy_count_in_slot: open_slot.successful_buy_count,
            sell_quote_lamports: None,
            reserve_before: None,
            reserve_after: None,
            executable_position_value_before: None,
            executable_position_value_after: None,
            data_completeness: RugScalpDataCompletenessV1::Complete,
        }
    }

    fn data_gap(&mut self) -> Option<RugScalpMarketFactV1> {
        if self.gap_reported {
            return None;
        }
        self.gap_reported = true;
        Some(RugScalpMarketFactV1 {
            position_id: self.position_id.clone(),
            mint: self.mint,
            slot: self
                .current_slot
                .map(|open_slot| open_slot.slot)
                .unwrap_or_default(),
            tx_index: None,
            event_ordinal: None,
            fact_kind: RugScalpMarketFactKindV1::DataGap,
            successful_buy_count_in_slot: 0,
            sell_quote_lamports: None,
            reserve_before: None,
            reserve_after: None,
            executable_position_value_before: None,
            executable_position_value_after: None,
            data_completeness: RugScalpDataCompletenessV1::Gap,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum EntryWatermarkOrderingV2 {
    BeforeEntry,
    AfterEntry,
    AmbiguousSameSlot,
}

impl ActiveRugScalpFactStreamV2 {
    fn classify_against_entry(&self, order: TradeOrder) -> EntryWatermarkOrderingV2 {
        if order.slot < self.entry_watermark.slot {
            return EntryWatermarkOrderingV2::BeforeEntry;
        }
        if order.slot > self.entry_watermark.slot {
            return EntryWatermarkOrderingV2::AfterEntry;
        }
        match (
            self.entry_watermark.tx_index,
            self.entry_watermark.event_ordinal,
        ) {
            (Some(entry_tx), Some(entry_ordinal)) => {
                if (order.tx_index, order.event_ordinal) <= (entry_tx, entry_ordinal) {
                    EntryWatermarkOrderingV2::BeforeEntry
                } else {
                    EntryWatermarkOrderingV2::AfterEntry
                }
            }
            _ => EntryWatermarkOrderingV2::AmbiguousSameSlot,
        }
    }
}

#[derive(Debug, Default)]
struct MintSignalState {
    pool_id: String,
    birth_slot: Option<u64>,
    birth_ingress_ms: u64,
    universe_eligible: bool,
    trades: VecDeque<ObservedBuy>,
    seen_signatures: HashSet<String>,
    last_order: Option<TradeOrder>,
    sell_seen: bool,
    accepted_window_has_gap: bool,
    terminal_reason: Option<RugScalpReasonCodeV2>,
    signal_emitted: bool,
}

impl MintSignalState {
    fn evaluate(
        &self,
        config: &RugScalpV2Config,
        quote_contract: Option<&RugScalpPumpQuoteContractV1>,
        _mint: &str,
        current_slot: u64,
        ingress_ms: u64,
        canonical_state: RugScalpCanonicalStateV2,
        curve: Option<BondingCurve>,
    ) -> (
        RugScalpAssessment,
        RugScalpReasonCodeV2,
        Option<RugScalpNotionalQuoteV2>,
        Option<RugScalpNotionalQuoteV2>,
    ) {
        let reject = |reason| (RugScalpAssessment::Rejected, reason, None, None);
        let non_evaluable = |reason| (RugScalpAssessment::NonEvaluable, reason, None, None);
        if !self.universe_eligible {
            return reject(RugScalpReasonCodeV2::UniverseIneligible);
        }
        let Some(birth_slot) = self.birth_slot else {
            return non_evaluable(RugScalpReasonCodeV2::MissingBirthSlot);
        };
        if current_slot <= birth_slot {
            return reject(RugScalpReasonCodeV2::MissingPreviousSlot);
        }
        if let Some(reason) = self.terminal_reason {
            return match reason {
                RugScalpReasonCodeV2::AgeExceeded | RugScalpReasonCodeV2::SellSeen => {
                    reject(reason)
                }
                _ => non_evaluable(reason),
            };
        }
        if self.accepted_window_has_gap || canonical_state.accepted_window_has_gap {
            return non_evaluable(RugScalpReasonCodeV2::AcceptedWindowGap);
        }
        if !canonical_state.state_clean {
            return non_evaluable(RugScalpReasonCodeV2::CanonicalStateDirty);
        }
        if !canonical_state.ordering_known {
            return non_evaluable(RugScalpReasonCodeV2::CanonicalOrderingUnknown);
        }
        if self.sell_seen && config.require_zero_sells_before_entry {
            return reject(RugScalpReasonCodeV2::SellSeen);
        }
        if ingress_ms.saturating_sub(self.birth_ingress_ms) > config.max_birth_age_ms {
            return reject(RugScalpReasonCodeV2::AgeExceeded);
        }

        let previous_slot = current_slot.saturating_sub(1);
        let current: Vec<&ObservedBuy> = self
            .trades
            .iter()
            .filter(|trade| trade.slot == current_slot)
            .collect();
        let previous: Vec<&ObservedBuy> = self
            .trades
            .iter()
            .filter(|trade| trade.slot == previous_slot)
            .collect();
        if previous.is_empty() {
            return reject(RugScalpReasonCodeV2::MissingPreviousSlot);
        }
        let n_prev = previous.len();
        let n_curr = current.len();
        let n_2 = n_prev + n_curr;
        if n_prev < config.min_prev_slot_buys {
            return reject(RugScalpReasonCodeV2::PreviousSlotBuyCount);
        }
        if n_curr < config.min_current_slot_buys {
            return reject(RugScalpReasonCodeV2::CurrentSlotBuyCount);
        }
        if n_2 < config.min_two_slot_buys {
            return reject(RugScalpReasonCodeV2::TwoSlotBuyCount);
        }

        let v_prev_lamports: u64 = previous
            .iter()
            .map(|trade| trade.effective_quote_lamports)
            .sum();
        let v_curr_lamports: u64 = current
            .iter()
            .map(|trade| trade.effective_quote_lamports)
            .sum();
        let v_2_lamports = v_prev_lamports.saturating_add(v_curr_lamports);
        let mut user_flow: HashMap<&str, u64> = HashMap::new();
        for trade in previous.iter().chain(current.iter()) {
            *user_flow.entry(trade.signer.as_str()).or_default() = user_flow
                .get(trade.signer.as_str())
                .copied()
                .unwrap_or_default()
                .saturating_add(trade.effective_quote_lamports);
        }
        if user_flow.len() < config.min_two_slot_unique_users {
            return reject(RugScalpReasonCodeV2::UniqueUsers);
        }
        let min_flow = sol_to_lamports(config.min_two_slot_effective_quote_sol).unwrap_or(u64::MAX);
        if v_2_lamports < min_flow {
            return reject(RugScalpReasonCodeV2::RecentFlow);
        }
        if v_curr_lamports.saturating_mul(2) < v_prev_lamports {
            return reject(RugScalpReasonCodeV2::CurrentFlowPersistence);
        }
        let top1_lamports = user_flow.values().copied().max().unwrap_or_default();
        if top1_lamports as f64 > v_2_lamports as f64 * config.max_top1_quote_share {
            return reject(RugScalpReasonCodeV2::Top1Share);
        }

        let Some(curve) = curve else {
            return non_evaluable(RugScalpReasonCodeV2::CanonicalStateUnavailable);
        };
        let Some(primary_notional) = config.primary_notional_lamports() else {
            return non_evaluable(RugScalpReasonCodeV2::QuoteMathUnavailable);
        };
        let Some(sensitivity_notional) = config.sensitivity_notional_lamports() else {
            return non_evaluable(RugScalpReasonCodeV2::QuoteMathUnavailable);
        };
        let Some(quote_contract) = quote_contract else {
            return non_evaluable(RugScalpReasonCodeV2::QuoteMathUnavailable);
        };
        let primary = quote_notional(
            quote_contract,
            curve,
            current_slot,
            primary_notional,
            config.profit_min_net_bps,
        );
        let sensitivity = quote_notional(
            quote_contract,
            curve,
            current_slot,
            sensitivity_notional,
            config.profit_min_net_bps,
        );
        if primary.notional_lamports as f64
            > v_2_lamports as f64 * config.max_position_to_recent_flow_ratio
        {
            return (
                RugScalpAssessment::RejectedLowCapacity,
                RugScalpReasonCodeV2::PrimaryCapacity,
                Some(primary),
                Some(sensitivity),
            );
        }
        if primary.self_impact_bps > config.max_entry_self_impact_bps {
            return reject_with_quotes(
                RugScalpReasonCodeV2::PrimarySelfImpact,
                primary,
                sensitivity,
            );
        }
        let primary_qtp = match primary.q_tp_lamports {
            Some(value) => value,
            None => {
                return reject_with_quotes(
                    RugScalpReasonCodeV2::PrimaryTargetFlow,
                    primary,
                    sensitivity,
                );
            }
        };
        if primary_qtp as f64 > v_2_lamports as f64 * config.max_required_flow_to_recent_flow_ratio
        {
            return reject_with_quotes(
                RugScalpReasonCodeV2::PrimaryTargetFlow,
                primary,
                sensitivity,
            );
        }
        (
            RugScalpAssessment::ShadowEdgeCandidate,
            RugScalpReasonCodeV2::CandidateAccepted,
            Some(primary),
            Some(sensitivity),
        )
    }

    fn assessment(
        &self,
        mint: &str,
        signal_ingress_ms: u64,
        assessment: RugScalpAssessment,
        reason: RugScalpReasonCodeV2,
        primary_quote: Option<RugScalpNotionalQuoteV2>,
        sensitivity_quote: Option<RugScalpNotionalQuoteV2>,
    ) -> RugScalpEntryAssessmentV2 {
        let signal_slot = self.last_order.map(|order| order.slot);
        let (n_prev, n_curr, v_prev, v_curr, users, top1) = signal_slot
            .map(|current_slot| self.window_stats(current_slot))
            .unwrap_or_default();
        RugScalpEntryAssessmentV2 {
            strategy_id: RUG_SCALP_V2_STRATEGY_ID,
            exit_profile_id: RUG_SCALP_EXIT_PROFILE_ID,
            mint: mint.to_string(),
            pool_id: self.pool_id.clone(),
            birth_slot: self.birth_slot,
            signal_slot,
            birth_ingress_ms: self.birth_ingress_ms,
            signal_ingress_ms,
            age_ms: signal_ingress_ms.saturating_sub(self.birth_ingress_ms),
            n_prev,
            n_curr,
            n_2: n_prev.saturating_add(n_curr),
            u_2: users,
            v_prev_sol: v_prev as f64 / LAMPORTS_PER_SOL as f64,
            v_curr_sol: v_curr as f64 / LAMPORTS_PER_SOL as f64,
            v_2_sol: v_prev.saturating_add(v_curr) as f64 / LAMPORTS_PER_SOL as f64,
            top1_share: (v_prev.saturating_add(v_curr) > 0)
                .then(|| top1 as f64 / v_prev.saturating_add(v_curr) as f64),
            sell_seen: self.sell_seen,
            assessment,
            reason,
            primary_quote,
            sensitivity_quote,
        }
    }

    fn window_stats(&self, current_slot: u64) -> (usize, usize, u64, u64, usize, u64) {
        let previous_slot = current_slot.saturating_sub(1);
        let mut n_prev = 0;
        let mut n_curr = 0;
        let mut v_prev: u64 = 0;
        let mut v_curr: u64 = 0;
        let mut users = HashSet::new();
        let mut user_flow: HashMap<&str, u64> = HashMap::new();
        for trade in &self.trades {
            if trade.slot == previous_slot || trade.slot == current_slot {
                users.insert(trade.signer.as_str());
                *user_flow.entry(trade.signer.as_str()).or_default() = user_flow
                    .get(trade.signer.as_str())
                    .copied()
                    .unwrap_or_default()
                    .saturating_add(trade.effective_quote_lamports);
                if trade.slot == previous_slot {
                    n_prev += 1;
                    v_prev = v_prev.saturating_add(trade.effective_quote_lamports);
                } else {
                    n_curr += 1;
                    v_curr = v_curr.saturating_add(trade.effective_quote_lamports);
                }
            }
        }
        (
            n_prev,
            n_curr,
            v_prev,
            v_curr,
            users.len(),
            user_flow.values().copied().max().unwrap_or_default(),
        )
    }
}

#[derive(Debug)]
struct ObservedBuy {
    slot: u64,
    signer: String,
    effective_quote_lamports: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
struct TradeOrder {
    slot: u64,
    tx_index: u32,
    event_ordinal: u32,
}

fn reject_with_quotes(
    reason: RugScalpReasonCodeV2,
    primary: RugScalpNotionalQuoteV2,
    sensitivity: RugScalpNotionalQuoteV2,
) -> (
    RugScalpAssessment,
    RugScalpReasonCodeV2,
    Option<RugScalpNotionalQuoteV2>,
    Option<RugScalpNotionalQuoteV2>,
) {
    (
        RugScalpAssessment::Rejected,
        reason,
        Some(primary),
        Some(sensitivity),
    )
}

fn sol_to_lamports(sol: f64) -> Option<u64> {
    if !sol.is_finite() || sol <= 0.0 {
        return None;
    }
    let lamports = sol * LAMPORTS_PER_SOL as f64;
    (lamports <= u64::MAX as f64).then_some(lamports.round() as u64)
}

fn is_sol_pair(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_uppercase().as_str(),
        "SOL" | "WSOL" | "SO11111111111111111111111111111111111111112"
    )
}

/// `DetectedPool::amm_program` is canonical program identity materialized by
/// Seer, never a display label.  Parse and compare the Pubkey so malformed or
/// legacy labels cannot expand the RUG universe.
fn is_canonical_rug_scalp_pump_program(value: &str) -> bool {
    value
        .trim()
        .parse::<Pubkey>()
        .map(|program_id| program_id == RUG_SCALP_PUMP_PROGRAM)
        .unwrap_or(false)
}

fn quote_notional(
    quote_contract: &RugScalpPumpQuoteContractV1,
    curve: BondingCurve,
    canonical_slot: u64,
    notional_lamports: u64,
    profit_min_net_bps: i32,
) -> RugScalpNotionalQuoteV2 {
    let invalid = || RugScalpNotionalQuoteV2 {
        notional_lamports,
        entry_token_amount_raw: 0,
        entry_wallet_debit_lamports: 0,
        entry_transaction_cost_lamports: 0,
        exit_transaction_cost_lamports: 0,
        entry_route_id: RUG_SCALP_ENTRY_ROUTE.as_str().to_string(),
        exit_route_id: RUG_SCALP_EXIT_ROUTE.as_str().to_string(),
        entry_fee_schedule_id: String::new(),
        exit_fee_schedule_id: String::new(),
        self_impact_bps: u32::MAX,
        q_tp_lamports: None,
        q_tp_status: RugScalpQuoteStatusV2::InvalidCurve,
    };
    let Some(reserves) = pump_reserves(curve) else {
        return invalid();
    };
    let Ok(entry_quote) =
        quote_contract.quote_buy_v2_under_wallet_cap(canonical_slot, reserves, notional_lamports)
    else {
        return invalid();
    };
    let entry_tokens = entry_quote.token_amount;
    let post_entry_reserves = reserves_after_buy(reserves, &entry_quote);
    let self_impact_bps = price_impact_bps(reserves, post_entry_reserves).unwrap_or(u32::MAX);
    let entry_transaction_cost_lamports = match quote_contract.entry_transaction_cost_lamports() {
        Ok(cost) => cost,
        Err(_) => return invalid(),
    };
    let exit_transaction_cost_lamports = match quote_contract.exit_transaction_cost_lamports() {
        Ok(cost) => cost,
        Err(_) => return invalid(),
    };
    let entry_total_debit = entry_quote
        .program_settlement
        .wallet_debit_or_credit
        .saturating_add(entry_transaction_cost_lamports);
    let target_net_lamports = (entry_total_debit as u128)
        .saturating_mul((10_000_i32.saturating_add(profit_min_net_bps)) as u128)
        / 10_000_u128;
    let required_exit_lamports = target_net_lamports
        .saturating_add(exit_transaction_cost_lamports as u128)
        .min(u64::MAX as u128) as u64;
    let q_tp_lamports = minimum_additional_buy_flow(
        quote_contract,
        canonical_slot,
        post_entry_reserves,
        entry_tokens,
        required_exit_lamports,
    );
    RugScalpNotionalQuoteV2 {
        notional_lamports,
        entry_token_amount_raw: entry_tokens,
        entry_wallet_debit_lamports: entry_quote.program_settlement.wallet_debit_or_credit,
        entry_transaction_cost_lamports,
        exit_transaction_cost_lamports,
        entry_route_id: entry_quote.route_variant.as_str().to_string(),
        exit_route_id: RUG_SCALP_EXIT_ROUTE.as_str().to_string(),
        entry_fee_schedule_id: entry_quote.fee_schedule_id,
        exit_fee_schedule_id: quote_contract
            .registry
            .resolve(RUG_SCALP_EXIT_ROUTE, canonical_slot)
            .map(|schedule| schedule.fee_schedule_id.clone())
            .unwrap_or_default(),
        self_impact_bps,
        q_tp_status: if q_tp_lamports.is_some() {
            RugScalpQuoteStatusV2::Resolved
        } else {
            RugScalpQuoteStatusV2::Unreachable
        },
        q_tp_lamports,
    }
}

fn minimum_additional_buy_flow(
    quote_contract: &RugScalpPumpQuoteContractV1,
    canonical_slot: u64,
    reserves_after_entry: PumpReserveState,
    entry_tokens: u64,
    required_exit_lamports: u64,
) -> Option<u64> {
    let can_exit = |additional_flow| {
        let quote = quote_contract
            .quote_buy_v2_under_wallet_cap(canonical_slot, reserves_after_entry, additional_flow)
            .ok()?;
        let (_, exit_value) = quote_contract
            .executable_exit_value_lamports(
                canonical_slot,
                reserves_after_buy(reserves_after_entry, &quote),
                entry_tokens,
            )
            .ok()?;
        Some(exit_value >= required_exit_lamports)
    };
    if quote_contract
        .executable_exit_value_lamports(canonical_slot, reserves_after_entry, entry_tokens)
        .map(|(_, value)| value >= required_exit_lamports)
        .unwrap_or(false)
    {
        return Some(0);
    }
    let mut upper = LAMPORTS_PER_SOL / 1_000;
    while upper < MAX_QTP_SEARCH_LAMPORTS && !can_exit(upper).unwrap_or(false) {
        upper = upper.saturating_mul(2).min(MAX_QTP_SEARCH_LAMPORTS);
    }
    if !can_exit(upper).unwrap_or(false) {
        return None;
    }
    let mut lower = 0_u64;
    while lower < upper {
        let midpoint = lower.saturating_add(upper.saturating_sub(lower) / 2);
        if can_exit(midpoint).unwrap_or(false) {
            upper = midpoint;
        } else {
            lower = midpoint.saturating_add(1);
        }
    }
    Some(lower)
}

pub(crate) fn pump_reserves(curve: BondingCurve) -> Option<PumpReserveState> {
    curve.is_active().then_some(PumpReserveState {
        virtual_base_reserves: curve.virtual_token_reserves,
        virtual_quote_reserves: curve.virtual_sol_reserves,
        real_base_reserves: curve.real_token_reserves,
        real_quote_reserves: curve.real_sol_reserves,
    })
}

pub(crate) fn reserves_after_buy(
    before: PumpReserveState,
    quote: &PumpQuoteV1,
) -> PumpReserveState {
    PumpReserveState {
        virtual_base_reserves: quote.reserve_transition.base_after,
        virtual_quote_reserves: quote.reserve_transition.quote_after,
        real_base_reserves: before.real_base_reserves.saturating_sub(quote.token_amount),
        real_quote_reserves: before
            .real_quote_reserves
            .saturating_add(quote.curve_quote_amount),
    }
}

fn price_impact_bps(before: PumpReserveState, after: PumpReserveState) -> Option<u32> {
    if before.virtual_quote_reserves == 0
        || before.virtual_base_reserves == 0
        || after.virtual_quote_reserves == 0
        || after.virtual_base_reserves == 0
    {
        return None;
    }
    let before_price = before.virtual_quote_reserves as f64 / before.virtual_base_reserves as f64;
    let after_price = after.virtual_quote_reserves as f64 / after.virtual_base_reserves as f64;
    let impact = ((after_price / before_price) - 1.0) * 10_000.0;
    (impact.is_finite() && impact >= 0.0).then_some(impact.ceil() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::EventSemanticEnvelope;
    use ghost_core::{FeeRounding, ProgramFeeRule, ProgramFeeScheduleEvidenceV1, PumpQuoteError};

    #[test]
    fn fee_authority_refresh_classifies_transient_errors_separately_from_semantic_errors() {
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!("request timed out")),
            RugScalpFeeAuthorityRefreshErrorClassV1::Timeout
        );
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!("HTTP status 429")),
            RugScalpFeeAuthorityRefreshErrorClassV1::RateLimited
        );
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!("connection reset by peer")),
            RugScalpFeeAuthorityRefreshErrorClassV1::Transport
        );
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!("HTTP status 503")),
            RugScalpFeeAuthorityRefreshErrorClassV1::HttpStatus
        );
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!(
                "account discriminator decode failed"
            )),
            RugScalpFeeAuthorityRefreshErrorClassV1::Decode
        );
        assert_eq!(
            classify_rug_scalp_fee_authority_refresh_error(&anyhow!(
                "Pump global/fee_config protocol fee conflict"
            )),
            RugScalpFeeAuthorityRefreshErrorClassV1::SemanticValidation
        );
    }

    fn runtime_schedule(
        route_variant: PumpRouteVariant,
        fee_schedule_id: &str,
        rules: Vec<ProgramFeeRule>,
    ) -> RugScalpPumpFeeScheduleV1 {
        RugScalpPumpFeeScheduleV1 {
            route_variant,
            schedule: ProgramFeeSchedule {
                fee_schedule_id: fee_schedule_id.to_string(),
                effective_slot: 0,
                evidence: ProgramFeeScheduleEvidenceV1::OnChainConfig {
                    config_pubkey: format!("pump-fee-config-{fee_schedule_id}"),
                    owner_program: "pfeeUxB6jkeY1Hxd7CsFCAjcbHA9rWtchMGdZ6VojVZ".to_string(),
                    account_data_hash: format!("onchain-hash-{fee_schedule_id}"),
                    observed_slot: 0,
                },
                rules,
            },
        }
    }

    fn runtime_quote_authority() -> RugScalpPumpQuoteAuthorityV1 {
        RugScalpPumpQuoteAuthorityV1 {
            schedules: vec![
                runtime_schedule(
                    RUG_SCALP_ENTRY_ROUTE,
                    "onchain-buy-v2-test-schedule",
                    vec![ProgramFeeRule {
                        component_id: "protocol_and_buyback".to_string(),
                        numerator: 95,
                        denominator: 10_000,
                        rounding: FeeRounding::Ceil,
                    }],
                ),
                runtime_schedule(
                    RUG_SCALP_EXIT_ROUTE,
                    "onchain-legacy-sell-test-schedule",
                    vec![
                        ProgramFeeRule {
                            component_id: "lp_fee".to_string(),
                            numerator: 3,
                            denominator: 1_000,
                            rounding: FeeRounding::Ceil,
                        },
                        ProgramFeeRule {
                            component_id: "protocol_fee_recipient".to_string(),
                            numerator: 95,
                            denominator: 20_000,
                            rounding: FeeRounding::Ceil,
                        },
                        ProgramFeeRule {
                            component_id: "buyback_fee_recipient".to_string(),
                            numerator: 95,
                            denominator: 20_000,
                            rounding: FeeRounding::Floor,
                        },
                        ProgramFeeRule {
                            component_id: "creator_fee".to_string(),
                            numerator: 395,
                            denominator: 40_000,
                            rounding: FeeRounding::Floor,
                        },
                    ],
                ),
            ],
            entry_transaction_costs: TransactionCosts::default(),
            exit_transaction_costs: TransactionCosts::default(),
        }
    }

    fn runtime_quote_contract() -> RugScalpPumpQuoteContractV1 {
        runtime_quote_authority()
            .materialize()
            .expect("on-chain evidence must authorise the test quote contract")
    }

    fn runtime_global_account() -> Account {
        let mut data = vec![0u8; PUMP_GLOBAL_ACCOUNT_LEN];
        data[..8].copy_from_slice(&PUMP_GLOBAL_DISCRIMINATOR);
        data[8] = 1; // initialized
        data[105..113].copy_from_slice(&95u64.to_le_bytes());
        data[450] = 1; // create_v2_enabled
        data[997..1005].copy_from_slice(&5_000u64.to_le_bytes());
        Account {
            owner: RUG_SCALP_PUMP_PROGRAM,
            data,
            ..Account::default()
        }
    }

    fn runtime_fee_config_account() -> Account {
        let mut data = Vec::with_capacity(PUMP_FEE_CONFIG_ACCOUNT_LEN);
        data.extend_from_slice(&PUMP_FEE_CONFIG_DISCRIMINATOR);
        data.push(253); // current PDA bump shape is an opaque layout field.
        data.extend_from_slice(&[0; 32]);
        let fees = |data: &mut Vec<u8>| {
            data.extend_from_slice(&0u64.to_le_bytes());
            data.extend_from_slice(&95u64.to_le_bytes());
            data.extend_from_slice(&30u64.to_le_bytes());
        };
        fees(&mut data); // flat fees
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes());
        fees(&mut data); // SOL fee tier
        data.extend_from_slice(&1u32.to_le_bytes());
        data.extend_from_slice(&0u128.to_le_bytes());
        fees(&mut data); // stable tier, required equal for this fixed route
        assert_eq!(data.len(), PUMP_FEE_CONFIG_KNOWN_PREFIX_LEN);
        data.resize(PUMP_FEE_CONFIG_ACCOUNT_LEN, 0);
        Account {
            owner: RUG_SCALP_PUMP_FEE_PROGRAM,
            data,
            ..Account::default()
        }
    }

    #[test]
    fn runtime_fee_authority_decodes_current_uniform_pump_layout() {
        let global = decode_rug_scalp_global_config(&runtime_global_account()).unwrap();
        let fees = decode_rug_scalp_fee_config(&runtime_fee_config_account()).unwrap();
        assert!(global.initialized);
        assert!(global.create_v2_enabled);
        assert_eq!(global.fee_basis_points, 95);
        assert_eq!(global.buyback_basis_points, 5_000);
        assert_eq!(fees.protocol_fee_bps, 95);
        assert_eq!(fees.creator_fee_bps, 30);

        let buy = runtime_buy_v2_fee_rules(fees, global).unwrap();
        let sell = runtime_legacy_sell_fee_rules(fees, global).unwrap();
        assert_eq!(buy[0].component_id, "fee_recipient");
        assert_eq!(buy[0].numerator, 95 * 5_000);
        assert_eq!(buy[0].denominator, 100_000_000);
        assert_eq!(buy[0].rounding, FeeRounding::Ceil);
        assert_eq!(buy[1].component_id, "buyback_fee_recipient");
        assert_eq!(buy[1].rounding, FeeRounding::Floor);
        assert_eq!(buy[2].component_id, "creator_fee");
        assert_eq!(buy[2].rounding, FeeRounding::Ceil);
        assert_eq!(sell.last().unwrap().component_id, "creator_fee");
        assert_eq!(sell.last().unwrap().rounding, FeeRounding::Floor);
    }

    #[test]
    fn runtime_fee_authority_rejects_evolved_fee_config_surface() {
        let mut account = runtime_fee_config_account();
        account.data[PUMP_FEE_CONFIG_KNOWN_PREFIX_LEN] = 1;
        assert!(decode_rug_scalp_fee_config(&account).is_err());
    }

    fn enabled_config() -> RugScalpV2Config {
        RugScalpV2Config {
            enabled: true,
            primary_entry_latency_slots: Some(1),
            primary_exit_latency_slots: Some(1),
            entry_fixed_cost_lamports: Some(0),
            exit_fixed_cost_lamports: Some(0),
            pump_quote_authority: Some(runtime_quote_authority()),
            ..RugScalpV2Config::default()
        }
    }

    fn reducer(config: RugScalpV2Config) -> RugScalpSignalReducerV2 {
        let quote_contract = config
            .pump_quote_authority
            .as_ref()
            .expect("enabled test config has typed fee authority")
            .materialize()
            .expect("test authority materializes");
        RugScalpSignalReducerV2::new(config, Some(quote_contract))
    }

    fn curve() -> BondingCurve {
        BondingCurve {
            discriminator: 0,
            virtual_token_reserves: 1_073_000_000_000_000,
            virtual_sol_reserves: 30 * LAMPORTS_PER_SOL,
            real_token_reserves: 793_100_000_000_000,
            real_sol_reserves: 0,
            token_total_supply: 1_000_000_000_000_000,
            complete: 0,
            _padding: [0; 7],
        }
    }

    fn birth(slot: u64) -> DetectedPool {
        DetectedPool {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool".to_string(),
            base_mint: "mint".to_string(),
            quote_mint: "SOL".to_string(),
            amm_program: RUG_SCALP_PUMP_PROGRAM.to_string(),
            bonding_curve: "curve".to_string(),
            creator: "creator".to_string(),
            slot: Some(slot),
            tx_index: Some(0),
            timestamp_ms: 1_000,
            event_time: Default::default(),
            detected_wall_ts_ms: Some(1_000),
            initial_liquidity_sol: None,
            signature: "birth".to_string(),
        }
    }

    #[test]
    fn universe_eligibility_accepts_only_canonical_pump_program_id() {
        let pool = birth(10);
        assert!(is_canonical_rug_scalp_pump_program(&pool.amm_program));

        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&pool, 1_000);
        assert!(reducer.mints["mint"].universe_eligible);
    }

    #[test]
    fn universe_eligibility_rejects_legacy_pumpfun_label() {
        let mut pool = birth(10);
        pool.amm_program = "pumpfun".to_string();

        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&pool, 1_000);
        assert!(!reducer.mints["mint"].universe_eligible);
    }

    #[test]
    fn universe_eligibility_rejects_other_program_id() {
        let mut pool = birth(10);
        pool.amm_program = Pubkey::new_unique().to_string();

        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&pool, 1_000);
        assert!(!reducer.mints["mint"].universe_eligible);
    }

    #[test]
    fn universe_eligibility_keeps_non_program_requirements_fail_closed() {
        let mut invalid_quote = birth(10);
        invalid_quote.quote_mint = Pubkey::new_unique().to_string();
        let mut missing_curve = birth(10);
        missing_curve.bonding_curve.clear();
        let mut missing_slot = birth(10);
        missing_slot.slot = None;

        for pool in [invalid_quote, missing_curve, missing_slot] {
            let mut reducer = reducer(enabled_config());
            reducer.on_birth(&pool, 1_000);
            assert!(!reducer.mints["mint"].universe_eligible);
        }
    }

    fn buy(slot: u64, index: u32, signer: &str, amount: f64) -> PoolTransaction {
        PoolTransaction {
            semantic: EventSemanticEnvelope::default(),
            pool_amm_id: "pool".to_string(),
            slot: Some(slot),
            event_ordinal: Some(0),
            tx_index: Some(index),
            outer_instruction_index: None,
            inner_group_index: None,
            outer_program_id: None,
            cpi_stack_height: None,
            timestamp_ms: 1_000 + index as u64,
            event_time: Default::default(),
            arrival_ts_ms: 1_000 + index as u64,
            signer: signer.to_string(),
            is_buy: true,
            volume_sol: amount,
            sol_amount_lamports: sol_to_lamports(amount),
            token_amount_units: Some(1),
            reserve_base: None,
            reserve_quote: None,
            price_quote: None,
            is_dev_buy: false,
            dev_buy_lamports: 0,
            signature: format!("{slot}-{index}"),
            success: true,
            error_code: None,
            compute_units_consumed: None,
            owner_token_deltas: vec![],
            mpcf_payload: vec![],
            mpcf_payload_missing_reason: Default::default(),
            token_mint: Some("mint".to_string()),
            v_tokens_in_bonding_curve: None,
            v_sol_in_bonding_curve: None,
            virtual_sol_reserves: None,
            virtual_token_reserves: None,
            real_sol_reserves: None,
            real_token_reserves: None,
            complete: None,
            market_cap_sol: None,
            global_config: None,
            fee_recipient: None,
            token_program: None,
            buy_variant: None,
            associated_bonding_curve: None,
            creator_vault: None,
            bonding_curve_v2: None,
            bonding_curve_v2_provenance: None,
            buy_remaining_accounts: vec![],
            is_mayhem_mode: None,
            cu_price_micro_lamports: None,
            compute_unit_limit: None,
            inner_ix_count: None,
            cpi_depth: None,
            ata_create_count: None,
            signer_pre_balance_lamports: None,
            signer_post_balance_lamports: None,
            jito_tip_detected: None,
            toolchain_fingerprint: Default::default(),
            curve_data_known: true,
            curve_finality: Default::default(),
        }
    }

    #[test]
    fn adapter_binds_only_after_fill_and_emits_two_complete_empty_slots() {
        let mint = Pubkey::new_unique().to_string();
        let pool = Pubkey::new_unique().to_string();
        let mut config = enabled_config();
        config.min_prev_slot_buys = 1;
        config.min_current_slot_buys = 1;
        config.min_two_slot_buys = 2;
        config.min_two_slot_unique_users = 1;
        config.min_two_slot_effective_quote_sol = 0.50;
        config.min_current_to_previous_quote_ratio = 0.0;
        config.max_top1_quote_share = 1.0;
        config.max_position_to_recent_flow_ratio = 1.0;
        config.max_entry_self_impact_bps = 10_000;
        config.max_required_flow_to_recent_flow_ratio = 1.0;

        let mut adapter = RugScalpRuntimeAdapterV2::new(config);
        let mut detected = birth(1);
        detected.base_mint = mint.clone();
        detected.pool_amm_id = pool;
        adapter.on_birth(&detected, 1_000);

        let canonical = RugScalpCanonicalStateV2 {
            state_clean: true,
            ordering_known: true,
            accepted_window_has_gap: false,
        };
        // The typed BuyV2/LegacySell settlement includes the current program
        // fees, so the fixture supplies enough observed two-slot flow to
        // satisfy the unchanged `Q_TP / V_2 <= 1.0` admission contract.
        let mut first = buy(2, 1, "buyer-a", 10.00);
        first.token_mint = Some(mint.clone());
        let mut second = buy(3, 1, "buyer-a", 10.00);
        second.token_mint = Some(mint.clone());
        adapter.on_trade(&first, 1_100, canonical, Some(curve()));
        let actions = adapter.on_trade(&second, 1_200, canonical, Some(curve()));
        let intent = actions
            .iter()
            .find_map(|action| match action {
                RugScalpRuntimeActionV2::EntryIntent(intent) => Some(intent.clone()),
                _ => None,
            })
            .expect("accepted assessment must emit exactly one primary intent");
        assert_eq!(intent.primary_notional_lamports, 100_000_000);
        assert_eq!(
            intent
                .assessment
                .sensitivity_quote
                .as_ref()
                .map(|quote| quote.notional_lamports),
            Some(200_000_000),
            "0.20 SOL is evidence-only sensitivity, never a second intent"
        );
        assert!(adapter.pending_intent(&mint).is_some());

        let position_id = format!("rug-scalp-position:{}", intent.candidate_id);
        assert_eq!(
            adapter.bind_confirmed_or_modelled_fill(
                &mint,
                &intent.candidate_id,
                position_id.clone(),
                0,
                RugScalpEntryWatermarkV1::canonical(3, 1, 0),
            ),
            Err(RugScalpAdapterBindErrorV2::ZeroFilledTokens),
            "unknown or empty fill must stop before a PM lifecycle exists"
        );
        assert!(
            !adapter.active_positions.contains_key(&mint)
                && adapter.pending_intent(&mint).is_some(),
            "zero fill preserves no active position and no fact stream"
        );
        assert!(adapter
            .bind_confirmed_or_modelled_fill(
                &mint,
                &intent.candidate_id,
                position_id.clone(),
                intent.expected_entry_token_amount_raw,
                RugScalpEntryWatermarkV1::canonical(3, 1, 0),
            )
            .expect("modelled fill binds only after PM registration")
            .is_empty());

        let mut observed_buy = buy(3, 2, "buyer-b", 0.01);
        observed_buy.token_mint = Some(mint.clone());
        let mut slot4 = buy(4, 1, "ignored", 0.0);
        slot4.token_mint = Some(mint.clone());
        slot4.success = false;
        slot4.is_buy = false;
        let mut slot5 = slot4.clone();
        slot5.slot = Some(5);
        slot5.tx_index = Some(1);
        slot5.signature = "slot-5".to_string();
        let mut slot6 = slot4.clone();
        slot6.slot = Some(6);
        slot6.tx_index = Some(1);
        slot6.signature = "slot-6".to_string();

        let facts: Vec<_> = [&observed_buy, &slot4, &slot5, &slot6]
            .into_iter()
            .flat_map(|tx| adapter.on_trade(tx, 1_300, canonical, Some(curve())))
            .filter_map(|action| match action {
                RugScalpRuntimeActionV2::MarketFact(fact) => Some(fact),
                _ => None,
            })
            .collect();
        assert!(facts
            .iter()
            .any(|fact| matches!(fact.fact_kind, RugScalpMarketFactKindV1::SuccessfulBuy)));
        assert_eq!(
            facts
                .iter()
                .filter(|fact| matches!(fact.fact_kind, RugScalpMarketFactKindV1::SlotComplete))
                .count(),
            3,
            "one non-empty slot plus two complete empty slots"
        );

        let mint_pubkey = mint.parse().expect("test mint pubkey");
        let profile = RugScalpExitProfileConfigV1 {
            enabled: true,
            entry_fixed_cost_lamports: 0,
            exit_fixed_cost_lamports: 0,
            ..RugScalpExitProfileConfigV1::default()
        };
        let mut pm_facts = ghost_brain::guardian::post_buy::RugScalpMarketFactStateV1::new(
            position_id,
            mint_pubkey,
        );
        for fact in facts {
            assert!(matches!(
                pm_facts.apply_fact(fact, &profile),
                ghost_brain::guardian::post_buy::RugScalpFactIngressResultV1::Applied
            ));
        }
        assert_eq!(
            ghost_brain::guardian::post_buy::evaluate_rug_scalp_exit_v1(
                &pm_facts,
                &profile,
                false,
                false,
                Some(0),
                Some(3),
                Some(6),
                100,
            ),
            ghost_brain::guardian::post_buy::RugScalpExitReasonV1::FlowExhausted
        );
    }

    #[test]
    fn buffered_post_fill_same_slot_sell_replays_after_pm_ack_and_beats_target() {
        let mint = Pubkey::new_unique().to_string();
        let pool = Pubkey::new_unique().to_string();
        let mut config = enabled_config();
        config.min_prev_slot_buys = 1;
        config.min_current_slot_buys = 1;
        config.min_two_slot_buys = 2;
        config.min_two_slot_unique_users = 1;
        config.min_two_slot_effective_quote_sol = 0.50;
        config.min_current_to_previous_quote_ratio = 0.0;
        config.max_top1_quote_share = 1.0;
        config.max_position_to_recent_flow_ratio = 1.0;
        config.max_entry_self_impact_bps = 10_000;
        config.max_required_flow_to_recent_flow_ratio = 1.0;

        let mut adapter = RugScalpRuntimeAdapterV2::new(config);
        let mut detected = birth(1);
        detected.base_mint = mint.clone();
        detected.pool_amm_id = pool;
        adapter.on_birth(&detected, 1_000);
        let canonical = RugScalpCanonicalStateV2 {
            state_clean: true,
            ordering_known: true,
            accepted_window_has_gap: false,
        };
        let mut first = buy(2, 1, "buyer-a", 10.00);
        first.token_mint = Some(mint.clone());
        let mut entry_signal = buy(3, 5, "buyer-b", 10.00);
        entry_signal.token_mint = Some(mint.clone());
        adapter.on_trade(&first, 1_100, canonical, Some(curve()));
        let intent = adapter
            .on_trade(&entry_signal, 1_200, canonical, Some(curve()))
            .into_iter()
            .find_map(|action| match action {
                RugScalpRuntimeActionV2::EntryIntent(intent) => Some(intent),
                _ => None,
            })
            .expect("accepted signal emits one primary intent");

        // The earlier same-slot sell is delivered while the PM ACK is still
        // pending.  It must be retained only long enough to prove that the
        // entry watermark discards it during replay.
        let mut pre_fill_sell = buy(3, 4, "earlier-seller", 0.1);
        pre_fill_sell.token_mint = Some(mint.clone());
        pre_fill_sell.is_buy = false;
        pre_fill_sell.signature = "pre-fill-sell".to_string();
        let mut material_sell = buy(3, 6, "post-fill-seller", 0.1);
        material_sell.token_mint = Some(mint.clone());
        material_sell.is_buy = false;
        material_sell.signature = "post-fill-material-sell".to_string();
        let mut curve_after_sell = curve();
        curve_after_sell.virtual_sol_reserves = curve_after_sell
            .virtual_sol_reserves
            .saturating_sub(2 * LAMPORTS_PER_SOL);
        curve_after_sell.real_sol_reserves = curve_after_sell
            .real_sol_reserves
            .saturating_sub(2 * LAMPORTS_PER_SOL);
        let mut slot_complete_trigger = buy(4, 1, "slot-advance", 0.0);
        slot_complete_trigger.token_mint = Some(mint.clone());
        slot_complete_trigger.success = false;
        slot_complete_trigger.is_buy = false;
        slot_complete_trigger.signature = "slot-advance".to_string();

        for (trade, ingress_ms) in [
            (&pre_fill_sell, 1_210),
            (&material_sell, 1_220),
            (&slot_complete_trigger, 1_230),
        ] {
            let pending_actions =
                adapter.on_trade(trade, ingress_ms, canonical, Some(curve_after_sell));
            assert!(
                pending_actions
                    .iter()
                    .all(|action| !matches!(action, RugScalpRuntimeActionV2::MarketFact(_))),
                "facts remain retained until PM ACK; assessment evidence is allowed"
            );
        }

        let position_id = format!("rug-scalp-position:{}", intent.candidate_id);
        let replayed_facts: Vec<_> = adapter
            .bind_confirmed_or_modelled_fill(
                &mint,
                &intent.candidate_id,
                position_id.clone(),
                intent.expected_entry_token_amount_raw,
                RugScalpEntryWatermarkV1::canonical(3, 5, 0),
            )
            .expect("PM ACK permits replay from canonical entry watermark")
            .into_iter()
            .filter_map(|action| match action {
                RugScalpRuntimeActionV2::MarketFact(fact) => Some(fact),
                _ => None,
            })
            .collect();
        assert!(replayed_facts.iter().all(|fact| fact.tx_index != Some(4)));
        assert!(replayed_facts.iter().any(|fact| {
            matches!(fact.fact_kind, RugScalpMarketFactKindV1::SuccessfulSell)
                && fact.tx_index == Some(6)
        }));
        assert!(replayed_facts.iter().any(|fact| {
            matches!(fact.fact_kind, RugScalpMarketFactKindV1::SlotComplete) && fact.slot == 3
        }));

        let mint_pubkey = mint.parse().expect("test mint pubkey");
        let profile = RugScalpExitProfileConfigV1 {
            enabled: true,
            entry_fixed_cost_lamports: 0,
            exit_fixed_cost_lamports: 0,
            ..RugScalpExitProfileConfigV1::default()
        };
        let mut pm_facts =
            ghost_brain::guardian::post_buy::RugScalpMarketFactStateV1::with_entry_watermark(
                position_id,
                mint_pubkey,
                RugScalpEntryWatermarkV1::canonical(3, 5, 0),
            );
        for fact in replayed_facts {
            assert_eq!(
                pm_facts.apply_fact(fact, &profile),
                ghost_brain::guardian::post_buy::RugScalpFactIngressResultV1::Applied
            );
        }
        assert_eq!(
            ghost_brain::guardian::post_buy::evaluate_rug_scalp_exit_v1(
                &pm_facts,
                &profile,
                false,
                false,
                Some(2_000),
                Some(3),
                Some(4),
                100,
            ),
            ghost_brain::guardian::post_buy::RugScalpExitReasonV1::MaterialSellEmergency,
            "replayed post-fill dump outranks an otherwise reached target"
        );
    }

    #[test]
    fn typed_quote_keeps_buy_v2_cap_settlement_and_legacy_sell_separate() {
        let quote_contract = runtime_quote_contract();
        let reserves = pump_reserves(curve()).expect("active canonical reserves");
        let max_sol_cost = 100_000_000;
        let buy = quote_contract
            .quote_buy_v2_under_wallet_cap(3, reserves, max_sol_cost)
            .expect("typed buy_v2 quote under cap");
        let after_buy = reserves_after_buy(reserves, &buy);
        let (sell, executable_value) = quote_contract
            .executable_exit_value_lamports(3, after_buy, buy.token_amount)
            .expect("typed legacy_sell quote");

        assert_eq!(buy.route_variant, RUG_SCALP_ENTRY_ROUTE);
        assert_eq!(sell.route_variant, RUG_SCALP_EXIT_ROUTE);
        assert_eq!(buy.fee_schedule_id, "onchain-buy-v2-test-schedule");
        assert_eq!(sell.fee_schedule_id, "onchain-legacy-sell-test-schedule");
        assert!(buy.instruction_limit_check.passed);
        assert!(buy.program_settlement.wallet_debit_or_credit <= max_sol_cost);
        assert_eq!(
            buy.reserve_transition.quote_after,
            reserves.virtual_quote_reserves + buy.curve_quote_amount,
            "curve transition never includes program or transaction fees"
        );
        assert_eq!(
            executable_value, sell.program_settlement.wallet_debit_or_credit,
            "zero envelope costs leave net exit equal to program settlement only"
        );
        assert_ne!(
            buy.curve_quote_amount, max_sol_cost,
            "buy_v2 max_sol_cost is an instruction cap, not a curve input"
        );
    }

    fn canonical() -> RugScalpCanonicalStateV2 {
        RugScalpCanonicalStateV2 {
            state_clean: true,
            ordering_known: true,
            accepted_window_has_gap: false,
        }
    }

    #[test]
    fn defaults_are_disabled_and_unfrozen() {
        let config = RugScalpV2Config::default();
        assert!(!config.enabled);
        assert!(config.primary_entry_latency_slots.is_none());
        assert!(config.validate_enabled_contract().is_ok());
    }

    #[test]
    fn enabled_contract_requires_frozen_latency_and_costs() {
        let mut config = RugScalpV2Config {
            enabled: true,
            ..RugScalpV2Config::default()
        };
        assert_eq!(
            config.validate_enabled_contract().unwrap_err(),
            RugScalpConfigError::LatencyNotFrozen
        );
        config.primary_entry_latency_slots = Some(1);
        config.primary_exit_latency_slots = Some(1);
        assert_eq!(
            config.validate_enabled_contract().unwrap_err(),
            RugScalpConfigError::CostModelNotFrozen
        );
    }

    #[test]
    fn exact_quotes_keep_primary_and_sensitivity_separate() {
        let config = enabled_config();
        let quote_contract = runtime_quote_contract();
        let primary = quote_notional(
            &quote_contract,
            curve(),
            12,
            config.primary_notional_lamports().unwrap(),
            config.profit_min_net_bps,
        );
        let sensitivity = quote_notional(
            &quote_contract,
            curve(),
            12,
            config.sensitivity_notional_lamports().unwrap(),
            config.profit_min_net_bps,
        );
        assert_eq!(primary.notional_lamports, 100_000_000);
        assert_eq!(sensitivity.notional_lamports, 200_000_000);
        assert!(sensitivity.self_impact_bps >= primary.self_impact_bps);
        assert!(primary.q_tp_lamports.is_some());
        assert_eq!(primary.entry_route_id, "buy_v2");
        assert_eq!(primary.exit_route_id, "legacy_sell");
        assert_eq!(
            primary.entry_fee_schedule_id,
            "onchain-buy-v2-test-schedule"
        );
        assert_eq!(
            primary.exit_fee_schedule_id,
            "onchain-legacy-sell-test-schedule"
        );
    }

    #[test]
    fn canonical_fixture_never_authorizes_runtime_rug_quote() {
        let mut authority = runtime_quote_authority();
        authority.schedules[0].schedule.evidence = ProgramFeeScheduleEvidenceV1::CanonicalFixture {
            fixture_id: "offline-buy-v2-fixture".to_string(),
            transaction_signature: "fixture-signature".to_string(),
            observed_slot: 0,
        };
        assert_eq!(
            authority.materialize(),
            Err(PumpQuoteError::FixtureEvidenceCannotAuthorizeRuntime)
        );
        let mut config = enabled_config();
        config.pump_quote_authority = Some(authority);
        assert_eq!(
            config.validate_enabled_contract(),
            Err(RugScalpConfigError::InvalidPumpQuoteAuthority)
        );
    }

    #[test]
    fn reducer_accepts_only_the_first_complete_two_slot_burst() {
        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&birth(10), 1_000);
        let mut outcome = None;
        for (slot, index, signer) in [
            (11, 0, "a"),
            (11, 2, "b"),
            (11, 3, "c"),
            (12, 1, "a"),
            (12, 2, "b"),
            (12, 3, "d"),
        ] {
            outcome = reducer.on_trade(
                &buy(slot, index, signer, 1.00),
                1_100,
                canonical(),
                Some(curve()),
            );
        }
        let outcome = outcome.expect("last buy must produce assessment");
        assert!(outcome.is_candidate(), "{outcome:?}");
        assert_eq!(outcome.n_prev, 3);
        assert_eq!(outcome.n_curr, 3);
        assert_eq!(outcome.u_2, 4);
        assert_eq!(outcome.assessment, RugScalpAssessment::ShadowEdgeCandidate);
        assert_ne!(outcome.reason, RugScalpReasonCodeV2::MissingTradeOrder);
    }

    #[test]
    fn successful_sell_terminally_invalidates_before_entry() {
        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&birth(10), 1_000);
        let mut sell = buy(11, 1, "seller", 0.1);
        sell.is_buy = false;
        sell.signature = "sell".to_string();
        let assessment = reducer
            .on_trade(&sell, 1_010, canonical(), Some(curve()))
            .expect("sell produces evidence");
        assert_eq!(assessment.reason, RugScalpReasonCodeV2::SellSeen);
        assert_eq!(assessment.assessment, RugScalpAssessment::Rejected);
    }

    #[test]
    fn missing_canonical_curve_is_non_evaluable_not_an_optimistic_signal() {
        let mut reducer = reducer(enabled_config());
        reducer.on_birth(&birth(10), 1_000);
        let mut outcome = None;
        for (slot, index, signer) in [
            (11, 1, "a"),
            (11, 2, "b"),
            (11, 3, "c"),
            (12, 1, "a"),
            (12, 2, "b"),
            (12, 3, "d"),
        ] {
            outcome = reducer.on_trade(&buy(slot, index, signer, 0.10), 1_100, canonical(), None);
        }
        let outcome = outcome.unwrap();
        assert_eq!(outcome.assessment, RugScalpAssessment::NonEvaluable);
        assert_eq!(
            outcome.reason,
            RugScalpReasonCodeV2::CanonicalStateUnavailable
        );
    }
}
