//! Hash-pinned, offline Pump semantics authority for Exact-State Tape V2.
//!
//! The raw recorder deliberately preserves source bytes without assigning a
//! financial meaning to them.  This module is the separate, offline boundary
//! that binds a later qualifier to one vendored public Pump IDL revision and
//! to one ProgramData hash observed by the V2 raw run.  It contains no RPC,
//! Yellowstone, active-runtime, or execution path.

use anyhow::{bail, Context, Result};
use serde::Deserialize;
use serde_json::Value;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, OpenOptions},
    io::{Read, Seek, SeekFrom},
    path::{Component, Path, PathBuf},
    str::FromStr,
};

#[cfg(unix)]
use std::os::unix::fs::OpenOptionsExt;

use ghost_core::pump_research_tape::PumpProgramDataReceiptV1;

/// Schema two makes the distinction between a generic curve mutation and a
/// trade explicit.  An exact-state `Qualified` receipt needs at least one
/// successful rooted *trade* with both states; a manifest must not satisfy
/// that gate by relabelling an arbitrary curve-adjacent instruction.
/// Schema three adds a literal Event-CPI parent contract.  Structural Anchor
/// provenance alone is not sufficient: every event field must be tied to a
/// hash-pinned parent account/argument, a fixed Borsh literal, or the final
/// anchored curve state for that exact parent transaction.
/// Schema four moves the exact curve/mint role selection itself into that
/// authority.  A future IDL spelling change such as `bondingCurve` may not be
/// interpreted through a Rust fallback or guessed account position.
/// Schema five separates exact Borsh decoding from exact-state authority.
/// Every field of a permitted Anchor event remains fully decoded and
/// trailing-byte-free, but dynamic fields such as timestamps, fee topology,
/// volume counters, and shareholder vectors may be declared
/// `strict_decode_only`.  Such a field proves only that the pinned event
/// layout decoded; it may never be used to establish parent identity or
/// canonical curve state.
/// Schema seven adds an explicit `strict_decode_only_unbound_fields` contract
/// for a parent/event pair.  It is not a shortcut around exact-state proof:
/// the loader expands it into one `StrictDecodeOnly` binding for every
/// remaining *IDL-declared* field, and still rejects it for the mandatory
/// authority subset of an exact Trade/Create/Complete parent.  This keeps a
/// full public IDL loadable without inventing sources for dynamic telemetry.
pub const PUMP_EXACT_STATE_SEMANTICS_SCHEMA_VERSION_V2: u16 = 7;
const SEMANTICS_MANIFEST_MAX_BYTES_V2: u64 = 4 * 1024 * 1024;
const VENDORED_IDL_MAX_BYTES_V2: u64 = 16 * 1024 * 1024;
const MAX_BORSH_RECURSION_DEPTH_V2: usize = 32;
const MAX_BORSH_COLLECTION_ITEMS_V2: usize = 1_000_000;

/// The only offline behavior classes accepted for a Pump instruction in a
/// prospective V2 qualifier.  Every instruction in the vendored IDL must
/// appear in the manifest exactly once; absent or extra names are authority
/// errors rather than an invitation to silently shrink a denominator.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpExactStateInstructionEffectV2 {
    SupportedExactTrade,
    SupportedExactCreate,
    KnownReserveOrDependencyUnsupported,
    GlobalDependencyMutation,
    ProvenNonReserve,
}

impl PumpExactStateInstructionEffectV2 {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "supported_exact_trade" => Ok(Self::SupportedExactTrade),
            "supported_exact_create" => Ok(Self::SupportedExactCreate),
            "known_reserve_or_dependency_unsupported" => {
                Ok(Self::KnownReserveOrDependencyUnsupported)
            }
            "global_dependency_mutation" => Ok(Self::GlobalDependencyMutation),
            "proven_non_reserve" => Ok(Self::ProvenNonReserve),
            other => bail!("unknown V2 Pump instruction effect class {other:?}"),
        }
    }

    #[must_use]
    pub const fn is_candidate(self) -> bool {
        matches!(
            self,
            Self::SupportedExactTrade
                | Self::SupportedExactCreate
                | Self::KnownReserveOrDependencyUnsupported
                | Self::GlobalDependencyMutation
        )
    }

    /// The only effect class that may contribute to the literal
    /// `successful_rooted_exact_trade_with_both_states_count` capability
    /// gate.  Creates are exact-state evidence, but they are births rather
    /// than trades and may not stand in for this requirement.
    #[must_use]
    pub const fn is_supported_exact_trade(self) -> bool {
        matches!(self, Self::SupportedExactTrade)
    }
}

/// Recognition class for a Pump-owned account.  The recorder retains every
/// Pump-owned account; an unrecognized discriminator stays visible as a
/// blocker rather than being discarded on the way to exact-state coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PumpExactStateAccountClassV2 {
    ExactBondingCurve,
    KnownGlobalDependency,
    KnownNonState,
}

