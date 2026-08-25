//! Bounded read-only feasibility census over the preserved PRXTAPE3 raw.
//!
//! This module is a test-gated census harness, deliberately outside the
//! production qualification path.  It replays the exact same inventory,
//! scope, anchor and Event-CPI machinery as the materializer and then asks,
//! per preserved multi-mutation transaction and per inventory-incomplete
//! candidate, whether the preserved evidence is sufficient for a
//! deterministic transaction-local exact-state reconstruction under the
//! strict authority contract:
//!
//! * state may only arise from exact pre-anchors, exact Create genesis,
//!   strict manifest-bound Event-CPI fields, the versioned hash-pinned
//!   reserve transition rule, or the final same-signature account anchor;
//! * StrictDecodeOnly event fields and compatibility suffix bytes carry no
//!   state authority;
//! * every proposed reconstruction must bit-exactly reproduce the final
//!   same-signature anchor on all canonical curve-state fields.
//!
//! The census never writes to the raw or to any exact-output directory.  It
//! writes one deterministic JSON report to `/tmp` and prints the
//! feasibility-gate arithmetic.  Run explicitly:
//!
//! ```text
//! cargo test --locked --offline -p seer --lib -- --ignored \
//!     feasibility_census_v2
//! ```

use super::*;
use crate::research_exact_tape_v2_semantics::PumpExactStateInstructionContractV2;
use ghost_core::pump_quote::{
    quote_exact_base_in_sell, quote_exact_base_out, quote_exact_quote_in, FeeRounding,
    ProgramFeeRule, ProgramFeeSchedule, ProgramFeeScheduleEvidenceV1, PumpReserveState,
    PumpRouteVariant,
};
use serde::Serialize;
use std::fs::OpenOptions;
use std::io::{BufRead, Write};
#[cfg(unix)]
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};

const CENSUS_RAW_DIR: &str =
    "/protected/research/pump-exact-state-v2-runs/pump-exact-state-v2-1787539185686-2720125/raw-v2";
const CENSUS_EXPECTED_START_MANIFEST_SHA256: &str =
    "c1bff463888475786fc012fc3c3414f37de8c63c8ef499fe631b307e941a4045";
const CENSUS_EXPECTED_COMPLETION_RECEIPT_SHA256: &str =
    "f50c86538ca2229bb772ebe412f7920b36483d299aa92a967213599207a87ea3";
/// Baseline exact output whose receipt binds every census baseline counter.
const CENSUS_BASELINE_EXACT_DIR: &str =
    "/protected/research/exact-v2/pump-exact-state-v2-1787539185686-2720125-621b693-legacy-buy-compat";
const CENSUS_BASELINE_RECEIPT_SHA256: &str =
    "43737a29d3a194571e65b6a3b5d6a41767079755aac6c21a7b9c71a7ff092d74";
/// Expected residual population under the retired blanket rules, taken from
/// the receipt-bound coverage artifact: multi-mutation rejections plus
/// inventory-incomplete rejections.  The census must classify exactly this
/// population, each candidate exactly once.
const CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES: u64 = 402;
const CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES: u64 = 163;
const CENSUS_SCHEMA_VERSION: u16 = 4;
const CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES: u128 = 4_096;
const CENSUS_SOURCE_BYTES: &[u8] = include_bytes!("feasibility_census_v2.rs");

fn census_report_path(run_id: &str) -> PathBuf {
    // Create-new publication: a deterministic name bound to the raw run id,
    // never overwriting an existing report.
    PathBuf::from(format!(
        "/tmp/pump-v2-multi-mutation-feasibility-census-{run_id}.json"
    ))
}

/// Open a census report only once and with owner-private permissions. The
/// report carries raw-derived locators and anchor evidence, so its privacy
/// must not depend on the caller's process umask.
fn create_private_census_report(path: &std::path::Path) -> std::io::Result<std::fs::File> {
    let mut options = OpenOptions::new();
    options.create_new(true).write(true);
    #[cfg(unix)]
    options.mode(0o600);
    options.open(path)
}