impl PumpExactStateAccountClassV2 {
    fn parse(value: &str) -> Result<Self> {
        match value {
            "exact_bonding_curve" => Ok(Self::ExactBondingCurve),
            "known_global_dependency" => Ok(Self::KnownGlobalDependency),
            "known_non_state" => Ok(Self::KnownNonState),
            other => bail!("unknown V2 Pump account class {other:?}"),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateSemanticsDigestV2 {
    pub sha256: String,
    pub blake3: String,
    pub bytes: u64,
}

#[derive(Clone, Debug)]
pub struct PumpExactStateInstructionContractV2 {
    pub name: String,
    pub discriminator: [u8; 8],
    pub effect: PumpExactStateInstructionEffectV2,
    pub accounts: Vec<PumpExactStateAccountMetaContractV2>,
    state_roles: Option<PumpExactStateInstructionStateRolesV2>,
    args: Vec<Value>,
}

#[derive(Clone, Debug)]
struct PumpExactStateInstructionStateRolesV2 {
    bonding_curve_role: String,
    mint_role: String,
}

/// Fully decoded evidence from one instruction that has already passed the
/// manifest's account-vector and payload contracts.  It deliberately keeps
/// serialized argument fields rather than guessing their Rust meaning: an
/// Event-CPI binding compares the exact Borsh bytes selected by the same
/// vendored IDL.
#[derive(Clone, Debug)]
pub struct PumpExactStateInstructionSemanticEvidenceV2 {
    pub discriminator: [u8; 8],
    pub account_roles: BTreeMap<String, Pubkey>,
    pub argument_fields: BTreeMap<String, Vec<u8>>,
}

/// One Event-CPI field whose value is expected to match the final anchored
/// BondingCurve state belonging to its immediate parent instruction.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateEventFinalStateBindingV2 {
    pub curve_state_field: String,
    pub event_value_borsh: Vec<u8>,
}

#[derive(Clone, Debug)]
pub struct PumpExactStateAccountMetaContractV2 {
    pub name: String,
    pub writable: bool,
    pub signer: bool,
    pub address: Option<Pubkey>,
}

#[derive(Clone, Debug)]
pub struct PumpExactStateEventContractV2 {
    pub name: String,
    pub discriminator: [u8; 8],
    fields: Vec<Value>,
    parent_contracts: BTreeMap<[u8; 8], PumpExactStateEventParentContractV2>,
}

#[derive(Clone, Debug)]
struct PumpExactStateEventParentContractV2 {
    parent_instruction_name: String,
    field_bindings: Vec<PumpExactStateEventFieldBindingV2>,
}

#[derive(Clone, Debug)]
struct PumpExactStateEventFieldBindingV2 {
    event_field: String,
    source: PumpExactStateEventFieldSourceV2,
}

#[derive(Clone, Debug)]
enum PumpExactStateEventFieldSourceV2 {
    ParentAccountRole(String),
    ParentInstructionArgument(String),
    LiteralBorsh(Vec<u8>),
    FinalCurveStateField(String),
    /// The field is still decoded as part of the complete, exact Borsh event
    /// payload, but has no derivable parent/state authority under the pinned
    /// semantics.  It is deliberately unavailable to the semantic comparer.
    StrictDecodeOnly,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateCurveStateV2 {
    pub virtual_token_reserves: u64,
    pub virtual_quote_reserves: u64,
    pub real_token_reserves: u64,
    pub real_quote_reserves: u64,
    pub token_total_supply: u64,
    pub complete: bool,
    pub creator: Pubkey,
    pub is_mayhem_mode: bool,
    pub is_cashback_coin: bool,
    pub quote_mint: Pubkey,
}

#[derive(Clone, Debug)]
struct PumpExactStateAccountLayoutV2 {
    name: String,
    discriminator: [u8; 8],
    allowed_serialized_bytes: BTreeSet<usize>,
    prefix_bytes: usize,
    fields: Vec<Value>,
}

/// One fully validated, vendored semantics authority.  It is intentionally
/// runtime-independent: the materializer consumes this object only after a
/// raw V2 run has already passed its own frozen segment and source-completeness
/// checks.
#[derive(Clone, Debug)]
pub struct PumpExactStateSemanticsAuthorityV2 {
    pub semantics_id: String,
    pub manifest_digest: PumpExactStateSemanticsDigestV2,
    pub idl_digest: PumpExactStateSemanticsDigestV2,
    pub program_id: Pubkey,
    expected_program_data_hash_blake3: [u8; 32],
    instructions_by_discriminator: BTreeMap<[u8; 8], PumpExactStateInstructionContractV2>,
    events_by_discriminator: BTreeMap<[u8; 8], PumpExactStateEventContractV2>,
    accounts_by_discriminator: BTreeMap<[u8; 8], PumpExactStateAccountClassV2>,
    curve_layout: PumpExactStateAccountLayoutV2,
    global_layout: PumpExactStateAccountLayoutV2,
    defined_types: BTreeMap<String, Value>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateSemanticsManifestFileV2 {
    schema_version: u16,
    semantics_id: String,
    source_repository: String,
    source_commit: String,
    vendored_idl_relative_path: String,
    vendored_idl_sha256: String,
    vendored_idl_blake3: String,
    program_id: String,
    program_data_hash_blake3: String,
    instruction_effects: BTreeMap<String, String>,
    instruction_state_roles: BTreeMap<String, PumpExactStateInstructionStateRolesManifestV2>,
    event_parent_contracts: BTreeMap<String, Vec<PumpExactStateEventParentManifestV2>>,
    account_classes: BTreeMap<String, String>,
    account_layouts: Vec<PumpExactStateAccountLayoutManifestV2>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateInstructionStateRolesManifestV2 {
    bonding_curve_role: String,
    mint_role: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateEventParentManifestV2 {
    parent_instruction: String,
    /// Expand every event field omitted from `field_bindings` into a strict
    /// decode-only binding.  It may not cover an authority field required by
    /// a supported exact parent.
    #[serde(default)]
    strict_decode_only_unbound_fields: bool,
    field_bindings: Vec<PumpExactStateEventFieldBindingManifestV2>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateEventFieldBindingManifestV2 {
    event_field: String,
    #[serde(default)]
    parent_account_role: Option<String>,
    #[serde(default)]
    parent_instruction_argument: Option<String>,
    #[serde(default)]
    expected_borsh_hex: Option<String>,
    #[serde(default)]
    final_curve_state_field: Option<String>,
    #[serde(default)]
    strict_decode_only: bool,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateAccountLayoutManifestV2 {
    account: String,
    discriminator: String,
    allowed_serialized_bytes: Vec<u64>,
    prefix_bytes: u64,
    field_layout_sha256: String,
    field_layout_blake3: String,
}

/// Load a pinned semantics manifest and its vendored public IDL.  The manifest
/// is rejected when its declared effect/account maps are not exhaustive for
/// the referenced IDL, which keeps an IDL upgrade from silently becoming a
/// denominator reduction.
pub fn load_pump_exact_state_semantics_authority_v2(
    manifest_path: &Path,
) -> Result<PumpExactStateSemanticsAuthorityV2> {
    let manifest_bytes = read_regular_file_exact_v2(
        manifest_path,
        "V2 Pump semantics manifest",
        SEMANTICS_MANIFEST_MAX_BYTES_V2,
    )?;
    let manifest: PumpExactStateSemanticsManifestFileV2 = serde_json::from_slice(&manifest_bytes)
        .with_context(|| {
        format!(
            "decode V2 Pump semantics manifest {}",
            manifest_path.display()
        )
    })?;
    if manifest.schema_version != PUMP_EXACT_STATE_SEMANTICS_SCHEMA_VERSION_V2 {
        bail!("V2 Pump semantics manifest schema is not accepted");
    }
    validate_nonempty("semantics_id", &manifest.semantics_id)?;
    validate_https_source_repository(&manifest.source_repository)?;
    validate_commit(&manifest.source_commit)?;
    let program_id = Pubkey::from_str(&manifest.program_id)
        .context("V2 Pump semantics manifest program_id is invalid")?;
    let expected_program_data_hash_blake3 = parse_hex_32(
        "V2 Pump semantics ProgramData BLAKE3",
        &manifest.program_data_hash_blake3,
    )?;
    let relative_idl_path = relative_manifest_path(&manifest.vendored_idl_relative_path)?;
    let manifest_parent = manifest_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 Pump semantics manifest has no parent"))?;
    let idl_path = manifest_parent.join(relative_idl_path);
    let idl_bytes =
        read_regular_file_exact_v2(&idl_path, "vendored V2 Pump IDL", VENDORED_IDL_MAX_BYTES_V2)?;
    let idl_digest = digest_bytes(&idl_bytes);
    if idl_digest.sha256 != manifest.vendored_idl_sha256
        || idl_digest.blake3 != manifest.vendored_idl_blake3
    {
        bail!("vendored V2 Pump IDL digest differs from semantics manifest");
    }
    let idl: Value = serde_json::from_slice(&idl_bytes)
        .with_context(|| format!("decode vendored V2 Pump IDL {}", idl_path.display()))?;
    let idl_program_id = required_string(&idl, "address", "vendored V2 Pump IDL")?;
    if Pubkey::from_str(idl_program_id).context("vendored V2 Pump IDL address is invalid")?
        != program_id
    {
        bail!("V2 semantics manifest program_id differs from vendored IDL address");
    }

    let defined_types = idl_defined_types(&idl)?;
    let idl_instructions = idl_array(&idl, "instructions", "vendored V2 Pump IDL")?;
    let idl_instruction_names = idl_instructions
        .iter()
        .map(|value| required_string(value, "name", "V2 Pump instruction").map(str::to_owned))
        .collect::<Result<BTreeSet<_>>>()?;
    let manifest_instruction_names = manifest
        .instruction_effects
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if idl_instruction_names != manifest_instruction_names {
        bail!("V2 semantics instruction-effect map is not an exact vendored-IDL key set");
    }
    let mut expected_state_role_instruction_names = BTreeSet::new();
    for (instruction_name, effect_name) in &manifest.instruction_effects {
        let effect = PumpExactStateInstructionEffectV2::parse(effect_name)?;
        if matches!(
            effect,
            PumpExactStateInstructionEffectV2::SupportedExactTrade
                | PumpExactStateInstructionEffectV2::SupportedExactCreate
        ) {
            expected_state_role_instruction_names.insert(instruction_name.clone());
        }
    }
    let manifest_state_role_instruction_names = manifest
        .instruction_state_roles
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if manifest_state_role_instruction_names != expected_state_role_instruction_names {
        bail!(
            "V2 semantics exact state-role map must cover exactly the supported exact instruction key set"
        );
    }
    let mut instructions_by_discriminator = BTreeMap::new();
    let mut instructions_by_name = BTreeMap::new();
    for instruction in idl_instructions {
        let name = required_string(instruction, "name", "V2 Pump instruction")?.to_owned();
        let discriminator = idl_discriminator(instruction, "V2 Pump instruction")?;
        let effect = PumpExactStateInstructionEffectV2::parse(
            manifest
                .instruction_effects
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("missing V2 Pump effect class for {name}"))?,
        )?;
        let accounts = idl_instruction_accounts(instruction)?;
        validate_instruction_effect_account_invariants_v2(&name, effect, &accounts)?;
        let state_roles = load_instruction_state_roles_v2(
            &name,
            effect,
            &accounts,
            manifest.instruction_state_roles.get(&name),
        )?;
        let args = idl_array(instruction, "args", "V2 Pump instruction")?.clone();
        let contract = PumpExactStateInstructionContractV2 {
            name: name.clone(),
            discriminator,
            effect,
            accounts,
            state_roles,
            args,
        };
        if instructions_by_discriminator
            .insert(discriminator, contract.clone())
            .is_some()
        {
            bail!("vendored V2 Pump IDL has duplicate instruction discriminator");
        }
        if instructions_by_name.insert(name, contract).is_some() {
            bail!("vendored V2 Pump IDL has duplicate instruction name");
        }
    }

    let idl_events = idl_array(&idl, "events", "vendored V2 Pump IDL")?;
    let idl_event_names = idl_events
        .iter()
        .map(|value| required_string(value, "name", "V2 Pump event").map(str::to_owned))
        .collect::<Result<BTreeSet<_>>>()?;
    let manifest_event_names = manifest
        .event_parent_contracts
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if idl_event_names != manifest_event_names {
        bail!("V2 semantics event-parent map is not an exact vendored-IDL key set");
    }
    let mut events_by_discriminator = BTreeMap::new();
    for event in idl_events {
        let name = required_string(event, "name", "V2 Pump event")?.to_owned();
        let discriminator = idl_discriminator(event, "V2 Pump event")?;
        let fields = idl_event_fields(event, &defined_types)?;
        let parent_contracts = load_event_parent_contracts_v2(
            &name,
            &fields,
            manifest
                .event_parent_contracts
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("missing V2 event-parent contract for {name}"))?,
            &instructions_by_name,
            &defined_types,
        )?;
        if events_by_discriminator
            .insert(
                discriminator,
                PumpExactStateEventContractV2 {
                    name,
                    discriminator,
                    fields,
                    parent_contracts,
                },
            )
            .is_some()
        {
            bail!("vendored V2 Pump IDL has duplicate event discriminator");
        }
    }

    let idl_accounts = idl_array(&idl, "accounts", "vendored V2 Pump IDL")?;
    let idl_account_names = idl_accounts
        .iter()
        .map(|value| required_string(value, "name", "V2 Pump account").map(str::to_owned))
        .collect::<Result<BTreeSet<_>>>()?;
    let manifest_account_names = manifest
        .account_classes
        .keys()
        .cloned()
        .collect::<BTreeSet<_>>();
    if idl_account_names != manifest_account_names {
        bail!("V2 semantics account-class map is not an exact vendored-IDL key set");
    }
    let mut accounts_by_discriminator = BTreeMap::new();
    for account in idl_accounts {
        let name = required_string(account, "name", "V2 Pump account")?.to_owned();
        let discriminator = idl_discriminator(account, "V2 Pump account")?;
        let class = PumpExactStateAccountClassV2::parse(
            manifest
                .account_classes
                .get(&name)
                .ok_or_else(|| anyhow::anyhow!("missing V2 Pump account class for {name}"))?,
        )?;
        if accounts_by_discriminator
            .insert(discriminator, class)
            .is_some()
        {
            bail!("vendored V2 Pump IDL has duplicate account discriminator");
        }
    }

    // The prospective qualifier reads state only from these two account
    // layouts. Requiring the literal, unique set prevents a semantics file
    // from carrying an unused duplicate or alternative layout whose selection
    // would be ambiguous during a later review.
    let manifest_layout_names = manifest
        .account_layouts
        .iter()
        .map(|entry| entry.account.as_str())
        .collect::<BTreeSet<_>>();
    let required_layout_names = BTreeSet::from(["BondingCurve", "Global"]);
    if manifest.account_layouts.len() != required_layout_names.len()
        || manifest_layout_names != required_layout_names
    {
        bail!(
            "V2 semantics account_layouts must contain exactly one BondingCurve and one Global layout"
        );
    }

    let curve_layout = load_account_layout(
        "BondingCurve",
        &manifest.account_layouts,
        &idl,
        &defined_types,
    )?;
    let global_layout =
        load_account_layout("Global", &manifest.account_layouts, &idl, &defined_types)?;
    if accounts_by_discriminator.get(&curve_layout.discriminator)
        != Some(&PumpExactStateAccountClassV2::ExactBondingCurve)
        || accounts_by_discriminator.get(&global_layout.discriminator)
            != Some(&PumpExactStateAccountClassV2::KnownGlobalDependency)
    {
        bail!("V2 semantics required BondingCurve/Global account classes are invalid");
    }

    Ok(PumpExactStateSemanticsAuthorityV2 {
        semantics_id: manifest.semantics_id,
        manifest_digest: digest_bytes(&manifest_bytes),
        idl_digest,
        program_id,
        expected_program_data_hash_blake3,
        instructions_by_discriminator,
        events_by_discriminator,
        accounts_by_discriminator,
        curve_layout,
        global_layout,
        defined_types,
    })
}

/// Effect classes are selected by the manifest, but they are not allowed to
/// contradict the literal writable-account contract from the vendored IDL.
/// In particular, a writable Global or BondingCurve is state/dependency
/// evidence and cannot disappear into `ProvenNonReserve` merely because a
/// manifest author labels it "administrative".
fn validate_instruction_effect_account_invariants_v2(
    instruction_name: &str,
    effect: PumpExactStateInstructionEffectV2,
    accounts: &[PumpExactStateAccountMetaContractV2],
) -> Result<()> {
    let writable_global = accounts
        .iter()
        .any(|account| is_global_account_role_v2(&account.name) && account.writable);
    let writable_bonding_curve = accounts
        .iter()
        .any(|account| is_bonding_curve_account_role_v2(&account.name) && account.writable);

    if writable_global && effect != PumpExactStateInstructionEffectV2::GlobalDependencyMutation {
        bail!(
            "V2 Pump instruction {instruction_name} writes Global and must be global_dependency_mutation"
        );
    }
    if writable_bonding_curve && !effect.is_candidate() {
        bail!(
            "V2 Pump instruction {instruction_name} writes BondingCurve and cannot be proven_non_reserve"
        );
    }
    if matches!(effect, PumpExactStateInstructionEffectV2::ProvenNonReserve)
        && (writable_global || writable_bonding_curve)
    {
        bail!(
            "V2 Pump instruction {instruction_name} with writable state/dependency account cannot be proven_non_reserve"
        );
    }
    if effect.is_supported_exact_trade() && !writable_bonding_curve {
        bail!("V2 supported_exact_trade {instruction_name} must write its BondingCurve account");
    }
    if effect.is_supported_exact_trade()
        && !SUPPORTED_EXACT_TRADE_INSTRUCTION_NAMES_V2.contains(&instruction_name)
    {
        bail!(
            "V2 supported_exact_trade {instruction_name} is outside the closed exact-trade instruction family"
        );
    }
    if matches!(
        effect,
        PumpExactStateInstructionEffectV2::SupportedExactCreate
    ) && !writable_bonding_curve
    {
        bail!("V2 supported_exact_create {instruction_name} must write its BondingCurve account");
    }
    if matches!(
        effect,
        PumpExactStateInstructionEffectV2::SupportedExactCreate
    ) && !SUPPORTED_EXACT_CREATE_INSTRUCTION_NAMES_V2.contains(&instruction_name)
    {
        bail!(
            "V2 supported_exact_create {instruction_name} is outside the closed exact-create instruction family"
        );
    }
    Ok(())
}

/// Closed, reviewed instruction families that may contribute exact
/// state evidence.  A hash-pinned manifest may select *less* than this set,
/// but it may never promote an arbitrary curve-adjacent transition such as
/// `migrate` or `set_creator` to an exact trade or birth.
const SUPPORTED_EXACT_TRADE_INSTRUCTION_NAMES_V2: &[&str] = &[
    "buy",
    "sell",
    "buy_exact_sol_in",
    "buy_v2",
    "buy_exact_quote_in_v2",
    "sell_v2",
];

const SUPPORTED_EXACT_CREATE_INSTRUCTION_NAMES_V2: &[&str] = &["create", "create_v2"];

fn event_authority_fields_for_exact_parent_v2(
    event_name: &str,
    effect: PumpExactStateInstructionEffectV2,
) -> Result<&'static [&'static str]> {
    match (event_name, effect) {
        ("TradeEvent", PumpExactStateInstructionEffectV2::SupportedExactTrade) => Ok(&[
            "mint",
            "user",
            "is_buy",
            "ix_name",
            "quote_mint",
            "virtual_token_reserves",
            "real_token_reserves",
        ]),
        ("CreateEvent", PumpExactStateInstructionEffectV2::SupportedExactCreate) => Ok(&[
            "mint",
            "bonding_curve",
            "user",
            "creator",
            "token_program",
            "is_mayhem_mode",
            "is_cashback_enabled",
            "quote_mint",
            "virtual_token_reserves",
            "real_token_reserves",
            "token_total_supply",
        ]),
        ("CompleteEvent", PumpExactStateInstructionEffectV2::SupportedExactTrade) => {
            Ok(&["mint", "bonding_curve", "user", "quote_mint"])
        }
        (
            _,
            PumpExactStateInstructionEffectV2::SupportedExactTrade
            | PumpExactStateInstructionEffectV2::SupportedExactCreate,
        ) => bail!(
            "V2 exact instruction class may not authorize unsupported Anchor event {event_name}"
        ),
        _ => Ok(&[]),
    }
}

fn required_final_curve_state_field_v2(
    event_name: &str,
    event_field: &str,
) -> Option<&'static str> {
    match (event_name, event_field) {
        ("TradeEvent", "quote_mint") => Some("quote_mint"),
        ("TradeEvent", "virtual_token_reserves") => Some("virtual_token_reserves"),
        ("TradeEvent", "real_token_reserves") => Some("real_token_reserves"),
        ("CreateEvent", "is_mayhem_mode") => Some("is_mayhem_mode"),
        ("CreateEvent", "is_cashback_enabled") => Some("is_cashback_coin"),
        ("CreateEvent", "quote_mint") => Some("quote_mint"),
        ("CreateEvent", "virtual_token_reserves") => Some("virtual_token_reserves"),
        ("CreateEvent", "real_token_reserves") => Some("real_token_reserves"),
        ("CreateEvent", "token_total_supply") => Some("token_total_supply"),
        ("CompleteEvent", "quote_mint") => Some("quote_mint"),
        _ => None,
    }
}

/// Exact-state candidates need an explicit, manifest-selected curve and mint
/// role.  The role strings are checked against the same pinned IDL account
/// vector that later validates order, signer and writable bits.  This makes a
/// role spelling/revision change an authority failure instead of a fallback to
/// guessed positions or Rust-owned aliases.
fn load_instruction_state_roles_v2(
    instruction_name: &str,
    effect: PumpExactStateInstructionEffectV2,
    accounts: &[PumpExactStateAccountMetaContractV2],
    manifest_roles: Option<&PumpExactStateInstructionStateRolesManifestV2>,
) -> Result<Option<PumpExactStateInstructionStateRolesV2>> {
    let requires_state_roles = matches!(
        effect,
        PumpExactStateInstructionEffectV2::SupportedExactTrade
            | PumpExactStateInstructionEffectV2::SupportedExactCreate
    );
    if !requires_state_roles {
        if manifest_roles.is_some() {
            bail!(
                "V2 Pump instruction {instruction_name} has state roles but is not a supported exact instruction"
            );
        }
        return Ok(None);
    }
    let manifest_roles = manifest_roles.ok_or_else(|| {
        anyhow::anyhow!(
            "V2 supported exact instruction {instruction_name} lacks pinned state-account roles"
        )
    })?;
    validate_nonempty(
        "V2 instruction bonding_curve_role",
        &manifest_roles.bonding_curve_role,
    )?;
    validate_nonempty("V2 instruction mint_role", &manifest_roles.mint_role)?;
    if manifest_roles.bonding_curve_role == manifest_roles.mint_role {
        bail!(
            "V2 supported exact instruction {instruction_name} reuses one account role for BondingCurve and mint"
        );
    }
    let curve = accounts
        .iter()
        .find(|account| account.name == manifest_roles.bonding_curve_role)
        .ok_or_else(|| {
            anyhow::anyhow!(
                "V2 supported exact instruction {instruction_name} lacks manifest BondingCurve role {}",
                manifest_roles.bonding_curve_role
            )
        })?;
    if !curve.writable {
        bail!(
            "V2 supported exact instruction {instruction_name} has non-writable manifest BondingCurve role {}",
            manifest_roles.bonding_curve_role
        );
    }
    if !is_bonding_curve_account_role_v2(&curve.name) {
        bail!(
            "V2 supported exact instruction {instruction_name} manifest BondingCurve role {} is not a recognized BondingCurve role",
            curve.name
        );
    }
    if !accounts
        .iter()
        .any(|account| account.name == manifest_roles.mint_role)
    {
        bail!(
            "V2 supported exact instruction {instruction_name} lacks manifest mint role {}",
            manifest_roles.mint_role
        );
    }
    Ok(Some(PumpExactStateInstructionStateRolesV2 {
        bonding_curve_role: manifest_roles.bonding_curve_role.clone(),
        mint_role: manifest_roles.mint_role.clone(),
    }))
}

/// Anchor IDLs have used both snake_case and lower-camel account role names.
/// This is a *name normalization for the pinned IDL contract*, not positional
/// inference from a live instruction: it prevents `bondingCurve` or `Global`
/// from escaping the denominator invariant merely through spelling drift.
fn normalized_account_role_name_v2(name: &str) -> String {
    name.bytes()
        .filter(|byte| *byte != b'_' && *byte != b'-')
        .map(char::from)
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_global_account_role_v2(name: &str) -> bool {
    normalized_account_role_name_v2(name) == "global"
}

fn is_bonding_curve_account_role_v2(name: &str) -> bool {
    normalized_account_role_name_v2(name) == "bondingcurve"
}

/// Resolve the manifest's complete semantic contract for one Anchor event.
/// Every field remains present in the pinned IDL and is decoded exactly from
/// the event payload.  Fields which cannot be independently tied to a parent
/// instruction or canonical curve state may be declared `strict_decode_only`.
/// That declaration is deliberately one-way: it proves the Borsh layout but
/// cannot become authority for parent identity or exact-state promotion.
fn load_event_parent_contracts_v2(
    event_name: &str,
    event_fields: &[Value],
    manifest_contracts: &[PumpExactStateEventParentManifestV2],
    instructions_by_name: &BTreeMap<String, PumpExactStateInstructionContractV2>,
    defined_types: &BTreeMap<String, Value>,
) -> Result<BTreeMap<[u8; 8], PumpExactStateEventParentContractV2>> {
    if manifest_contracts.is_empty() {
        bail!("V2 event {event_name} has no permitted parent instruction contract");
    }
    let event_field_types = named_borsh_field_types_v2(event_fields, "V2 Pump event")?;
    let mut contracts = BTreeMap::new();
    for manifest_contract in manifest_contracts {
        let parent = instructions_by_name
            .get(&manifest_contract.parent_instruction)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 event {event_name} references unknown parent instruction {}",
                    manifest_contract.parent_instruction
                )
            })?;
        let parent_argument_types =
            named_borsh_field_types_v2(&parent.args, "V2 Pump instruction argument")?;
        let required_authority_fields =
            event_authority_fields_for_exact_parent_v2(event_name, parent.effect)?;
        let mut seen_event_fields = BTreeSet::new();
        let mut bindings = Vec::with_capacity(event_field_types.len());
        for binding in &manifest_contract.field_bindings {
            validate_nonempty("event field binding event_field", &binding.event_field)?;
            if !seen_event_fields.insert(binding.event_field.clone()) {
                bail!(
                    "V2 event {event_name} parent {} binds event field {} more than once",
                    parent.name,
                    binding.event_field
                );
            }
            let event_type = event_field_types.get(&binding.event_field).ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 event {event_name} parent {} binds absent event field {}",
                    parent.name,
                    binding.event_field
                )
            })?;
            let source_count = usize::from(binding.parent_account_role.is_some())
                .checked_add(usize::from(binding.parent_instruction_argument.is_some()))
                .and_then(|count| {
                    count.checked_add(usize::from(binding.expected_borsh_hex.is_some()))
                })
                .and_then(|count| {
                    count.checked_add(usize::from(binding.final_curve_state_field.is_some()))
                })
                .and_then(|count| count.checked_add(usize::from(binding.strict_decode_only)))
                .ok_or_else(|| anyhow::anyhow!("V2 event field source count overflow"))?;
            if source_count != 1 {
                bail!(
                    "V2 event {event_name} field {} must declare exactly one semantic source",
                    binding.event_field
                );
            }
            let source = if let Some(role) = &binding.parent_account_role {
                validate_nonempty("event parent account role", role)?;
                if event_type.as_str() != Some("pubkey") {
                    bail!(
                        "V2 event {event_name} field {} bound to parent account role {role} is not a pubkey",
                        binding.event_field
                    );
                }
                if !parent.accounts.iter().any(|account| account.name == *role) {
                    bail!(
                        "V2 event {event_name} parent {} lacks account role {role}",
                        parent.name
                    );
                }
                PumpExactStateEventFieldSourceV2::ParentAccountRole(role.clone())
            } else if let Some(argument) = &binding.parent_instruction_argument {
                validate_nonempty("event parent instruction argument", argument)?;
                let parent_type = parent_argument_types.get(argument).ok_or_else(|| {
                    anyhow::anyhow!(
                        "V2 event {event_name} parent {} lacks argument {argument}",
                        parent.name
                    )
                })?;
                if canonical_json_bytes(event_type)? != canonical_json_bytes(parent_type)? {
                    bail!(
                        "V2 event {event_name} field {} type differs from parent {} argument {argument}",
                        binding.event_field,
                        parent.name
                    );
                }
                PumpExactStateEventFieldSourceV2::ParentInstructionArgument(argument.clone())
            } else if let Some(encoded) = &binding.expected_borsh_hex {
                let bytes = decode_hex(encoded).with_context(|| {
                    format!(
                        "decode V2 event {event_name} field {} literal Borsh bytes",
                        binding.event_field
                    )
                })?;
                validate_single_borsh_value_exact_v2(event_type, &bytes, defined_types)?;
                PumpExactStateEventFieldSourceV2::LiteralBorsh(bytes)
            } else if let Some(curve_field) = &binding.final_curve_state_field {
                validate_nonempty("event final curve-state field", curve_field)?;
                let expected_type = curve_state_field_type_v2(curve_field)?;
                if canonical_json_bytes(event_type)? != canonical_json_bytes(&expected_type)? {
                    bail!(
                        "V2 event {event_name} field {} type differs from final curve-state field {curve_field}",
                        binding.event_field
                    );
                }
                PumpExactStateEventFieldSourceV2::FinalCurveStateField(curve_field.clone())
            } else if binding.strict_decode_only {
                PumpExactStateEventFieldSourceV2::StrictDecodeOnly
            } else {
                bail!("V2 event {event_name} field source is absent")
            };
            if required_authority_fields.contains(&binding.event_field.as_str())
                && matches!(&source, PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
            {
                bail!(
                    "V2 exact parent {} may not declare required {event_name} authority field {} strict_decode_only",
                    parent.name,
                    binding.event_field
                );
            }
            if let Some(expected_curve_field) =
                required_final_curve_state_field_v2(event_name, &binding.event_field)
            {
                match &source {
                    PumpExactStateEventFieldSourceV2::FinalCurveStateField(actual)
                        if actual == expected_curve_field => {}
                    _ if required_authority_fields.contains(&binding.event_field.as_str()) => {
                        bail!(
                            "V2 exact parent {} must bind {event_name} field {} to final curve-state field {expected_curve_field}",
                            parent.name,
                            binding.event_field
                        );
                    }
                    _ => {}
                }
            }
            bindings.push(PumpExactStateEventFieldBindingV2 {
                event_field: binding.event_field.clone(),
                source,
            });
        }
        if manifest_contract.strict_decode_only_unbound_fields {
            for event_field in event_field_types.keys() {
                if seen_event_fields.contains(event_field) {
                    continue;
                }
                if required_authority_fields.contains(&event_field.as_str()) {
                    bail!(
                        "V2 exact parent {} may not cover required {event_name} authority field {event_field} with strict_decode_only_unbound_fields",
                        parent.name
                    );
                }
                seen_event_fields.insert(event_field.clone());
                bindings.push(PumpExactStateEventFieldBindingV2 {
                    event_field: event_field.clone(),
                    source: PumpExactStateEventFieldSourceV2::StrictDecodeOnly,
                });
            }
        }
        if seen_event_fields != event_field_types.keys().cloned().collect() {
            bail!(
                "V2 event {event_name} parent {} does not bind every event field exactly once",
                parent.name
            );
        }
        for required in required_authority_fields {
            if !seen_event_fields.contains(*required) {
                bail!(
                    "V2 exact parent {} omits required {event_name} authority field {required}",
                    parent.name
                );
            }
        }
        let contract = PumpExactStateEventParentContractV2 {
            parent_instruction_name: parent.name.clone(),
            field_bindings: bindings,
        };
        if contracts.insert(parent.discriminator, contract).is_some() {
            bail!(
                "V2 event {event_name} repeats parent instruction {}",
                parent.name
            );
        }
    }
    Ok(contracts)
}