#[cfg(unix)]
#[test]
fn census_report_publication_is_private_and_create_new() {
    let temporary = tempfile::tempdir().expect("temporary census report root");
    let report_path = temporary.path().join("census.json");
    let mut report = create_private_census_report(&report_path).expect("create private report");
    report.write_all(b"{}\n").expect("write report");
    report.sync_all().expect("sync report");

    assert_eq!(
        PermissionsExt::mode(
            &std::fs::metadata(&report_path)
                .expect("report metadata")
                .permissions(),
        ) & 0o777,
        0o600,
        "census report must not inherit a permissive caller umask"
    );
    assert!(
        create_private_census_report(&report_path).is_err(),
        "census report publication must refuse overwrite"
    );
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[allow(dead_code)]
enum CensusRecoverability {
    /// Retained for taxonomy completeness: no preserved trade event currently
    /// binds all ten canonical state fields, so this class is not expected to
    /// appear in the census output.
    #[serde(rename = "RECOVERABLE_BY_STRICT_EVENT_STATE")]
    RecoverableByStrictEventState,
    #[serde(rename = "RECOVERABLE_BY_HASH_PINNED_TRANSITION")]
    RecoverableByHashPinnedTransition,
    #[serde(rename = "IRREDUCIBLE_MISSING_PRIMARY_EVIDENCE")]
    IrreducibleMissingPrimaryEvidence,
    #[serde(rename = "AMBIGUOUS_ORDER_OR_PARENT")]
    AmbiguousOrderOrParent,
    #[serde(rename = "UNSUPPORTED_SEQUENCE")]
    UnsupportedSequence,
}

// The layout-fix hypothesis is reported per candidate through
// `payload_layout_class`; recovered layout-fix candidates are counted through
// the normal chain classes.

#[derive(Clone, Debug, Serialize)]
struct CensusLocator {
    outer_instruction_index: u32,
    inner_instruction_path: Vec<u16>,
    stack_height: Option<u32>,
}

#[derive(Clone, Debug, Serialize)]
struct CensusEventEvidence {
    locator: CensusLocator,
    event_name: String,
    parent_binding_status: String,
    identity_binding_status: String,
    bound_state_fields: Vec<String>,
    strict_decode_only_fields_present: Vec<String>,
    /// Exact Borsh field bytes retained for audit only.  Presence here does
    /// not promote a `StrictDecodeOnly` field to state authority.
    decoded_field_borsh_hex: BTreeMap<String, String>,
}

#[derive(Clone, Debug, Serialize)]
struct CensusCurveStateEvidence {
    virtual_token_reserves: u64,
    virtual_quote_reserves: u64,
    real_token_reserves: u64,
    real_quote_reserves: u64,
    token_total_supply: u64,
    complete: bool,
    creator: String,
    is_mayhem_mode: bool,
    is_cashback_coin: bool,
    quote_mint: String,
}

impl From<&PumpExactStateCurveStateV2> for CensusCurveStateEvidence {
    fn from(state: &PumpExactStateCurveStateV2) -> Self {
        Self {
            virtual_token_reserves: state.virtual_token_reserves,
            virtual_quote_reserves: state.virtual_quote_reserves,
            real_token_reserves: state.real_token_reserves,
            real_quote_reserves: state.real_quote_reserves,
            token_total_supply: state.token_total_supply,
            complete: state.complete,
            creator: state.creator.to_string(),
            is_mayhem_mode: state.is_mayhem_mode,
            is_cashback_coin: state.is_cashback_coin,
            quote_mint: state.quote_mint.to_string(),
        }
    }
}

#[derive(Clone, Debug, Serialize)]
struct CensusCandidate {
    locator: CensusLocator,
    effect: String,
    instruction_name: String,
    instruction_discriminator: String,
    bonding_curve: String,
    mint: Option<String>,
    instruction_payload_exact: bool,
    account_vector_exact: bool,
    payload_layout_class: String,
    matched_event_cpis: Vec<CensusEventEvidence>,
    exact_pre_anchor_available: bool,
    exact_pre_anchor_state: Option<CensusCurveStateEvidence>,
    exact_create_genesis_applicable: bool,
    final_same_signature_anchor_available: bool,
    final_same_signature_anchor_state: Option<CensusCurveStateEvidence>,
    transition_rule_available: bool,
    recoverability: CensusRecoverability,
    missing_evidence_reason: Option<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CensusTransaction {
    signature: String,
    slot: u64,
    tx_index: u32,
    candidate_count: u32,
    cohort_candidate_count: u32,
    inventory_complete: bool,
    unknown_occurrence_reasons: Vec<String>,
    ordered_candidate_locators: Vec<CensusLocator>,
    candidates: Vec<CensusCandidate>,
    chain_classification: CensusRecoverability,
    chain_reasons: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct CensusSummary {
    census_schema_version: u16,
    census_source_sha256: String,
    census_source_blake3: String,
    census_source_bytes: u64,
    census_running_executable_sha256: String,
    census_running_executable_blake3: String,
    census_running_executable_bytes: u64,
    raw_run_id: String,
    raw_start_manifest_sha256: String,
    raw_completion_receipt_sha256: String,
    baseline_receipt_sha256: String,
    baseline_global_successful_rooted_candidate_count: u64,
    baseline_scoped_denominator: u64,
    baseline_exact_count: u64,
    baseline_explicit_non_exact_count: u64,
    baseline_coverage_ppm: u64,
    baseline_required_coverage_ppm: u64,
    required_exact_count: u64,
    transition_authority_statement: String,
    multi_mutation_transaction_count: u64,
    multi_mutation_cohort_candidate_count: u64,
    inventory_incomplete_transaction_count: u64,
    inventory_incomplete_cohort_candidate_count: u64,
    residual_conservation_exact: bool,
    multi_residual_classified: u64,
    inventory_residual_classified: u64,
    layout_resolved_event_unknown_count: u64,
    unresolvable_unknown_occurrence_count: u64,
    all_residual_classified_exactly_once: u64,
    unclassified_residual_count: u64,
    duplicate_classification_count: u64,
    recoverable_multi_candidate_count: u64,
    recoverable_after_closed_layout_fix_count: u64,
    recoverable_residual_count: u64,
    irreducible_residual_count: u64,
    maximum_provable_exact_count: u64,
    maximum_provable_coverage_ppm: u64,
    projected_scoped_denominator: u64,
    projected_exact_count: u64,
    projected_explicit_non_exact_count: u64,
    projected_coverage_ppm: u64,
    projected_required_exact_count: u64,
    projected_global_unknown_occurrence_count: u64,
    projected_global_malformed_candidate_count: u64,
    projected_global_dependency_candidate_count: u64,
    projected_unscoped_curve_candidate_count: u64,
    projected_scope_incomplete_occurrence_count: u64,
    projected_global_blocker_samples: Vec<String>,
    production_replay_parity: bool,
    feasibility_gate_passed: bool,
    chain_class_counts: BTreeMap<String, u64>,
    reason_counts: BTreeMap<String, u64>,
    layout_class_counts: BTreeMap<String, u64>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CensusBaselineResidualClass {
    MultiMutation,
    InventoryIncomplete,
}

/// Instruction-argument shape classifier ONLY.  This enum selects which
/// preserved instruction argument feeds a transition; it is never a state
/// authority.  The reserve-movement authority is the shared versioned quote
/// contract in `ghost_core::pump_quote` (see `census_shared_forward`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum CensusTransitionFamily {
    /// `buy` / `buy_v2`: instruction argument `amount` is the exact base out.
    ExactBaseOutBuy,
    /// `buy_exact_quote_in_v2` (`spendable_quote_in`): the instruction
    /// argument is the exact curve quote input.
    ExactQuoteInBuy,
    /// `sell` / `sell_v2`: instruction argument `amount` is the exact base in.
    ExactBaseInSell,
}

fn census_transition_family(instruction_name: &str) -> Option<CensusTransitionFamily> {
    match instruction_name {
        "buy" | "buy_v2" | "buy_exact_sol_in" => Some(CensusTransitionFamily::ExactBaseOutBuy),
        "buy_exact_quote_in_v2" => Some(CensusTransitionFamily::ExactQuoteInBuy),
        "sell" | "sell_v2" => Some(CensusTransitionFamily::ExactBaseInSell),
        _ => None,
    }
}

fn census_transition_rule_available(instruction_name: &str) -> bool {
    census_transition_family(instruction_name).is_some()
        || matches!(
            instruction_name,
            "migrate" | "migrate_v2" | "migrate_bonding_curve_creator"
        )
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct CensusReserveState {
    virtual_token_reserves: u64,
    virtual_quote_reserves: u64,
    real_token_reserves: u64,
    real_quote_reserves: u64,
}

impl CensusReserveState {
    fn from_curve_state(state: &PumpExactStateCurveStateV2) -> Self {
        Self {
            virtual_token_reserves: state.virtual_token_reserves,
            virtual_quote_reserves: state.virtual_quote_reserves,
            real_token_reserves: state.real_token_reserves,
            real_quote_reserves: state.real_quote_reserves,
        }
    }
}

fn ceil_div_u128(a: u128, b: u128) -> Result<u128, String> {
    if b == 0 {
        return Err("transition_division_by_zero".to_owned());
    }
    a.checked_add(b - 1)
        .map(|value| value / b)
        .ok_or_else(|| "transition_invariant_overflow".to_owned())
}

/// Offline zero-fee fixture schedule for the shared quote contract.  PLAN
/// V1.1 §2.8 freezes that fee schedule, fee recipient, creator-fee and
/// buyback configuration never move bonding-curve reserves; the census
/// consumes only the reserve-transition fields of the shared contract.
fn census_zero_fee_schedule() -> ProgramFeeSchedule {
    ProgramFeeSchedule {
        fee_schedule_id: "feasibility_census_v2_zero_fee_offline_fixture".to_owned(),
        effective_slot: 0,
        evidence: ProgramFeeScheduleEvidenceV1::CanonicalFixture {
            fixture_id: "feasibility_census_v2".to_owned(),
            transaction_signature: "census_offline_no_transaction".to_owned(),
            observed_slot: 0,
        },
        rules: vec![ProgramFeeRule {
            component_id: "census_zero_fee".to_owned(),
            numerator: 0,
            denominator: 10_000,
            rounding: FeeRounding::Floor,
        }],
    }
}

/// The reserve-movement authority for census reconstruction.  It delegates to
/// the shared versioned quote contract in `ghost_core::pump_quote`; the
/// census deliberately does not restate that math locally.  Real-reserve
/// movement applies the PLAN V1.1 §2.8 exact-amount rule (no fee-config
/// dependency) and every result is bit-exactly verified against anchors.
/// Instructions without a shared route variant return a typed
/// `transition_authority_not_shared` failure and remain non-recoverable.
fn census_shared_forward(
    instruction_name: &str,
    state: &CensusReserveState,
    amount: u64,
) -> Result<CensusReserveState, String> {
    let reserves = PumpReserveState {
        virtual_base_reserves: state.virtual_token_reserves,
        virtual_quote_reserves: state.virtual_quote_reserves,
        real_base_reserves: state.real_token_reserves,
        real_quote_reserves: state.real_quote_reserves,
    };
    let schedule = census_zero_fee_schedule();
    let transition = match instruction_name {
        "buy" | "buy_exact_sol_in" => quote_exact_base_out(
            PumpRouteVariant::LegacyBuy,
            reserves,
            amount,
            u64::MAX,
            &schedule,
        ),
        "buy_v2" => quote_exact_base_out(
            PumpRouteVariant::BuyV2,
            reserves,
            amount,
            u64::MAX,
            &schedule,
        ),
        "sell" => {
            quote_exact_base_in_sell(PumpRouteVariant::LegacySell, reserves, amount, 0, &schedule)
        }
        "sell_v2" => {
            quote_exact_base_in_sell(PumpRouteVariant::SellV2, reserves, amount, 0, &schedule)
        }
        "buy_exact_quote_in_v2" => quote_exact_quote_in(reserves, amount, 0, &schedule),
        other => return Err(format!("transition_authority_not_shared_for:{other}")),
    }
    .map_err(|error| format!("shared_transition_rejected:{error}"))?
    .reserve_transition;
    let is_sell = matches!(instruction_name, "sell" | "sell_v2");
    let real_token_after = if is_sell {
        state
            .real_token_reserves
            .checked_add(transition.base_amount)
    } else {
        state
            .real_token_reserves
            .checked_sub(transition.base_amount)
    }
    .ok_or_else(|| "v1_1_real_token_movement_overflow".to_owned())?;
    let real_quote_after = if is_sell {
        state
            .real_quote_reserves
            .checked_sub(transition.curve_quote_amount)
    } else {
        state
            .real_quote_reserves
            .checked_add(transition.curve_quote_amount)
    }
    .ok_or_else(|| "v1_1_real_quote_movement_overflow".to_owned())?;
    Ok(CensusReserveState {
        virtual_token_reserves: transition.base_after,
        virtual_quote_reserves: transition.quote_after,
        real_token_reserves: real_token_after,
        real_quote_reserves: real_quote_after,
    })
}

/// Census-only inverse PROPOSALS for the constant-product reserve movement.
/// These derivations are not an authority: every proposed pre-state is
/// separately forward-verified through the shared `ghost_core::pump_quote`
/// contract and bit-exactly compared against anchor/event evidence before it
/// can enter a recoverable chain.  More than one solution is a genuine
/// ambiguity and fails closed; zero solutions fail closed.
fn census_inverse_proposal(
    family: CensusTransitionFamily,
    after: &CensusReserveState,
    amount: u64,
) -> Result<Vec<CensusReserveState>, String> {
    if amount == 0 {
        return Err("transition_zero_amount".to_owned());
    }
    let mut candidates = Vec::new();
    match family {
        CensusTransitionFamily::ExactBaseOutBuy => {
            let vt_pre = after
                .virtual_token_reserves
                .checked_add(amount)
                .ok_or_else(|| "transition_token_overflow".to_owned())?;
            let rt_pre = after
                .real_token_reserves
                .checked_add(amount)
                .ok_or_else(|| "transition_real_token_overflow".to_owned())?;
            for vq_pre in ceil_preimage_candidates(
                after.virtual_quote_reserves,
                after.virtual_token_reserves,
                vt_pre,
            )? {
                let curve_quote_amount = match after.virtual_quote_reserves.checked_sub(vq_pre) {
                    Some(value) => value,
                    None => continue,
                };
                let rq_pre = match after.real_quote_reserves.checked_sub(curve_quote_amount) {
                    Some(value) => value,
                    None => continue,
                };
                candidates.push(CensusReserveState {
                    virtual_token_reserves: vt_pre,
                    virtual_quote_reserves: vq_pre,
                    real_token_reserves: rt_pre,
                    real_quote_reserves: rq_pre,
                });
            }
        }
        CensusTransitionFamily::ExactBaseInSell => {
            let vt_pre = after
                .virtual_token_reserves
                .checked_sub(amount)
                .ok_or_else(|| "transition_virtual_token_underflow".to_owned())?;
            let rt_pre = after
                .real_token_reserves
                .checked_sub(amount)
                .ok_or_else(|| "transition_real_token_underflow".to_owned())?;
            for vq_pre in ceil_preimage_candidates(
                after.virtual_quote_reserves,
                after.virtual_token_reserves,
                vt_pre,
            )? {
                let curve_quote_amount = match vq_pre.checked_sub(after.virtual_quote_reserves) {
                    Some(value) => value,
                    None => continue,
                };
                let rq_pre = match after.real_quote_reserves.checked_add(curve_quote_amount) {
                    Some(value) => value,
                    None => continue,
                };
                candidates.push(CensusReserveState {
                    virtual_token_reserves: vt_pre,
                    virtual_quote_reserves: vq_pre,
                    real_token_reserves: rt_pre,
                    real_quote_reserves: rq_pre,
                });
            }
        }
        CensusTransitionFamily::ExactQuoteInBuy => {
            let vq_pre = after
                .virtual_quote_reserves
                .checked_sub(amount)
                .ok_or_else(|| "transition_quote_movement_underflow".to_owned())?;
            if vq_pre == 0 {
                return Err("transition_pre_quote_reserve_zero".to_owned());
            }
            let rq_pre = after
                .real_quote_reserves
                .checked_sub(amount)
                .ok_or_else(|| "transition_real_quote_underflow".to_owned())?;
            // Forward: base_after = floor(vq_pre * vt_pre / after.vq) = after.vt.
            let low = ceil_div_u128(
                (after.virtual_token_reserves as u128) * (after.virtual_quote_reserves as u128),
                vq_pre as u128,
            )?;
            let high = (((after.virtual_token_reserves as u128) + 1)
                * (after.virtual_quote_reserves as u128)
                - 1)
                / (vq_pre as u128);
            if high >= low {
                let cardinality = high - low + 1;
                if cardinality > CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES {
                    return Err(format!(
                        "inverse_preimage_cardinality_exceeds_bound:{cardinality}:{}",
                        CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES
                    ));
                }
                for vt_pre_u128 in low..=high {
                    let vt_pre = match u64::try_from(vt_pre_u128) {
                        Ok(value) => value,
                        Err(_) => continue,
                    };
                    let base_amount = match vt_pre.checked_sub(after.virtual_token_reserves) {
                        Some(value) if value > 0 => value,
                        _ => continue,
                    };
                    let rt_pre = match after.real_token_reserves.checked_add(base_amount) {
                        Some(value) => value,
                        None => continue,
                    };
                    candidates.push(CensusReserveState {
                        virtual_token_reserves: vt_pre,
                        virtual_quote_reserves: vq_pre,
                        real_token_reserves: rt_pre,
                        real_quote_reserves: rq_pre,
                    });
                }
            }
        }
    }
    Ok(candidates)
}

/// Integer solutions of `ceil(x * vt_pre / vt_after) = quote_after`, the
/// rounding-closed preimage of the constant-product quote movement.
fn ceil_preimage_candidates(
    quote_after: u64,
    vt_after: u64,
    vt_pre: u64,
) -> Result<Vec<u64>, String> {
    if vt_after == 0 || vt_pre == 0 {
        return Err("transition_virtual_token_exhausted".to_owned());
    }
    let mut candidates = Vec::new();
    if quote_after == 0 {
        candidates.push(0);
        return Ok(candidates);
    }
    let x_max = ((quote_after as u128) * (vt_after as u128)) / (vt_pre as u128);
    let x_min = (((quote_after as u128) - 1) * (vt_after as u128)) / (vt_pre as u128) + 1;
    if x_max >= x_min {
        let cardinality = x_max - x_min + 1;
        if cardinality > CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES {
            return Err(format!(
                "inverse_preimage_cardinality_exceeds_bound:{cardinality}:{}",
                CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES
            ));
        }
        for value in x_min..=x_max {
            if let Ok(value) = u64::try_from(value) {
                candidates.push(value);
            }
        }
    }
    Ok(candidates)
}

/// Resolve only the derived Event-CPI unknown produced because the strict
/// baseline parent payload lacked semantic evidence.  The structural ledger
/// must contain the same occurrence as a validated transport, the parent must
/// resolve under one of the closed compatibility grammars, and the event must
/// pass the complete manifest-bound parent contract.  No other unknown reason
/// is eligible for this reconciliation.
fn census_layout_resolved_event_unknown_keys(
    context: &PumpExactStateTransactionContextV2,
    semantic_inventory: &PumpExactStateTransactionInventoryV2,
    structural_inventory: &PumpExactStateTransactionInventoryV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
) -> BTreeSet<PumpExactStateInstructionOccurrenceKeyV2> {
    let semantic_by_key = semantic_inventory
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.key.clone(), occurrence))
        .collect::<BTreeMap<_, _>>();
    let structural_by_key = structural_inventory
        .occurrences
        .iter()
        .map(|occurrence| (occurrence.key.clone(), occurrence))
        .collect::<BTreeMap<_, _>>();
    let mut resolved = BTreeSet::new();

    for (key, semantic_occurrence) in &semantic_by_key {
        let PumpExactStateOccurrenceClassV2::Unknown { reason } = &semantic_occurrence.class else {
            continue;
        };
        if reason != "anchor_event_transport_parent_contract_not_exact" {
            continue;
        }
        let Some(structural_occurrence) = structural_by_key.get(key) else {
            continue;
        };
        let PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
            immediate_parent,
            event_discriminator,
            event_fields,
            ..
        } = &structural_occurrence.class
        else {
            continue;
        };
        let Some(parent_occurrence) = semantic_by_key.get(immediate_parent) else {
            continue;
        };
        let Some(parent_evidence) =
            census_hypothesis_parent_evidence(context, semantics, parent_occurrence)
        else {
            continue;
        };
        let Some(event) = semantics.event(event_discriminator) else {
            continue;
        };
        if semantics
            .validate_event_parent_semantics(event, event_fields, &parent_evidence)
            .is_ok()
        {
            resolved.insert(key.clone());
        }
    }
    resolved
}

fn census_arg_u64(fields: &BTreeMap<String, Vec<u8>>, name: &str) -> Result<u64, String> {
    let bytes = fields
        .get(name)
        .ok_or_else(|| format!("transition_missing_argument:{name}"))?;
    let bytes: [u8; 8] = bytes
        .as_slice()
        .try_into()
        .map_err(|_| format!("transition_argument_not_u64:{name}"))?;
    Ok(u64::from_le_bytes(bytes))
}

/// Closed compatibility layout census for the two runtime-IDL differences
/// observed in preserved raw.  These classifications are evidence-only: they
/// describe which closed grammar a malformed payload matches, without
/// granting the suffix or the missing field any state authority.
fn census_payload_layout_class(
    instruction_name: &str,
    payload_after_discriminator: &[u8],
    semantics: &PumpExactStateSemanticsAuthorityV2,
) -> String {
    match instruction_name {
        "buy_exact_quote_in_v2" | "sell" | "buy_v2" => {
            // Canonical: spendable_quote_in u64 + min_tokens_out u64 (16 B).
            // The other two instructions also have a canonical 2 x u64
            // prefix. Preserved runtime variants carry a closed suffix of
            // literal booleans: one for buy_exact_quote_in_v2 / buy_v2 and
            // one or two for sell. The suffix is decoded but remains
            // unassigned: no argument name, state, or Event-CPI authority.
            let suffix_len = payload_after_discriminator.len().saturating_sub(16);
            let expected_suffix = match instruction_name {
                "sell" => matches!(suffix_len, 1 | 2),
                "buy_exact_quote_in_v2" | "buy_v2" => suffix_len == 1,
                _ => false,
            };
            if expected_suffix {
                let prefix_ok = semantics
                    .census_decode_named_fields_exact(
                        &canonical_two_u64_fields(),
                        &payload_after_discriminator[..16],
                        "census buy_exact_quote_in_v2 prefix",
                    )
                    .is_ok();
                let suffix_is_borsh_bool = payload_after_discriminator[16..]
                    .iter()
                    .all(|value| matches!(value, 0 | 1));
                if prefix_ok && suffix_is_borsh_bool {
                    return match instruction_name {
                        "buy_exact_quote_in_v2" => {
                            "buy_exact_quote_in_v2_trailing_unassigned_boolean".to_owned()
                        }
                        "buy_v2" => "buy_v2_trailing_unassigned_boolean".to_owned(),
                        "sell" => format!("sell_trailing_unassigned_booleans_{suffix_len}"),
                        _ => unreachable!(),
                    };
                }
                return format!("{instruction_name}_unrecognized_boolean_suffix");
            }
            format!("{instruction_name}_other_malformed")
        }
        "create_v2" => {
            // Canonical: name + symbol + uri strings, creator pubkey,
            // is_mayhem_mode bool, is_cashback_enabled OptionBool.  Observed
            // runtime variant: the payload is fully consumed exactly after
            // `is_mayhem_mode`; the terminal OptionBool is absent.  The
            // missing field is not imputed to any value.
            match create_v2_prefix_consumption(payload_after_discriminator) {
                Some(offset) if offset == payload_after_discriminator.len() => {
                    "create_v2_missing_terminal_optionbool".to_owned()
                }
                Some(_) => "create_v2_trailing_bytes_after_prefix".to_owned(),
                None => "create_v2_prefix_not_decodable".to_owned(),
            }
        }
        _ => "other_malformed_payload".to_owned(),
    }
}

fn canonical_two_u64_fields() -> Vec<serde_json::Value> {
    vec![
        serde_json::json!({ "name": "first_u64", "type": "u64" }),
        serde_json::json!({ "name": "second_u64", "type": "u64" }),
    ]
}

/// Byte offset after consuming `name`, `symbol`, `uri` (Borsh strings),
/// `creator` (pubkey) and `is_mayhem_mode` (bool), or None when the prefix
/// does not strictly decode.
fn create_v2_prefix_consumption(payload: &[u8]) -> Option<usize> {
    let mut offset = 0usize;
    for _ in 0..3 {
        let len_bytes: [u8; 4] = payload.get(offset..offset + 4)?.try_into().ok()?;
        let len = usize::try_from(u32::from_le_bytes(len_bytes)).ok()?;
        offset = offset.checked_add(4)?.checked_add(len)?;
        if offset > payload.len() {
            return None;
        }
    }
    offset = offset.checked_add(32)?; // creator pubkey
    offset = offset.checked_add(1)?; // is_mayhem_mode bool
    if offset > payload.len() {
        return None;
    }
    // The bool must be a valid boolean for the prefix to count as strict.
    if !matches!(payload[offset - 1], 0 | 1) {
        return None;
    }
    Some(offset)
}

/// Decode the preserved instruction payload under the layout-fix hypothesis
/// used by the census: canonical strict grammar first, then the two closed
/// proven compatibility grammars.  Returns argument bytes for chain math.
fn census_args_under_layout_hypothesis(
    contract: &PumpExactStateInstructionContractV2,
    payload_after_discriminator: &[u8],
    semantics: &PumpExactStateSemanticsAuthorityV2,
) -> Result<BTreeMap<String, Vec<u8>>, String> {
    let canonical_args = semantics.census_instruction_arg_fields(contract);
    semantics
        .census_decode_named_fields_exact(
            canonical_args,
            payload_after_discriminator,
            "census strict instruction payload",
        )
        .or_else(|_| {
            match contract.name.as_str() {
                "buy_exact_quote_in_v2" | "buy_v2" | "sell"
                    if payload_after_discriminator.len() > 16
                        && payload_after_discriminator[16..]
                            .iter()
                            .all(|value| matches!(value, 0 | 1))
                        && match contract.name.as_str() {
                            "sell" => matches!(payload_after_discriminator.len() - 16, 1 | 2),
                            "buy_exact_quote_in_v2" | "buy_v2" => {
                                payload_after_discriminator.len() - 16 == 1
                            }
                            _ => false,
                        } =>
                {
                    let mut fields = semantics
                        .census_decode_named_fields_exact(
                            canonical_args,
                            &payload_after_discriminator[..16],
                            "census closed boolean-suffix compatibility prefix",
                        )
                        .map_err(|error| error.to_string())?;
                    // The trailing boolean is decoded and classified but never
                    // assigned to an instruction argument.
                    fields.insert(
                        "census_unassigned_trailing_booleans".to_owned(),
                        payload_after_discriminator[16..].to_vec(),
                    );
                    Ok(fields)
                }
                "create_v2" => {
                    let offset = create_v2_prefix_consumption(payload_after_discriminator)
                        .ok_or_else(|| "census create_v2 prefix not decodable".to_owned())?;
                    if offset != payload_after_discriminator.len() {
                        return Err("census create_v2 payload has unexpected suffix".to_owned());
                    }
                    // Decode exactly the present prefix arguments; the absent
                    // OptionBool is not imputed.
                    let prefix_args: Vec<serde_json::Value> = canonical_args
                        .iter()
                        .take(canonical_args.len().saturating_sub(1))
                        .cloned()
                        .collect();
                    if prefix_args.len() + 1 != canonical_args.len() {
                        return Err("census create_v2 IDL shape unexpected".to_owned());
                    }
                    semantics
                        .census_decode_named_fields_exact(
                            &prefix_args,
                            payload_after_discriminator,
                            "census create_v2 compatibility prefix",
                        )
                        .map_err(|error| error.to_string())
                }
                _ => Err("census_no_compatibility_grammar".to_owned()),
            }
        })
        .map_err(|error| error.to_string())
}

struct CensusEventMatch {
    locator: CensusLocator,
    event_name: String,
    parent_binding_status: String,
    identity_binding_status: String,
    bound_state_fields: Vec<String>,
    strict_decode_only_fields_present: Vec<String>,
    decoded_field_borsh_hex: BTreeMap<String, String>,
    final_state_bindings: Vec<PumpExactStateEventFinalStateBindingV2>,
    is_primary_state_event: bool,
    is_completion_event: bool,
}

/// Collect the Event-CPI evidence attached to one candidate occurrence and
/// re-derive its parent/identity bindings against the preserved evidence.
/// For candidates with a later same-curve mutation the materializer skips the
/// final-anchor comparison; the census instead compares bound state fields
/// against the reconstructed chain state.
fn census_event_matches(
    occurrences: &[PumpExactStateInstructionOccurrenceV2],
    candidate_key: &PumpExactStateInstructionOccurrenceKeyV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    hypothesis_parent_evidence: Option<PumpExactStateInstructionSemanticEvidenceV2>,
) -> Vec<CensusEventMatch> {
    let parent_evidence = occurrences
        .iter()
        .find_map(|occurrence| {
            if occurrence.key != *candidate_key {
                return None;
            }
            match &occurrence.class {
                PumpExactStateOccurrenceClassV2::ProvenNonReserve { semantic_evidence } => {
                    Some(semantic_evidence.clone())
                }
                PumpExactStateOccurrenceClassV2::Candidate {
                    semantic_evidence: Some(semantic_evidence),
                    ..
                } => Some(semantic_evidence.clone()),
                _ => None,
            }
        })
        .or(hypothesis_parent_evidence);
    let mut matches = Vec::new();
    for occurrence in occurrences {
        let PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
            immediate_parent,
            event_discriminator,
            event_fields,
            ..
        } = &occurrence.class
        else {
            continue;
        };
        if immediate_parent != candidate_key {
            continue;
        }
        let Some(event) = semantics.event(event_discriminator) else {
            matches.push(CensusEventMatch {
                locator: census_locator_from_occurrence(occurrence),
                event_name: "unknown".to_owned(),
                parent_binding_status: "unknown_nested_event".to_owned(),
                identity_binding_status: "unknown_nested_event".to_owned(),
                bound_state_fields: Vec::new(),
                strict_decode_only_fields_present: Vec::new(),
                decoded_field_borsh_hex: BTreeMap::new(),
                final_state_bindings: Vec::new(),
                is_primary_state_event: false,
                is_completion_event: false,
            });
            continue;
        };
        let Some(parent) = &parent_evidence else {
            matches.push(CensusEventMatch {
                locator: census_locator_from_occurrence(occurrence),
                event_name: event.name.clone(),
                parent_binding_status: "parent_contract_not_exact".to_owned(),
                identity_binding_status: "parent_contract_not_exact".to_owned(),
                bound_state_fields: Vec::new(),
                strict_decode_only_fields_present: Vec::new(),
                decoded_field_borsh_hex: BTreeMap::new(),
                final_state_bindings: Vec::new(),
                is_primary_state_event: false,
                is_completion_event: false,
            });
            continue;
        };
        let binding_result = semantics.validate_event_parent_semantics(event, event_fields, parent);
        let (parent_binding_status, final_state_bindings, bound_state_fields) =
            match &binding_result {
                Ok(bindings) => (
                    "validated".to_owned(),
                    bindings.clone(),
                    bindings
                        .iter()
                        .map(|binding| binding.curve_state_field.clone())
                        .collect::<Vec<_>>(),
                ),
                Err(error) => (error.to_string(), Vec::new(), Vec::new()),
            };
        let strict_decode_only_fields_present = semantics
            .census_strict_decode_only_event_fields(event, &parent.discriminator)
            .unwrap_or_default()
            .into_iter()
            .filter(|field| event_fields.contains_key(field))
            .collect::<Vec<_>>();
        let decoded_field_borsh_hex = event_fields
            .iter()
            .map(|(name, bytes)| (name.clone(), hex_bytes(bytes)))
            .collect::<BTreeMap<_, _>>();
        let semantically_valid = binding_result.is_ok();
        let is_primary_state_event = semantically_valid
            && matches!(event.name.as_str(), "TradeEvent" | "CreateEvent")
            && !final_state_bindings.is_empty();
        matches.push(CensusEventMatch {
            locator: census_locator_from_occurrence(occurrence),
            event_name: event.name.clone(),
            parent_binding_status: parent_binding_status.clone(),
            identity_binding_status: if binding_result.is_ok() {
                "validated".to_owned()
            } else {
                parent_binding_status.clone()
            },
            bound_state_fields,
            strict_decode_only_fields_present,
            decoded_field_borsh_hex,
            final_state_bindings,
            is_primary_state_event,
            is_completion_event: semantically_valid && event.name == "CompleteEvent",
        });
    }
    matches
}

struct CensusChainCandidate {
    effect: PumpExactStateInstructionEffectV2,
    instruction_name: String,
    args: Option<BTreeMap<String, Vec<u8>>>,
    events: Vec<CensusEventMatch>,
    /// Closed grammar that resolved a malformed payload, when applicable.
    /// `None` means the strict canonical layout decoded the payload.
    layout_resolution: Option<String>,
}

fn census_primary_event(candidate: &CensusChainCandidate) -> Result<&CensusEventMatch, String> {
    let expected_name = match candidate.effect {
        PumpExactStateInstructionEffectV2::SupportedExactCreate => "CreateEvent",
        PumpExactStateInstructionEffectV2::SupportedExactTrade => "TradeEvent",
        _ => return Err("candidate_effect_has_no_primary_state_event".to_owned()),
    };
    let matching = candidate
        .events
        .iter()
        .filter(|event| event.is_primary_state_event && event.event_name == expected_name)
        .collect::<Vec<_>>();
    match matching.as_slice() {
        [event] => Ok(*event),
        [] => Err(format!("missing_validated_state_event_cpi:{expected_name}")),
        _ => Err(format!("ambiguous_event_parent_for:{expected_name}")),
    }
}

fn census_bound_state_u64(
    candidate: &CensusChainCandidate,
    curve_state_field: &str,
) -> Result<u64, String> {
    let event = census_primary_event(candidate)?;
    let values = event
        .final_state_bindings
        .iter()
        .filter(|binding| binding.curve_state_field == curve_state_field)
        .collect::<Vec<_>>();
    let binding = match values.as_slice() {
        [binding] => *binding,
        [] => return Err(format!("event_state_field_missing:{curve_state_field}")),
        _ => return Err(format!("event_state_field_ambiguous:{curve_state_field}")),
    };
    let bytes: [u8; 8] = binding
        .event_value_borsh
        .as_slice()
        .try_into()
        .map_err(|_| format!("event_state_field_not_u64:{curve_state_field}"))?;
    Ok(u64::from_le_bytes(bytes))
}