fn named_borsh_field_types_v2(fields: &[Value], label: &str) -> Result<BTreeMap<String, Value>> {
    let mut named = BTreeMap::new();
    for field in fields {
        let name = required_string(field, "name", label)?.to_owned();
        let ty = field
            .get("type")
            .ok_or_else(|| anyhow::anyhow!("{label} field {name} lacks type"))?
            .clone();
        if named.insert(name.clone(), ty).is_some() {
            bail!("{label} repeats field {name}");
        }
    }
    Ok(named)
}

fn validate_single_borsh_value_exact_v2(
    ty: &Value,
    bytes: &[u8],
    defined_types: &BTreeMap<String, Value>,
) -> Result<()> {
    let mut offset = 0usize;
    consume_borsh_type(ty, bytes, &mut offset, defined_types, 0)?;
    if offset != bytes.len() {
        bail!("V2 literal Borsh field has trailing bytes");
    }
    Ok(())
}

fn curve_state_field_type_v2(name: &str) -> Result<Value> {
    let primitive = match name {
        "virtual_token_reserves"
        | "virtual_quote_reserves"
        | "real_token_reserves"
        | "real_quote_reserves"
        | "token_total_supply" => "u64",
        "complete" | "is_mayhem_mode" | "is_cashback_coin" => "bool",
        "creator" | "quote_mint" => "pubkey",
        _ => bail!("unknown V2 final curve-state field {name:?}"),
    };
    Ok(Value::String(primitive.to_owned()))
}