fn census_parent_argument_transition_amount(
    candidate: &CensusChainCandidate,
) -> Result<u64, String> {
    let argument = match candidate.instruction_name.as_str() {
        "buy" | "buy_v2" | "sell" | "sell_v2" => "amount",
        "buy_exact_quote_in_v2" => "spendable_quote_in",
        "buy_exact_sol_in" => return Err("buy_exact_sol_in_requires_event_state_delta".to_owned()),
        other => return Err(format!("transition_family_unknown:{other}")),
    };
    census_arg_u64(
        candidate
            .args
            .as_ref()
            .ok_or_else(|| "transition_args_missing".to_owned())?,
        argument,
    )
}

/// Resolve the exact reserve-transition amount for one ordered candidate.
/// `buy_exact_sol_in.spendable_sol_in` is only a wallet budget, never a curve
/// input.  Its exact base output is therefore derived solely from the two
/// manifest-bound canonical token-reserve states (previous anchor/event and
/// current TradeEvent), with the real-token delta required to agree.
fn census_transition_amount_at(
    chain: &[CensusChainCandidate],
    position: usize,
    preceding_anchor_state: Option<&PumpExactStateCurveStateV2>,
) -> Result<u64, String> {
    let candidate = chain
        .get(position)
        .ok_or_else(|| "transition_position_out_of_range".to_owned())?;
    if candidate.instruction_name != "buy_exact_sol_in" {
        return census_parent_argument_transition_amount(candidate);
    }
    let (before_virtual, before_real) = if position == 0 {
        let before = preceding_anchor_state
            .ok_or_else(|| "buy_exact_sol_in_pre_state_unavailable".to_owned())?;
        (before.virtual_token_reserves, before.real_token_reserves)
    } else {
        let previous = &chain[position - 1];
        (
            census_bound_state_u64(previous, "virtual_token_reserves")?,
            census_bound_state_u64(previous, "real_token_reserves")?,
        )
    };
    let after_virtual = census_bound_state_u64(candidate, "virtual_token_reserves")?;
    let after_real = census_bound_state_u64(candidate, "real_token_reserves")?;
    let virtual_delta = before_virtual
        .checked_sub(after_virtual)
        .ok_or_else(|| "buy_exact_sol_in_virtual_token_direction_invalid".to_owned())?;
    let real_delta = before_real
        .checked_sub(after_real)
        .ok_or_else(|| "buy_exact_sol_in_real_token_direction_invalid".to_owned())?;
    if virtual_delta == 0 || virtual_delta != real_delta {
        return Err(format!(
            "buy_exact_sol_in_token_delta_mismatch:{virtual_delta}:{real_delta}"
        ));
    }
    Ok(virtual_delta)
}

struct CensusInstructionRef<'a> {
    account_indices: &'a [u8],
    data: &'a [u8],
}

fn instruction_ref_for_locator<'a>(
    context: &'a PumpExactStateTransactionContextV2,
    locator: &CensusLocator,
) -> Option<CensusInstructionRef<'a>> {
    if locator.inner_instruction_path.is_empty() {
        let instruction = context
            .outer
            .get(usize::try_from(locator.outer_instruction_index).ok()?)?;
        return Some(CensusInstructionRef {
            account_indices: instruction.accounts.as_slice(),
            data: instruction.data.as_slice(),
        });
    }
    let group = context.inner.get(&locator.outer_instruction_index)?;
    let mut instructions = group.iter();
    let mut found: Option<&yellowstone_grpc_proto::prelude::InnerInstruction> = None;
    for inner_index in &locator.inner_instruction_path {
        found = instructions.nth(usize::from(*inner_index));
    }
    let instruction = found?;
    Some(CensusInstructionRef {
        account_indices: instruction.accounts.as_slice(),
        data: instruction.data.as_slice(),
    })
}

/// Reconstruct parent evidence for a malformed candidate under a proven
/// closed layout grammar: strict account-vector roles plus grammar-resolved
/// argument bytes.  Returns None unless the occurrence is a candidate whose
/// payload resolves under the grammar and whose account vector is strict.
fn census_hypothesis_parent_evidence(
    context: &PumpExactStateTransactionContextV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    occurrence: &PumpExactStateInstructionOccurrenceV2,
) -> Option<PumpExactStateInstructionSemanticEvidenceV2> {
    let is_malformed_candidate = matches!(
        &occurrence.class,
        PumpExactStateOccurrenceClassV2::Candidate {
            instruction_payload_exact,
            account_vector_exact,
            ..
        } if !instruction_payload_exact || !account_vector_exact
    );
    if !is_malformed_candidate {
        return None;
    }
    let contract = semantics.instruction(&occurrence.key.discriminator)?;
    let instruction_ref =
        instruction_ref_for_locator(context, &census_locator_from_occurrence(occurrence))?;
    let payload = &instruction_ref.data[8.min(instruction_ref.data.len())..];
    let resolved_args = census_args_under_layout_hypothesis(contract, payload, semantics).ok()?;
    let privilege_evidence = if occurrence.key.inner_instruction_path.is_empty() {
        PumpExactStateInvocationPrivilegeEvidenceV2::OuterTransactionMessageHeader
    } else {
        PumpExactStateInvocationPrivilegeEvidenceV2::InnerInstructionPrivilegesUnavailable
    };
    let account_roles = validate_instruction_account_vector_v2(
        context,
        contract,
        instruction_ref.account_indices,
        privilege_evidence,
    )
    .ok()?;
    Some(PumpExactStateInstructionSemanticEvidenceV2 {
        discriminator: contract.discriminator,
        account_roles,
        argument_fields: resolved_args,
    })
}

fn census_locator_from_occurrence(
    occurrence: &PumpExactStateInstructionOccurrenceV2,
) -> CensusLocator {
    CensusLocator {
        outer_instruction_index: occurrence.key.outer_instruction_index,
        inner_instruction_path: occurrence.key.inner_instruction_path.clone(),
        stack_height: occurrence.key.stack_height,
    }
}

/// A single candidate in a transaction whose inventory is complete under the
/// closed layout-fix hypothesis is decided by the existing anchor path: no
/// transaction-local replay is involved.  The census verifies exactly the
/// conditions the qualifier applies.
fn classify_single_candidate_standard(
    anchors: &PumpExactStateAnchorIndexV2,
    candidate: &CensusChainCandidate,
    signature: [u8; 64],
    slot: u64,
    tx_index: u32,
    curve: Pubkey,
) -> (CensusRecoverability, Vec<String>) {
    if !matches!(
        candidate.effect,
        PumpExactStateInstructionEffectV2::SupportedExactTrade
            | PumpExactStateInstructionEffectV2::SupportedExactCreate
    ) {
        return (
            CensusRecoverability::UnsupportedSequence,
            vec![format!(
                "unsupported_effect:{}",
                exact_state_effect_label_v2(candidate.effect)
            )],
        );
    }
    if candidate.args.is_none() {
        return (
            CensusRecoverability::IrreducibleMissingPrimaryEvidence,
            vec!["payload_not_resolved_by_proven_closed_grammar".to_owned()],
        );
    }
    if candidate.layout_resolution.as_deref() == Some("create_v2_missing_terminal_optionbool") {
        let primary = match census_primary_event(candidate) {
            Ok(event) => event,
            Err(error) => {
                return (
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence,
                    vec![error],
                )
            }
        };
        for required_field in ["is_mayhem_mode", "is_cashback_coin"] {
            if !primary
                .bound_state_fields
                .iter()
                .any(|field| field == required_field)
            {
                return (
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence,
                    vec![format!(
                        "create_v2_missing_optionbool_no_event_authority_for:{required_field}"
                    )],
                );
            }
        }
    }
    if anchors
        .unique_final_anchor(signature, curve, slot, tx_index)
        .is_none()
    {
        return (
            CensusRecoverability::IrreducibleMissingPrimaryEvidence,
            vec!["missing_final_same_signature_anchor".to_owned()],
        );
    }
    if matches!(
        candidate.effect,
        PumpExactStateInstructionEffectV2::SupportedExactTrade
    ) && anchors.unique_pre_anchor(curve, slot, tx_index).is_none()
    {
        return (
            CensusRecoverability::IrreducibleMissingPrimaryEvidence,
            vec!["missing_exact_pre_anchor".to_owned()],
        );
    }
    (
        CensusRecoverability::RecoverableByHashPinnedTransition,
        vec!["standard_single_candidate_anchor_path_after_closed_layout_fix".to_owned()],
    )
}

/// Analyze one same-curve candidate chain inside one signature under the
/// strict authority contract.  Returns the classification plus typed reasons.
fn analyze_same_curve_chain(
    anchors: &PumpExactStateAnchorIndexV2,
    signature: [u8; 64],
    slot: u64,
    tx_index: u32,
    curve: Pubkey,
    chain: &[CensusChainCandidate],
) -> (CensusRecoverability, Vec<String>) {
    let mut reasons: Vec<String> = Vec::new();
    let irreducible = |reasons: &mut Vec<String>, reason: String| {
        reasons.push(reason);
        (
            CensusRecoverability::IrreducibleMissingPrimaryEvidence,
            reasons.clone(),
        )
    };
    if chain.is_empty() {
        return irreducible(&mut reasons, "empty_chain".to_owned());
    }
    let Some(final_anchor) = anchors.unique_final_anchor(signature, curve, slot, tx_index) else {
        return irreducible(
            &mut reasons,
            "missing_final_same_signature_anchor".to_owned(),
        );
    };
    // Every candidate must be a supported effect with exact arguments and one
    // unambiguous primary state event.
    for candidate in chain {
        if !matches!(
            candidate.effect,
            PumpExactStateInstructionEffectV2::SupportedExactTrade
                | PumpExactStateInstructionEffectV2::SupportedExactCreate
        ) {
            reasons.push(format!(
                "unsupported_same_curve_mutation:{}",
                exact_state_effect_label_v2(candidate.effect)
            ));
            return (CensusRecoverability::UnsupportedSequence, reasons);
        }
        let primary = match census_primary_event(candidate) {
            Ok(event) => event,
            Err(error) if error.starts_with("ambiguous_event_parent_for:") => {
                reasons.push(error);
                return (CensusRecoverability::AmbiguousOrderOrParent, reasons);
            }
            Err(error) => {
                let event_status = candidate
                    .events
                    .first()
                    .map(|event| event.parent_binding_status.clone())
                    .unwrap_or_else(|| "no_event_transport".to_owned());
                return irreducible(
                    &mut reasons,
                    format!("{error}:{}:{event_status}", candidate.instruction_name),
                );
            }
        };
        if primary.parent_binding_status != "validated" {
            return irreducible(
                &mut reasons,
                format!(
                    "missing_validated_state_event_cpi:{}:{}",
                    candidate.instruction_name, primary.parent_binding_status
                ),
            );
        }
        if candidate.layout_resolution.as_deref() == Some("create_v2_missing_terminal_optionbool") {
            // The absent OptionBool is never imputed.  The candidate is
            // recoverable only when the preserved CreateEvent binding
            // supplies unambiguous authority for both state fields that the
            // missing argument could have influenced.
            for required_field in ["is_mayhem_mode", "is_cashback_coin"] {
                if !primary
                    .bound_state_fields
                    .iter()
                    .any(|field| field == required_field)
                {
                    return irreducible(
                        &mut reasons,
                        format!(
                            "create_v2_missing_optionbool_no_event_authority_for:{required_field}"
                        ),
                    );
                }
            }
        }
        if matches!(
            candidate.effect,
            PumpExactStateInstructionEffectV2::SupportedExactTrade
        ) {
            if census_transition_family(&candidate.instruction_name).is_none() {
                return irreducible(
                    &mut reasons,
                    format!("transition_family_unknown:{}", candidate.instruction_name),
                );
            }
            if candidate.instruction_name != "buy_exact_sol_in"
                && census_parent_argument_transition_amount(candidate).is_err()
            {
                return irreducible(
                    &mut reasons,
                    format!(
                        "transition_argument_unavailable:{}",
                        candidate.instruction_name
                    ),
                );
            }
        }
    }

    let completion_positions = chain
        .iter()
        .enumerate()
        .flat_map(|(position, candidate)| {
            candidate
                .events
                .iter()
                .filter(|event| event.is_completion_event)
                .map(move |_| position)
        })
        .collect::<Vec<_>>();
    if final_anchor.state.complete {
        if completion_positions.as_slice() != [chain.len() - 1]
            || !matches!(
                chain.last().map(|candidate| candidate.effect),
                Some(PumpExactStateInstructionEffectV2::SupportedExactTrade)
            )
        {
            reasons.push("final_complete_without_unique_last_complete_event".to_owned());
            return (CensusRecoverability::UnsupportedSequence, reasons);
        }
    } else if !completion_positions.is_empty() {
        reasons.push("complete_event_present_but_final_anchor_incomplete".to_owned());
        return (CensusRecoverability::UnsupportedSequence, reasons);
    }

    let carry_fields_agree = |a: &PumpExactStateCurveStateV2, b: &PumpExactStateCurveStateV2| {
        a.creator == b.creator
            && a.token_total_supply == b.token_total_supply
            && a.quote_mint == b.quote_mint
            && a.is_mayhem_mode == b.is_mayhem_mode
            && a.is_cashback_coin == b.is_cashback_coin
    };
    let full_state_from_reserves =
        |reserves: CensusReserveState, complete: bool| PumpExactStateCurveStateV2 {
            virtual_token_reserves: reserves.virtual_token_reserves,
            virtual_quote_reserves: reserves.virtual_quote_reserves,
            real_token_reserves: reserves.real_token_reserves,
            real_quote_reserves: reserves.real_quote_reserves,
            token_total_supply: final_anchor.state.token_total_supply,
            complete,
            creator: final_anchor.state.creator,
            is_mayhem_mode: final_anchor.state.is_mayhem_mode,
            is_cashback_coin: final_anchor.state.is_cashback_coin,
            quote_mint: final_anchor.state.quote_mint,
        };
    let verify_event_bindings = |state: &PumpExactStateCurveStateV2,
                                 candidate: &CensusChainCandidate|
     -> Result<(), String> {
        for event in candidate.events.iter().filter(|event| {
            event.parent_binding_status == "validated" && !event.final_state_bindings.is_empty()
        }) {
            validate_event_final_state_bindings_v2(state, &event.final_state_bindings)
                .map_err(|error| format!("{}:{error}", event.event_name))?;
        }
        Ok(())
    };
    let forward_step = |state: &PumpExactStateCurveStateV2,
                        position: usize,
                        candidate: &CensusChainCandidate|
     -> Result<PumpExactStateCurveStateV2, String> {
        if state.complete {
            return Err("mutation_after_complete_state".to_owned());
        }
        let amount = census_transition_amount_at(chain, position, Some(state))?;
        let reserves = census_shared_forward(
            &candidate.instruction_name,
            &CensusReserveState::from_curve_state(state),
            amount,
        )?;
        let completes = candidate
            .events
            .iter()
            .filter(|event| event.is_completion_event)
            .count();
        if completes > 1 {
            return Err("multiple_complete_events_for_candidate".to_owned());
        }
        Ok(full_state_from_reserves(reserves, completes == 1))
    };

    let first_is_create = matches!(
        chain[0].effect,
        PumpExactStateInstructionEffectV2::SupportedExactCreate
    );

    if first_is_create {
        if chain.len() < 2 {
            return irreducible(
                &mut reasons,
                "create_chain_without_following_mutation".to_owned(),
            );
        }
        if chain.iter().skip(1).any(|candidate| {
            matches!(
                candidate.effect,
                PumpExactStateInstructionEffectV2::SupportedExactCreate
            )
        }) {
            reasons.push("duplicate_same_curve_create_in_chain".to_owned());
            return (CensusRecoverability::UnsupportedSequence, reasons);
        }
        // Backward pass from the final anchor through every trade gives the
        // bounded set of possible Create genesis states. Integer rounding can
        // make one local inverse step non-unique; rejecting at that step would
        // be premature because preserved adjacent Event-CPI state, genesis
        // invariants and the full forward replay can eliminate all but one.
        // Keep every bounded proposal until the whole chain is proven unique.
        let mut possible_states =
            BTreeSet::from([CensusReserveState::from_curve_state(&final_anchor.state)]);
        let mut inverse_bound_encountered = false;
        let mut inverse_failure_reasons = BTreeSet::new();
        // Invert every trade in reverse locator order, from the final anchor
        // down to the Create. `chain[1..]` excludes the leading Create; the
        // reversed iteration then yields candidates n-1 .. 1.
        for (idx, candidate) in chain[1..].iter().enumerate().rev() {
            let position = idx + 1;
            let family = match census_transition_family(&candidate.instruction_name) {
                Some(value) => value,
                None => {
                    return irreducible(
                        &mut reasons,
                        format!("transition_family_unknown:{}", candidate.instruction_name),
                    )
                }
            };
            let amount = match census_transition_amount_at(chain, position, None) {
                Ok(value) => value,
                Err(error) => return irreducible(&mut reasons, error),
            };
            let completes = candidate
                .events
                .iter()
                .filter(|event| event.is_completion_event)
                .count();
            let mut preceding_states = BTreeSet::new();
            for after in possible_states {
                let after_full = full_state_from_reserves(after, completes == 1);
                if let Err(error) = verify_event_bindings(&after_full, candidate) {
                    inverse_failure_reasons
                        .insert(format!("post_event_binding_mismatch_at:{position}:{error}"));
                    continue;
                }
                let proposals = match census_inverse_proposal(family, &after, amount) {
                    Ok(solutions) => solutions,
                    Err(error)
                        if error.starts_with("inverse_preimage_cardinality_exceeds_bound:") =>
                    {
                        inverse_bound_encountered = true;
                        inverse_failure_reasons.insert(format!("{error}:at:{position}"));
                        continue;
                    }
                    Err(error) => {
                        inverse_failure_reasons.insert(format!("{error}:at:{position}"));
                        continue;
                    }
                };
                for proposed in proposals {
                    let verified =
                        match census_shared_forward(&candidate.instruction_name, &proposed, amount)
                        {
                            Ok(verified) if verified == after => verified,
                            Ok(_) => {
                                inverse_failure_reasons.insert(format!(
                                "inverse_proposal_fails_shared_forward_verification_at:{position}"
                            ));
                                continue;
                            }
                            Err(error) => {
                                inverse_failure_reasons.insert(format!("{error}:at:{position}"));
                                continue;
                            }
                        };
                    debug_assert_eq!(verified, after);
                    let preceding_full = full_state_from_reserves(proposed, false);
                    if let Err(error) = verify_event_bindings(&preceding_full, &chain[position - 1])
                    {
                        inverse_failure_reasons.insert(format!(
                            "preceding_event_binding_mismatch_at:{}:{error}",
                            position - 1
                        ));
                        continue;
                    }
                    preceding_states.insert(proposed);
                    if preceding_states.len()
                        > usize::try_from(CENSUS_MAX_INVERSE_PREIMAGE_CANDIDATES)
                            .unwrap_or(usize::MAX)
                    {
                        inverse_bound_encountered = true;
                        break;
                    }
                }
                if inverse_bound_encountered {
                    break;
                }
            }
            if inverse_bound_encountered {
                reasons.extend(inverse_failure_reasons);
                reasons.push(format!("bounded_chain_preimage_exceeded_at:{position}"));
                return (CensusRecoverability::AmbiguousOrderOrParent, reasons);
            }
            if preceding_states.is_empty() {
                reasons.extend(inverse_failure_reasons);
                return irreducible(
                    &mut reasons,
                    format!("inverse_transition_unsolvable_at:{position}"),
                );
            }
            possible_states = preceding_states;
        }
        // Genesis invariants plus a complete forward replay decide the whole
        // chain. Only one bit-exact genesis/intermediate trajectory may pass.
        let mut valid_genesis_states = BTreeSet::new();
        for state in possible_states {
            if state.real_quote_reserves != 0
                || state.virtual_quote_reserves == 0
                || state.virtual_token_reserves == 0
            {
                continue;
            }
            let genesis_state = full_state_from_reserves(state, false);
            if verify_event_bindings(&genesis_state, &chain[0]).is_err() {
                continue;
            }
            let mut current = genesis_state;
            let mut valid = true;
            for (position, candidate) in chain.iter().enumerate().skip(1) {
                current = match forward_step(&current, position, candidate) {
                    Ok(value) => value,
                    Err(_) => {
                        valid = false;
                        break;
                    }
                };
                if verify_event_bindings(&current, candidate).is_err() {
                    valid = false;
                    break;
                }
            }
            if valid && current == final_anchor.state {
                valid_genesis_states.insert(state);
            }
        }
        match valid_genesis_states.len() {
            0 => {
                return irreducible(
                    &mut reasons,
                    "no_genesis_state_passes_full_bit_exact_chain".to_owned(),
                )
            }
            1 => {}
            count => {
                reasons.push(format!("ambiguous_full_chain_genesis_count:{count}"));
                return (CensusRecoverability::AmbiguousOrderOrParent, reasons);
            }
        }
        reasons.push("create_genesis_derived_and_bit_exact_verified".to_owned());
        return (
            CensusRecoverability::RecoverableByHashPinnedTransition,
            reasons,
        );
    }

    // Trade-first chain: requires the exact pre-transaction anchor.
    let Some(pre_anchor) = anchors.unique_pre_anchor(curve, slot, tx_index) else {
        return irreducible(&mut reasons, "missing_exact_pre_anchor".to_owned());
    };
    if pre_anchor.state.complete {
        reasons.push("pre_anchor_complete_true".to_owned());
        return (CensusRecoverability::UnsupportedSequence, reasons);
    }
    if !carry_fields_agree(&pre_anchor.state, &final_anchor.state) {
        reasons.push("carry_field_changed_without_authority".to_owned());
        return (CensusRecoverability::UnsupportedSequence, reasons);
    }
    let mut current = pre_anchor.state.clone();
    for (position, candidate) in chain.iter().enumerate() {
        current = match forward_step(&current, position, candidate) {
            Ok(value) => value,
            Err(error) => return irreducible(&mut reasons, error),
        };
        if let Err(error) = verify_event_bindings(&current, candidate) {
            return irreducible(
                &mut reasons,
                format!("intermediate_event_binding_mismatch:{error}"),
            );
        }
    }
    if current != final_anchor.state {
        return irreducible(
            &mut reasons,
            "forward_chain_final_anchor_mismatch".to_owned(),
        );
    }
    reasons.push("forward_chain_bit_exact_verified".to_owned());
    (
        CensusRecoverability::RecoverableByHashPinnedTransition,
        reasons,
    )
}

fn census_verify_raw_integrity(raw_dir: &Path) {
    let mut partial_present = false;
    for entry in fs::read_dir(raw_dir).expect("read preserved raw directory") {
        let entry = entry.expect("raw directory entry");
        if entry.file_name().to_string_lossy().contains(".partial") {
            partial_present = true;
        }
    }
    assert!(
        !partial_present,
        "preserved raw must not contain .partial artifacts before census use"
    );
    let start_digest = digest_private_artifact_v2(&raw_dir.join("run_start_manifest_v2.json"))
        .expect("digest preserved start manifest");
    assert_eq!(
        start_digest.sha256, CENSUS_EXPECTED_START_MANIFEST_SHA256,
        "preserved raw start manifest hash drifted before census"
    );
    let completion_digest =
        digest_private_artifact_v2(&raw_dir.join("run_completion_receipt_v2.json"))
            .expect("digest preserved completion receipt");
    assert_eq!(
        completion_digest.sha256, CENSUS_EXPECTED_COMPLETION_RECEIPT_SHA256,
        "preserved raw completion receipt hash drifted before census"
    );
}

/// Load the receipt-bound residual population from the baseline exact output:
/// every scoped candidate rejected under the retired blanket rules, keyed by
/// (signature, candidate position within its coverage row).  The census must
/// classify exactly this set, each candidate exactly once.
fn census_baseline_residual_keys(
    receipt_value: &serde_json::Value,
) -> (
    BTreeMap<(String, usize), CensusBaselineResidualClass>,
    u64,
    u64,
) {
    let baseline_dir = Path::new(CENSUS_BASELINE_EXACT_DIR);
    let coverage_path = baseline_dir.join("coverage_v2.jsonl");
    let coverage_digest =
        digest_private_artifact_v2(&coverage_path).expect("digest baseline coverage artifact");
    let expected_coverage_sha256 = receipt_value["coverage_artifact"]["sha256"]
        .as_str()
        .expect("baseline receipt binds coverage digest");
    assert_eq!(
        coverage_digest.sha256, expected_coverage_sha256,
        "baseline coverage artifact digest differs from the receipt binding"
    );
    let file = fs::File::open(&coverage_path).expect("open baseline coverage artifact");
    let mut residual_keys: BTreeMap<(String, usize), CensusBaselineResidualClass> = BTreeMap::new();
    let mut multi_residual = 0u64;
    let mut inventory_residual = 0u64;
    for line in std::io::BufReader::new(file).lines() {
        let line = line.expect("baseline coverage line readable");
        let record: PumpExactStateCoverageRecordV2 =
            serde_json::from_str(&line).expect("baseline coverage row parses");
        for (position, candidate) in record.candidates.iter().enumerate() {
            if !matches!(
                candidate.qualification_scope,
                PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort
            ) || !candidate.counted_in_qualification_denominator
            {
                continue;
            }
            let class = match candidate.non_exact_reason.as_deref() {
                Some("transaction_has_multiple_reserve_or_dependency_candidates") => {
                    multi_residual += 1;
                    Some(CensusBaselineResidualClass::MultiMutation)
                }
                Some("transaction_mutation_inventory_incomplete") => {
                    inventory_residual += 1;
                    Some(CensusBaselineResidualClass::InventoryIncomplete)
                }
                _ => None,
            };
            if let Some(class) = class {
                assert!(
                    residual_keys
                        .insert((record.signature.clone(), position), class)
                        .is_none(),
                    "baseline residual candidate key appears more than once"
                );
            }
        }
    }
    (residual_keys, multi_residual, inventory_residual)
}