impl PumpExactStateSemanticsAuthorityV2 {
    /// The ProgramData bytes observed by the raw V2 capture are the semantic
    /// selection key. A valid public IDL by itself never authorizes a different
    /// deployed program image.
    pub fn validate_program_data(&self, receipt: &PumpProgramDataReceiptV1) -> Result<()> {
        if receipt.pump_program_id.into_inner() != self.program_id.to_bytes()
            || receipt.program_data_hash_blake3.into_inner()
                != self.expected_program_data_hash_blake3
        {
            bail!("V2 semantics manifest does not match raw run Pump ProgramData authority");
        }
        Ok(())
    }

    /// The hash-pinned ProgramData selection that the prospective recorder
    /// must observe before it allocates a V2 raw run.  Returning the bytes
    /// rather than an executable/path reference keeps the preflight binding
    /// local, deterministic, and free of provider or filesystem authority.
    #[must_use]
    pub const fn expected_program_data_hash_blake3(&self) -> [u8; 32] {
        self.expected_program_data_hash_blake3
    }

    #[must_use]
    pub fn instruction(
        &self,
        discriminator: &[u8; 8],
    ) -> Option<&PumpExactStateInstructionContractV2> {
        self.instructions_by_discriminator.get(discriminator)
    }

    #[must_use]
    pub fn event(&self, discriminator: &[u8; 8]) -> Option<&PumpExactStateEventContractV2> {
        self.events_by_discriminator.get(discriminator)
    }

    #[must_use]
    pub fn account_class(&self, data: &[u8]) -> Option<PumpExactStateAccountClassV2> {
        let discriminator: [u8; 8] = data.get(..8)?.try_into().ok()?;
        self.accounts_by_discriminator.get(&discriminator).copied()
    }

    pub fn decode_curve_state(&self, data: &[u8]) -> Result<PumpExactStateCurveStateV2> {
        validate_account_layout(&self.curve_layout, data, &self.defined_types)?;
        Ok(PumpExactStateCurveStateV2 {
            virtual_token_reserves: read_named_u64(
                &self.curve_layout,
                data,
                "virtual_token_reserves",
                &self.defined_types,
            )?,
            virtual_quote_reserves: read_named_u64(
                &self.curve_layout,
                data,
                "virtual_quote_reserves",
                &self.defined_types,
            )?,
            real_token_reserves: read_named_u64(
                &self.curve_layout,
                data,
                "real_token_reserves",
                &self.defined_types,
            )?,
            real_quote_reserves: read_named_u64(
                &self.curve_layout,
                data,
                "real_quote_reserves",
                &self.defined_types,
            )?,
            token_total_supply: read_named_u64(
                &self.curve_layout,
                data,
                "token_total_supply",
                &self.defined_types,
            )?,
            complete: read_named_bool(&self.curve_layout, data, "complete", &self.defined_types)?,
            creator: read_named_pubkey(&self.curve_layout, data, "creator", &self.defined_types)?,
            is_mayhem_mode: read_named_bool(
                &self.curve_layout,
                data,
                "is_mayhem_mode",
                &self.defined_types,
            )?,
            is_cashback_coin: read_named_bool(
                &self.curve_layout,
                data,
                "is_cashback_coin",
                &self.defined_types,
            )?,
            quote_mint: read_named_pubkey(
                &self.curve_layout,
                data,
                "quote_mint",
                &self.defined_types,
            )?,
        })
    }

    pub fn validate_global_account(&self, data: &[u8]) -> Result<()> {
        validate_account_layout(&self.global_layout, data, &self.defined_types)
    }

    /// Verify full Borsh consumption for an instruction payload that follows
    /// an Anchor eight-byte discriminator.
    pub fn validate_instruction_payload(
        &self,
        contract: &PumpExactStateInstructionContractV2,
        payload: &[u8],
    ) -> Result<()> {
        validate_borsh_fields_exact(&contract.args, payload, &self.defined_types)
    }

    /// Verify full Borsh consumption for the payload after an Anchor event
    /// discriminator. Event transport is not itself a curve mutation, but a
    /// malformed transport must remain visible to the qualifier.
    pub fn validate_event_payload(
        &self,
        contract: &PumpExactStateEventContractV2,
        payload: &[u8],
    ) -> Result<()> {
        validate_borsh_fields_exact(&contract.fields, payload, &self.defined_types)
    }

    /// Decode one already-validated instruction payload into exact Borsh
    /// slices keyed by the vendored argument names.  The caller supplies the
    /// account-role map only after it has checked count, order, static address,
    /// signer, and writable authority against the same instruction contract.
    pub fn instruction_semantic_evidence(
        &self,
        contract: &PumpExactStateInstructionContractV2,
        payload: &[u8],
        account_roles: BTreeMap<String, Pubkey>,
    ) -> Result<PumpExactStateInstructionSemanticEvidenceV2> {
        Ok(PumpExactStateInstructionSemanticEvidenceV2 {
            discriminator: contract.discriminator,
            account_roles,
            argument_fields: self.instruction_argument_fields(contract, payload)?,
        })
    }

    pub fn instruction_argument_fields(
        &self,
        contract: &PumpExactStateInstructionContractV2,
        payload: &[u8],
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        decode_named_borsh_fields_exact_v2(
            &contract.args,
            payload,
            &self.defined_types,
            "V2 instruction payload",
        )
    }

    /// Return the only curve/mint identity permitted for an exact candidate.
    /// These role names come from the same hash-pinned manifest that selected
    /// this instruction contract; the qualifier does not infer them from
    /// account position or a fallback list of names.
    pub fn exact_state_account_pubkeys(
        &self,
        contract: &PumpExactStateInstructionContractV2,
        account_roles: &BTreeMap<String, Pubkey>,
    ) -> Result<(Option<Pubkey>, Option<Pubkey>)> {
        let Some(roles) = &contract.state_roles else {
            return Ok((None, None));
        };
        let curve = account_roles
            .get(&roles.bonding_curve_role)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 exact instruction {} lost manifest BondingCurve role {} after account validation",
                    contract.name,
                    roles.bonding_curve_role
                )
            })?;
        let mint = account_roles
            .get(&roles.mint_role)
            .copied()
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 exact instruction {} lost manifest mint role {} after account validation",
                    contract.name,
                    roles.mint_role
                )
            })?;
        Ok((Some(curve), Some(mint)))
    }

    /// Decode every event field exactly once.  This is separate from the
    /// tolerant active runtime decoder: a prospective qualification event is
    /// evidence only when all bytes are consumed by its vendored field layout.
    pub fn event_semantic_fields(
        &self,
        contract: &PumpExactStateEventContractV2,
        payload: &[u8],
    ) -> Result<BTreeMap<String, Vec<u8>>> {
        decode_named_borsh_fields_exact_v2(
            &contract.fields,
            payload,
            &self.defined_types,
            "V2 Anchor event payload",
        )
    }

    /// Validate the hash-pinned semantic relation between a decoded Anchor
    /// event and its immediate Pump parent.  The returned bindings must still
    /// be compared with the final anchored curve state for that exact parent
    /// transaction; keeping that last comparison explicit prevents an event
    /// payload from becoming state authority by itself.
    pub fn validate_event_parent_semantics(
        &self,
        event: &PumpExactStateEventContractV2,
        event_fields: &BTreeMap<String, Vec<u8>>,
        parent: &PumpExactStateInstructionSemanticEvidenceV2,
    ) -> Result<Vec<PumpExactStateEventFinalStateBindingV2>> {
        let contract = event
            .parent_contracts
            .get(&parent.discriminator)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 event {} is not permitted under its immediate parent instruction discriminator",
                    event.name
                )
            })?;
        let mut final_state_bindings = Vec::new();
        for binding in &contract.field_bindings {
            let observed = event_fields.get(&binding.event_field).ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 event {} omitted contract-bound field {}",
                    event.name,
                    binding.event_field
                )
            })?;
            match &binding.source {
                PumpExactStateEventFieldSourceV2::ParentAccountRole(role) => {
                    let expected = parent.account_roles.get(role).ok_or_else(|| {
                        anyhow::anyhow!(
                            "V2 event {} parent {} lacks bound account role {role}",
                            event.name,
                            contract.parent_instruction_name
                        )
                    })?;
                    if observed.as_slice() != expected.to_bytes() {
                        bail!(
                            "V2 event {} field {} differs from parent account role {role}",
                            event.name,
                            binding.event_field
                        );
                    }
                }
                PumpExactStateEventFieldSourceV2::ParentInstructionArgument(argument) => {
                    let expected = parent.argument_fields.get(argument).ok_or_else(|| {
                        anyhow::anyhow!(
                            "V2 event {} parent {} lacks bound argument {argument}",
                            event.name,
                            contract.parent_instruction_name
                        )
                    })?;
                    if observed != expected {
                        bail!(
                            "V2 event {} field {} differs from parent argument {argument}",
                            event.name,
                            binding.event_field
                        );
                    }
                }
                PumpExactStateEventFieldSourceV2::LiteralBorsh(expected) => {
                    if observed != expected {
                        bail!(
                            "V2 event {} field {} differs from its pinned literal",
                            event.name,
                            binding.event_field
                        );
                    }
                }
                PumpExactStateEventFieldSourceV2::FinalCurveStateField(field) => {
                    final_state_bindings.push(PumpExactStateEventFinalStateBindingV2 {
                        curve_state_field: field.clone(),
                        event_value_borsh: observed.clone(),
                    });
                }
                PumpExactStateEventFieldSourceV2::StrictDecodeOnly => {
                    // Complete Borsh decoding happened before this semantic
                    // comparison.  This field is intentionally unavailable
                    // as parent or state authority.
                }
            }
        }
        Ok(final_state_bindings)
    }
}

fn decode_named_borsh_fields_exact_v2(
    fields: &[Value],
    bytes: &[u8],
    defined_types: &BTreeMap<String, Value>,
    label: &str,
) -> Result<BTreeMap<String, Vec<u8>>> {
    let mut offset = 0usize;
    let mut result = BTreeMap::new();
    for field in fields {
        let name = required_string(field, "name", label)?.to_owned();
        let ty = field
            .get("type")
            .ok_or_else(|| anyhow::anyhow!("{label} field {name} lacks type"))?;
        let start = offset;
        consume_borsh_type(ty, bytes, &mut offset, defined_types, 0)?;
        let value = bytes
            .get(start..offset)
            .ok_or_else(|| anyhow::anyhow!("{label} field {name} range is invalid"))?
            .to_vec();
        if result.insert(name.clone(), value).is_some() {
            bail!("{label} repeats field {name}");
        }
    }
    if offset != bytes.len() {
        bail!("{label} has trailing bytes");
    }
    Ok(result)
}

fn load_account_layout(
    name: &str,
    manifest_layouts: &[PumpExactStateAccountLayoutManifestV2],
    idl: &Value,
    defined_types: &BTreeMap<String, Value>,
) -> Result<PumpExactStateAccountLayoutV2> {
    let entry = manifest_layouts
        .iter()
        .find(|entry| entry.account == name)
        .ok_or_else(|| anyhow::anyhow!("V2 semantics manifest lacks {name} account layout"))?;
    let discriminator = parse_hex_8(
        &format!("{name} account discriminator"),
        &entry.discriminator,
    )?;
    let allowed_serialized_bytes = entry
        .allowed_serialized_bytes
        .iter()
        .map(|value| usize::try_from(*value).context("V2 account layout byte length exceeds usize"))
        .collect::<Result<BTreeSet<_>>>()?;
    let prefix_bytes = usize::try_from(entry.prefix_bytes)
        .context("V2 account layout prefix length exceeds usize")?;
    if allowed_serialized_bytes.is_empty()
        || prefix_bytes == 0
        || allowed_serialized_bytes
            .iter()
            .any(|size| *size < prefix_bytes)
    {
        bail!("V2 {name} account layout has invalid size envelope");
    }
    let idl_account = idl_array(idl, "accounts", "vendored V2 Pump IDL")?
        .iter()
        .find(|account| {
            matches!(
                required_string(account, "name", "V2 Pump account"),
                Ok(account_name) if account_name == name
            )
        })
        .ok_or_else(|| anyhow::anyhow!("vendored V2 Pump IDL lacks {name} account"))?;
    if idl_discriminator(idl_account, "V2 Pump account")? != discriminator {
        bail!("V2 {name} account discriminator differs from vendored IDL");
    }
    let fields = defined_types
        .get(name)
        .and_then(|value| value.get("type"))
        .and_then(|value| value.get("fields"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("vendored V2 Pump IDL type {name} lacks struct fields"))?;
    let field_layout_digest = digest_bytes(&canonical_json_bytes(&Value::Array(fields.clone()))?);
    if field_layout_digest.sha256 != entry.field_layout_sha256
        || field_layout_digest.blake3 != entry.field_layout_blake3
    {
        bail!("V2 {name} account field layout digest differs from vendored IDL fields");
    }
    let calculated_prefix = fields.iter().try_fold(8usize, |offset, field| {
        fixed_borsh_type_len(
            field
                .get("type")
                .ok_or_else(|| anyhow::anyhow!("V2 {name} field lacks type"))?,
            defined_types,
            0,
        )
        .and_then(|length| {
            offset
                .checked_add(length)
                .ok_or_else(|| anyhow::anyhow!("V2 {name} account prefix length overflow"))
        })
    })?;
    if calculated_prefix != prefix_bytes {
        bail!("V2 {name} manifest prefix length differs from vendored IDL fields");
    }
    Ok(PumpExactStateAccountLayoutV2 {
        name: name.to_owned(),
        discriminator,
        allowed_serialized_bytes,
        prefix_bytes,
        fields,
    })
}

fn validate_account_layout(
    layout: &PumpExactStateAccountLayoutV2,
    data: &[u8],
    defined_types: &BTreeMap<String, Value>,
) -> Result<()> {
    if !layout.allowed_serialized_bytes.contains(&data.len())
        || data.get(..8).and_then(|value| value.try_into().ok()) != Some(layout.discriminator)
    {
        bail!(
            "V2 {} account has an unsupported discriminator or allocation envelope",
            layout.name
        );
    }
    if layout.prefix_bytes > data.len() {
        bail!(
            "V2 {} account is shorter than its selected layout prefix",
            layout.name
        );
    }
    let mut offset = 8usize;
    for field in &layout.fields {
        consume_borsh_type(
            field
                .get("type")
                .ok_or_else(|| anyhow::anyhow!("V2 {} field lacks type", layout.name))?,
            &data[..layout.prefix_bytes],
            &mut offset,
            defined_types,
            0,
        )?;
    }
    if offset != layout.prefix_bytes {
        bail!(
            "V2 {} account layout does not consume its selected prefix",
            layout.name
        );
    }
    Ok(())
}

fn read_named_u64(
    layout: &PumpExactStateAccountLayoutV2,
    data: &[u8],
    name: &str,
    defined_types: &BTreeMap<String, Value>,
) -> Result<u64> {
    let offset = named_fixed_field_offset(layout, name, defined_types)?;
    let bytes: [u8; 8] = data
        .get(offset..offset.saturating_add(8))
        .ok_or_else(|| anyhow::anyhow!("V2 {} field {name} is truncated", layout.name))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("V2 {} field {name} has wrong width", layout.name))?;
    Ok(u64::from_le_bytes(bytes))
}

fn read_named_bool(
    layout: &PumpExactStateAccountLayoutV2,
    data: &[u8],
    name: &str,
    defined_types: &BTreeMap<String, Value>,
) -> Result<bool> {
    let offset = named_fixed_field_offset(layout, name, defined_types)?;
    match data
        .get(offset)
        .copied()
        .ok_or_else(|| anyhow::anyhow!("V2 {} field {name} is truncated", layout.name))?
    {
        0 => Ok(false),
        1 => Ok(true),
        _ => bail!("V2 {} field {name} has an invalid bool", layout.name),
    }
}

fn read_named_pubkey(
    layout: &PumpExactStateAccountLayoutV2,
    data: &[u8],
    name: &str,
    defined_types: &BTreeMap<String, Value>,
) -> Result<Pubkey> {
    let offset = named_fixed_field_offset(layout, name, defined_types)?;
    let bytes: [u8; 32] = data
        .get(offset..offset.saturating_add(32))
        .ok_or_else(|| anyhow::anyhow!("V2 {} field {name} is truncated", layout.name))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("V2 {} field {name} has wrong width", layout.name))?;
    Ok(Pubkey::new_from_array(bytes))
}

fn named_fixed_field_offset(
    layout: &PumpExactStateAccountLayoutV2,
    name: &str,
    defined_types: &BTreeMap<String, Value>,
) -> Result<usize> {
    let mut offset = 8usize;
    for field in &layout.fields {
        let field_name = required_string(field, "name", "V2 account layout field")?;
        let field_type = field
            .get("type")
            .ok_or_else(|| anyhow::anyhow!("V2 account layout field lacks type"))?;
        if field_name == name {
            return Ok(offset);
        }
        offset = offset
            .checked_add(fixed_borsh_type_len(field_type, defined_types, 0)?)
            .ok_or_else(|| anyhow::anyhow!("V2 account layout field offset overflow"))?;
    }
    bail!(
        "V2 {} account layout lacks required field {name}",
        layout.name
    )
}

fn validate_borsh_fields_exact(
    fields: &[Value],
    bytes: &[u8],
    defined_types: &BTreeMap<String, Value>,
) -> Result<()> {
    let mut offset = 0usize;
    for field in fields {
        consume_borsh_type(
            field
                .get("type")
                .ok_or_else(|| anyhow::anyhow!("V2 IDL field lacks type"))?,
            bytes,
            &mut offset,
            defined_types,
            0,
        )?;
    }
    if offset != bytes.len() {
        bail!("V2 Borsh payload has trailing bytes");
    }
    Ok(())
}

fn consume_borsh_type(
    ty: &Value,
    bytes: &[u8],
    offset: &mut usize,
    defined_types: &BTreeMap<String, Value>,
    depth: usize,
) -> Result<()> {
    if depth > MAX_BORSH_RECURSION_DEPTH_V2 {
        bail!("V2 Borsh type recursion exceeds frozen bound");
    }
    if let Some(primitive) = ty.as_str() {
        let length = match primitive {
            "bool" | "u8" | "i8" => 1,
            "u16" | "i16" => 2,
            "u32" | "i32" | "f32" => 4,
            "u64" | "i64" | "f64" => 8,
            "u128" | "i128" => 16,
            "pubkey" => 32,
            "string" => {
                let length = read_u32(bytes, offset)?;
                advance(
                    offset,
                    usize::try_from(length).context("V2 Borsh string length exceeds usize")?,
                    bytes.len(),
                )?;
                return Ok(());
            }
            "bytes" => {
                let length = read_u32(bytes, offset)?;
                advance(
                    offset,
                    usize::try_from(length).context("V2 Borsh bytes length exceeds usize")?,
                    bytes.len(),
                )?;
                return Ok(());
            }
            other => bail!("unsupported V2 Borsh primitive {other:?}"),
        };
        if primitive == "bool" {
            match bytes.get(*offset).copied() {
                Some(0 | 1) => {}
                Some(_) => bail!("V2 Borsh bool has an invalid value"),
                None => bail!("V2 Borsh bool is truncated"),
            }
        }
        return advance(offset, length, bytes.len());
    }
    let object = ty
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("V2 IDL type must be string or object"))?;
    if let Some(array) = object.get("array") {
        let parts = array
            .as_array()
            .filter(|parts| parts.len() == 2)
            .ok_or_else(|| anyhow::anyhow!("V2 Borsh array type is invalid"))?;
        let count = parts[1]
            .as_u64()
            .ok_or_else(|| anyhow::anyhow!("V2 Borsh array length is invalid"))?;
        let count = usize::try_from(count).context("V2 Borsh array length exceeds usize")?;
        if count > MAX_BORSH_COLLECTION_ITEMS_V2 {
            bail!("V2 Borsh array length exceeds frozen bound");
        }
        for _ in 0..count {
            consume_borsh_type(&parts[0], bytes, offset, defined_types, depth + 1)?;
        }
        return Ok(());
    }
    if let Some(element) = object.get("vec") {
        let count = usize::try_from(read_u32(bytes, offset)?)
            .context("V2 Borsh vec length exceeds usize")?;
        if count > MAX_BORSH_COLLECTION_ITEMS_V2 {
            bail!("V2 Borsh vec length exceeds frozen bound");
        }
        for _ in 0..count {
            consume_borsh_type(element, bytes, offset, defined_types, depth + 1)?;
        }
        return Ok(());
    }
    if let Some(element) = object.get("option") {
        match bytes.get(*offset).copied() {
            Some(0) => {
                *offset = offset
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 Borsh option offset overflow"))?;
                return Ok(());
            }
            Some(1) => {
                *offset = offset
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 Borsh option offset overflow"))?;
                return consume_borsh_type(element, bytes, offset, defined_types, depth + 1);
            }
            Some(_) => bail!("V2 Borsh option has an invalid tag"),
            None => bail!("V2 Borsh option is truncated"),
        }
    }
    if let Some(defined) = object.get("defined") {
        let name = defined
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| defined.as_str())
            .ok_or_else(|| anyhow::anyhow!("V2 Borsh defined type lacks name"))?;
        return consume_defined_type(name, bytes, offset, defined_types, depth + 1);
    }
    bail!("unsupported V2 Borsh compound type")
}

fn consume_defined_type(
    name: &str,
    bytes: &[u8],
    offset: &mut usize,
    defined_types: &BTreeMap<String, Value>,
    depth: usize,
) -> Result<()> {
    let definition = defined_types
        .get(name)
        .ok_or_else(|| anyhow::anyhow!("V2 Borsh defined type {name:?} is absent"))?;
    let ty = definition
        .get("type")
        .and_then(Value::as_object)
        .ok_or_else(|| anyhow::anyhow!("V2 Borsh defined type {name:?} is malformed"))?;
    match ty.get("kind").and_then(Value::as_str) {
        Some("struct") => {
            let fields = ty
                .get("fields")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("V2 Borsh struct {name:?} lacks fields"))?;
            for field in fields {
                consume_borsh_type(
                    field.get("type").unwrap_or(field),
                    bytes,
                    offset,
                    defined_types,
                    depth + 1,
                )?;
            }
            Ok(())
        }
        Some("enum") => {
            let tag = bytes
                .get(*offset)
                .copied()
                .ok_or_else(|| anyhow::anyhow!("V2 Borsh enum {name:?} is truncated"))?;
            *offset = offset
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 Borsh enum offset overflow"))?;
            let variants = ty
                .get("variants")
                .and_then(Value::as_array)
                .ok_or_else(|| anyhow::anyhow!("V2 Borsh enum {name:?} lacks variants"))?;
            let variant = variants
                .get(usize::from(tag))
                .ok_or_else(|| anyhow::anyhow!("V2 Borsh enum {name:?} has unknown tag"))?;
            if let Some(fields) = variant.get("fields").and_then(Value::as_array) {
                for field in fields {
                    let type_value = field.get("type").unwrap_or(field);
                    consume_borsh_type(type_value, bytes, offset, defined_types, depth + 1)?;
                }
            }
            Ok(())
        }
        other => bail!("unsupported V2 Borsh defined type kind {other:?}"),
    }
}