#[test]
#[ignore = "reads the immutable preserved PRXTAPE3 raw; run explicitly with --ignored"]
fn bounded_feasibility_census_of_preserved_multi_mutation_evidence() {
    let raw_dir = Path::new(CENSUS_RAW_DIR);
    census_verify_raw_integrity(raw_dir);
    let census_source_sha256 = hex_bytes(&Sha256::digest(CENSUS_SOURCE_BYTES));
    let census_source_blake3 = hex_bytes(blake3::hash(CENSUS_SOURCE_BYTES).as_bytes());
    let census_source_bytes =
        u64::try_from(CENSUS_SOURCE_BYTES.len()).expect("census source length exceeds u64");
    let census_running_executable =
        std::env::current_exe().expect("resolve census test executable");
    let census_running_executable_digest = digest_private_artifact_v2(&census_running_executable)
        .expect("digest census test executable");

    // Baseline binding: every baseline counter comes from the receipt with
    // the pinned SHA-256, never from a census-local constant.
    let baseline_receipt_path =
        Path::new(CENSUS_BASELINE_EXACT_DIR).join("exact_state_capability_v2.json");
    let baseline_receipt_digest = digest_private_artifact_v2(&baseline_receipt_path)
        .expect("digest baseline capability receipt");
    assert_eq!(
        baseline_receipt_digest.sha256, CENSUS_BASELINE_RECEIPT_SHA256,
        "baseline exact receipt SHA-256 drifted; census baseline binding broken"
    );
    let receipt_value: serde_json::Value = serde_json::from_slice(
        &fs::read(&baseline_receipt_path).expect("read baseline capability receipt"),
    )
    .expect("parse baseline capability receipt");
    let baseline_global = receipt_value["successful_rooted_candidate_count"]
        .as_u64()
        .expect("receipt global candidate count") as u64;
    let baseline_denominator = receipt_value["successful_rooted_mutation_denominator"]
        .as_u64()
        .expect("receipt scoped denominator") as u64;
    let baseline_exact = receipt_value["exact_rooted_mutation_count"]
        .as_u64()
        .expect("receipt exact count") as u64;
    let baseline_non_exact = receipt_value["explicit_non_exact_mutation_count"]
        .as_u64()
        .expect("receipt non-exact count") as u64;
    let baseline_coverage_ppm = receipt_value["exact_rooted_coverage_ppm"]
        .as_u64()
        .expect("receipt coverage ppm") as u64;
    let baseline_required_ppm = receipt_value["required_exact_rooted_coverage_ppm"]
        .as_u64()
        .expect("receipt required ppm") as u64;
    assert_eq!(baseline_global, 23_908, "baseline global universe changed");
    assert_eq!(
        baseline_denominator, 8_408,
        "baseline scoped denominator changed"
    );
    assert_eq!(baseline_exact, 7_843, "baseline exact count changed");
    assert_eq!(
        baseline_non_exact,
        CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES + CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES,
        "baseline explicit non-exact count changed"
    );
    assert_eq!(baseline_coverage_ppm, 932_802, "baseline coverage changed");
    assert_eq!(baseline_required_ppm, 999_000, "required coverage changed");
    let (baseline_residual_keys, baseline_multi_residual, baseline_inventory_residual) =
        census_baseline_residual_keys(&receipt_value);
    assert_eq!(
        baseline_multi_residual, CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES,
        "receipt-bound coverage must contain exactly 402 multi-mutation residual candidates"
    );
    assert_eq!(
        baseline_inventory_residual, CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES,
        "receipt-bound coverage must contain exactly 163 inventory-incomplete residual candidates"
    );
    assert_eq!(
        baseline_residual_keys.len(),
        (CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES + CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES)
            as usize,
        "every residual candidate key must be unique"
    );

    let unsealed_raw =
        index_prospective_exact_state_raw_run_v2(raw_dir).expect("index preserved raw");
    let manifest_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../../configs/research/pump_exact_state_semantics_manifest_v2.json");
    let semantics = load_pump_exact_state_semantics_authority_v2(&manifest_path)
        .expect("load pinned semantics authority");
    validate_raw_semantics_binding_v2(&unsealed_raw.start_manifest, &semantics)
        .expect("raw manifest binds pinned semantics");
    semantics
        .validate_program_data(&unsealed_raw.start_manifest.program_data_at_start)
        .expect("raw ProgramData matches pinned semantics authority");

    // Record reads require the same anonymous-snapshot sealing the qualify
    // path uses.  The snapshot is a census-local copy of the immutable raw
    // segments inside a private temporary directory; it never touches the
    // preserved raw directory.  Storage precheck: the snapshot filesystem
    // must hold the full 7.194 GB segment set plus a 14 GB floor.
    let census_snapshot_parent = tempfile::tempdir().expect("private census snapshot parent");
    let total_segment_bytes = unsealed_raw
        .total_receipt_bound_segment_bytes()
        .expect("receipt-bound segment byte total");
    // The storage floor comes from the preserved raw start manifest
    // (`min_free_bytes`), plus the same bounded 64 MiB metadata allowance the
    // qualifier uses.  The check runs here, after compilation, immediately
    // before the snapshot copy.
    let manifest_storage_floor = unsealed_raw.start_manifest.min_free_bytes;
    let required_snapshot_bytes = total_segment_bytes
        .checked_add(manifest_storage_floor)
        .and_then(|value| value.checked_add(V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES))
        .expect("census storage requirement overflow");
    let available_snapshot_bytes =
        available_v2_qualification_filesystem_bytes(census_snapshot_parent.path())
            .expect("census snapshot filesystem availability");
    assert!(
        available_snapshot_bytes >= required_snapshot_bytes,
        "census snapshot filesystem has {} bytes available; need {} (segment set {} + manifest floor {} + bounded metadata allowance {})",
        available_snapshot_bytes,
        required_snapshot_bytes,
        total_segment_bytes,
        manifest_storage_floor,
        V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES
    );
    let raw = unsealed_raw
        .seal_anonymous_snapshot_v2(census_snapshot_parent.path())
        .expect("seal census snapshot");

    let rooted_slots = raw.rooted_slots();
    let anchors = PumpExactStateAnchorIndexV2::build(&raw, &semantics, &rooted_slots)
        .expect("build anchor index");
    let cohort_slots_strictly_after = raw
        .completion_receipt
        .cohort_slots_strictly_after
        .expect("verified raw has cohort boundary");
    let mut transactions = raw.transactions.clone();
    transactions.sort_by_key(|transaction| {
        (
            transaction.slot,
            transaction.tx_index,
            transaction.signature,
        )
    });
    let prospective_birth_cohort = build_prospective_birth_cohort_v2(
        &raw,
        &semantics,
        &anchors,
        &rooted_slots,
        cohort_slots_strictly_after,
        &transactions,
    )
    .expect("build prospective birth cohort");

    let mut census_transactions: Vec<CensusTransaction> = Vec::new();
    let mut chain_class_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut reason_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut layout_class_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut multi_tx_count = 0u64;
    let mut multi_cohort_candidates = 0u64;
    let mut inventory_tx_count = 0u64;
    let mut inventory_cohort_candidates = 0u64;
    let mut recoverable_multi = 0u64;
    let mut recoverable_layout_fix = 0u64;
    let mut layout_resolved_event_unknown_count = 0u64;
    let mut unresolvable_unknown_occurrence_count = 0u64;
    let mut duplicate_classification_count = 0u64;
    let mut census_residual_keys: BTreeSet<(String, usize)> = BTreeSet::new();
    let baseline_residual_signatures = baseline_residual_keys
        .keys()
        .map(|(signature, _)| signature.clone())
        .collect::<BTreeSet<_>>();
    let mut projected_scoped_denominator = 0u64;
    let mut projected_exact_count = 0u64;
    let mut projected_explicit_non_exact_count = 0u64;
    let mut projected_global_unknown_occurrence_count = 0u64;
    let mut projected_global_malformed_candidate_count = 0u64;
    let mut projected_global_dependency_candidate_count = 0u64;
    let mut projected_unscoped_curve_candidate_count = 0u64;
    let mut projected_scope_incomplete_occurrence_count = 0u64;
    let mut projected_global_blocker_samples = Vec::new();

    for indexed in &transactions {
        // No silent skips: the qualify path treats every one of these steps
        // as a hard error, and the census conservation proof requires the
        // identical typed behavior.
        let transaction = raw
            .read_transaction(indexed)
            .expect("census read_transaction must match qualify semantics");
        let context = decode_v2_transaction_context(&transaction)
            .expect("census transaction context decode must match qualify semantics");
        let inventory = inventory_v2_from_transaction_context(&context, &semantics, &anchors)
            .expect("census inventory must match qualify semantics");
        let structural_inventory =
            structural_inventory_v2_from_transaction_context(&context, &semantics)
                .expect("census structural inventory must match qualify semantics");
        let rooted = rooted_slots.contains(&inventory.slot);
        let successful_rooted = rooted && inventory.success;
        if !successful_rooted {
            continue;
        }
        let mutation_stats = transaction_mutation_stats_v2(&inventory)
            .expect("census mutation stats must match qualify semantics");
        let candidates = candidate_descriptors_v2(&inventory)
            .expect("census candidate descriptors must match qualify semantics");
        let signature_string = bs58::encode(inventory.signature).into_string();
        let candidate_scopes = candidates
            .iter()
            .map(|candidate| {
                prospective_birth_cohort.candidate_scope(
                    rooted,
                    inventory.success,
                    inventory.slot,
                    candidate.effect,
                    candidate.bonding_curve,
                )
            })
            .collect::<Vec<_>>();
        let mut production_evaluations =
            evaluate_transaction_candidates_v2(&inventory, &candidates, &anchors, &semantics)
                .expect("production transaction-local evaluator must classify every candidate");
        assert_eq!(
            production_evaluations.len(),
            candidates.len(),
            "production transaction-local evaluator lost a candidate"
        );
        for ((candidate, scope), evaluation) in candidates
            .iter()
            .zip(candidate_scopes.iter().copied())
            .zip(production_evaluations.iter_mut())
        {
            // Keep the census arithmetic identical to production: only the
            // selected first Create for a prospective curve may become its
            // exact birth.
            if matches!(
                scope,
                PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort
            ) && matches!(
                candidate.effect,
                PumpExactStateInstructionEffectV2::SupportedExactCreate
            ) && !candidate.bonding_curve.is_some_and(|curve| {
                prospective_birth_cohort.is_selected_birth(curve, &candidate.order)
            }) {
                *evaluation =
                    blocked_candidate_evaluation_v2("duplicate_or_noninitial_prospective_create");
            }
            match scope {
                PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort => {
                    projected_scoped_denominator = projected_scoped_denominator
                        .checked_add(1)
                        .expect("projected denominator overflow");
                    if evaluation.exact {
                        projected_exact_count = projected_exact_count
                            .checked_add(1)
                            .expect("projected exact count overflow");
                    } else {
                        projected_explicit_non_exact_count = projected_explicit_non_exact_count
                            .checked_add(1)
                            .expect("projected non-exact count overflow");
                    }
                }
                PumpExactStateCandidateQualificationScopeV2::GlobalDependencyBlocker => {
                    projected_global_dependency_candidate_count =
                        projected_global_dependency_candidate_count
                            .checked_add(1)
                            .expect("projected global-dependency count overflow");
                    if projected_global_blocker_samples.len() < 64 {
                        projected_global_blocker_samples.push(format!(
                            "{signature_string}:global_dependency:{}",
                            exact_state_effect_label_v2(candidate.effect)
                        ));
                    }
                }
                PumpExactStateCandidateQualificationScopeV2::UnscopedCurveMutationBlocker => {
                    projected_unscoped_curve_candidate_count =
                        projected_unscoped_curve_candidate_count
                            .checked_add(1)
                            .expect("projected unscoped-candidate count overflow");
                    if projected_global_blocker_samples.len() < 64 {
                        projected_global_blocker_samples.push(format!(
                            "{signature_string}:unscoped:{}:{:?}",
                            exact_state_effect_label_v2(candidate.effect),
                            candidate.failure_reason
                        ));
                    }
                }
                PumpExactStateCandidateQualificationScopeV2::OutsideRootedSuccessfulUniverse
                | PumpExactStateCandidateQualificationScopeV2::PreBoundaryOutOfScope
                | PumpExactStateCandidateQualificationScopeV2::PreExistingCurveOutOfScope => {}
            }
            if (!candidate.instruction_payload_exact || !candidate.account_vector_exact)
                && matches!(
                    scope,
                    PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort
                        | PumpExactStateCandidateQualificationScopeV2::GlobalDependencyBlocker
                        | PumpExactStateCandidateQualificationScopeV2::UnscopedCurveMutationBlocker
                )
            {
                projected_scope_incomplete_occurrence_count =
                    projected_scope_incomplete_occurrence_count
                        .checked_add(1)
                        .expect("projected scope-incomplete count overflow");
                if projected_global_blocker_samples.len() < 64 {
                    projected_global_blocker_samples.push(format!(
                        "{signature_string}:scope_incomplete:{}:{:?}",
                        exact_state_effect_label_v2(candidate.effect),
                        candidate.failure_reason
                    ));
                }
            }
        }
        projected_global_unknown_occurrence_count = projected_global_unknown_occurrence_count
            .checked_add(u64::from(mutation_stats.unknown_occurrence_count))
            .expect("projected unknown-occurrence count overflow");
        projected_global_malformed_candidate_count = projected_global_malformed_candidate_count
            .checked_add(u64::from(mutation_stats.malformed_candidate_count))
            .expect("projected malformed-candidate count overflow");
        projected_scope_incomplete_occurrence_count = projected_scope_incomplete_occurrence_count
            .checked_add(u64::from(mutation_stats.unknown_occurrence_count))
            .expect("projected scope-incomplete count overflow");
        if mutation_stats.unknown_occurrence_count != 0
            && projected_global_blocker_samples.len() < 64
        {
            let reasons = inventory
                .occurrences
                .iter()
                .filter_map(|occurrence| match &occurrence.class {
                    PumpExactStateOccurrenceClassV2::Unknown { reason } => Some(reason.as_str()),
                    _ => None,
                })
                .collect::<Vec<_>>()
                .join("|");
            projected_global_blocker_samples.push(format!("{signature_string}:unknown:{reasons}"));
        }
        let cohort_candidate_count = candidate_scopes
            .iter()
            .filter(|scope| {
                matches!(
                    scope,
                    PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort
                )
            })
            .count();
        // The population is selected from the receipt-bound historical
        // baseline, never from the current parser result.  Otherwise a fixed
        // decoder would make its own former residuals disappear from the
        // census before they could be proven recoverable.
        if !baseline_residual_signatures.contains(&signature_string) {
            continue;
        }
        let baseline_multi_positions = baseline_residual_keys
            .iter()
            .filter(|((signature, _), class)| {
                signature == &signature_string
                    && matches!(class, CensusBaselineResidualClass::MultiMutation)
            })
            .count();
        let baseline_inventory_positions = baseline_residual_keys
            .iter()
            .filter(|((signature, _), class)| {
                signature == &signature_string
                    && matches!(class, CensusBaselineResidualClass::InventoryIncomplete)
            })
            .count();
        if baseline_multi_positions != 0 {
            multi_tx_count += 1;
        }
        if baseline_inventory_positions != 0 {
            inventory_tx_count += 1;
        }
        // Collect unknown-occurrence reasons (typed, transaction-level).
        let mut unknown_reasons: Vec<String> = Vec::new();
        for occurrence in &inventory.occurrences {
            if let PumpExactStateOccurrenceClassV2::Unknown { reason } = &occurrence.class {
                if !unknown_reasons.contains(reason) {
                    unknown_reasons.push(reason.clone());
                }
            }
        }

        // Closed layout-fix hypothesis (transaction-level): every malformed
        // candidate is either resolved by one of the two proven closed
        // grammars or keeps a typed unresolved reason.  Unknown occurrences
        // are never layout-resolvable.  Nothing here imputes a value for a
        // missing field: a resolved grammar only supplies decoded argument
        // bytes, and event/anchor authority decides state.
        let layout_resolved_event_unknown_keys = census_layout_resolved_event_unknown_keys(
            &context,
            &inventory,
            &structural_inventory,
            &semantics,
        );
        layout_resolved_event_unknown_count = layout_resolved_event_unknown_count
            .checked_add(
                u64::try_from(layout_resolved_event_unknown_keys.len()).unwrap_or(u64::MAX),
            )
            .expect("layout-resolved Event-CPI unknown count overflow");
        let unresolvable_unknowns = inventory
            .occurrences
            .iter()
            .filter(|occurrence| {
                matches!(
                    occurrence.class,
                    PumpExactStateOccurrenceClassV2::Unknown { .. }
                ) && !layout_resolved_event_unknown_keys.contains(&occurrence.key)
            })
            .collect::<Vec<_>>();
        unresolvable_unknown_occurrence_count = unresolvable_unknown_occurrence_count
            .checked_add(u64::try_from(unresolvable_unknowns.len()).unwrap_or(u64::MAX))
            .expect("unresolvable unknown occurrence count overflow");
        let has_unresolvable_unknown_occurrences = !unresolvable_unknowns.is_empty();
        let mut unresolved_layout_reasons: Vec<String> = Vec::new();
        let mut layout_resolution_by_position: BTreeMap<usize, String> = BTreeMap::new();
        for (position, candidate) in candidates.iter().enumerate() {
            if candidate.instruction_payload_exact && candidate.account_vector_exact {
                continue;
            }
            if !candidate.account_vector_exact {
                unresolved_layout_reasons.push("account_vector_not_layout_resolvable".to_owned());
                continue;
            }
            let occurrence = inventory
                .occurrences
                .iter()
                .find(|occurrence| {
                    PumpExactStateCandidateOrderV2::from_occurrence(&inventory, occurrence)
                        == candidate.order
                })
                .expect("candidate order resolves to its occurrence");
            let instruction_name = semantics
                .instruction(&occurrence.key.discriminator)
                .map(|contract| contract.name.clone())
                .unwrap_or_else(|| "unknown".to_owned());
            let Some(instruction_ref) =
                instruction_ref_for_locator(&context, &census_locator_from_occurrence(occurrence))
            else {
                unresolved_layout_reasons.push("malformed_payload_locator_unresolvable".to_owned());
                continue;
            };
            let payload = &instruction_ref.data[8.min(instruction_ref.data.len())..];
            let class = census_payload_layout_class(&instruction_name, payload, &semantics);
            match class.as_str() {
                "buy_exact_quote_in_v2_trailing_unassigned_boolean"
                | "buy_v2_trailing_unassigned_boolean"
                | "sell_trailing_unassigned_booleans_1"
                | "sell_trailing_unassigned_booleans_2"
                | "create_v2_missing_terminal_optionbool" => {
                    layout_resolution_by_position.insert(position, class);
                }
                other => unresolved_layout_reasons.push(format!("layout_not_proven:{other}")),
            }
        }
        let hypothesis_resolves_transaction = !has_unresolvable_unknown_occurrences
            && unresolved_layout_reasons.is_empty()
            && (mutation_stats.malformed_candidate_count as usize)
                == layout_resolution_by_position.len();

        // Group candidates by curve in stable locator order.
        let mut groups: BTreeMap<Pubkey, Vec<usize>> = BTreeMap::new();
        for (position, candidate) in candidates.iter().enumerate() {
            if let Some(curve) = candidate.bonding_curve {
                groups.entry(curve).or_default().push(position);
            }
        }
        // Chain classification per curve group (cohort-relevant groups).
        let mut chain_class_by_position: BTreeMap<usize, (CensusRecoverability, Vec<String>)> =
            BTreeMap::new();
        for (curve, positions) in &groups {
            let has_cohort = positions.iter().any(|position| {
                matches!(
                    candidate_scopes[*position],
                    PumpExactStateCandidateQualificationScopeV2::ProspectiveBirthCohort
                )
            });
            if !has_cohort {
                continue;
            }
            let chain_inputs = positions
                .iter()
                .map(|position| {
                    let candidate = &candidates[*position];
                    let occurrence = inventory
                        .occurrences
                        .iter()
                        .find(|occurrence| {
                            PumpExactStateCandidateOrderV2::from_occurrence(&inventory, occurrence)
                                == candidate.order
                        })
                        .expect("candidate order resolves to its occurrence");
                    let instruction_name = semantics
                        .instruction(&occurrence.key.discriminator)
                        .map(|contract| contract.name.clone())
                        .unwrap_or_else(|| "unknown".to_owned());
                    let layout_resolution = layout_resolution_by_position.get(position).cloned();
                    // Arguments: strict evidence first; otherwise the closed
                    // layout-fix hypothesis on the preserved payload bytes.
                    let mut args = match &occurrence.class {
                        PumpExactStateOccurrenceClassV2::Candidate {
                            semantic_evidence: Some(evidence),
                            instruction_payload_exact: true,
                            ..
                        } => Some(evidence.argument_fields.clone()),
                        _ => None,
                    };
                    // Under the closed layout-fix hypothesis, reconstruct
                    // parent evidence for Event-CPI binding validation and
                    // supply grammar-resolved arguments.  No state authority
                    // is granted to suffix bytes or to a missing field.
                    let hypothesis_evidence =
                        census_hypothesis_parent_evidence(&context, &semantics, occurrence);
                    if args.is_none() {
                        args = hypothesis_evidence
                            .as_ref()
                            .map(|evidence| evidence.argument_fields.clone());
                    }
                    CensusChainCandidate {
                        effect: candidate.effect,
                        instruction_name,
                        args,
                        events: census_event_matches(
                            &structural_inventory.occurrences,
                            &occurrence.key,
                            &semantics,
                            hypothesis_evidence,
                        ),
                        layout_resolution,
                    }
                })
                .collect::<Vec<_>>();
            let classification = if has_unresolvable_unknown_occurrences {
                // Unknown occurrences carry no curve identity and no closed
                // layout grammar resolves them; the whole transaction stays
                // irreducible with its typed reasons.
                let mut reasons = unresolvable_unknowns
                    .iter()
                    .filter_map(|occurrence| match &occurrence.class {
                        PumpExactStateOccurrenceClassV2::Unknown { reason } => Some(reason.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                reasons.push("unknown_occurrence_not_layout_resolvable".to_owned());
                (
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence,
                    reasons,
                )
            } else if !hypothesis_resolves_transaction {
                (
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence,
                    unresolved_layout_reasons.clone(),
                )
            } else if chain_inputs.len() == 1 {
                classify_single_candidate_standard(
                    &anchors,
                    &chain_inputs[0],
                    inventory.signature,
                    inventory.slot,
                    inventory.tx_index,
                    *curve,
                )
            } else {
                analyze_same_curve_chain(
                    &anchors,
                    inventory.signature,
                    inventory.slot,
                    inventory.tx_index,
                    *curve,
                    &chain_inputs,
                )
            };
            for position in positions {
                chain_class_by_position.insert(*position, classification.clone());
            }
        }

        // The authoritative census verdict is the production evaluator's
        // verdict.  The older hypothesis analysis above remains diagnostic
        // detail only; it is deliberately overwritten here so the reported
        // recoverability proof cannot diverge from the exact qualifier path.
        for (position, evaluation) in production_evaluations.iter().enumerate() {
            let (class, reasons) = if evaluation.exact {
                (
                    CensusRecoverability::RecoverableByHashPinnedTransition,
                    vec!["production_transaction_local_exact_replay_passed".to_owned()],
                )
            } else {
                let reason = evaluation
                    .non_exact_reason
                    .clone()
                    .unwrap_or_else(|| "production_evaluator_non_exact_without_reason".to_owned());
                let class = if reason.contains("unsupported") {
                    CensusRecoverability::UnsupportedSequence
                } else if reason.contains("ambiguous") || reason.contains("ordering") {
                    CensusRecoverability::AmbiguousOrderOrParent
                } else {
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence
                };
                (class, vec![reason])
            };
            chain_class_by_position.insert(position, (class, reasons));
        }

        // Build per-candidate census records.
        let mut census_candidates: Vec<CensusCandidate> = Vec::new();
        for (position, candidate) in candidates.iter().enumerate() {
            let occurrence = inventory
                .occurrences
                .iter()
                .find(|occurrence| {
                    PumpExactStateCandidateOrderV2::from_occurrence(&inventory, occurrence)
                        == candidate.order
                })
                .expect("candidate order resolves");
            let locator = CensusLocator {
                outer_instruction_index: occurrence.key.outer_instruction_index,
                inner_instruction_path: occurrence.key.inner_instruction_path.clone(),
                stack_height: occurrence.key.stack_height,
            };
            let instruction_name = semantics
                .instruction(&occurrence.key.discriminator)
                .map(|contract| contract.name.clone())
                .unwrap_or_else(|| "unknown".to_owned());
            let payload_layout_class = if candidate.instruction_payload_exact {
                "strict_canonical".to_owned()
            } else if let Some(instruction_ref) = instruction_ref_for_locator(&context, &locator) {
                let class = census_payload_layout_class(
                    &instruction_name,
                    &instruction_ref.data[8.min(instruction_ref.data.len())..],
                    &semantics,
                );
                *layout_class_counts.entry(class.clone()).or_insert(0) += 1;
                class
            } else {
                "payload_unavailable".to_owned()
            };
            let events = census_event_matches(
                &structural_inventory.occurrences,
                &occurrence.key,
                &semantics,
                census_hypothesis_parent_evidence(&context, &semantics, occurrence),
            );
            let pre_anchor_available = candidate.bonding_curve.is_some_and(|curve| {
                anchors
                    .unique_pre_anchor(curve, inventory.slot, inventory.tx_index)
                    .is_some()
            });
            let pre_anchor_state = candidate.bonding_curve.and_then(|curve| {
                anchors
                    .unique_pre_anchor(curve, inventory.slot, inventory.tx_index)
                    .map(|anchor| CensusCurveStateEvidence::from(&anchor.state))
            });
            let final_anchor_available = candidate.bonding_curve.is_some_and(|curve| {
                anchors
                    .unique_final_anchor(
                        inventory.signature,
                        curve,
                        inventory.slot,
                        inventory.tx_index,
                    )
                    .is_some()
            });
            let final_anchor_state = candidate.bonding_curve.and_then(|curve| {
                anchors
                    .unique_final_anchor(
                        inventory.signature,
                        curve,
                        inventory.slot,
                        inventory.tx_index,
                    )
                    .map(|anchor| CensusCurveStateEvidence::from(&anchor.state))
            });
            let transition_rule_available = census_transition_rule_available(&instruction_name);
            let (recoverability, chain_reasons) =
                chain_class_by_position.get(&position).cloned().unwrap_or((
                    CensusRecoverability::IrreducibleMissingPrimaryEvidence,
                    vec!["no_curve_group_analysis".to_owned()],
                ));
            let missing_evidence_reason = if matches!(
                recoverability,
                CensusRecoverability::RecoverableByHashPinnedTransition
                    | CensusRecoverability::RecoverableByStrictEventState
            ) {
                None
            } else {
                Some(chain_reasons.join("; "))
            };
            let baseline_class = baseline_residual_keys
                .get(&(signature_string.clone(), position))
                .copied();
            if let Some(baseline_class) = baseline_class {
                match baseline_class {
                    CensusBaselineResidualClass::InventoryIncomplete => {
                        inventory_cohort_candidates += 1;
                        if !census_residual_keys.insert((signature_string.clone(), position)) {
                            duplicate_classification_count += 1;
                        }
                        if matches!(
                            recoverability,
                            CensusRecoverability::RecoverableByHashPinnedTransition
                                | CensusRecoverability::RecoverableByStrictEventState
                        ) {
                            recoverable_layout_fix += 1;
                        }
                    }
                    CensusBaselineResidualClass::MultiMutation => {
                        multi_cohort_candidates += 1;
                        if !census_residual_keys.insert((signature_string.clone(), position)) {
                            duplicate_classification_count += 1;
                        }
                        if matches!(
                            recoverability,
                            CensusRecoverability::RecoverableByHashPinnedTransition
                                | CensusRecoverability::RecoverableByStrictEventState
                        ) {
                            recoverable_multi += 1;
                        }
                    }
                }
                let class_label = format!("{recoverability:?}");
                *chain_class_counts.entry(class_label).or_insert(0) += 1;
                for reason in &chain_reasons {
                    *reason_counts.entry(reason.clone()).or_insert(0) += 1;
                }
            }
            census_candidates.push(CensusCandidate {
                locator,
                effect: exact_state_effect_label_v2(candidate.effect).to_owned(),
                instruction_name,
                instruction_discriminator: hex_bytes(&occurrence.key.discriminator),
                bonding_curve: candidate
                    .bonding_curve
                    .map(|curve| curve.to_string())
                    .unwrap_or_else(|| "absent".to_owned()),
                mint: candidate.mint.map(|mint| mint.to_string()),
                instruction_payload_exact: candidate.instruction_payload_exact,
                account_vector_exact: candidate.account_vector_exact,
                payload_layout_class,
                matched_event_cpis: events
                    .iter()
                    .map(|event| CensusEventEvidence {
                        locator: event.locator.clone(),
                        event_name: event.event_name.clone(),
                        parent_binding_status: event.parent_binding_status.clone(),
                        identity_binding_status: event.identity_binding_status.clone(),
                        bound_state_fields: event.bound_state_fields.clone(),
                        strict_decode_only_fields_present: event
                            .strict_decode_only_fields_present
                            .clone(),
                        decoded_field_borsh_hex: event.decoded_field_borsh_hex.clone(),
                    })
                    .collect(),
                exact_pre_anchor_available: pre_anchor_available,
                exact_pre_anchor_state: pre_anchor_state,
                exact_create_genesis_applicable: matches!(
                    candidate.effect,
                    PumpExactStateInstructionEffectV2::SupportedExactCreate
                ),
                final_same_signature_anchor_available: final_anchor_available,
                final_same_signature_anchor_state: final_anchor_state,
                transition_rule_available,
                recoverability,
                missing_evidence_reason,
            });
        }

        let ordered_candidate_locators = census_candidates
            .iter()
            .map(|candidate| candidate.locator.clone())
            .collect();
        let chain_classification = chain_class_by_position
            .values()
            .map(|(class, _)| *class)
            .max()
            .unwrap_or(CensusRecoverability::IrreducibleMissingPrimaryEvidence);
        let mut chain_reasons_all: Vec<String> = Vec::new();
        for (_, reasons) in chain_class_by_position.values() {
            for reason in reasons {
                if !chain_reasons_all.contains(reason) {
                    chain_reasons_all.push(reason.clone());
                }
            }
        }
        census_transactions.push(CensusTransaction {
            signature: signature_string.clone(),
            slot: inventory.slot,
            tx_index: inventory.tx_index,
            candidate_count: mutation_stats.candidate_count,
            cohort_candidate_count: u32::try_from(cohort_candidate_count).unwrap_or(u32::MAX),
            inventory_complete: !mutation_stats.unknown_or_malformed(),
            unknown_occurrence_reasons: unknown_reasons,
            ordered_candidate_locators,
            candidates: census_candidates,
            chain_classification,
            chain_reasons: chain_reasons_all,
        });
    }

    // Deterministic order: (slot, tx_index, signature) compared field-wise.
    census_transactions.sort_by(|a, b| {
        (a.slot, a.tx_index, a.signature.as_str()).cmp(&(b.slot, b.tx_index, b.signature.as_str()))
    });

    // Strict typed conservation against the receipt-bound residual population:
    // exactly 402 + 163 candidates, each classified exactly once, and the
    // census key set identical to the receipt-bound key set.
    assert_eq!(
        multi_cohort_candidates, CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES,
        "census multi-mutation residual population differs from the receipt-bound 402"
    );
    assert_eq!(
        inventory_cohort_candidates, CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES,
        "census inventory-incomplete residual population differs from the receipt-bound 163"
    );
    assert_eq!(
        census_residual_keys.len(),
        baseline_residual_keys.len(),
        "census residual key count differs from the receipt-bound population"
    );
    let missing_from_census: Vec<&(String, usize)> = baseline_residual_keys
        .keys()
        .filter(|key| !census_residual_keys.contains(*key))
        .collect();
    let extra_in_census: Vec<&(String, usize)> = census_residual_keys
        .iter()
        .filter(|key| !baseline_residual_keys.contains_key(*key))
        .collect();
    assert!(
        missing_from_census.is_empty() && extra_in_census.is_empty(),
        "census residual keys do not exactly match the receipt-bound population: missing={missing_from_census:?} extra={extra_in_census:?}"
    );
    let unclassified_residual_count = u64::try_from(missing_from_census.len()).unwrap_or(u64::MAX);
    let residual_conservation_exact = missing_from_census.is_empty()
        && extra_in_census.is_empty()
        && duplicate_classification_count == 0
        && census_residual_keys.len()
            == (CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES
                + CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES) as usize;
    assert_eq!(
        unclassified_residual_count, 0,
        "every receipt-bound residual candidate must be classified"
    );
    assert_eq!(
        duplicate_classification_count, 0,
        "no residual candidate may be classified more than once"
    );
    assert!(
        residual_conservation_exact,
        "residual conservation must hold exactly: 402 + 163 = 565, each once"
    );

    assert_eq!(
        projected_scoped_denominator,
        projected_exact_count + projected_explicit_non_exact_count,
        "production projected denominator must conserve exactly"
    );
    let projected_required_exact_count =
        (projected_scoped_denominator * baseline_required_ppm + 999_999) / 1_000_000;
    let projected_coverage_ppm = projected_exact_count
        .checked_mul(1_000_000)
        .map(|value| value / projected_scoped_denominator)
        .unwrap_or(0);
    let required_exact_count = projected_required_exact_count;
    let recoverable_residual_count = recoverable_multi + recoverable_layout_fix;
    let irreducible_residual_count = (CENSUS_EXPECTED_MULTI_RESIDUAL_CANDIDATES
        + CENSUS_EXPECTED_INVENTORY_RESIDUAL_CANDIDATES)
        - recoverable_multi
        - recoverable_layout_fix;
    let maximum_provable_exact_count = projected_exact_count;
    let maximum_provable_coverage_ppm = projected_coverage_ppm;
    let production_replay_parity =
        projected_scoped_denominator == projected_exact_count + projected_explicit_non_exact_count;
    let global_blocker_count = projected_global_unknown_occurrence_count
        + projected_global_malformed_candidate_count
        + projected_global_dependency_candidate_count
        + projected_unscoped_curve_candidate_count
        + projected_scope_incomplete_occurrence_count;
    let transition_authority_statement = format!(
        "production replay authority = strict current-IDL parent arguments and budgets + strict manifest-bound Anchor Event-CPI identity/amount/reserve tuples + checked adjacent reserve conservation + bit-exact final same-signature BondingCurve anchor; closed migrations additionally require literal parent/event identity and two observed account states, while creator migration permits only the event-bound creator field to change; legacy migrate requires the preserved native-SOL zero quote_mint literal; Create real-quote state is derived only from the immediately adjacent strict TradeEvent preimage; compatibility suffix bytes and StrictDecodeOnly fields grant no standalone authority; census classifications are the production evaluator results; baseline population bound to receipt sha256 {}",
        CENSUS_BASELINE_RECEIPT_SHA256
    );
    let summary = CensusSummary {
        census_schema_version: CENSUS_SCHEMA_VERSION,
        census_source_sha256,
        census_source_blake3,
        census_source_bytes,
        census_running_executable_sha256: census_running_executable_digest.sha256,
        census_running_executable_blake3: census_running_executable_digest.blake3,
        census_running_executable_bytes: census_running_executable_digest.bytes,
        raw_run_id: raw.start_manifest.run_id.clone(),
        raw_start_manifest_sha256: CENSUS_EXPECTED_START_MANIFEST_SHA256.to_owned(),
        raw_completion_receipt_sha256: CENSUS_EXPECTED_COMPLETION_RECEIPT_SHA256.to_owned(),
        baseline_receipt_sha256: CENSUS_BASELINE_RECEIPT_SHA256.to_owned(),
        baseline_global_successful_rooted_candidate_count: baseline_global,
        baseline_scoped_denominator: baseline_denominator,
        baseline_exact_count: baseline_exact,
        baseline_explicit_non_exact_count: baseline_non_exact,
        baseline_coverage_ppm,
        baseline_required_coverage_ppm: baseline_required_ppm,
        required_exact_count,
        transition_authority_statement,
        multi_mutation_transaction_count: multi_tx_count,
        multi_mutation_cohort_candidate_count: multi_cohort_candidates,
        inventory_incomplete_transaction_count: inventory_tx_count,
        inventory_incomplete_cohort_candidate_count: inventory_cohort_candidates,
        residual_conservation_exact,
        multi_residual_classified: multi_cohort_candidates,
        inventory_residual_classified: inventory_cohort_candidates,
        layout_resolved_event_unknown_count,
        unresolvable_unknown_occurrence_count,
        all_residual_classified_exactly_once: u64::try_from(census_residual_keys.len())
            .unwrap_or(u64::MAX),
        unclassified_residual_count,
        duplicate_classification_count,
        recoverable_multi_candidate_count: recoverable_multi,
        recoverable_after_closed_layout_fix_count: recoverable_layout_fix,
        recoverable_residual_count,
        irreducible_residual_count,
        maximum_provable_exact_count,
        maximum_provable_coverage_ppm,
        projected_scoped_denominator,
        projected_exact_count,
        projected_explicit_non_exact_count,
        projected_coverage_ppm,
        projected_required_exact_count,
        projected_global_unknown_occurrence_count,
        projected_global_malformed_candidate_count,
        projected_global_dependency_candidate_count,
        projected_unscoped_curve_candidate_count,
        projected_scope_incomplete_occurrence_count,
        projected_global_blocker_samples,
        production_replay_parity,
        feasibility_gate_passed: recoverable_residual_count >= 557
            && production_replay_parity
            && projected_exact_count >= projected_required_exact_count
            && global_blocker_count == 0,
        chain_class_counts,
        reason_counts,
        layout_class_counts,
    };
    let report = serde_json::json!({
        "summary": summary,
        "transactions": census_transactions,
    });
    // Create-new publication: the helper refuses to overwrite atomically at
    // open time and forces owner-private report permissions.
    let report_path = census_report_path(&raw.start_manifest.run_id);
    let mut report_file = create_private_census_report(&report_path).unwrap_or_else(|error| {
        panic!(
            "census report {} must be create-new (refusing to overwrite): {error}",
            report_path.display()
        )
    });
    report_file
        .write_all(&serde_json::to_vec_pretty(&report).expect("serialize census report"))
        .expect("write census report");
    report_file.sync_all().expect("sync census report");
    println!(
        "feasibility census report: {}\n{}",
        report_path.display(),
        serde_json::to_string_pretty(&serde_json::json!(summary))
            .expect("serialize census summary")
    );
    assert!(
        recoverable_residual_count >= 557,
        "production replay proves only {recoverable_residual_count}/565 residuals recoverable"
    );
    assert_eq!(
        global_blocker_count, 0,
        "production projection still contains global inventory/scope blockers"
    );
    assert!(
        projected_exact_count >= projected_required_exact_count,
        "production projection remains below the exact coverage gate: {projected_exact_count}/{projected_scoped_denominator}, required {projected_required_exact_count}"
    );
}