fn fixed_borsh_type_len(
    ty: &Value,
    defined_types: &BTreeMap<String, Value>,
    depth: usize,
) -> Result<usize> {
    if depth > MAX_BORSH_RECURSION_DEPTH_V2 {
        bail!("V2 fixed Borsh type recursion exceeds frozen bound");
    }
    if let Some(primitive) = ty.as_str() {
        return match primitive {
            "bool" | "u8" | "i8" => Ok(1),
            "u16" | "i16" => Ok(2),
            "u32" | "i32" | "f32" => Ok(4),
            "u64" | "i64" | "f64" => Ok(8),
            "u128" | "i128" => Ok(16),
            "pubkey" => Ok(32),
            "string" | "bytes" => bail!("V2 account layout has a variable-length primitive"),
            other => bail!("unsupported V2 fixed Borsh primitive {other:?}"),
        };
    }
    let object = ty
        .as_object()
        .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh type must be string or object"))?;
    if let Some(array) = object.get("array") {
        let parts = array
            .as_array()
            .filter(|parts| parts.len() == 2)
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh array is invalid"))?;
        let length = fixed_borsh_type_len(&parts[0], defined_types, depth + 1)?;
        let count = usize::try_from(
            parts[1]
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh array length is invalid"))?,
        )
        .context("V2 fixed Borsh array length exceeds usize")?;
        return length
            .checked_mul(count)
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh array byte length overflows"));
    }
    if let Some(defined) = object.get("defined") {
        let name = defined
            .get("name")
            .and_then(Value::as_str)
            .or_else(|| defined.as_str())
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh defined type lacks name"))?;
        let definition = defined_types
            .get(name)
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh type {name:?} is absent"))?;
        let type_value = definition
            .get("type")
            .and_then(Value::as_object)
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh type {name:?} is malformed"))?;
        if type_value.get("kind").and_then(Value::as_str) != Some("struct") {
            bail!("V2 account layout defined type {name:?} is not fixed struct");
        }
        return type_value
            .get("fields")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh struct {name:?} lacks fields"))?
            .iter()
            .try_fold(0usize, |total, field| {
                fixed_borsh_type_len(field.get("type").unwrap_or(field), defined_types, depth + 1)
                    .and_then(|length| {
                        total
                            .checked_add(length)
                            .ok_or_else(|| anyhow::anyhow!("V2 fixed Borsh struct length overflow"))
                    })
            });
    }
    bail!("V2 account layout uses a variable-length or unsupported type")
}

fn read_u32(bytes: &[u8], offset: &mut usize) -> Result<u32> {
    let slice: [u8; 4] = bytes
        .get(*offset..offset.saturating_add(4))
        .ok_or_else(|| anyhow::anyhow!("V2 Borsh length prefix is truncated"))?
        .try_into()
        .map_err(|_| anyhow::anyhow!("V2 Borsh length prefix has wrong width"))?;
    *offset = offset
        .checked_add(4)
        .ok_or_else(|| anyhow::anyhow!("V2 Borsh offset overflow"))?;
    Ok(u32::from_le_bytes(slice))
}

fn advance(offset: &mut usize, length: usize, total: usize) -> Result<()> {
    let next = offset
        .checked_add(length)
        .ok_or_else(|| anyhow::anyhow!("V2 Borsh offset overflow"))?;
    if next > total {
        bail!("V2 Borsh payload is truncated");
    }
    *offset = next;
    Ok(())
}

fn idl_defined_types(idl: &Value) -> Result<BTreeMap<String, Value>> {
    idl_array(idl, "types", "vendored V2 Pump IDL")?
        .iter()
        .map(|value| {
            Ok((
                required_string(value, "name", "V2 Pump IDL type")?.to_owned(),
                value.clone(),
            ))
        })
        .collect()
}

fn idl_instruction_accounts(
    instruction: &Value,
) -> Result<Vec<PumpExactStateAccountMetaContractV2>> {
    idl_array(instruction, "accounts", "V2 Pump instruction")?
        .iter()
        .map(|account| {
            let name = required_string(account, "name", "V2 Pump instruction account")?.to_owned();
            let writable = account
                .get("writable")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let signer = account
                .get("signer")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let address = account
                .get("address")
                .and_then(Value::as_str)
                .map(Pubkey::from_str)
                .transpose()
                .with_context(|| format!("parse V2 Pump instruction static account {name}"))?;
            Ok(PumpExactStateAccountMetaContractV2 {
                name,
                writable,
                signer,
                address,
            })
        })
        .collect()
}

fn idl_event_fields(event: &Value, types: &BTreeMap<String, Value>) -> Result<Vec<Value>> {
    let name = required_string(event, "name", "V2 Pump event")?;
    types
        .get(name)
        .and_then(|value| value.get("type"))
        .and_then(|value| value.get("fields"))
        .and_then(Value::as_array)
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("vendored V2 Pump event {name} lacks type fields"))
}

fn idl_array<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a Vec<Value>> {
    value
        .get(field)
        .and_then(Value::as_array)
        .ok_or_else(|| anyhow::anyhow!("{label} lacks array {field}"))
}

fn required_string<'a>(value: &'a Value, field: &str, label: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(Value::as_str)
        .filter(|value| !value.trim().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{label} lacks non-empty string {field}"))
}

fn idl_discriminator(value: &Value, label: &str) -> Result<[u8; 8]> {
    let values = value
        .get("discriminator")
        .and_then(Value::as_array)
        .filter(|values| values.len() == 8)
        .ok_or_else(|| anyhow::anyhow!("{label} lacks an eight-byte discriminator"))?;
    let mut discriminator = [0u8; 8];
    for (index, value) in values.iter().enumerate() {
        discriminator[index] = u8::try_from(
            value
                .as_u64()
                .ok_or_else(|| anyhow::anyhow!("{label} discriminator byte is invalid"))?,
        )
        .context("V2 Pump IDL discriminator byte exceeds u8")?;
    }
    Ok(discriminator)
}

fn relative_manifest_path(value: &str) -> Result<PathBuf> {
    let path = PathBuf::from(value);
    if path.is_absolute()
        || path.as_os_str().is_empty()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("V2 semantics vendored IDL path must be a safe relative path");
    }
    Ok(path)
}

fn validate_nonempty(label: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim() != value {
        bail!("V2 semantics {label} must be non-empty and trimmed");
    }
    Ok(())
}

fn validate_https_source_repository(value: &str) -> Result<()> {
    validate_nonempty("source_repository", value)?;
    let without_scheme = value
        .strip_prefix("https://")
        .ok_or_else(|| anyhow::anyhow!("V2 semantics source_repository must use HTTPS"))?;
    if without_scheme.is_empty()
        || without_scheme.starts_with('/')
        || without_scheme.contains('?')
        || without_scheme.contains('#')
        || without_scheme.split('/').next().is_none_or(str::is_empty)
    {
        bail!("V2 semantics source_repository must be a root HTTPS repository URL");
    }
    Ok(())
}

fn validate_commit(value: &str) -> Result<()> {
    if value.len() != 40 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("V2 semantics source_commit must be 40 lowercase/uppercase hex bytes");
    }
    Ok(())
}

fn parse_hex_32(label: &str, value: &str) -> Result<[u8; 32]> {
    let bytes = decode_hex(value).with_context(|| format!("decode {label}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} must contain exactly 32 bytes"))
}

fn parse_hex_8(label: &str, value: &str) -> Result<[u8; 8]> {
    let bytes = decode_hex(value).with_context(|| format!("decode {label}"))?;
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("{label} must contain exactly 8 bytes"))
}

fn decode_hex(value: &str) -> Result<Vec<u8>> {
    if value.len() % 2 != 0 || value.is_empty() {
        bail!("hex string must have a non-zero even length");
    }
    (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).context("invalid hex byte"))
        .collect()
}

/// Serialize a JSON value in a deterministic, map-order-independent form for
/// field-layout authority.  The public IDL file digest already pins its exact
/// bytes; this second digest makes the selected `type.fields` contract
/// explicit and insensitive to unrelated whitespace or object-key order.
fn canonical_json_bytes(value: &Value) -> Result<Vec<u8>> {
    fn append(value: &Value, output: &mut String) -> Result<()> {
        match value {
            Value::Null => output.push_str("null"),
            Value::Bool(value) => output.push_str(if *value { "true" } else { "false" }),
            Value::Number(value) => output.push_str(&value.to_string()),
            Value::String(value) => output
                .push_str(&serde_json::to_string(value).context("encode canonical JSON string")?),
            Value::Array(values) => {
                output.push('[');
                for (index, value) in values.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    append(value, output)?;
                }
                output.push(']');
            }
            Value::Object(values) => {
                output.push('{');
                let mut keys = values.keys().collect::<Vec<_>>();
                keys.sort_unstable();
                for (index, key) in keys.into_iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(
                        &serde_json::to_string(key).context("encode canonical JSON object key")?,
                    );
                    output.push(':');
                    append(
                        values
                            .get(key)
                            .ok_or_else(|| anyhow::anyhow!("canonical JSON key disappeared"))?,
                        output,
                    )?;
                }
                output.push('}');
            }
        }
        Ok(())
    }

    let mut output = String::new();
    append(value, &mut output)?;
    Ok(output.into_bytes())
}

fn digest_bytes(bytes: &[u8]) -> PumpExactStateSemanticsDigestV2 {
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    let blake3 = blake3::hash(bytes);
    PumpExactStateSemanticsDigestV2 {
        sha256: hex_bytes(&sha256),
        blake3: hex_bytes(blake3.as_bytes()),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    }
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

fn read_regular_file_exact_v2(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>> {
    #[cfg(unix)]
    let file = {
        let before = fs::symlink_metadata(path)
            .with_context(|| format!("inspect {label} {}", path.display()))?;
        if before.file_type().is_symlink() || !before.is_file() {
            bail!(
                "{label} {} must be a regular non-symlink file",
                path.display()
            );
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        let file = options
            .open(path)
            .with_context(|| format!("open {label} {}", path.display()))?;
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            bail!("opened {label} {} is not regular", path.display());
        }
        file
    };
    #[cfg(not(unix))]
    let file = { bail!("V2 semantics authority requires Unix no-follow regular-file reads") };
    let expected_bytes = file.metadata()?.len();
    if expected_bytes > max_bytes {
        bail!("{label} {} exceeds {} bytes", path.display(), max_bytes);
    }
    let expected_len =
        usize::try_from(expected_bytes).context("authority file length exceeds usize")?;
    let mut reader = file;
    reader.seek(SeekFrom::Start(0))?;
    let mut bytes = vec![0u8; expected_len];
    reader
        .read_exact(&mut bytes)
        .with_context(|| format!("read {label} {}", path.display()))?;
    let metadata = reader.metadata()?;
    if !metadata.is_file() || metadata.len() != expected_bytes {
        bail!("{label} {} changed during bounded read", path.display());
    }
    Ok(bytes)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generic_borsh_validator_rejects_trailing_or_invalid_bool() {
        let fields = vec![serde_json::json!({"name":"flag", "type":"bool"})];
        let types = BTreeMap::new();
        assert!(validate_borsh_fields_exact(&fields, &[1], &types).is_ok());
        assert!(validate_borsh_fields_exact(&fields, &[2], &types).is_err());
        assert!(validate_borsh_fields_exact(&fields, &[0, 0], &types).is_err());
    }

    #[test]
    fn relative_vendor_path_rejects_parent_escape() {
        assert!(relative_manifest_path("../pump.json").is_err());
        assert!(relative_manifest_path("idl/pump.json").is_ok());
    }

    #[test]
    fn writable_state_role_invariant_covers_anchor_role_spelling() {
        assert!(is_global_account_role_v2("global"));
        assert!(is_global_account_role_v2("Global"));
        assert!(is_bonding_curve_account_role_v2("bonding_curve"));
        assert!(is_bonding_curve_account_role_v2("bondingCurve"));
        assert!(
            validate_instruction_effect_account_invariants_v2(
                "camel_case_curve",
                PumpExactStateInstructionEffectV2::ProvenNonReserve,
                &[PumpExactStateAccountMetaContractV2 {
                    name: "bondingCurve".to_owned(),
                    writable: true,
                    signer: false,
                    address: None,
                }],
            )
            .is_err(),
            "a writable lower-camel BondingCurve role may not shrink the denominator"
        );
    }

    #[test]
    fn exact_effect_classes_are_closed_over_reviewed_instruction_families() {
        let writable_curve = [PumpExactStateAccountMetaContractV2 {
            name: "bonding_curve".to_owned(),
            writable: true,
            signer: false,
            address: None,
        }];
        assert!(
            validate_instruction_effect_account_invariants_v2(
                "migrate",
                PumpExactStateInstructionEffectV2::SupportedExactTrade,
                &writable_curve,
            )
            .is_err(),
            "migrate must remain a typed unsupported/dependency candidate"
        );
        assert!(
            validate_instruction_effect_account_invariants_v2(
                "set_creator",
                PumpExactStateInstructionEffectV2::SupportedExactCreate,
                &writable_curve,
            )
            .is_err(),
            "set_creator must never manufacture an exact birth"
        );
        assert!(
            validate_instruction_effect_account_invariants_v2(
                "buy_v2",
                PumpExactStateInstructionEffectV2::SupportedExactTrade,
                &writable_curve,
            )
            .is_ok(),
            "reviewed exact trade family must remain usable"
        );
        assert!(
            validate_instruction_effect_account_invariants_v2(
                "create_v2",
                PumpExactStateInstructionEffectV2::SupportedExactCreate,
                &writable_curve,
            )
            .is_ok(),
            "reviewed exact create family must remain usable"
        );
    }

    #[test]
    fn real_vendored_pump_manifest_accepts_dynamic_trade_fields_as_strict_decode_only() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root from seer manifest directory");
        let manifest_path =
            repository_root.join("configs/research/pump_exact_state_semantics_manifest_v2.json");
        let authority = load_pump_exact_state_semantics_authority_v2(&manifest_path)
            .expect("real vendored Pump semantics manifest must load");
        assert_eq!(
            authority.semantics_id,
            "pump_public_docs_3c6721a67c0b206b39130b454c8ba22a83ce972e_v8"
        );

        let idl_path = repository_root.join(
            "configs/research/vendor/pump-public-docs/3c6721a67c0b206b39130b454c8ba22a83ce972e/idl/pump.json",
        );
        let idl: Value = serde_json::from_slice(
            &fs::read(&idl_path).expect("read checked-in vendored Pump IDL"),
        )
        .expect("decode checked-in vendored Pump IDL");
        let trade_event_idl = idl_array(&idl, "events", "real vendored Pump IDL")
            .expect("real Pump events")
            .iter()
            .find(|event| event["name"] == "TradeEvent")
            .expect("real Pump TradeEvent");
        let trade_event_discriminator = idl_discriminator(trade_event_idl, "real Pump TradeEvent")
            .expect("event discriminator");
        let trade_event = authority
            .event(&trade_event_discriminator)
            .expect("loaded TradeEvent contract");
        let buy_instruction = idl_array(&idl, "instructions", "real vendored Pump IDL")
            .expect("real Pump instructions")
            .iter()
            .find(|instruction| instruction["name"] == "buy")
            .expect("real Pump buy instruction");
        let buy_discriminator = idl_discriminator(buy_instruction, "real Pump buy instruction")
            .expect("buy discriminator");
        let buy_parent = trade_event
            .parent_contracts
            .get(&buy_discriminator)
            .expect("real TradeEvent buy parent contract");

        let trade_field_count = idl_event_fields(
            trade_event_idl,
            &idl_defined_types(&idl).expect("real Pump defined types"),
        )
        .expect("real TradeEvent fields")
        .len();
        assert_eq!(
            buy_parent.field_bindings.len(),
            trade_field_count,
            "each checked-in TradeEvent field must have exactly one semantic disposition"
        );
        let sources_by_field = buy_parent
            .field_bindings
            .iter()
            .map(|binding| (binding.event_field.as_str(), &binding.source))
            .collect::<BTreeMap<_, _>>();
        assert!(matches!(
            sources_by_field.get("timestamp"),
            Some(PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        ));
        assert!(matches!(
            sources_by_field.get("current_sol_volume"),
            Some(PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        ));
        assert!(matches!(
            sources_by_field.get("shareholders"),
            Some(PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        ));
        assert!(matches!(
            sources_by_field.get("mint"),
            Some(PumpExactStateEventFieldSourceV2::ParentAccountRole(role)) if role == "mint"
        ));
        assert!(matches!(
            sources_by_field.get("virtual_quote_reserves"),
            Some(PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        ));
        assert!(matches!(
            sources_by_field.get("real_quote_reserves"),
            Some(PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        ));
        assert_eq!(
            authority
                .instruction(&buy_discriminator)
                .expect("loaded buy instruction")
                .effect,
            PumpExactStateInstructionEffectV2::SupportedExactTrade
        );
    }

    #[test]
    fn real_vendored_quote_regime_reserves_are_strict_decode_only_until_variant_contract_exists() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root from seer manifest directory");
        let manifest_path =
            repository_root.join("configs/research/pump_exact_state_semantics_manifest_v2.json");
        let authority = load_pump_exact_state_semantics_authority_v2(&manifest_path)
            .expect("real vendored Pump semantics manifest must load");

        let assert_source =
            |event_name: &str,
             parent_name: &str,
             field_name: &str,
             expected: fn(&PumpExactStateEventFieldSourceV2) -> bool| {
                let (event_discriminator, parent_discriminator) = match (event_name, parent_name) {
                    ("TradeEvent", "buy") => (
                        [189, 219, 127, 211, 78, 230, 97, 238],
                        [102, 6, 61, 18, 1, 218, 235, 234],
                    ),
                    ("TradeEvent", "buy_v2") => (
                        [189, 219, 127, 211, 78, 230, 97, 238],
                        [184, 23, 238, 97, 103, 197, 211, 61],
                    ),
                    ("CreateEvent", "create") => (
                        [27, 114, 169, 77, 222, 235, 99, 118],
                        [24, 30, 200, 40, 5, 28, 7, 119],
                    ),
                    ("CreateEvent", "create_v2") => (
                        [27, 114, 169, 77, 222, 235, 99, 118],
                        [214, 144, 76, 236, 95, 139, 49, 180],
                    ),
                    _ => panic!(
                        "fixture has no pinned discriminator pair for {event_name}/{parent_name}"
                    ),
                };
                let parent = authority
                    .instruction(&parent_discriminator)
                    .expect("pinned parent instruction");
                let event = authority
                    .event(&event_discriminator)
                    .expect("pinned event contract");
                let binding = event
                    .parent_contracts
                    .get(&parent.discriminator)
                    .expect("pinned event/parent contract")
                    .field_bindings
                    .iter()
                    .find(|binding| binding.event_field == field_name)
                    .expect("every vendored event field has one disposition");
                assert!(
                    expected(&binding.source),
                    "{event_name}/{parent_name} field {field_name} has unexpected source {:?}",
                    binding.source
                );
            };
        let strict = |source: &PumpExactStateEventFieldSourceV2| {
            matches!(source, PumpExactStateEventFieldSourceV2::StrictDecodeOnly)
        };
        let canonical_token_reserve = |source: &PumpExactStateEventFieldSourceV2| {
            matches!(
                source,
                PumpExactStateEventFieldSourceV2::FinalCurveStateField(field)
                    if field == "virtual_token_reserves" || field == "real_token_reserves"
            )
        };

        // `buy` represents the legacy/native-SOL family and `buy_v2` the
        // token-quote family. Neither may smuggle a quote-regime dependent
        // event reserve into exact-state authority until an explicit,
        // variant-proven contract exists.
        for parent in ["buy", "buy_v2"] {
            assert_source("TradeEvent", parent, "virtual_quote_reserves", strict);
            assert_source("TradeEvent", parent, "real_quote_reserves", strict);
            assert_source(
                "TradeEvent",
                parent,
                "virtual_token_reserves",
                canonical_token_reserve,
            );
            assert_source(
                "TradeEvent",
                parent,
                "real_token_reserves",
                canonical_token_reserve,
            );
        }
        for parent in ["create", "create_v2"] {
            assert_source("CreateEvent", parent, "virtual_sol_reserves", strict);
            assert_source("CreateEvent", parent, "virtual_quote_reserves", strict);
            assert_source(
                "CreateEvent",
                parent,
                "virtual_token_reserves",
                canonical_token_reserve,
            );
            assert_source(
                "CreateEvent",
                parent,
                "real_token_reserves",
                canonical_token_reserve,
            );
        }
    }

    #[test]
    fn real_vendored_option_bool_instruction_argument_is_decoded_exactly() {
        let repository_root = Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root from seer manifest directory");
        let manifest_path =
            repository_root.join("configs/research/pump_exact_state_semantics_manifest_v2.json");
        let authority = load_pump_exact_state_semantics_authority_v2(&manifest_path)
            .expect("real vendored Pump semantics manifest must load");
        let buy = authority
            .instruction(&[102, 6, 61, 18, 1, 218, 235, 234])
            .expect("real vendored buy contract");
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&2u64.to_le_bytes());
        payload.push(0); // Pump OptionBool is a one-field struct with shorthand "bool".
        assert!(
            authority
                .validate_instruction_payload(buy, &payload)
                .is_ok(),
            "the real vendored OptionBool shorthand must not falsely taint buy inventory"
        );
        payload.push(0);
        assert!(
            authority
                .validate_instruction_payload(buy, &payload)
                .is_err(),
            "a correctly decoded OptionBool still may not hide trailing bytes"
        );
        payload.pop();
        payload[16] = 2;
        assert!(
            authority
                .validate_instruction_payload(buy, &payload)
                .is_err(),
            "invalid bool values remain fail-closed"
        );
    }

    #[test]
    fn canonical_field_layout_digest_is_independent_of_object_key_order() {
        let left = serde_json::json!([
            {"name":"reserve", "type":"u64", "docs":["fixture"]}
        ]);
        let right = serde_json::json!([
            {"docs":["fixture"], "type":"u64", "name":"reserve"}
        ]);
        assert_eq!(
            canonical_json_bytes(&left).expect("left canonical JSON"),
            canonical_json_bytes(&right).expect("right canonical JSON")
        );
    }

    #[test]
    fn semantics_manifest_requires_exact_idl_coverage_and_selected_account_layout_digests() {
        let temporary = tempfile::tempdir().expect("temporary semantics root");
        let idl_path = temporary.path().join("pump.json");
        let curve_fields = serde_json::json!([
            {"name":"virtual_token_reserves","type":"u64"},
            {"name":"virtual_quote_reserves","type":"u64"},
            {"name":"real_token_reserves","type":"u64"},
            {"name":"real_quote_reserves","type":"u64"},
            {"name":"token_total_supply","type":"u64"},
            {"name":"complete","type":"bool"},
            {"name":"creator","type":"pubkey"},
            {"name":"is_mayhem_mode","type":"bool"},
            {"name":"is_cashback_coin","type":"bool"},
            {"name":"quote_mint","type":"pubkey"}
        ]);
        let global_fields = serde_json::json!([]);
        let trade_event_fields = serde_json::json!([
            {"name":"mint","type":"pubkey"},
            {"name":"user","type":"pubkey"},
            {"name":"is_buy","type":"bool"},
            {"name":"ix_name","type":"string"},
            {"name":"quote_mint","type":"pubkey"},
            {"name":"virtual_token_reserves","type":"u64"},
            {"name":"real_token_reserves","type":"u64"},
            {"name":"virtual_quote_reserves","type":"u64"},
            {"name":"real_quote_reserves","type":"u64"},
            {"name":"timestamp","type":"i64"}
        ]);
        let idl = serde_json::json!({
            "address":"11111111111111111111111111111111",
            "instructions":[{
                "name":"buy",
                "discriminator":[1,2,3,4,5,6,7,8],
                "accounts":[{
                    "name":"bonding_curve",
                    "writable":true,
                    "signer":false
                },{
                    "name":"mint",
                    "writable":false,
                    "signer":false
                },{
                    "name":"user",
                    "writable":false,
                    "signer":true
                },{
                    "name":"quote_mint",
                    "writable":false,
                    "signer":false
                }],
                "args":[]
            }],
            "events":[{
                "name":"TradeEvent",
                "discriminator":[6,6,6,6,6,6,6,6]
            }],
            "accounts":[
                {"name":"BondingCurve","discriminator":[8,7,6,5,4,3,2,1]},
                {"name":"Global","discriminator":[9,9,9,9,9,9,9,9]}
            ],
            "types":[
                {"name":"BondingCurve","type":{"kind":"struct","fields":curve_fields}},
                {"name":"Global","type":{"kind":"struct","fields":global_fields}},
                {"name":"TradeEvent","type":{"kind":"struct","fields":trade_event_fields}}
            ]
        });
        let idl_bytes = serde_json::to_vec(&idl).expect("IDL bytes");
        fs::write(&idl_path, &idl_bytes).expect("write IDL");
        let idl_digest = digest_bytes(&idl_bytes);
        let curve_digest = digest_bytes(
            &canonical_json_bytes(
                idl["types"]
                    .as_array()
                    .expect("types")
                    .iter()
                    .find(|value| value["name"] == "BondingCurve")
                    .expect("curve")
                    .get("type")
                    .and_then(|value| value.get("fields"))
                    .expect("curve fields"),
            )
            .expect("canonical curve fields"),
        );
        let global_digest = digest_bytes(
            &canonical_json_bytes(
                idl["types"]
                    .as_array()
                    .expect("types")
                    .iter()
                    .find(|value| value["name"] == "Global")
                    .expect("global")
                    .get("type")
                    .and_then(|value| value.get("fields"))
                    .expect("global fields"),
            )
            .expect("canonical global fields"),
        );
        let manifest = serde_json::json!({
            "schema_version":7,
            "semantics_id":"fixture_v2",
            "source_repository":"https://example.invalid/pump",
            "source_commit":"0123456789abcdef0123456789abcdef01234567",
            "vendored_idl_relative_path":"pump.json",
            "vendored_idl_sha256":idl_digest.sha256,
            "vendored_idl_blake3":idl_digest.blake3,
            "program_id":"11111111111111111111111111111111",
            "program_data_hash_blake3":"00".repeat(32),
            "instruction_effects":{"buy":"supported_exact_trade"},
            "instruction_state_roles":{
                "buy":{"bonding_curve_role":"bonding_curve","mint_role":"mint"}
            },
            "event_parent_contracts":{
                "TradeEvent":[{
                    "parent_instruction":"buy",
                    "strict_decode_only_unbound_fields":true,
                    "field_bindings":[
                        {"event_field":"mint","parent_account_role":"mint"},
                        {"event_field":"user","parent_account_role":"user"},
                        {"event_field":"is_buy","expected_borsh_hex":"01"},
                        {"event_field":"ix_name","expected_borsh_hex":"03000000627579"},
                        {"event_field":"quote_mint","final_curve_state_field":"quote_mint"},
                        {"event_field":"virtual_token_reserves","final_curve_state_field":"virtual_token_reserves"},
                        {"event_field":"real_token_reserves","final_curve_state_field":"real_token_reserves"}
                    ]
                }]
            },
            "account_classes":{
                "BondingCurve":"exact_bonding_curve",
                "Global":"known_global_dependency"
            },
            "account_layouts":[
                {
                    "account":"BondingCurve",
                    "discriminator":"0807060504030201",
                    "allowed_serialized_bytes":[115],
                    "prefix_bytes":115,
                    "field_layout_sha256":curve_digest.sha256,
                    "field_layout_blake3":curve_digest.blake3
                },
                {
                    "account":"Global",
                    "discriminator":"0909090909090909",
                    "allowed_serialized_bytes":[8],
                    "prefix_bytes":8,
                    "field_layout_sha256":global_digest.sha256,
                    "field_layout_blake3":global_digest.blake3
                }
            ]
        });
        let manifest_path = temporary.path().join("semantics.json");
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("manifest bytes"),
        )
        .expect("write manifest");
        let authority = load_pump_exact_state_semantics_authority_v2(&manifest_path)
            .expect("complete manifest authority");
        let event = authority.event(&[6; 8]).expect("fixture event contract");
        let parent = PumpExactStateInstructionSemanticEvidenceV2 {
            discriminator: [1, 2, 3, 4, 5, 6, 7, 8],
            account_roles: BTreeMap::from([
                ("bonding_curve".to_owned(), Pubkey::new_from_array([8; 32])),
                ("mint".to_owned(), Pubkey::new_from_array([7; 32])),
                ("user".to_owned(), Pubkey::new_from_array([6; 32])),
                ("quote_mint".to_owned(), Pubkey::new_from_array([5; 32])),
            ]),
            argument_fields: BTreeMap::new(),
        };
        let trade = authority
            .instruction(&[1, 2, 3, 4, 5, 6, 7, 8])
            .expect("fixture trade contract");
        assert_eq!(
            authority
                .exact_state_account_pubkeys(trade, &parent.account_roles)
                .expect("pinned exact state roles"),
            (
                Some(Pubkey::new_from_array([8; 32])),
                Some(Pubkey::new_from_array([7; 32]))
            ),
            "exact state identity must originate from the manifest-selected roles"
        );
        let event_fields = BTreeMap::from([
            ("mint".to_owned(), vec![7; 32]),
            ("user".to_owned(), vec![6; 32]),
            ("is_buy".to_owned(), vec![1]),
            ("ix_name".to_owned(), vec![3, 0, 0, 0, b'b', b'u', b'y']),
            ("quote_mint".to_owned(), vec![5; 32]),
            (
                "virtual_token_reserves".to_owned(),
                41u64.to_le_bytes().to_vec(),
            ),
            (
                "real_token_reserves".to_owned(),
                40u64.to_le_bytes().to_vec(),
            ),
            (
                "virtual_quote_reserves".to_owned(),
                42u64.to_le_bytes().to_vec(),
            ),
            (
                "real_quote_reserves".to_owned(),
                39u64.to_le_bytes().to_vec(),
            ),
            ("timestamp".to_owned(), 17i64.to_le_bytes().to_vec()),
        ]);
        assert_eq!(
            authority
                .validate_event_parent_semantics(event, &event_fields, &parent)
                .expect("strict parent/event relation"),
            vec![
                PumpExactStateEventFinalStateBindingV2 {
                    curve_state_field: "quote_mint".to_owned(),
                    event_value_borsh: vec![5; 32],
                },
                PumpExactStateEventFinalStateBindingV2 {
                    curve_state_field: "virtual_token_reserves".to_owned(),
                    event_value_borsh: 41u64.to_le_bytes().to_vec(),
                },
                PumpExactStateEventFinalStateBindingV2 {
                    curve_state_field: "real_token_reserves".to_owned(),
                    event_value_borsh: 40u64.to_le_bytes().to_vec(),
                },
            ]
        );
        let mut mismatched_event_fields = event_fields.clone();
        mismatched_event_fields.insert("mint".to_owned(), vec![9; 32]);
        assert!(
            authority
                .validate_event_parent_semantics(event, &mismatched_event_fields, &parent)
                .is_err(),
            "event identity mismatch must be fail-closed"
        );
        let wrong_parent = PumpExactStateInstructionSemanticEvidenceV2 {
            discriminator: [8; 8],
            ..parent.clone()
        };
        assert!(
            authority
                .validate_event_parent_semantics(event, &event_fields, &wrong_parent)
                .is_err(),
            "a strict event may not attach to a different known Pump parent variant"
        );

        let mut incomplete_event_contract = manifest.clone();
        incomplete_event_contract["event_parent_contracts"]["TradeEvent"][0]["field_bindings"] = serde_json::json!([
            {"event_field":"mint","parent_account_role":"mint"}
        ]);
        fs::write(
            &manifest_path,
            serde_json::to_vec(&incomplete_event_contract)
                .expect("incomplete event manifest bytes"),
        )
        .expect("write incomplete event contract");
        assert!(
            load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err(),
            "a semantics manifest may not omit one event field from strict parent binding"
        );
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("restore complete manifest bytes"),
        )
        .expect("restore complete event contract");

        let mut missing_state_roles = manifest.clone();
        missing_state_roles["instruction_state_roles"] = serde_json::json!({});
        fs::write(
            &manifest_path,
            serde_json::to_vec(&missing_state_roles).expect("missing state-role manifest bytes"),
        )
        .expect("write missing state-role manifest");
        assert!(
            load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err(),
            "a supported exact instruction may not fall back when its state roles are absent"
        );
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("restore complete state-role manifest bytes"),
        )
        .expect("restore complete state-role manifest");

        let mut invalid_curve_effect = manifest.clone();
        invalid_curve_effect["instruction_effects"]["buy"] =
            Value::String("proven_non_reserve".to_owned());
        fs::write(
            &manifest_path,
            serde_json::to_vec(&invalid_curve_effect).expect("invalid curve manifest bytes"),
        )
        .expect("write invalid curve manifest");
        assert!(
            load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err(),
            "a writable BondingCurve may not disappear into ProvenNonReserve"
        );
        fs::write(
            &manifest_path,
            serde_json::to_vec(&manifest).expect("restore complete manifest bytes"),
        )
        .expect("restore complete curve manifest");

        let mut duplicate_layout = manifest.clone();
        let duplicate = duplicate_layout["account_layouts"]
            .as_array()
            .expect("layout array")
            .first()
            .expect("BondingCurve layout")
            .clone();
        duplicate_layout["account_layouts"]
            .as_array_mut()
            .expect("mutable layout array")
            .push(duplicate);
        fs::write(
            &manifest_path,
            serde_json::to_vec(&duplicate_layout).expect("duplicate manifest bytes"),
        )
        .expect("write duplicate-layout manifest");
        assert!(load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err());

        let mut global_mutation = idl.clone();
        global_mutation["instructions"]
            .as_array_mut()
            .expect("fixture instruction array")
            .push(serde_json::json!({
                "name":"set_params",
                "discriminator":[2,2,3,4,5,6,7,8],
                "accounts":[{
                    "name":"global",
                    "writable":true,
                    "signer":false
                }],
                "args":[]
            }));
        let global_idl_bytes = serde_json::to_vec(&global_mutation).expect("global IDL bytes");
        fs::write(&idl_path, &global_idl_bytes).expect("write global mutation IDL");
        let global_idl_digest = digest_bytes(&global_idl_bytes);
        let mut invalid_global_effect = manifest.clone();
        invalid_global_effect["vendored_idl_sha256"] = Value::String(global_idl_digest.sha256);
        invalid_global_effect["vendored_idl_blake3"] = Value::String(global_idl_digest.blake3);
        invalid_global_effect["instruction_effects"] = serde_json::json!({
            "buy":"supported_exact_trade",
            "set_params":"proven_non_reserve"
        });
        fs::write(
            &manifest_path,
            serde_json::to_vec(&invalid_global_effect).expect("invalid global manifest bytes"),
        )
        .expect("write invalid global manifest");
        assert!(
            load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err(),
            "writable Global may not be relabelled proven_non_reserve"
        );

        invalid_global_effect["instruction_effects"]["set_params"] =
            Value::String("global_dependency_mutation".to_owned());
        fs::write(
            &manifest_path,
            serde_json::to_vec(&invalid_global_effect).expect("valid global manifest bytes"),
        )
        .expect("write valid global manifest");
        load_pump_exact_state_semantics_authority_v2(&manifest_path)
            .expect("writable Global is a dependency candidate");

        let mut missing_instruction = manifest;
        missing_instruction["instruction_effects"] = serde_json::json!({});
        fs::write(
            &manifest_path,
            serde_json::to_vec(&missing_instruction).expect("bad manifest bytes"),
        )
        .expect("write bad manifest");
        assert!(load_pump_exact_state_semantics_authority_v2(&manifest_path).is_err());
    }
}
