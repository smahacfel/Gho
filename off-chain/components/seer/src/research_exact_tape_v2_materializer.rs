//! Offline-only raw-contract validator for prospective Exact-State Tape V2.
//!
//! This is deliberately a V2 reader, not a V1 conversion layer.  It validates
//! the V2 run controls and every frozen segment before a later exact-state
//! materializer is allowed to interpret one Pump transaction or account
//! update.  It never opens Yellowstone, JSON-RPC, GO-E, or the active Ghost
//! runtime.

use crate::research_exact_tape_v2::{
    classify_pump_owned_account_evidence_v2, program_data_receipts_match_v2,
    PumpExactStateCaptureRunStatusV2, PumpExactStateDigestV2, PumpExactStateRunCompletionReceiptV2,
    PumpExactStateRunStartManifestV2, PumpExactStateSegmentReceiptV2,
    EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION, EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
};
use crate::research_exact_tape_v2_semantics::{
    load_pump_exact_state_semantics_authority_v2, PumpExactStateAccountClassV2,
    PumpExactStateCurveStateV2, PumpExactStateEventFinalStateBindingV2,
    PumpExactStateInstructionEffectV2, PumpExactStateInstructionSemanticEvidenceV2,
    PumpExactStateSemanticsAuthorityV2,
};
use anyhow::{bail, Context, Result};
use ghost_core::{
    pump_research_exact_tape_v2::{
        PumpExactStateBlockMetaEvidenceV2, PumpExactStateFullBlockPayloadChunkV2,
        PumpExactStateFullBlockPayloadCompletedV2, PumpExactStateFullBlockPayloadStartedV2,
        PumpExactStatePumpOwnedAccountUpdateV2, PumpExactStateRawCodecV2,
        PumpExactStateRawRecordV2, PumpExactStateSlotEvidenceV2,
        PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2, PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2,
        PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
    },
    pump_research_tape::{PumpResearchEventTimeV1, PumpResearchStorageHashV1},
};
use prost::Message;
use serde::Serialize;
use sha2::{Digest, Sha256};
use solana_sdk::pubkey::Pubkey;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::{self, DirBuilder, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Path, PathBuf},
    str::FromStr,
    sync::Arc,
};
use yellowstone_grpc_proto::prelude::{
    subscribe_update::UpdateOneof, CommitmentLevel, SubscribeUpdate, SubscribeUpdateTransactionInfo,
};

#[cfg(unix)]
use std::os::unix::{
    ffi::OsStrExt,
    fs::FileExt,
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    io::AsRawFd,
};

const V2_RAW_CONTROL_MAX_BYTES: u64 = 4 * 1024 * 1024;
const V2_WINDOW_EXPORT_MAX_BIRTHS: usize = 1_000_000;
const V2_WINDOW_EXPORT_MAX_RELEVANT_COVERAGE_EVENTS: usize = 8_000_000;
const V2_WINDOW_EXPORT_MAX_JSONL_LINE_BYTES: usize = 16 * 1024 * 1024;
// No V2 raw or Qualified artifact has been admitted.  These prospective
// revisions make monotonic ingress time the duration/cutoff authority and
// bind the forward boundary to a reconciled BlockMeta + full-block frontier.
const PUMP_EXACT_STATE_WINDOW_EXPORT_SCHEMA_VERSION_V2: u16 = 3;
const PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2: u64 = 150_000;
const PUMP_EXACT_STATE_WINDOW_FORWARD_MS_V2: u64 = 90_000;
/// Extra space reserved for the three JSONL streams and two JSON authority
/// files emitted beside an anonymous raw snapshot. The snapshot itself is
/// sized from the immutable receipt set rather than from a mutable directory
/// walk or the capture-time maximum budget.
const V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES: u64 = 64 * 1024 * 1024;
const PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2: u16 = 3;
const PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2: u16 = 2;
const PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2: u64 = 999_000;
const PUMP_EXACT_STATE_MIN_QUALIFICATION_COHORT_ELAPSED_MS_V2: u64 = 1_800_000;
const PUMP_EXACT_STATE_MIN_QUALIFICATION_MUTATION_DENOMINATOR_V2: u64 = 10_000;

/// The result of the pre-semantic V2 raw authority check.  It is intentionally
/// narrow: exact-state qualification will add account, mutation, and coverage
/// evidence later, but it may only begin from this reconciled raw universe.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateRawInspectionSummaryV2 {
    pub run_id: String,
    pub cohort_slots_strictly_after: u64,
    pub segment_count: u64,
    pub source_update_count: u64,
    pub filtered_transaction_count: u64,
    pub full_block_count: u64,
    pub full_block_pump_transaction_count: u64,
    pub pump_owned_account_update_count: u64,
    pub raw_segment_set_blake3: String,
}

/// The only capability result that may later authorize a V2 research export.
/// `Blocked` is a successful, durable qualification result: it preserves the
/// measured diagnostics but intentionally grants no strategy-use authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpExactStateCapabilityStatusV2 {
    Qualified,
    Blocked,
}

/// Typed, fail-closed reasons for a V2 exact-state capability result.  The
/// qualifier never repairs any of these from an RPC, an inferred reserve
/// state, or a later account value.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpExactStateCapabilityBlockerV2 {
    AccountDecodeIncomplete,
    MutationInventoryIncomplete,
    NoRootedCandidateMutation,
    ExactCoverageBelowThreshold,
    NoExactTrajectory,
    NoSuccessfulRootedTradeWithBothStates,
    NoExactBirth,
    CanonicalSlotEvidenceMissing,
    QualificationRunBelowMinimum,
    RawAuthorityRevalidationFailed,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateArtifactDigestV2 {
    sha256: String,
    blake3: String,
    bytes: u64,
    line_count: u64,
    newline_complete: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateCurveStateArtifactV2 {
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

impl From<&PumpExactStateCurveStateV2> for PumpExactStateCurveStateArtifactV2 {
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateTrajectoryRecordV2 {
    schema_version: u16,
    source_run_id: String,
    source_capture_sequence: u64,
    observed_ingress_wall_ms: Option<u64>,
    observed_ingress_monotonic_ms: Option<u64>,
    slot: u64,
    tx_index: u32,
    signature: String,
    bonding_curve: String,
    mint: Option<String>,
    effect: String,
    state_before: Option<PumpExactStateCurveStateArtifactV2>,
    state_after: Option<PumpExactStateCurveStateArtifactV2>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateBirthRecordV2 {
    schema_version: u16,
    source_run_id: String,
    source_capture_sequence: u64,
    observed_ingress_wall_ms: Option<u64>,
    observed_ingress_monotonic_ms: Option<u64>,
    slot: u64,
    tx_index: u32,
    signature: String,
    bonding_curve: String,
    mint: Option<String>,
    initial_state: PumpExactStateCurveStateArtifactV2,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateCandidateCoverageRecordV2 {
    bonding_curve: Option<String>,
    mint: Option<String>,
    effect: String,
    exact: bool,
    non_exact_reason: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateCoverageRecordV2 {
    schema_version: u16,
    source_run_id: String,
    source_capture_sequence: u64,
    observed_ingress_wall_ms: Option<u64>,
    observed_ingress_monotonic_ms: Option<u64>,
    slot: u64,
    tx_index: u32,
    signature: String,
    rooted: bool,
    success: bool,
    occurrence_count: u32,
    candidate_count: u32,
    exact_candidate_count: u32,
    inventory_complete: bool,
    reason_codes: Vec<String>,
    candidates: Vec<PumpExactStateCandidateCoverageRecordV2>,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateCapabilityReceiptV2 {
    schema_version: u16,
    kind: String,
    source_run_id: String,
    status: PumpExactStateCapabilityStatusV2,
    blockers: Vec<PumpExactStateCapabilityBlockerV2>,
    source_storage_format_version: u16,
    source_raw_segment_set_blake3: String,
    source_start_manifest_digest: PumpExactStateDigestV2,
    source_completion_receipt_digest: PumpExactStateDigestV2,
    semantics_id: String,
    semantics_manifest_digest: PumpExactStateDigestV2,
    vendored_idl_digest: PumpExactStateDigestV2,
    materializer_running_executable_digest: PumpExactStateDigestV2,
    cohort_slots_strictly_after: u64,
    rooted_canonical_slot_count: u64,
    filtered_pump_transaction_count: u64,
    full_block_pump_transaction_count: u64,
    pump_owned_account_update_count: u64,
    bonding_curve_account_count: u64,
    bonding_curve_decoded_count: u64,
    global_account_count: u64,
    global_validated_count: u64,
    unknown_pump_owned_account_count: u64,
    account_decode_failure_count: u64,
    successful_rooted_instruction_occurrence_count: u64,
    successful_rooted_proven_non_reserve_count: u64,
    successful_rooted_validated_event_transport_count: u64,
    successful_rooted_candidate_count: u64,
    successful_rooted_unknown_occurrence_count: u64,
    successful_rooted_malformed_candidate_count: u64,
    occurrence_ledger_reconciled: bool,
    successful_rooted_mutation_denominator: u64,
    exact_rooted_mutation_count: u64,
    explicit_non_exact_mutation_count: u64,
    denominator_reconciled: bool,
    exact_rooted_coverage_ppm: u64,
    qualification_run_below_minimum: bool,
    required_exact_rooted_coverage_ppm: u64,
    exact_trajectory_count: u64,
    successful_rooted_exact_trade_with_both_states_count: u64,
    exact_birth_count: u64,
    births_artifact: PumpExactStateArtifactDigestV2,
    trajectories_artifact: PumpExactStateArtifactDigestV2,
    coverage_artifact: PumpExactStateArtifactDigestV2,
}

#[derive(Clone, Debug, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateExactManifestV2 {
    schema_version: u16,
    kind: String,
    source_run_id: String,
    exact_state_capability_status: PumpExactStateCapabilityStatusV2,
    source_raw_segment_set_blake3: String,
    semantics_manifest_sha256: String,
    semantics_manifest_blake3: String,
    materializer_running_executable_sha256: String,
    materializer_running_executable_blake3: String,
    materializer_running_executable_bytes: u64,
    exact_state_capability_artifact: PumpExactStateArtifactDigestV2,
    births_artifact: PumpExactStateArtifactDigestV2,
    trajectories_artifact: PumpExactStateArtifactDigestV2,
    coverage_artifact: PumpExactStateArtifactDigestV2,
}

#[derive(Clone, Debug)]
pub struct PumpExactStateQualificationSummaryV2 {
    pub source_run_id: String,
    pub status: PumpExactStateCapabilityStatusV2,
    pub blockers: Vec<PumpExactStateCapabilityBlockerV2>,
    pub output_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub exact_rooted_coverage_ppm: u64,
    pub exact_rooted_mutation_count: u64,
    pub successful_rooted_mutation_denominator: u64,
}

/// An outcome-blind status for one prospective V2 birth window.  A `Complete`
/// record proves only that exact-state observation evidence and later source
/// availability cover the preregistered interval. It contains no outcome,
/// return, PnL, entry/exit, score, or execution claim.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpExactStateOutcomeBlindWindowStatusV2 {
    Complete,
    MissingBirthObservedTimestamp,
    TruncatedAtRunStart,
    TruncatedAtRunEnd,
    ObservationContainsNonExactMutation,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateOutcomeBlindWindowV2 {
    schema_version: u16,
    source_run_id: String,
    candidate_id: String,
    bonding_curve: String,
    mint: Option<String>,
    time_axis: String,
    source_first_observed_ingress_wall_ms: u64,
    source_first_observed_ingress_monotonic_ms: u64,
    source_reconciled_full_block_frontier_slot: u64,
    source_reconciled_full_block_frontier_ingress_wall_ms: u64,
    source_reconciled_full_block_frontier_ingress_monotonic_ms: u64,
    birth_observed_ingress_wall_ms: Option<u64>,
    birth_observed_ingress_monotonic_ms: Option<u64>,
    observation_end_exclusive_monotonic_ms: Option<u64>,
    forward_coverage_end_monotonic_ms: Option<u64>,
    exact_trade_count_in_observation: u64,
    non_exact_candidate_count_in_observation: u64,
    status: PumpExactStateOutcomeBlindWindowStatusV2,
}

#[derive(Clone, Debug, Serialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateOutcomeBlindWindowManifestV2 {
    schema_version: u16,
    kind: String,
    source_run_id: String,
    exact_state_capability_status: PumpExactStateCapabilityStatusV2,
    outcome_blind: bool,
    time_axis: String,
    observation_ms: u64,
    forward_ms: u64,
    source_first_observed_ingress_wall_ms: u64,
    source_first_observed_ingress_monotonic_ms: u64,
    source_reconciled_full_block_frontier_slot: u64,
    source_reconciled_full_block_frontier_ingress_wall_ms: u64,
    source_reconciled_full_block_frontier_ingress_monotonic_ms: u64,
    source_raw_segment_set_blake3: String,
    semantics_id: String,
    semantics_manifest_digest: PumpExactStateDigestV2,
    vendored_idl_digest: PumpExactStateDigestV2,
    materializer_running_executable_digest: PumpExactStateDigestV2,
    exact_state_capability_artifact: PumpExactStateArtifactDigestV2,
    exact_manifest_artifact: PumpExactStateArtifactDigestV2,
    births_artifact: PumpExactStateArtifactDigestV2,
    trajectories_artifact: PumpExactStateArtifactDigestV2,
    coverage_artifact: PumpExactStateArtifactDigestV2,
    windows_artifact: PumpExactStateArtifactDigestV2,
    exported_birth_count: u64,
    complete_window_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateOutcomeBlindWindowExportSummaryV2 {
    pub source_run_id: String,
    pub output_dir: PathBuf,
    pub exported_birth_count: u64,
    pub complete_window_count: u64,
}

impl PumpExactStateOutcomeBlindWindowExportSummaryV2 {
    #[must_use]
    pub const fn has_complete_window(&self) -> bool {
        self.complete_window_count != 0
    }
}

/// Descriptor-pinned exact-state evidence consumed only by the V2
/// outcome-blind window exporter. It is not a strategy result, outcome, score,
/// execution decision, or active-runtime authority.
pub struct PumpExactStateValidatedStrategyInputV2 {
    pub source_run_id: String,
    pub raw_dir: PathBuf,
    pub exact_dir: PathBuf,
    pub semantics_id: String,
    pub exact_rooted_coverage_ppm: u64,
    raw_segment_set_blake3: String,
    source_availability_bounds: PumpExactStateSourceAvailabilityBoundsV2,
    source_start_manifest_digest: PumpExactStateDigestV2,
    source_completion_receipt_digest: PumpExactStateDigestV2,
    semantics_manifest_path: PathBuf,
    semantics_manifest_digest: PumpExactStateDigestV2,
    vendored_idl_digest: PumpExactStateDigestV2,
    materializer_running_executable_digest: PumpExactStateDigestV2,
    receipt_file: Arc<File>,
    receipt_digest: PumpExactStateArtifactDigestV2,
    manifest_file: Arc<File>,
    manifest_digest: PumpExactStateArtifactDigestV2,
    births_file: Arc<File>,
    births_digest: PumpExactStateArtifactDigestV2,
    trajectories_file: Arc<File>,
    trajectories_digest: PumpExactStateArtifactDigestV2,
    coverage_file: Arc<File>,
    coverage_digest: PumpExactStateArtifactDigestV2,
}

impl PumpExactStateValidatedStrategyInputV2 {
    /// Consume the retained birth JSONL only through the descriptor that was
    /// hashed during authority validation.  A later export must not reopen the
    /// mutable pathname.
    #[must_use]
    pub fn births_file(&self) -> &File {
        &self.births_file
    }

    /// Consume the retained trajectory JSONL only through its pinned
    /// descriptor.
    #[must_use]
    pub fn trajectories_file(&self) -> &File {
        &self.trajectories_file
    }

    /// Consume the retained coverage JSONL only through its pinned
    /// descriptor.
    #[must_use]
    pub fn coverage_file(&self) -> &File {
        &self.coverage_file
    }

    /// Recheck every authority immediately before a future strategy export
    /// atomically publishes its own output.  This method remains read-only and
    /// performs no provider I/O, outcome calculation, or strategy decision.
    pub fn revalidate_before_strategy_export_v2(&self) -> Result<()> {
        let raw = index_prospective_exact_state_raw_run_v2(&self.raw_dir)
            .context("revalidate V2 raw authority before strategy export")?;
        if raw.start_manifest.run_id != self.source_run_id
            || raw.raw_segment_set_blake3 != self.raw_segment_set_blake3
            || raw.source_availability_bounds != self.source_availability_bounds
        {
            bail!("V2 raw authority differs from the qualified exact artifact");
        }
        if artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
            &raw.raw_dir.join("run_start_manifest_v2.json"),
        )?) != self.source_start_manifest_digest
            || artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
                &raw.raw_dir.join("run_completion_receipt_v2.json"),
            )?) != self.source_completion_receipt_digest
        {
            bail!("V2 raw control authority differs from the qualified exact artifact");
        }
        let semantics = load_pump_exact_state_semantics_authority_v2(&self.semantics_manifest_path)
            .context("revalidate V2 semantics authority before strategy export")?;
        validate_raw_semantics_binding_v2(&raw.start_manifest, &semantics)?;
        semantics.validate_program_data(&raw.start_manifest.program_data_at_start)?;
        if semantics.semantics_id != self.semantics_id
            || semantics_digest_to_runtime_v2(&semantics.manifest_digest)
                != self.semantics_manifest_digest
            || semantics_digest_to_runtime_v2(&semantics.idl_digest) != self.vendored_idl_digest
        {
            bail!("V2 semantics authority differs from the qualified exact artifact");
        }
        if digest_v2_running_executable()? != self.materializer_running_executable_digest {
            bail!("V2 running executable differs from the qualified exact artifact");
        }
        revalidate_open_exact_artifact_v2(
            &self.receipt_file,
            &self.receipt_digest,
            "V2 exact capability receipt",
        )?;
        revalidate_open_exact_artifact_v2(
            &self.manifest_file,
            &self.manifest_digest,
            "V2 exact manifest",
        )?;
        revalidate_open_exact_artifact_v2(
            &self.births_file,
            &self.births_digest,
            "V2 exact births JSONL",
        )?;
        revalidate_open_exact_artifact_v2(
            &self.trajectories_file,
            &self.trajectories_digest,
            "V2 exact trajectories JSONL",
        )?;
        revalidate_open_exact_artifact_v2(
            &self.coverage_file,
            &self.coverage_digest,
            "V2 exact coverage JSONL",
        )?;
        Ok(())
    }
}

impl PumpExactStateQualificationSummaryV2 {
    #[must_use]
    pub const fn is_qualified(&self) -> bool {
        matches!(self.status, PumpExactStateCapabilityStatusV2::Qualified)
    }
}

const V2_EXACT_OUTPUT_CONTROL_MAX_BYTES: u64 = 4 * 1024 * 1024;
const V2_EXACT_OUTPUT_ARTIFACT_NAMES: [&str; 5] = [
    "births_v2.jsonl",
    "trajectories_v2.jsonl",
    "coverage_v2.jsonl",
    "exact_state_capability_v2.json",
    "manifest_v2.json",
];

struct PumpExactStateOpenArtifactV2 {
    file: Arc<File>,
    digest: PumpExactStateArtifactDigestV2,
    control_bytes: Option<Vec<u8>>,
}

struct PumpExactStateValidatedExactOutputV2 {
    receipt: PumpExactStateCapabilityReceiptV2,
    receipt_artifact: PumpExactStateOpenArtifactV2,
    manifest: PumpExactStateExactManifestV2,
    manifest_artifact: PumpExactStateOpenArtifactV2,
    births_artifact: PumpExactStateOpenArtifactV2,
    trajectories_artifact: PumpExactStateOpenArtifactV2,
    coverage_artifact: PumpExactStateOpenArtifactV2,
}

/// Validate the full V2 authority chain required before any separately
/// reviewed strategy exporter can read a record.  It is deliberately
/// read-only: success returns descriptor-pinned input, not an export window,
/// outcome, strategy score, or runtime side effect.
pub fn validate_prospective_exact_state_strategy_input_v2(
    raw_dir: &Path,
    semantics_manifest_path: &Path,
    exact_dir: &Path,
) -> Result<PumpExactStateValidatedStrategyInputV2> {
    let raw = index_prospective_exact_state_raw_run_v2(raw_dir)
        .context("validate V2 raw authority for strategy-input adapter")?;
    let semantics = load_pump_exact_state_semantics_authority_v2(semantics_manifest_path)
        .context("load V2 semantics authority for strategy-input adapter")?;
    validate_raw_semantics_binding_v2(&raw.start_manifest, &semantics)?;
    semantics.validate_program_data(&raw.start_manifest.program_data_at_start)?;
    let exact = validate_exact_output_artifacts_v2(exact_dir)?;
    validate_qualified_exact_output_binding_v2(&raw, &semantics, &exact)?;
    let running_executable = digest_v2_running_executable()?;
    if running_executable != exact.receipt.materializer_running_executable_digest {
        bail!("V2 strategy-input adapter must execute the exact artifact materializer image");
    }
    Ok(PumpExactStateValidatedStrategyInputV2 {
        source_run_id: exact.receipt.source_run_id.clone(),
        raw_dir: raw_dir.to_path_buf(),
        exact_dir: exact_dir.to_path_buf(),
        semantics_id: exact.receipt.semantics_id.clone(),
        exact_rooted_coverage_ppm: exact.receipt.exact_rooted_coverage_ppm,
        raw_segment_set_blake3: raw.raw_segment_set_blake3,
        source_availability_bounds: raw.source_availability_bounds,
        source_start_manifest_digest: exact.receipt.source_start_manifest_digest.clone(),
        source_completion_receipt_digest: exact.receipt.source_completion_receipt_digest.clone(),
        semantics_manifest_path: semantics_manifest_path.to_path_buf(),
        semantics_manifest_digest: exact.receipt.semantics_manifest_digest.clone(),
        vendored_idl_digest: exact.receipt.vendored_idl_digest.clone(),
        materializer_running_executable_digest: exact
            .receipt
            .materializer_running_executable_digest
            .clone(),
        receipt_file: exact.receipt_artifact.file,
        receipt_digest: exact.receipt_artifact.digest,
        manifest_file: exact.manifest_artifact.file,
        manifest_digest: exact.manifest_artifact.digest,
        births_file: exact.births_artifact.file,
        births_digest: exact.births_artifact.digest,
        trajectories_file: exact.trajectories_artifact.file,
        trajectories_digest: exact.trajectories_artifact.digest,
        coverage_file: exact.coverage_artifact.file,
        coverage_digest: exact.coverage_artifact.digest,
    })
}

#[derive(Clone, Debug)]
struct PumpExactStateOutcomeBlindWindowAccumulatorV2 {
    birth: PumpExactStateBirthRecordV2,
    exact_trade_count_in_observation: u64,
    non_exact_candidate_count_in_observation: u64,
}

#[derive(Clone, Debug)]
struct PumpExactStateRelevantCoverageEventV2 {
    observed_ingress_monotonic_ms: u64,
    source_capture_sequence: u64,
    bonding_curve: String,
    effect: String,
    exact: bool,
}

/// Create a strategy-neutral, outcome-blind V2 window artifact.  It consumes
/// only descriptor-pinned Qualified evidence and uses the observed ingress
/// monotonic time axis.  Wall-clock timestamps remain audit labels in the
/// emitted artifact, but cannot alter the fixed 150s observation / 90s
/// forward-availability contract.  This is a source-coverage gate, not an
/// outcome calculation: no post-cutoff state, return, PnL, trade simulation,
/// or execution policy is read or emitted.
pub fn export_prospective_exact_state_outcome_blind_windows_v2(
    raw_dir: &Path,
    semantics_manifest_path: &Path,
    exact_dir: &Path,
    output_dir: &Path,
) -> Result<PumpExactStateOutcomeBlindWindowExportSummaryV2> {
    let authority = validate_prospective_exact_state_strategy_input_v2(
        raw_dir,
        semantics_manifest_path,
        exact_dir,
    )?;
    authority.revalidate_before_strategy_export_v2()?;
    validate_v2_outcome_blind_window_output_path(&authority, output_dir)?;

    let mut births = Vec::new();
    visit_pinned_jsonl_v2(
        &authority.births_file,
        &authority.births_digest,
        "V2 exact births JSONL",
        |birth: PumpExactStateBirthRecordV2| {
            if birth.schema_version != PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2
                || birth.source_run_id != authority.source_run_id
            {
                bail!("V2 exact birth row differs from the Qualified source authority");
            }
            if births.len() >= V2_WINDOW_EXPORT_MAX_BIRTHS {
                bail!(
                    "V2 outcome-blind export exceeds bounded birth count {}",
                    V2_WINDOW_EXPORT_MAX_BIRTHS
                );
            }
            births.push(birth);
            Ok(())
        },
    )?;
    if births.is_empty() {
        bail!("V2 Qualified exact artifact has no birth rows for outcome-blind export");
    }

    let mut windows = births
        .into_iter()
        .map(|birth| PumpExactStateOutcomeBlindWindowAccumulatorV2 {
            birth,
            exact_trade_count_in_observation: 0,
            non_exact_candidate_count_in_observation: 0,
        })
        .collect::<Vec<_>>();
    windows.sort_by_key(|window| {
        (
            window.birth.observed_ingress_monotonic_ms,
            window.birth.source_capture_sequence,
            window.birth.slot,
            window.birth.tx_index,
            window.birth.signature.clone(),
        )
    });

    let mut window_by_curve = BTreeMap::new();
    for (index, window) in windows.iter().enumerate() {
        if window_by_curve
            .insert(window.birth.bonding_curve.clone(), index)
            .is_some()
        {
            bail!(
                "V2 Qualified exact artifact has duplicate exact birth authority for curve {}",
                window.birth.bonding_curve
            );
        }
    }

    // The forward boundary comes from the verified, reconciled BlockMeta +
    // full-block frontier, not from a quiet Pump lane and not from a naked
    // Slot update.  This prevents two jointly incomplete Pump inventories
    // from manufacturing outcome-window availability.
    let first_available_observed_monotonic_ms = authority
        .source_availability_bounds
        .first_observed_ingress
        .monotonic_ms;
    let last_available_observed_monotonic_ms = authority
        .source_availability_bounds
        .reconciled_full_block_frontier_ingress
        .monotonic_ms;
    let mut relevant_events = Vec::new();
    visit_pinned_jsonl_v2(
        &authority.coverage_file,
        &authority.coverage_digest,
        "V2 exact coverage JSONL",
        |coverage: PumpExactStateCoverageRecordV2| {
            if coverage.schema_version != PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2
                || coverage.source_run_id != authority.source_run_id
            {
                bail!("V2 exact coverage row differs from the Qualified source authority");
            }
            let observed_ingress_monotonic_ms = coverage
                .observed_ingress_monotonic_ms
                .ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 outcome-blind export requires observed ingress-monotonic timestamps for every filtered Pump transaction"
                )
            })?;
            for candidate in coverage.candidates {
                let bonding_curve = outcome_blind_candidate_curve_v2(&candidate)?;
                if window_by_curve.contains_key(bonding_curve) {
                    if relevant_events.len() >= V2_WINDOW_EXPORT_MAX_RELEVANT_COVERAGE_EVENTS {
                        bail!(
                            "V2 outcome-blind export exceeds bounded relevant coverage-event count {}",
                            V2_WINDOW_EXPORT_MAX_RELEVANT_COVERAGE_EVENTS
                        );
                    }
                    relevant_events.push(PumpExactStateRelevantCoverageEventV2 {
                        observed_ingress_monotonic_ms,
                        source_capture_sequence: coverage.source_capture_sequence,
                        bonding_curve: bonding_curve.to_owned(),
                        effect: candidate.effect,
                        exact: candidate.exact,
                    });
                }
            }
            Ok(())
        },
    )?;
    relevant_events.sort_by_key(|event| {
        (
            event.observed_ingress_monotonic_ms,
            event.source_capture_sequence,
            event.bonding_curve.clone(),
            event.effect.clone(),
        )
    });
    for event in relevant_events {
        let Some(window_index) = window_by_curve.get(&event.bonding_curve).copied() else {
            continue;
        };
        let window = windows
            .get_mut(window_index)
            .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind window index disappeared"))?;
        let Some(birth_time) = window.birth.observed_ingress_monotonic_ms else {
            continue;
        };
        let Some(observation_end) =
            birth_time.checked_add(PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2)
        else {
            continue;
        };
        // The observation interval is half-open. It prevents a source record
        // with the same millisecond timestamp but a later capture sequence
        // from being silently treated as decision-cutoff information.
        if (
            event.observed_ingress_monotonic_ms,
            event.source_capture_sequence,
        ) < (birth_time, window.birth.source_capture_sequence)
            || event.observed_ingress_monotonic_ms >= observation_end
        {
            continue;
        }
        if event.exact && event.effect == "supported_exact_trade" {
            window.exact_trade_count_in_observation = window
                .exact_trade_count_in_observation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind exact-trade count overflow"))?;
        }
        if !event.exact {
            window.non_exact_candidate_count_in_observation = window
                .non_exact_candidate_count_in_observation
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind non-exact count overflow"))?;
        }
    }

    let parent = output_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind window output has no parent"))?;
    let output_name = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .filter(|name| !name.is_empty() && *name != "." && *name != "..")
        .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind window output needs a UTF-8 name"))?;
    let partial_dir = parent.join(format!(".{output_name}.partial"));
    if partial_dir.exists() {
        bail!(
            "V2 outcome-blind window partial output {} already exists; retain it for forensics",
            partial_dir.display()
        );
    }
    let mut builder = DirBuilder::new();
    #[cfg(unix)]
    builder.mode(0o700);
    builder
        .create(&partial_dir)
        .with_context(|| format!("create V2 outcome-blind partial {}", partial_dir.display()))?;
    let windows_path = partial_dir.join("outcome_blind_windows_v2.jsonl");
    let mut windows_writer = BufWriter::new(create_private_output_file_v2(
        &windows_path,
        "outcome-blind windows",
    )?);
    let mut complete_window_count = 0u64;
    for window in &windows {
        let birth_time = window.birth.observed_ingress_monotonic_ms;
        let observation_end =
            birth_time.and_then(|time| time.checked_add(PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2));
        let forward_coverage_end = observation_end
            .and_then(|time| time.checked_add(PUMP_EXACT_STATE_WINDOW_FORWARD_MS_V2));
        let status = outcome_blind_window_status_v2(
            birth_time,
            first_available_observed_monotonic_ms,
            last_available_observed_monotonic_ms,
            window.non_exact_candidate_count_in_observation,
        );
        if matches!(status, PumpExactStateOutcomeBlindWindowStatusV2::Complete) {
            complete_window_count = complete_window_count.checked_add(1).ok_or_else(|| {
                anyhow::anyhow!("V2 outcome-blind complete-window count overflow")
            })?;
        }
        write_jsonl_v2(
            &mut windows_writer,
            &PumpExactStateOutcomeBlindWindowV2 {
                schema_version: PUMP_EXACT_STATE_WINDOW_EXPORT_SCHEMA_VERSION_V2,
                source_run_id: authority.source_run_id.clone(),
                candidate_id: format!(
                    "{}:{}:{}",
                    authority.source_run_id, window.birth.signature, window.birth.bonding_curve
                ),
                bonding_curve: window.birth.bonding_curve.clone(),
                mint: window.birth.mint.clone(),
                time_axis: "observed_ingress_monotonic_ms".to_owned(),
                source_first_observed_ingress_wall_ms: authority
                    .source_availability_bounds
                    .first_observed_ingress
                    .wall_ms,
                source_first_observed_ingress_monotonic_ms: first_available_observed_monotonic_ms,
                source_reconciled_full_block_frontier_slot: authority
                    .source_availability_bounds
                    .reconciled_full_block_frontier_slot,
                source_reconciled_full_block_frontier_ingress_wall_ms: authority
                    .source_availability_bounds
                    .reconciled_full_block_frontier_ingress
                    .wall_ms,
                source_reconciled_full_block_frontier_ingress_monotonic_ms:
                    last_available_observed_monotonic_ms,
                birth_observed_ingress_wall_ms: window.birth.observed_ingress_wall_ms,
                birth_observed_ingress_monotonic_ms: birth_time,
                observation_end_exclusive_monotonic_ms: observation_end,
                forward_coverage_end_monotonic_ms: forward_coverage_end,
                exact_trade_count_in_observation: window.exact_trade_count_in_observation,
                non_exact_candidate_count_in_observation: window
                    .non_exact_candidate_count_in_observation,
                status,
            },
        )?;
    }
    sync_jsonl_v2(windows_writer)?;
    let windows_artifact = digest_private_artifact_v2(&windows_path)?;
    let manifest_path = partial_dir.join("manifest_v2.json");
    write_json_create_new_v2(
        &manifest_path,
        &PumpExactStateOutcomeBlindWindowManifestV2 {
            schema_version: PUMP_EXACT_STATE_WINDOW_EXPORT_SCHEMA_VERSION_V2,
            kind: "pump_exact_state_outcome_blind_windows_v2".to_owned(),
            source_run_id: authority.source_run_id.clone(),
            exact_state_capability_status: PumpExactStateCapabilityStatusV2::Qualified,
            outcome_blind: true,
            time_axis: "observed_ingress_monotonic_ms".to_owned(),
            observation_ms: PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2,
            forward_ms: PUMP_EXACT_STATE_WINDOW_FORWARD_MS_V2,
            source_first_observed_ingress_wall_ms: authority
                .source_availability_bounds
                .first_observed_ingress
                .wall_ms,
            source_first_observed_ingress_monotonic_ms: first_available_observed_monotonic_ms,
            source_reconciled_full_block_frontier_slot: authority
                .source_availability_bounds
                .reconciled_full_block_frontier_slot,
            source_reconciled_full_block_frontier_ingress_wall_ms: authority
                .source_availability_bounds
                .reconciled_full_block_frontier_ingress
                .wall_ms,
            source_reconciled_full_block_frontier_ingress_monotonic_ms:
                last_available_observed_monotonic_ms,
            source_raw_segment_set_blake3: authority.raw_segment_set_blake3.clone(),
            semantics_id: authority.semantics_id.clone(),
            semantics_manifest_digest: authority.semantics_manifest_digest.clone(),
            vendored_idl_digest: authority.vendored_idl_digest.clone(),
            materializer_running_executable_digest: authority
                .materializer_running_executable_digest
                .clone(),
            exact_state_capability_artifact: authority.receipt_digest.clone(),
            exact_manifest_artifact: authority.manifest_digest.clone(),
            births_artifact: authority.births_digest.clone(),
            trajectories_artifact: authority.trajectories_digest.clone(),
            coverage_artifact: authority.coverage_digest.clone(),
            windows_artifact: windows_artifact.clone(),
            exported_birth_count: u64::try_from(windows.len()).unwrap_or(u64::MAX),
            complete_window_count,
        },
        "V2 outcome-blind window manifest",
    )?;
    let manifest_digest = digest_private_artifact_v2(&manifest_path)?;
    if !windows_artifact.newline_complete || !manifest_digest.newline_complete {
        bail!("V2 outcome-blind window artifact is not newline-complete");
    }
    sync_directory_v2(&partial_dir)?;
    authority.revalidate_before_strategy_export_v2()?;
    validate_v2_outcome_blind_window_output_path(&authority, output_dir)?;
    fs::rename(&partial_dir, output_dir).with_context(|| {
        format!(
            "atomically publish V2 outcome-blind windows {} -> {}",
            partial_dir.display(),
            output_dir.display()
        )
    })?;
    sync_directory_v2(parent)?;
    Ok(PumpExactStateOutcomeBlindWindowExportSummaryV2 {
        source_run_id: authority.source_run_id,
        output_dir: output_dir.to_path_buf(),
        exported_birth_count: u64::try_from(windows.len()).unwrap_or(u64::MAX),
        complete_window_count,
    })
}

fn outcome_blind_window_status_v2(
    birth_observed_ingress_monotonic_ms: Option<u64>,
    first_available_observed_ms: u64,
    last_available_observed_ms: u64,
    non_exact_candidate_count_in_observation: u64,
) -> PumpExactStateOutcomeBlindWindowStatusV2 {
    let Some(birth_time) = birth_observed_ingress_monotonic_ms else {
        return PumpExactStateOutcomeBlindWindowStatusV2::MissingBirthObservedTimestamp;
    };
    if birth_time < first_available_observed_ms {
        return PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunStart;
    }
    let Some(observation_end) = birth_time.checked_add(PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2)
    else {
        return PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunEnd;
    };
    let Some(forward_coverage_end) =
        observation_end.checked_add(PUMP_EXACT_STATE_WINDOW_FORWARD_MS_V2)
    else {
        return PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunEnd;
    };
    if forward_coverage_end > last_available_observed_ms {
        return PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunEnd;
    }
    if non_exact_candidate_count_in_observation != 0 {
        return PumpExactStateOutcomeBlindWindowStatusV2::ObservationContainsNonExactMutation;
    }
    PumpExactStateOutcomeBlindWindowStatusV2::Complete
}

/// A Qualified receipt may retain a bounded number of explicit non-exact
/// candidates.  A window may only exclude those candidates when the frozen
/// coverage record names the affected curve.  Silently ignoring an
/// unattributed candidate would create a superficially complete window despite
/// evidence whose state impact cannot be scoped from the source tape.
fn outcome_blind_candidate_curve_v2(
    candidate: &PumpExactStateCandidateCoverageRecordV2,
) -> Result<&str> {
    candidate.bonding_curve.as_deref().ok_or_else(|| {
        anyhow::anyhow!(
            "V2 outcome-blind export cannot scope {} candidate without a BondingCurve identity",
            if candidate.exact {
                "exact"
            } else {
                "non-exact"
            }
        )
    })
}

fn validate_v2_outcome_blind_window_output_path(
    authority: &PumpExactStateValidatedStrategyInputV2,
    output_dir: &Path,
) -> Result<()> {
    validate_v2_exact_output_path(&authority.raw_dir, output_dir)?;
    let exact_dir = fs::canonicalize(&authority.exact_dir).with_context(|| {
        format!(
            "canonicalize V2 Qualified exact directory {}",
            authority.exact_dir.display()
        )
    })?;
    let parent = output_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind window output has no parent"))?;
    let canonical_parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "canonicalize V2 outcome-blind window parent {}",
            parent.display()
        )
    })?;
    let final_candidate = canonical_parent.join(
        output_dir
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("V2 outcome-blind output lacks final component"))?,
    );
    if final_candidate.starts_with(&exact_dir) || exact_dir.starts_with(&final_candidate) {
        bail!("V2 outcome-blind output must be disjoint from its Qualified exact directory");
    }
    Ok(())
}

#[cfg(unix)]
fn reopen_pinned_exact_artifact_v2(file: &File, label: &str) -> Result<File> {
    let before = file
        .metadata()
        .with_context(|| format!("inspect {label}"))?;
    if !before.is_file() {
        bail!("{label} descriptor is not a regular file");
    }
    let proc_path = PathBuf::from(format!("/proc/self/fd/{}", file.as_raw_fd()));
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    let reopened = options
        .open(&proc_path)
        .with_context(|| format!("reopen pinned {label} through {}", proc_path.display()))?;
    let after = reopened.metadata()?;
    if !after.is_file()
        || after.len() != before.len()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
    {
        bail!("reopened {label} does not retain the validated descriptor authority");
    }
    Ok(reopened)
}

#[cfg(not(unix))]
fn reopen_pinned_exact_artifact_v2(_file: &File, _label: &str) -> Result<File> {
    bail!("V2 outcome-blind export requires Unix descriptor authority")
}

fn visit_pinned_jsonl_v2<T, F>(
    file: &File,
    expected_digest: &PumpExactStateArtifactDigestV2,
    label: &str,
    mut visitor: F,
) -> Result<()>
where
    T: serde::de::DeserializeOwned,
    F: FnMut(T) -> Result<()>,
{
    revalidate_open_exact_artifact_v2(file, expected_digest, label)?;
    let mut reader =
        BufReader::with_capacity(64 * 1024, reopen_pinned_exact_artifact_v2(file, label)?);
    let mut line = Vec::with_capacity(4096);
    let mut buffer = [0u8; 8192];
    let mut line_count = 0u64;
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("read {label}"))?;
        if read == 0 {
            break;
        }
        for byte in &buffer[..read] {
            if *byte == b'\n' {
                if line.is_empty() {
                    bail!("{label} contains an empty JSONL record");
                }
                let value = serde_json::from_slice(&line)
                    .with_context(|| format!("parse bounded JSONL record from {label}"))?;
                visitor(value)?;
                line.clear();
                line_count = line_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("{label} JSONL line count overflow"))?;
            } else {
                if line.len() >= V2_WINDOW_EXPORT_MAX_JSONL_LINE_BYTES {
                    bail!(
                        "{label} has a JSONL record above bounded {} byte limit",
                        V2_WINDOW_EXPORT_MAX_JSONL_LINE_BYTES
                    );
                }
                line.push(*byte);
            }
        }
    }
    if !line.is_empty() {
        bail!("{label} is not newline-complete");
    }
    if line_count != expected_digest.line_count {
        bail!("{label} parsed line count differs from its descriptor-pinned digest");
    }
    revalidate_open_exact_artifact_v2(file, expected_digest, label)?;
    Ok(())
}

fn validate_exact_output_artifacts_v2(
    exact_dir: &Path,
) -> Result<PumpExactStateValidatedExactOutputV2> {
    validate_private_exact_output_directory_v2(exact_dir)?;
    let found_names = fs::read_dir(exact_dir)
        .with_context(|| format!("list V2 exact output {}", exact_dir.display()))?
        .map(|entry| {
            let entry = entry.context("read V2 exact output directory entry")?;
            let metadata = entry
                .file_type()
                .context("inspect V2 exact output directory entry type")?;
            if !metadata.is_file() || metadata.is_symlink() {
                bail!("V2 exact output may contain only regular non-symlink authority files");
            }
            entry
                .file_name()
                .into_string()
                .map_err(|_| anyhow::anyhow!("V2 exact output authority filename is not UTF-8"))
        })
        .collect::<Result<BTreeSet<_>>>()?;
    let expected_names = V2_EXACT_OUTPUT_ARTIFACT_NAMES
        .into_iter()
        .map(str::to_owned)
        .collect::<BTreeSet<_>>();
    if found_names != expected_names {
        bail!("V2 exact output authority file set is not exact");
    }

    let receipt_artifact = open_exact_artifact_control_v2(
        &exact_dir.join("exact_state_capability_v2.json"),
        "V2 exact capability receipt",
    )?;
    let receipt: PumpExactStateCapabilityReceiptV2 = serde_json::from_slice(
        receipt_artifact
            .control_bytes
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("V2 exact capability receipt bytes are unavailable"))?,
    )
    .context("parse V2 exact capability receipt")?;
    let manifest_artifact =
        open_exact_artifact_control_v2(&exact_dir.join("manifest_v2.json"), "V2 exact manifest")?;
    let manifest: PumpExactStateExactManifestV2 = serde_json::from_slice(
        manifest_artifact
            .control_bytes
            .as_deref()
            .ok_or_else(|| anyhow::anyhow!("V2 exact manifest bytes are unavailable"))?,
    )
    .context("parse V2 exact manifest")?;
    let births_artifact = open_exact_artifact_streaming_v2(
        &exact_dir.join("births_v2.jsonl"),
        "V2 exact births JSONL",
    )?;
    let trajectories_artifact = open_exact_artifact_streaming_v2(
        &exact_dir.join("trajectories_v2.jsonl"),
        "V2 exact trajectories JSONL",
    )?;
    let coverage_artifact = open_exact_artifact_streaming_v2(
        &exact_dir.join("coverage_v2.jsonl"),
        "V2 exact coverage JSONL",
    )?;

    validate_exact_output_receipt_v2(
        &receipt,
        &manifest,
        &receipt_artifact.digest,
        &manifest_artifact.digest,
        &births_artifact.digest,
        &trajectories_artifact.digest,
        &coverage_artifact.digest,
    )?;
    Ok(PumpExactStateValidatedExactOutputV2 {
        receipt,
        receipt_artifact,
        manifest,
        manifest_artifact,
        births_artifact,
        trajectories_artifact,
        coverage_artifact,
    })
}

fn validate_qualified_exact_output_binding_v2(
    raw: &PumpExactStateRawTapeIndexV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    exact: &PumpExactStateValidatedExactOutputV2,
) -> Result<()> {
    if exact.receipt.source_run_id != raw.start_manifest.run_id
        || exact.manifest.source_run_id != raw.start_manifest.run_id
        || exact.receipt.source_raw_segment_set_blake3 != raw.raw_segment_set_blake3
        || exact.manifest.source_raw_segment_set_blake3 != raw.raw_segment_set_blake3
        || exact.receipt.semantics_id != semantics.semantics_id
        || exact.receipt.semantics_manifest_digest
            != semantics_digest_to_runtime_v2(&semantics.manifest_digest)
        || exact.receipt.vendored_idl_digest
            != semantics_digest_to_runtime_v2(&semantics.idl_digest)
    {
        bail!("V2 exact output does not bind the supplied raw/semantics authority");
    }
    let start_digest = artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
        &raw.raw_dir.join("run_start_manifest_v2.json"),
    )?);
    let completion_digest = artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
        &raw.raw_dir.join("run_completion_receipt_v2.json"),
    )?);
    if exact.receipt.source_start_manifest_digest != start_digest
        || exact.receipt.source_completion_receipt_digest != completion_digest
    {
        bail!("V2 exact output source control binding differs from current raw authority");
    }
    let expected_qualification_run_below_minimum = qualification_run_below_minimum_v2(
        raw.completion_receipt.cohort_capture_elapsed_ms,
        exact.receipt.successful_rooted_mutation_denominator,
    );
    if exact.receipt.qualification_run_below_minimum != expected_qualification_run_below_minimum {
        bail!("V2 exact receipt qualification-run minimum flag differs from raw cohort authority");
    }
    if expected_qualification_run_below_minimum {
        bail!("V2 raw authority remains below the literal qualification-run minimum");
    }
    Ok(())
}

fn validate_private_exact_output_directory_v2(exact_dir: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(exact_dir)
        .with_context(|| format!("inspect V2 exact output directory {}", exact_dir.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("V2 exact output must be a regular non-symlink directory");
    }
    require_private_authority_directory_v2(&metadata, "V2 exact output directory")?;
    Ok(())
}

fn require_private_authority_directory_v2(metadata: &fs::Metadata, label: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 || mode & 0o700 != 0o700 {
            bail!(
                "{label} must be owner-private and accessible (mode {:o})",
                mode
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, label);
        bail!("V2 authority directories require Unix permission checks");
    }
    Ok(())
}

fn require_private_authority_file_v2(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        bail!(
            "{label} {} must be a regular non-symlink file",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        let mode = metadata.permissions().mode() & 0o777;
        if mode & 0o077 != 0 || mode & 0o600 != 0o600 {
            bail!(
                "{label} {} must be owner-private and readable (mode {:o})",
                path.display(),
                mode
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (metadata, label);
        bail!("V2 authority files require Unix permission checks");
    }
    Ok(())
}

fn open_exact_artifact_control_v2(
    path: &Path,
    label: &str,
) -> Result<PumpExactStateOpenArtifactV2> {
    let file = Arc::new(open_regular_nofollow(path, label)?);
    require_private_exact_artifact_mode_v2(&file, label)?;
    let (bytes, digest) =
        read_open_exact_artifact_bytes_v2(&file, label, V2_EXACT_OUTPUT_CONTROL_MAX_BYTES)?;
    if !digest.newline_complete {
        bail!("{label} is not newline-complete");
    }
    Ok(PumpExactStateOpenArtifactV2 {
        file,
        digest,
        control_bytes: Some(bytes),
    })
}

fn open_exact_artifact_streaming_v2(
    path: &Path,
    label: &str,
) -> Result<PumpExactStateOpenArtifactV2> {
    let file = Arc::new(open_regular_nofollow(path, label)?);
    require_private_exact_artifact_mode_v2(&file, label)?;
    let digest = digest_open_exact_artifact_v2(&file, label)?;
    if !digest.newline_complete {
        bail!("{label} is not newline-complete");
    }
    Ok(PumpExactStateOpenArtifactV2 {
        file,
        digest,
        control_bytes: None,
    })
}

fn validate_exact_output_receipt_v2(
    receipt: &PumpExactStateCapabilityReceiptV2,
    manifest: &PumpExactStateExactManifestV2,
    receipt_digest: &PumpExactStateArtifactDigestV2,
    manifest_digest: &PumpExactStateArtifactDigestV2,
    births_digest: &PumpExactStateArtifactDigestV2,
    trajectories_digest: &PumpExactStateArtifactDigestV2,
    coverage_digest: &PumpExactStateArtifactDigestV2,
) -> Result<()> {
    if receipt.schema_version != PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2
        || receipt.kind != "pump_exact_state_capability_v2"
        || manifest.schema_version != PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2
        || manifest.kind != "pump_exact_state_tape_v2"
        || !matches!(receipt.status, PumpExactStateCapabilityStatusV2::Qualified)
        || !matches!(
            manifest.exact_state_capability_status,
            PumpExactStateCapabilityStatusV2::Qualified
        )
        || !receipt.blockers.is_empty()
        || receipt.qualification_run_below_minimum
    {
        bail!("V2 exact output is not a Qualified capability authority");
    }
    if receipt.source_run_id.trim().is_empty()
        || receipt.required_exact_rooted_coverage_ppm != PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2
        || receipt.successful_rooted_mutation_denominator == 0
        || receipt.exact_rooted_mutation_count == 0
        || receipt.exact_trajectory_count == 0
        || receipt.successful_rooted_exact_trade_with_both_states_count == 0
        || receipt.exact_birth_count == 0
        || receipt.exact_rooted_coverage_ppm
            != coverage_ppm_v2(
                receipt.exact_rooted_mutation_count,
                receipt.successful_rooted_mutation_denominator,
            )?
        || receipt.exact_rooted_coverage_ppm < PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2
    {
        bail!("V2 Qualified receipt does not satisfy literal capability gates");
    }
    let occurrence_sum = receipt
        .successful_rooted_proven_non_reserve_count
        .checked_add(receipt.successful_rooted_validated_event_transport_count)
        .and_then(|value| value.checked_add(receipt.successful_rooted_candidate_count))
        .and_then(|value| value.checked_add(receipt.successful_rooted_unknown_occurrence_count))
        .ok_or_else(|| anyhow::anyhow!("V2 exact occurrence conservation overflow"))?;
    if !receipt.occurrence_ledger_reconciled
        || occurrence_sum != receipt.successful_rooted_instruction_occurrence_count
        || !receipt.denominator_reconciled
        || receipt
            .exact_rooted_mutation_count
            .checked_add(receipt.explicit_non_exact_mutation_count)
            != Some(receipt.successful_rooted_mutation_denominator)
        || receipt.successful_rooted_unknown_occurrence_count != 0
        || receipt.successful_rooted_malformed_candidate_count != 0
        || receipt.account_decode_failure_count != 0
        || receipt.unknown_pump_owned_account_count != 0
    {
        bail!("V2 Qualified receipt fails conservation or completeness authority");
    }
    if receipt.births_artifact != *births_digest
        || receipt.trajectories_artifact != *trajectories_digest
        || receipt.coverage_artifact != *coverage_digest
        || receipt.exact_birth_count != births_digest.line_count
        || receipt.exact_trajectory_count != trajectories_digest.line_count
        || receipt.filtered_pump_transaction_count != coverage_digest.line_count
    {
        bail!("V2 exact artifact digest/count binding differs from capability receipt");
    }
    if manifest.source_run_id != receipt.source_run_id
        || manifest.exact_state_capability_status != receipt.status
        || manifest.source_raw_segment_set_blake3 != receipt.source_raw_segment_set_blake3
        || manifest.semantics_manifest_sha256 != receipt.semantics_manifest_digest.sha256
        || manifest.semantics_manifest_blake3 != receipt.semantics_manifest_digest.blake3
        || manifest.materializer_running_executable_sha256
            != receipt.materializer_running_executable_digest.sha256
        || manifest.materializer_running_executable_blake3
            != receipt.materializer_running_executable_digest.blake3
        || manifest.materializer_running_executable_bytes
            != receipt.materializer_running_executable_digest.bytes
        || manifest.exact_state_capability_artifact != *receipt_digest
        || manifest.births_artifact != *births_digest
        || manifest.trajectories_artifact != *trajectories_digest
        || manifest.coverage_artifact != *coverage_digest
        || !receipt_digest.newline_complete
        || !manifest_digest.newline_complete
    {
        bail!("V2 exact manifest does not bind the complete Qualified artifact set");
    }
    Ok(())
}

/// A verified V2 record location.  The pointer is useful only together with
/// the exact segment receipt set from the same [`PumpExactStateRawTapeIndexV2`]
/// instance; it is never accepted as a standalone source authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PumpExactStateRawRecordPointerV2 {
    segment_position: usize,
    frame_offset: u64,
}

/// Minimal index entry for a source-lossless Pump transaction.  Payload bytes
/// remain in the frozen segment rather than being duplicated in RAM while the
/// qualifier builds account and mutation evidence.
#[derive(Clone, Debug)]
pub struct PumpExactStateIndexedTransactionV2 {
    pointer: PumpExactStateRawRecordPointerV2,
    pub source_capture_sequence: u64,
    pub slot: u64,
    pub tx_index: u32,
    pub signature: [u8; 64],
}

/// Minimal index entry for one canonical BondingCurve or Global account
/// update.  The raw account bytes are read only after the complete V2
/// segment-set contract has passed.
#[derive(Clone, Debug)]
pub struct PumpExactStateIndexedAccountUpdateV2 {
    pointer: PumpExactStateRawRecordPointerV2,
    pub source_capture_sequence: u64,
    pub slot: u64,
    pub write_version: u64,
    pub account_pubkey: [u8; 32],
    pub txn_signature: Option<[u8; 64]>,
}

#[derive(Clone, Debug, Default)]
struct PumpExactStateSlotNodeV2 {
    // This is deliberately populated only by retained `Slot` updates whose
    // source status is Finalized.  In particular, `None` is retained as
    // evidence: a finalized Slot that omits its parent cannot borrow a parent
    // from a BlockMeta record or an earlier Processed/Confirmed Slot update.
    finalized_parents: BTreeSet<Option<u64>>,
}

/// A pair of clocks taken from one admitted source update.  Monotonic time is
/// the only duration and cutoff authority.  Wall time is retained alongside
/// it so an operator can correlate the immutable tape with external logs
/// without giving a clock step the power to manufacture coverage.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpExactStateIngressTimestampV2 {
    wall_ms: u64,
    monotonic_ms: u64,
}

/// The fields that must agree between the source's BlockMeta and full-block
/// lanes for one executed slot.  A pair is more than a Pump-transaction map:
/// it proves that both lane payloads describe the same complete block.
#[derive(Clone, Debug, PartialEq, Eq)]
struct PumpExactStateBlockMetaSlotEvidenceV2 {
    parent_slot: u64,
    blockhash: String,
    parent_blockhash: String,
    executed_transaction_count: u64,
    ingress: PumpExactStateIngressTimestampV2,
    source_capture_sequence: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PumpExactStateFullBlockSlotEvidenceV2 {
    parent_slot: u64,
    blockhash: String,
    parent_blockhash: String,
    executed_transaction_count: u64,
    ingress: PumpExactStateIngressTimestampV2,
    source_capture_sequence: u64,
}

#[derive(Clone, Debug, Default)]
struct PumpExactStateBlockLaneSlotLedgerV2 {
    block_meta: Option<PumpExactStateBlockMetaSlotEvidenceV2>,
    full_block: Option<PumpExactStateFullBlockSlotEvidenceV2>,
}

/// Offline per-slot evidence ledger.  A `Slot` update remains canonicality
/// evidence, while the BlockMeta/full-block pair is completeness evidence.
/// Yellowstone Slot updates alone cannot distinguish a skipped Solana slot
/// from a provider omission, so V2 intentionally does *not* require a
/// BlockMeta for every finalized Slot.  It does require a bijective pair for
/// every BlockMeta/full-block record admitted inside the accepted cohort.
#[derive(Clone, Debug, Default)]
struct PumpExactStatePerSlotLedgerV2 {
    slot: PumpExactStateSlotNodeV2,
    block_lanes: PumpExactStateBlockLaneSlotLedgerV2,
}

#[derive(Clone, Debug)]
struct PumpExactStateIndexedSegmentV2 {
    path: std::path::PathBuf,
    receipt: PumpExactStateSegmentReceiptV2,
    pinned_file: Option<Arc<File>>,
}

/// A bounded, descriptor-verified source-time interval retained from the raw
/// V2 stream.  Its lower bound comes from the first admitted source ingress;
/// its upper bound comes only from the last fully reconciled BlockMeta +
/// full-block slot in the accepted cohort.  A naked late Slot update cannot
/// extend this authority.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpExactStateSourceAvailabilityBoundsV2 {
    first_observed_ingress: PumpExactStateIngressTimestampV2,
    reconciled_full_block_frontier_slot: u64,
    reconciled_full_block_frontier_ingress: PumpExactStateIngressTimestampV2,
}

/// Verified raw V2 authority for the future offline qualifier.  Construct it
/// only with [`index_prospective_exact_state_raw_run_v2`]; all indexed source
/// records were checked against their framed payload, footer, whole-file
/// receipt, segment chain, stream-readiness boundary and full-block
/// reconciliation.
#[derive(Clone, Debug)]
pub struct PumpExactStateRawTapeIndexV2 {
    raw_dir: std::path::PathBuf,
    start_manifest: PumpExactStateRunStartManifestV2,
    completion_receipt: PumpExactStateRunCompletionReceiptV2,
    segments: Vec<PumpExactStateIndexedSegmentV2>,
    transactions: Vec<PumpExactStateIndexedTransactionV2>,
    account_updates: Vec<PumpExactStateIndexedAccountUpdateV2>,
    slot_ledger: BTreeMap<u64, PumpExactStatePerSlotLedgerV2>,
    full_block_pump_transaction_count: u64,
    source_availability_bounds: PumpExactStateSourceAvailabilityBoundsV2,
    raw_segment_set_blake3: String,
}

impl PumpExactStateRawTapeIndexV2 {
    #[must_use]
    pub fn summary(&self) -> PumpExactStateRawInspectionSummaryV2 {
        PumpExactStateRawInspectionSummaryV2 {
            run_id: self.start_manifest.run_id.clone(),
            cohort_slots_strictly_after: self
                .completion_receipt
                .cohort_slots_strictly_after
                .expect("verified V2 completion receipt has stream-readiness boundary"),
            segment_count: u64::try_from(self.segments.len()).unwrap_or(u64::MAX),
            source_update_count: self.completion_receipt.writer.accepted_source_records,
            filtered_transaction_count: u64::try_from(self.transactions.len()).unwrap_or(u64::MAX),
            full_block_count: self
                .completion_receipt
                .writer
                .required_lane_census
                .full_blocks_completed,
            full_block_pump_transaction_count: self.full_block_pump_transaction_count,
            pump_owned_account_update_count: u64::try_from(self.account_updates.len())
                .unwrap_or(u64::MAX),
            raw_segment_set_blake3: self.raw_segment_set_blake3.clone(),
        }
    }

    #[must_use]
    pub fn transactions(&self) -> &[PumpExactStateIndexedTransactionV2] {
        &self.transactions
    }

    #[must_use]
    pub fn account_updates(&self) -> &[PumpExactStateIndexedAccountUpdateV2] {
        &self.account_updates
    }

    #[must_use]
    pub fn raw_dir(&self) -> &Path {
        &self.raw_dir
    }

    #[must_use]
    pub fn raw_segment_set_blake3(&self) -> &str {
        &self.raw_segment_set_blake3
    }

    fn total_receipt_bound_segment_bytes(&self) -> Result<u64> {
        self.segments.iter().try_fold(0u64, |total, segment| {
            total
                .checked_add(segment.receipt.file_bytes)
                .ok_or_else(|| anyhow::anyhow!("V2 receipt-bound raw segment bytes overflow u64"))
        })
    }

    /// Copy every receipt-bound source segment into a private Linux anonymous
    /// inode and return an index that reads payload frames only from those
    /// descriptors.  The copy is exact-size, whole-file SHA-256/BLAKE3 bound,
    /// and unlinked before the method returns.  It closes the raw A -> raw B
    /// path-reopen gap between semantic audit and exact output publication.
    fn seal_anonymous_snapshot_v2(&self, snapshot_parent: &Path) -> Result<Self> {
        let mut sealed = self.clone();
        for segment in &mut sealed.segments {
            segment.pinned_file = Some(copy_segment_to_anonymous_snapshot_v2(
                &segment.path,
                snapshot_parent,
                &segment.receipt,
            )?);
        }
        Ok(sealed)
    }

    fn rooted_slots(&self) -> BTreeSet<u64> {
        self.slot_ledger
            .iter()
            .filter_map(|(slot, ledger)| {
                let block_pair_matches = matches!(
                    (&ledger.block_lanes.block_meta, &ledger.block_lanes.full_block),
                    (Some(meta), Some(full))
                        if meta.parent_slot == full.parent_slot
                            && meta.blockhash == full.blockhash
                            && meta.parent_blockhash == full.parent_blockhash
                            && meta.executed_transaction_count == full.executed_transaction_count
                );
                (ledger.slot.finalized_parents.len() == 1
                    && ledger.block_lanes.block_meta.as_ref().is_some_and(|meta| {
                        ledger
                            .slot
                            .finalized_parents
                            .contains(&Some(meta.parent_slot))
                    })
                    && block_pair_matches)
                    .then_some(*slot)
            })
            .collect()
    }

    fn read_record(
        &self,
        pointer: PumpExactStateRawRecordPointerV2,
    ) -> Result<PumpExactStateRawRecordV2> {
        let segment = self.segments.get(pointer.segment_position).ok_or_else(|| {
            anyhow::anyhow!(
                "V2 indexed record references missing segment position {}",
                pointer.segment_position
            )
        })?;
        let file = segment.pinned_file.as_ref().ok_or_else(|| {
            anyhow::anyhow!(
                "V2 qualifier attempted to read a raw record without anonymous snapshot authority"
            )
        })?;
        let expected_bytes = segment.receipt.file_bytes;
        let before = file.metadata()?;
        if !before.is_file() || before.len() != expected_bytes {
            bail!("V2 pinned raw segment changed before record read");
        }
        let frame = read_v2_frame_from_open_file(file, pointer.frame_offset)?;
        let record = PumpExactStateRawCodecV2::decode_record(&frame).map_err(anyhow::Error::msg)?;
        let after = file.metadata()?;
        if !after.is_file()
            || after.len() != expected_bytes
            || after.dev() != before.dev()
            || after.ino() != before.ino()
        {
            bail!("V2 pinned raw segment changed during record read");
        }
        Ok(record)
    }

    fn read_transaction(
        &self,
        indexed: &PumpExactStateIndexedTransactionV2,
    ) -> Result<ghost_core::pump_research_exact_tape_v2::PumpExactStateTransactionEvidenceV2> {
        let PumpExactStateRawRecordV2::PrimaryTransaction(transaction) =
            self.read_record(indexed.pointer)?
        else {
            bail!("V2 transaction index points to a non-transaction record");
        };
        if transaction.source.capture_sequence != indexed.source_capture_sequence
            || transaction.slot != indexed.slot
            || transaction.tx_index != Some(indexed.tx_index)
            || transaction.signature.into_inner() != indexed.signature
        {
            bail!("V2 indexed transaction identity differs from frozen record");
        }
        Ok(transaction)
    }

    fn read_account_update(
        &self,
        indexed: &PumpExactStateIndexedAccountUpdateV2,
    ) -> Result<ghost_core::pump_research_exact_tape_v2::PumpExactStatePumpOwnedAccountUpdateV2>
    {
        let PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) =
            self.read_record(indexed.pointer)?
        else {
            bail!("V2 account index points to a non-account-update record");
        };
        if update.source.capture_sequence != indexed.source_capture_sequence
            || update.slot != indexed.slot
            || update.write_version != indexed.write_version
            || update.account_pubkey.into_inner() != indexed.account_pubkey
            || update.txn_signature.map(|value| value.into_inner()) != indexed.txn_signature
        {
            bail!("V2 indexed account update identity differs from frozen record");
        }
        Ok(update)
    }
}

impl PumpExactStateAnchorIndexV2 {
    fn build(
        raw: &PumpExactStateRawTapeIndexV2,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        rooted_slots: &BTreeSet<u64>,
    ) -> Result<Self> {
        let mut index = Self::default();
        let mut transaction_index_by_signature = BTreeMap::new();
        for transaction in &raw.transactions {
            if transaction_index_by_signature
                .insert(
                    transaction.signature,
                    (transaction.slot, transaction.tx_index),
                )
                .is_some()
            {
                bail!("V2 filtered transaction lane has duplicate signature authority");
            }
        }

        let mut updates = raw.account_updates.clone();
        updates.sort_by_key(|update| {
            (
                update.slot,
                update.write_version,
                update.source_capture_sequence,
                update.account_pubkey,
            )
        });
        for indexed in &updates {
            let update = raw.read_account_update(indexed)?;
            if !rooted_slots.contains(&update.slot) {
                // Raw V2 preserves non-canonical-slot updates for audit and
                // source completeness. A qualifier must not turn one into a
                // state anchor merely because its slot is numerically earlier
                // than a later rooted transaction.
                continue;
            }
            let transaction_index = update.txn_signature.and_then(|signature| {
                transaction_index_by_signature
                    .get(&signature.into_inner())
                    .and_then(|(transaction_slot, transaction_index)| {
                        (*transaction_slot == update.slot).then_some(*transaction_index)
                    })
            });
            index.observe_account(
                semantics,
                update.account_pubkey.into_inner(),
                &update.raw_account_data,
                update.slot,
                update.write_version,
                update.txn_signature.map(|signature| signature.into_inner()),
                transaction_index,
                Some(update.source.capture_sequence),
            )?;
        }

        for anchors in index.by_curve.values_mut() {
            anchors.sort_by_key(anchor_sort_key_v2);
        }
        for anchors in index.final_by_signature.values_mut() {
            anchors.sort_by_key(anchor_sort_key_v2);
        }
        Ok(index)
    }

    #[allow(clippy::too_many_arguments)]
    fn observe_account(
        &mut self,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        account_pubkey: [u8; 32],
        data: &[u8],
        slot: u64,
        write_version: u64,
        transaction_signature: Option<[u8; 64]>,
        transaction_index: Option<u32>,
        source_capture_sequence: Option<u64>,
    ) -> Result<()> {
        let Some(class) = semantics.account_class(data) else {
            self.unknown_account_count = self
                .unknown_account_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 unknown-account census overflow"))?;
            return Ok(());
        };
        match class {
            PumpExactStateAccountClassV2::ExactBondingCurve => {
                self.bonding_curve_account_count = self
                    .bonding_curve_account_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 curve-account census overflow"))?;
                let state = match semantics.decode_curve_state(data) {
                    Ok(state) => state,
                    Err(_) => {
                        self.account_decode_failure_count = self
                            .account_decode_failure_count
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("V2 account-decode census overflow"))?;
                        return Ok(());
                    }
                };
                self.bonding_curve_decoded_count = self
                    .bonding_curve_decoded_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 curve-decode census overflow"))?;
                let curve = Pubkey::new_from_array(account_pubkey);
                let anchor = PumpExactStateCurveAnchorV2 {
                    curve,
                    state,
                    slot,
                    write_version,
                    transaction_index,
                    source_capture_sequence,
                    // Streamed updates reach this point only after `build`
                    // admitted a rooted canonical slot. There is no snapshot
                    // or historical baseline anchor in PRXTAPE3.
                    canonical: true,
                };
                self.by_curve.entry(curve).or_default().push(anchor.clone());
                if let Some(signature) = transaction_signature {
                    self.final_by_signature
                        .entry((signature, curve))
                        .or_default()
                        .push(anchor);
                }
            }
            PumpExactStateAccountClassV2::KnownGlobalDependency => {
                self.global_account_count = self
                    .global_account_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 global-account census overflow"))?;
                if semantics.validate_global_account(data).is_ok() {
                    self.global_validated_count = self
                        .global_validated_count
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("V2 global-decode census overflow"))?;
                } else {
                    self.account_decode_failure_count = self
                        .account_decode_failure_count
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("V2 account-decode census overflow"))?;
                }
            }
            PumpExactStateAccountClassV2::KnownNonState => {}
        }
        Ok(())
    }

    fn unique_pre_anchor(
        &self,
        curve: Pubkey,
        transaction_slot: u64,
        transaction_index: u32,
    ) -> Option<&PumpExactStateCurveAnchorV2> {
        let mut candidates = self
            .by_curve
            .get(&curve)?
            .iter()
            .filter(|anchor| {
                anchor.canonical
                    && (anchor.slot < transaction_slot
                        || (anchor.slot == transaction_slot
                            && anchor
                                .transaction_index
                                .is_some_and(|index| index < transaction_index)))
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|anchor| anchor_sort_key_v2(anchor));
        let latest = candidates.pop()?;
        if candidates
            .last()
            .is_some_and(|previous| anchor_sort_key_v2(previous) == anchor_sort_key_v2(latest))
        {
            return None;
        }
        Some(latest)
    }

    fn unique_final_anchor(
        &self,
        signature: [u8; 64],
        curve: Pubkey,
        transaction_slot: u64,
        transaction_index: u32,
    ) -> Option<&PumpExactStateCurveAnchorV2> {
        let anchors = self.final_by_signature.get(&(signature, curve))?;
        let mut candidates = anchors
            .iter()
            .filter(|anchor| {
                anchor.canonical
                    && anchor.slot == transaction_slot
                    && anchor.transaction_index == Some(transaction_index)
            })
            .collect::<Vec<_>>();
        candidates.sort_by_key(|anchor| (anchor.write_version, anchor.source_capture_sequence));
        let final_anchor = candidates.pop()?;
        if candidates
            .last()
            .is_some_and(|previous| previous.write_version == final_anchor.write_version)
        {
            return None;
        }
        Some(final_anchor)
    }
}

fn anchor_sort_key_v2(anchor: &PumpExactStateCurveAnchorV2) -> (u64, u32, u64, u64, [u8; 32]) {
    (
        anchor.slot,
        anchor.transaction_index.unwrap_or(u32::MAX),
        anchor.write_version,
        anchor.source_capture_sequence.unwrap_or(0),
        anchor.curve.to_bytes(),
    )
}

/// Create-new, atomic V2 exact-state artifact writer.  It deliberately
/// publishes a final directory for both Qualified and Blocked receipts: a
/// Blocked result is valuable diagnostic evidence, but its status prevents a
/// later export gate from treating those rows as strategy input.
struct PumpExactStateExactOutputWriterV2 {
    final_root: PathBuf,
    partial_root: PathBuf,
    births: BufWriter<File>,
    trajectories: BufWriter<File>,
    coverage: BufWriter<File>,
}

impl PumpExactStateExactOutputWriterV2 {
    fn create(raw_dir: &Path, output_dir: &Path) -> Result<Self> {
        validate_v2_exact_output_path(raw_dir, output_dir)?;
        let parent = output_dir
            .parent()
            .ok_or_else(|| anyhow::anyhow!("V2 exact output has no parent"))?;
        let name = output_dir
            .file_name()
            .and_then(|value| value.to_str())
            .filter(|value| !value.is_empty() && *value != "." && *value != "..")
            .ok_or_else(|| anyhow::anyhow!("V2 exact output name must be non-empty UTF-8"))?;
        let partial_root = parent.join(format!(".{name}.partial"));
        if partial_root.exists() {
            bail!(
                "V2 exact partial output {} already exists; retain it for forensics before retrying",
                partial_root.display()
            );
        }
        let mut builder = DirBuilder::new();
        #[cfg(unix)]
        builder.mode(0o700);
        builder.create(&partial_root).with_context(|| {
            format!("create V2 exact partial output {}", partial_root.display())
        })?;
        let births =
            create_private_output_file_v2(&partial_root.join("births_v2.jsonl"), "births")?;
        let trajectories = create_private_output_file_v2(
            &partial_root.join("trajectories_v2.jsonl"),
            "trajectories",
        )?;
        let coverage =
            create_private_output_file_v2(&partial_root.join("coverage_v2.jsonl"), "coverage")?;
        Ok(Self {
            final_root: output_dir.to_path_buf(),
            partial_root,
            births: BufWriter::new(births),
            trajectories: BufWriter::new(trajectories),
            coverage: BufWriter::new(coverage),
        })
    }

    fn write_birth(&mut self, value: &PumpExactStateBirthRecordV2) -> Result<()> {
        write_jsonl_v2(&mut self.births, value)
    }

    fn write_trajectory(&mut self, value: &PumpExactStateTrajectoryRecordV2) -> Result<()> {
        write_jsonl_v2(&mut self.trajectories, value)
    }

    fn write_coverage(&mut self, value: &PumpExactStateCoverageRecordV2) -> Result<()> {
        write_jsonl_v2(&mut self.coverage, value)
    }

    fn finish<B, M, R>(
        self,
        build_receipt: B,
        build_manifest: M,
        before_publish: R,
    ) -> Result<(PathBuf, PumpExactStateArtifactDigestV2)>
    where
        B: FnOnce(
            PumpExactStateArtifactDigestV2,
            PumpExactStateArtifactDigestV2,
            PumpExactStateArtifactDigestV2,
        ) -> Result<PumpExactStateCapabilityReceiptV2>,
        M: FnOnce(
            PumpExactStateArtifactDigestV2,
            PumpExactStateArtifactDigestV2,
            PumpExactStateArtifactDigestV2,
            PumpExactStateArtifactDigestV2,
        ) -> Result<PumpExactStateExactManifestV2>,
        R: FnOnce() -> Result<()>,
    {
        let Self {
            final_root,
            partial_root,
            births,
            trajectories,
            coverage,
        } = self;
        sync_jsonl_v2(births)?;
        sync_jsonl_v2(trajectories)?;
        sync_jsonl_v2(coverage)?;
        let births_digest = digest_private_artifact_v2(&partial_root.join("births_v2.jsonl"))?;
        let trajectories_digest =
            digest_private_artifact_v2(&partial_root.join("trajectories_v2.jsonl"))?;
        let coverage_digest = digest_private_artifact_v2(&partial_root.join("coverage_v2.jsonl"))?;
        let receipt = build_receipt(
            births_digest.clone(),
            trajectories_digest.clone(),
            coverage_digest.clone(),
        )?;
        let receipt_path = partial_root.join("exact_state_capability_v2.json");
        write_json_create_new_v2(&receipt_path, &receipt, "V2 exact-state capability receipt")?;
        let receipt_digest = digest_private_artifact_v2(&receipt_path)?;
        let manifest = build_manifest(
            receipt_digest.clone(),
            births_digest,
            trajectories_digest,
            coverage_digest,
        )?;
        let manifest_path = partial_root.join("manifest_v2.json");
        write_json_create_new_v2(&manifest_path, &manifest, "V2 exact-state manifest")?;
        let manifest_digest = digest_private_artifact_v2(&manifest_path)?;
        if !receipt_digest.newline_complete || !manifest_digest.newline_complete {
            bail!("V2 exact JSON authority artifact is not newline-complete");
        }
        sync_directory_v2(&partial_root)?;
        before_publish()?;
        fs::rename(&partial_root, &final_root).with_context(|| {
            format!(
                "atomically publish V2 exact artifact {} -> {}",
                partial_root.display(),
                final_root.display()
            )
        })?;
        let parent = final_root
            .parent()
            .ok_or_else(|| anyhow::anyhow!("published V2 exact output has no parent"))?;
        sync_directory_v2(parent)?;
        let final_receipt_path = final_root.join("exact_state_capability_v2.json");
        if !final_receipt_path.is_file() {
            bail!("V2 exact output publication lost capability receipt");
        }
        Ok((final_receipt_path, receipt_digest))
    }
}

fn validate_v2_exact_output_path(raw_dir: &Path, output_dir: &Path) -> Result<()> {
    if output_dir.exists() {
        bail!(
            "V2 exact output directory {} already exists; qualification never overwrites an artifact",
            output_dir.display()
        );
    }
    if output_dir.as_os_str().is_empty() {
        bail!("V2 exact output path must not be empty");
    }
    let parent = output_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 exact output has no parent"))?;
    let parent_metadata = fs::symlink_metadata(parent)
        .with_context(|| format!("inspect V2 exact output parent {}", parent.display()))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        bail!("V2 exact output parent must be an existing non-symlink directory");
    }
    require_private_authority_directory_v2(&parent_metadata, "V2 exact output parent")?;
    let raw = fs::canonicalize(raw_dir)
        .with_context(|| format!("canonicalize V2 raw directory {}", raw_dir.display()))?;
    let canonical_parent = fs::canonicalize(parent)
        .with_context(|| format!("canonicalize V2 exact output parent {}", parent.display()))?;
    let final_candidate = canonical_parent.join(
        output_dir
            .file_name()
            .ok_or_else(|| anyhow::anyhow!("V2 exact output lacks final path component"))?,
    );
    if final_candidate.starts_with(&raw) || raw.starts_with(&final_candidate) {
        bail!("V2 exact output must be disjoint from the V2 immutable raw directory");
    }
    Ok(())
}

/// Return the available bytes on the filesystem that will host descriptor-only
/// raw snapshots and the create-new exact output. This is a local capacity
/// precondition, not an assertion that a shared filesystem cannot change
/// later; the snapshot copy and atomic writer still fail closed on I/O error.
#[cfg(target_os = "linux")]
fn available_v2_qualification_filesystem_bytes(snapshot_parent: &Path) -> Result<u64> {
    let path =
        std::ffi::CString::new(snapshot_parent.as_os_str().as_bytes()).with_context(|| {
            format!(
                "V2 qualification snapshot parent {} contains an interior NUL",
                snapshot_parent.display()
            )
        })?;
    // SAFETY: `path` is NUL-terminated and the caller has already validated
    // the existing non-symlink output parent. `statvfs` initializes only the
    // local structure passed by mutable pointer.
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error()).with_context(|| {
            format!(
                "statvfs V2 qualification snapshot filesystem {}",
                snapshot_parent.display()
            )
        });
    }
    // SAFETY: a zero return from `statvfs` initializes `stats` completely.
    let stats = unsafe { stats.assume_init() };
    let bytes = (stats.f_bavail as u128)
        .checked_mul(stats.f_frsize as u128)
        .ok_or_else(|| anyhow::anyhow!("V2 qualification available-byte count overflow"))?;
    u64::try_from(bytes).context("V2 qualification available bytes exceed u64")
}

#[cfg(not(target_os = "linux"))]
fn available_v2_qualification_filesystem_bytes(_snapshot_parent: &Path) -> Result<u64> {
    bail!("V2 exact-state qualification requires Linux statvfs storage authority")
}

fn required_v2_qualification_storage_bytes(
    min_free_bytes: u64,
    raw_snapshot_bytes: u64,
) -> Result<u64> {
    min_free_bytes
        .checked_add(raw_snapshot_bytes)
        .and_then(|bytes| bytes.checked_add(V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES))
        .ok_or_else(|| anyhow::anyhow!("V2 qualification storage budget overflows u64"))
}

/// Fail before creating the first anonymous inode if the output filesystem
/// cannot retain the capture's frozen reserve while holding a complete
/// descriptor-pinned copy of the exact receipt-bound raw set.
fn require_v2_qualification_storage_budget(
    snapshot_parent: &Path,
    min_free_bytes: u64,
    raw_snapshot_bytes: u64,
) -> Result<u64> {
    let available = available_v2_qualification_filesystem_bytes(snapshot_parent)?;
    let required = required_v2_qualification_storage_bytes(min_free_bytes, raw_snapshot_bytes)?;
    if available < required {
        bail!(
            "V2 qualification filesystem {} has {} bytes available but requires at least {} ({} anonymous raw snapshot + {} retained reserve + {} metadata allowance)",
            snapshot_parent.display(),
            available,
            required,
            raw_snapshot_bytes,
            min_free_bytes,
            V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES,
        );
    }
    Ok(available)
}

fn create_private_output_file_v2(path: &Path, label: &str) -> Result<File> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options
        .mode(0o600)
        .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
    options
        .open(path)
        .with_context(|| format!("create V2 exact {label} {}", path.display()))
}

fn write_jsonl_v2<T: Serialize>(writer: &mut BufWriter<File>, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).context("serialize V2 exact JSONL row")?;
    writer
        .write_all(b"\n")
        .context("write V2 exact JSONL newline")?;
    Ok(())
}

fn sync_jsonl_v2(writer: BufWriter<File>) -> Result<()> {
    let mut file = writer
        .into_inner()
        .map_err(|error| anyhow::anyhow!("flush V2 exact JSONL: {}", error.error()))?;
    file.flush().context("flush V2 exact JSONL")?;
    file.sync_all().context("sync V2 exact JSONL")?;
    Ok(())
}

fn write_json_create_new_v2<T: Serialize>(path: &Path, value: &T, label: &str) -> Result<()> {
    let mut file = create_private_output_file_v2(path, label)?;
    serde_json::to_writer_pretty(&mut file, value).with_context(|| format!("serialize {label}"))?;
    file.write_all(b"\n")
        .with_context(|| format!("terminate {label} JSON"))?;
    file.sync_all().with_context(|| format!("sync {label}"))?;
    Ok(())
}

fn digest_private_artifact_v2(path: &Path) -> Result<PumpExactStateArtifactDigestV2> {
    let file = open_regular_nofollow(path, "V2 exact output artifact")?;
    digest_open_exact_artifact_v2(&file, "V2 exact output artifact")
}

/// Stream a potentially multi-gigabyte exact artifact through a bounded
/// buffer. Qualification must never allocate an artifact-sized `Vec` merely
/// to calculate the digest that later authorizes it.
fn digest_open_exact_artifact_v2(
    file: &File,
    label: &str,
) -> Result<PumpExactStateArtifactDigestV2> {
    const BUFFER_BYTES: usize = 1024 * 1024;
    let before = file
        .metadata()
        .with_context(|| format!("inspect {label}"))?;
    if !before.is_file() {
        bail!("{label} is not a regular file");
    }
    let expected_bytes = before.len();
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut line_count = 0_u64;
    let mut last_byte = None;
    let mut offset = 0_u64;
    let mut buffer = [0_u8; BUFFER_BYTES];
    while offset < expected_bytes {
        let remaining = expected_bytes.saturating_sub(offset);
        let chunk_len = usize::try_from(remaining.min(BUFFER_BYTES as u64))
            .context("V2 exact artifact chunk length exceeds usize")?;
        #[cfg(unix)]
        file.read_exact_at(&mut buffer[..chunk_len], offset)
            .with_context(|| format!("read {label} at offset {offset}"))?;
        #[cfg(not(unix))]
        {
            let _ = (&buffer, chunk_len, offset);
            bail!("V2 exact artifact digest requires Unix positional reads");
        }
        let chunk = &buffer[..chunk_len];
        sha256.update(chunk);
        blake3.update(chunk);
        line_count = line_count
            .checked_add(
                u64::try_from(chunk.iter().filter(|byte| **byte == b'\n').count())
                    .unwrap_or(u64::MAX),
            )
            .ok_or_else(|| anyhow::anyhow!("V2 exact artifact line count overflow"))?;
        last_byte = chunk.last().copied();
        offset = offset
            .checked_add(u64::try_from(chunk_len).unwrap_or(u64::MAX))
            .ok_or_else(|| anyhow::anyhow!("V2 exact artifact offset overflow"))?;
    }
    let after = file
        .metadata()
        .with_context(|| format!("reinspect {label}"))?;
    #[cfg(unix)]
    if !after.is_file()
        || before.len() != after.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
    {
        bail!("{label} changed while digesting");
    }
    Ok(PumpExactStateArtifactDigestV2 {
        sha256: hex_bytes(&sha256.finalize()),
        blake3: hex_bytes(blake3.finalize().as_bytes()),
        bytes: expected_bytes,
        line_count,
        newline_complete: last_byte == Some(b'\n'),
    })
}

/// Parse authority JSON from the same exact bytes used for its digest. The
/// control artifacts are separately capped; JSONL payloads always use the
/// streaming digest path above.
fn read_open_exact_artifact_bytes_v2(
    file: &File,
    label: &str,
    max_bytes: u64,
) -> Result<(Vec<u8>, PumpExactStateArtifactDigestV2)> {
    let before = file
        .metadata()
        .with_context(|| format!("inspect {label}"))?;
    if !before.is_file() || before.len() > max_bytes {
        bail!("{label} is not a bounded regular authority file");
    }
    let length = usize::try_from(before.len()).context("V2 exact control length exceeds usize")?;
    let mut bytes = vec![0_u8; length];
    #[cfg(unix)]
    file.read_exact_at(&mut bytes, 0)
        .with_context(|| format!("read {label}"))?;
    #[cfg(not(unix))]
    {
        let _ = (&bytes, label);
        bail!("V2 exact authority parser requires Unix positional reads");
    }
    let after = file
        .metadata()
        .with_context(|| format!("reinspect {label}"))?;
    #[cfg(unix)]
    if !after.is_file()
        || before.len() != after.len()
        || before.dev() != after.dev()
        || before.ino() != after.ino()
    {
        bail!("{label} changed while being read");
    }
    let line_count =
        u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count()).unwrap_or(u64::MAX);
    let digest = PumpExactStateArtifactDigestV2 {
        sha256: hex_bytes(&Sha256::digest(&bytes)),
        blake3: hex_bytes(blake3::hash(&bytes).as_bytes()),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        line_count,
        newline_complete: bytes.last() == Some(&b'\n'),
    };
    Ok((bytes, digest))
}

fn require_private_exact_artifact_mode_v2(file: &File, label: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o777;
        if mode != 0o600 {
            bail!("{label} must have mode 0600");
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, label);
        bail!("V2 exact artifact authority requires Unix permissions");
    }
    Ok(())
}

fn revalidate_open_exact_artifact_v2(
    file: &File,
    expected: &PumpExactStateArtifactDigestV2,
    label: &str,
) -> Result<()> {
    require_private_exact_artifact_mode_v2(file, label)?;
    if digest_open_exact_artifact_v2(file, label)? != *expected {
        bail!("{label} differs from its validated digest");
    }
    Ok(())
}

fn sync_directory_v2(path: &Path) -> Result<()> {
    File::open(path)
        .with_context(|| format!("open V2 directory {} for sync", path.display()))?
        .sync_all()
        .with_context(|| format!("sync V2 directory {}", path.display()))
}

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PumpExactStateTransactionKeyV2 {
    slot: u64,
    tx_index: u64,
    signature: [u8; 64],
}

const ANCHOR_EVENT_CPI_WRAPPER_DISCRIMINATOR_V2: [u8; 8] =
    [0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord)]
struct PumpExactStateInstructionOccurrenceKeyV2 {
    signature: [u8; 64],
    outer_instruction_index: u32,
    inner_instruction_path: Vec<u16>,
    stack_height: Option<u32>,
    program_id: Pubkey,
    discriminator: [u8; 8],
}

#[derive(Clone, Debug)]
enum PumpExactStateOccurrenceClassV2 {
    ProvenNonReserve {
        semantic_evidence: PumpExactStateInstructionSemanticEvidenceV2,
    },
    /// A strict Anchor `emit_cpi!` envelope. It is not a reserve mutation,
    /// but its immediate Pump caller is retained as a concrete occurrence
    /// link so a standalone or ambiguously nested wrapper cannot disappear
    /// from the inventory as a supposedly harmless transport.
    ValidatedEventTransport {
        immediate_parent: PumpExactStateInstructionOccurrenceKeyV2,
        event_discriminator: [u8; 8],
        event_fields: BTreeMap<String, Vec<u8>>,
        final_state_bindings: Vec<PumpExactStateEventFinalStateBindingV2>,
    },
    Candidate {
        effect: PumpExactStateInstructionEffectV2,
        instruction_payload_exact: bool,
        account_vector_exact: bool,
        bonding_curve: Option<Pubkey>,
        mint: Option<Pubkey>,
        failure_reason: Option<String>,
        semantic_evidence: Option<PumpExactStateInstructionSemanticEvidenceV2>,
    },
    Unknown {
        reason: String,
    },
}

#[derive(Clone, Debug)]
struct PumpExactStateInstructionOccurrenceV2 {
    key: PumpExactStateInstructionOccurrenceKeyV2,
    class: PumpExactStateOccurrenceClassV2,
}

#[derive(Clone, Debug)]
struct PumpExactStateTransactionInventoryV2 {
    slot: u64,
    tx_index: u32,
    signature: [u8; 64],
    success: bool,
    occurrences: Vec<PumpExactStateInstructionOccurrenceV2>,
}

#[derive(Clone, Copy, Debug)]
struct PumpExactStateAccountMetaV2 {
    pubkey: Pubkey,
    signer: bool,
    writable: bool,
}

#[derive(Clone, Debug)]
struct PumpExactStateTransactionContextV2 {
    signature: [u8; 64],
    slot: u64,
    tx_index: u32,
    success: bool,
    accounts: Vec<PumpExactStateAccountMetaV2>,
    outer: Vec<yellowstone_grpc_proto::prelude::CompiledInstruction>,
    inner: BTreeMap<u32, Vec<yellowstone_grpc_proto::prelude::InnerInstruction>>,
}

#[derive(Clone, Debug)]
struct PumpExactStateCurveAnchorV2 {
    curve: Pubkey,
    state: PumpExactStateCurveStateV2,
    slot: u64,
    write_version: u64,
    transaction_index: Option<u32>,
    source_capture_sequence: Option<u64>,
    canonical: bool,
}

#[derive(Default)]
struct PumpExactStateAnchorIndexV2 {
    by_curve: BTreeMap<Pubkey, Vec<PumpExactStateCurveAnchorV2>>,
    final_by_signature: BTreeMap<([u8; 64], Pubkey), Vec<PumpExactStateCurveAnchorV2>>,
    bonding_curve_account_count: u64,
    bonding_curve_decoded_count: u64,
    global_account_count: u64,
    global_validated_count: u64,
    unknown_account_count: u64,
    account_decode_failure_count: u64,
}

struct PumpExactStateFullBlockAccumulatorV2 {
    started: PumpExactStateFullBlockPayloadStartedV2,
    next_chunk_index: u64,
    bytes: Vec<u8>,
    sha256: Sha256,
    blake3: blake3::Hasher,
}

#[derive(Default)]
struct PumpExactStateRawRecordCollectorV2 {
    expected_pump_program_id: [u8; 32],
    previous_source_capture_sequence: Option<u64>,
    source_capture_sequences: BTreeSet<u64>,
    source_stream_epoch: Option<u64>,
    filtered_transactions: BTreeMap<PumpExactStateTransactionKeyV2, [u8; 32]>,
    full_block_pump_transactions: BTreeMap<PumpExactStateTransactionKeyV2, [u8; 32]>,
    open_full_block: Option<PumpExactStateFullBlockAccumulatorV2>,
    slot_update_count: u64,
    block_meta_count: u64,
    full_block_started_count: u64,
    full_block_chunk_count: u64,
    full_block_count: u64,
    block_lanes_by_slot: BTreeMap<u64, PumpExactStateBlockLaneSlotLedgerV2>,
    pump_owned_account_update_count: u64,
    readiness_boundary:
        Option<ghost_core::pump_research_exact_tape_v2::PumpExactStateProspectiveStreamBoundaryV2>,
    first_observed_ingress: Option<PumpExactStateIngressTimestampV2>,
    previous_source_ingress_monotonic_ms: Option<u64>,
}

#[derive(Default)]
struct PumpExactStateRawIndexBuilderV2 {
    expected_pump_program_id: [u8; 32],
    collector: PumpExactStateRawRecordCollectorV2,
    transactions: Vec<PumpExactStateIndexedTransactionV2>,
    account_updates: Vec<PumpExactStateIndexedAccountUpdateV2>,
    slots: BTreeMap<u64, PumpExactStateSlotNodeV2>,
}

/// Validate a complete prospective raw run before exact-state semantics are
/// considered.  The function is read-only and does not create an output
/// directory; qualification will call it as its first authority boundary.
pub fn inspect_prospective_exact_state_raw_run_v2(
    raw_dir: &Path,
) -> Result<PumpExactStateRawInspectionSummaryV2> {
    Ok(index_prospective_exact_state_raw_run_v2(raw_dir)?.summary())
}

/// Open and validate the complete V2 raw authority while retaining only
/// bounded record pointers for the later offline semantic pass. This keeps the
/// raw scan independent from semantic interpretation and does not retain large
/// source payloads in memory.
pub fn index_prospective_exact_state_raw_run_v2(
    raw_dir: &Path,
) -> Result<PumpExactStateRawTapeIndexV2> {
    let raw_metadata = fs::symlink_metadata(raw_dir)
        .with_context(|| format!("inspect V2 raw directory {}", raw_dir.display()))?;
    if raw_metadata.file_type().is_symlink() || !raw_metadata.is_dir() {
        bail!("V2 raw authority must be an existing non-symlink directory");
    }
    require_private_authority_directory_v2(&raw_metadata, "V2 raw authority directory")?;
    let start_manifest_path = raw_dir.join("run_start_manifest_v2.json");
    let completion_receipt_path = raw_dir.join("run_completion_receipt_v2.json");
    require_private_authority_file_v2(&start_manifest_path, "V2 run start manifest")?;
    require_private_authority_file_v2(&completion_receipt_path, "V2 run completion receipt")?;
    let start_manifest: PumpExactStateRunStartManifestV2 =
        read_v2_json(&start_manifest_path, "V2 run start manifest")?;
    let completion_receipt: PumpExactStateRunCompletionReceiptV2 =
        read_v2_json(&completion_receipt_path, "V2 run completion receipt")?;

    validate_v2_controls(&start_manifest, &completion_receipt)?;

    let expected_pump_program_id =
        solana_sdk::pubkey::Pubkey::from_str(&start_manifest.pump_program_id)
            .context("V2 raw start manifest Pump program id is invalid")?
            .to_bytes();
    let mut index_builder = PumpExactStateRawIndexBuilderV2::new(expected_pump_program_id);
    let mut segments = Vec::with_capacity(completion_receipt.segment_list.len());
    let mut expected_previous_prefix_hash = None;
    let mut raw_segment_set_hasher = blake3::Hasher::new();

    for (expected_segment_index, receipt) in completion_receipt.segment_list.iter().enumerate() {
        let expected_segment_index = u64::try_from(expected_segment_index)
            .context("V2 segment position does not fit u64")?;
        if receipt.segment_index != expected_segment_index {
            bail!(
                "V2 completion receipt segment index {} is not contiguous at position {}",
                receipt.segment_index,
                expected_segment_index
            );
        }
        let filename = safe_v2_segment_filename(&receipt.filename)?;
        let path = raw_dir.join(filename);
        require_private_authority_file_v2(&path, "V2 raw segment")?;
        let segment_position = segments.len();
        // A rollover is an intentionally non-terminal close.  It must retain
        // `clean_shutdown = false`; only the final segment proves that the
        // recorder reached its planned clean close.  Do not accept either
        // value indiscriminately: the receipt order freezes which segment is
        // terminal.
        let expected_clean_shutdown = completion_receipt
            .segment_list
            .len()
            .checked_sub(1)
            .is_some_and(|last_segment_position| segment_position == last_segment_position);
        let prefix_hash = scan_v2_segment(
            &path,
            receipt,
            &start_manifest.run_id,
            expected_previous_prefix_hash,
            expected_clean_shutdown,
            true,
            |frame_offset, record| index_builder.observe(segment_position, frame_offset, record),
        )?;
        expected_previous_prefix_hash = Some(prefix_hash);
        raw_segment_set_hasher.update(&receipt.segment_index.to_le_bytes());
        raw_segment_set_hasher.update(receipt.filename.as_bytes());
        raw_segment_set_hasher.update(&receipt.file_bytes.to_le_bytes());
        raw_segment_set_hasher.update(receipt.file_sha256.as_array());
        raw_segment_set_hasher.update(receipt.file_blake3.as_array());
        segments.push(PumpExactStateIndexedSegmentV2 {
            path,
            receipt: receipt.clone(),
            pinned_file: None,
        });
    }

    if index_builder
        .slots
        .values()
        .any(|node| node.finalized_parents.len() > 1)
    {
        bail!("V2 finalized Slot evidence assigns more than one parent to a slot");
    }
    index_builder
        .collector
        .finish(&completion_receipt, &index_builder.slots)?;
    let source_availability_bounds = index_builder
        .collector
        .source_availability_bounds(&completion_receipt, &index_builder.slots)?;
    let block_lanes_by_slot = std::mem::take(&mut index_builder.collector.block_lanes_by_slot);
    let mut slot_ledger = BTreeMap::new();
    for (slot, slot_node) in index_builder.slots {
        slot_ledger.insert(
            slot,
            PumpExactStatePerSlotLedgerV2 {
                slot: slot_node,
                block_lanes: block_lanes_by_slot.get(&slot).cloned().unwrap_or_default(),
            },
        );
    }
    for (slot, block_lanes) in block_lanes_by_slot {
        slot_ledger.entry(slot).or_default().block_lanes = block_lanes;
    }
    let full_block_pump_transaction_count =
        u64::try_from(index_builder.collector.full_block_pump_transactions.len())
            .unwrap_or(u64::MAX);
    let raw_segment_set_blake3 = hex_bytes(raw_segment_set_hasher.finalize().as_bytes());
    Ok(PumpExactStateRawTapeIndexV2 {
        raw_dir: raw_dir.to_path_buf(),
        start_manifest,
        completion_receipt,
        segments,
        transactions: index_builder.transactions,
        account_updates: index_builder.account_updates,
        slot_ledger,
        full_block_pump_transaction_count,
        source_availability_bounds,
        raw_segment_set_blake3,
    })
}

/// Materialize one complete prospective V2 raw run into an atomic exact-state
/// artifact.  This is strictly offline: it reads only the V2 raw directory,
/// the explicitly supplied semantics authority, and the kernel-bound image of
/// this process.  It has no provider, RPC, GO-E, capture, or active-runtime
/// code path.
///
/// A returned [`PumpExactStateCapabilityStatusV2::Blocked`] is a normal
/// result.  The function publishes its diagnostic artifact but never treats a
/// blocked tape as exportable strategy input.
pub fn qualify_prospective_exact_state_raw_run_v2(
    raw_dir: &Path,
    semantics_manifest_path: &Path,
    output_dir: &Path,
) -> Result<PumpExactStateQualificationSummaryV2> {
    // Validate the output/raw boundary before copying or materializing any raw
    // byte. This is intentionally earlier than the raw scan: an operator typo
    // must not create a partial artifact inside immutable raw.
    validate_v2_exact_output_path(raw_dir, output_dir)?;
    let unsealed_raw = index_prospective_exact_state_raw_run_v2(raw_dir)?;
    let semantics = load_pump_exact_state_semantics_authority_v2(semantics_manifest_path)?;
    validate_raw_semantics_binding_v2(&unsealed_raw.start_manifest, &semantics)?;
    semantics.validate_program_data(&unsealed_raw.start_manifest.program_data_at_start)?;
    let running_executable = digest_v2_running_executable()?;
    let snapshot_parent = output_dir
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 exact output has no snapshot parent"))?;
    require_v2_qualification_storage_budget(
        snapshot_parent,
        unsealed_raw.start_manifest.min_free_bytes,
        unsealed_raw.total_receipt_bound_segment_bytes()?,
    )?;
    let raw = unsealed_raw.seal_anonymous_snapshot_v2(snapshot_parent)?;
    let rooted_slots = raw.rooted_slots();
    // The stream-only BondingCurve/Global updates are source evidence, not
    // canonical state authority by themselves. The prospective lane retains
    // updates from slots which later fail to become the unique finalized
    // full-block/Slot/BlockMeta view, but never lets one supply an exact
    // before/after anchor for a rooted transaction.
    let anchors = PumpExactStateAnchorIndexV2::build(&raw, &semantics, &rooted_slots)?;
    let cohort_slots_strictly_after = raw
        .completion_receipt
        .cohort_slots_strictly_after
        .ok_or_else(|| anyhow::anyhow!("verified V2 raw run lacks cohort boundary"))?;

    let mut writer = PumpExactStateExactOutputWriterV2::create(raw_dir, output_dir)?;
    let mut counters = PumpExactStateQualificationCountersV2::default();
    counters.rooted_canonical_slot_count = u64::try_from(rooted_slots.len()).unwrap_or(u64::MAX);
    counters.bonding_curve_account_count = anchors.bonding_curve_account_count;
    counters.bonding_curve_decoded_count = anchors.bonding_curve_decoded_count;
    counters.global_account_count = anchors.global_account_count;
    counters.global_validated_count = anchors.global_validated_count;
    counters.unknown_pump_owned_account_count = anchors.unknown_account_count;
    counters.account_decode_failure_count = anchors.account_decode_failure_count;

    let mut transactions = raw.transactions.clone();
    transactions.sort_by_key(|transaction| {
        (
            transaction.slot,
            transaction.tx_index,
            transaction.signature,
        )
    });
    for indexed in &transactions {
        let transaction = raw.read_transaction(indexed)?;
        let source_capture_sequence = transaction.source.capture_sequence;
        let observed_ingress_wall_ms = transaction.event_time.ingress_wall_ts_ms;
        let observed_ingress_monotonic_ms = transaction.event_time.ingress_monotonic_ts_ms;
        let context = decode_v2_transaction_context(&transaction)?;
        let inventory = inventory_v2_from_transaction_context(&context, &semantics, &anchors)?;
        if inventory.slot != indexed.slot
            || inventory.tx_index != indexed.tx_index
            || inventory.signature != indexed.signature
        {
            bail!("V2 semantic inventory identity differs from indexed raw transaction");
        }
        let rooted =
            rooted_slots.contains(&inventory.slot) && inventory.slot > cohort_slots_strictly_after;
        let capability_relevant = rooted && inventory.success;
        let mut transaction_reasons = BTreeSet::new();
        let mut candidate_count = 0u32;
        let mut exact_candidate_count = 0u32;
        let mut unknown_or_malformed = false;
        let mut candidate_coverage = Vec::new();

        for occurrence in &inventory.occurrences {
            match &occurrence.class {
                PumpExactStateOccurrenceClassV2::ProvenNonReserve { .. } => {
                    if capability_relevant {
                        counters.successful_rooted_proven_non_reserve_count = counters
                            .successful_rooted_proven_non_reserve_count
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("V2 occurrence census overflow"))?;
                    }
                }
                PumpExactStateOccurrenceClassV2::ValidatedEventTransport { .. } => {
                    if capability_relevant {
                        counters.successful_rooted_validated_event_transport_count = counters
                            .successful_rooted_validated_event_transport_count
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("V2 occurrence census overflow"))?;
                    }
                }
                PumpExactStateOccurrenceClassV2::Unknown { reason } => {
                    if capability_relevant {
                        unknown_or_malformed = true;
                        counters.successful_rooted_unknown_occurrence_count = counters
                            .successful_rooted_unknown_occurrence_count
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("V2 occurrence census overflow"))?;
                        insert_bounded_v2_reason(&mut transaction_reasons, reason.clone());
                    }
                }
                PumpExactStateOccurrenceClassV2::Candidate {
                    instruction_payload_exact,
                    account_vector_exact,
                    failure_reason,
                    ..
                } => {
                    if capability_relevant {
                        candidate_count = candidate_count
                            .checked_add(1)
                            .ok_or_else(|| anyhow::anyhow!("V2 candidate count overflow"))?;
                        if !instruction_payload_exact || !account_vector_exact {
                            unknown_or_malformed = true;
                            counters.successful_rooted_malformed_candidate_count = counters
                                .successful_rooted_malformed_candidate_count
                                .checked_add(1)
                                .ok_or_else(|| {
                                    anyhow::anyhow!("V2 malformed-candidate census overflow")
                                })?;
                            insert_bounded_v2_reason(
                                &mut transaction_reasons,
                                failure_reason.clone().unwrap_or_else(|| {
                                    "candidate_instruction_contract_not_exact".to_owned()
                                }),
                            );
                        }
                    }
                }
            }
        }

        if capability_relevant {
            counters.successful_rooted_instruction_occurrence_count = counters
                .successful_rooted_instruction_occurrence_count
                .checked_add(u64::try_from(inventory.occurrences.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| anyhow::anyhow!("V2 occurrence census overflow"))?;
        }

        for occurrence in &inventory.occurrences {
            let PumpExactStateOccurrenceClassV2::Candidate {
                effect,
                instruction_payload_exact,
                account_vector_exact,
                bonding_curve,
                mint,
                failure_reason,
                ..
            } = &occurrence.class
            else {
                continue;
            };
            if !capability_relevant {
                continue;
            }
            counters.successful_rooted_candidate_count = counters
                .successful_rooted_candidate_count
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 candidate census overflow"))?;
            counters.successful_rooted_mutation_denominator = counters
                .successful_rooted_mutation_denominator
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 mutation denominator overflow"))?;

            let evaluation = evaluate_candidate_exactness_v2(
                *effect,
                *instruction_payload_exact,
                *account_vector_exact,
                *bonding_curve,
                inventory.signature,
                inventory.slot,
                inventory.tx_index,
                candidate_count,
                unknown_or_malformed,
                &anchors,
            );
            if let Some(reason) = failure_reason {
                insert_bounded_v2_reason(&mut transaction_reasons, reason.clone());
            }
            if let Some(reason) = &evaluation.non_exact_reason {
                insert_bounded_v2_reason(&mut transaction_reasons, reason.clone());
            }
            candidate_coverage.push(PumpExactStateCandidateCoverageRecordV2 {
                bonding_curve: (*bonding_curve).map(|value| value.to_string()),
                mint: (*mint).map(|value| value.to_string()),
                effect: exact_state_effect_label_v2(*effect).to_owned(),
                exact: evaluation.exact,
                non_exact_reason: evaluation.non_exact_reason.clone(),
            });
            if evaluation.exact {
                exact_candidate_count = exact_candidate_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 exact candidate count overflow"))?;
                counters.exact_rooted_mutation_count = counters
                    .exact_rooted_mutation_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 exact mutation count overflow"))?;
                counters.exact_trajectory_count = counters
                    .exact_trajectory_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 exact trajectory count overflow"))?;
                if effect.is_supported_exact_trade()
                    && evaluation.state_before.is_some()
                    && evaluation.state_after.is_some()
                {
                    counters.successful_rooted_exact_trade_with_both_states_count = counters
                        .successful_rooted_exact_trade_with_both_states_count
                        .checked_add(1)
                        .ok_or_else(|| {
                            anyhow::anyhow!("V2 exact trade-with-both-states count overflow")
                        })?;
                }

                // Exact trajectories are the only rows published in this
                // stream. Every non-exact candidate remains represented in
                // coverage_v2.jsonl and in the receipt denominator, so a
                // Qualified artifact can legitimately contain a small number
                // of explicit non-exact candidates without contradicting its
                // exact-trajectory line-count binding.
                let curve = bonding_curve
                    .map(|value| value.to_string())
                    .unwrap_or_else(|| "unknown".to_owned());
                writer.write_trajectory(&PumpExactStateTrajectoryRecordV2 {
                    schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                    source_run_id: raw.start_manifest.run_id.clone(),
                    source_capture_sequence,
                    observed_ingress_wall_ms,
                    observed_ingress_monotonic_ms,
                    slot: inventory.slot,
                    tx_index: inventory.tx_index,
                    signature: bs58::encode(inventory.signature).into_string(),
                    bonding_curve: curve.clone(),
                    mint: mint.map(|value| value.to_string()),
                    effect: exact_state_effect_label_v2(*effect).to_owned(),
                    state_before: evaluation
                        .state_before
                        .as_ref()
                        .map(PumpExactStateCurveStateArtifactV2::from),
                    state_after: evaluation
                        .state_after
                        .as_ref()
                        .map(PumpExactStateCurveStateArtifactV2::from),
                })?;
                if matches!(
                    effect,
                    PumpExactStateInstructionEffectV2::SupportedExactCreate
                ) {
                    let state = evaluation.state_after.as_ref().ok_or_else(|| {
                        anyhow::anyhow!("V2 exact create lacks final curve state")
                    })?;
                    counters.exact_birth_count = counters
                        .exact_birth_count
                        .checked_add(1)
                        .ok_or_else(|| anyhow::anyhow!("V2 exact birth count overflow"))?;
                    writer.write_birth(&PumpExactStateBirthRecordV2 {
                        schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                        source_run_id: raw.start_manifest.run_id.clone(),
                        source_capture_sequence,
                        observed_ingress_wall_ms,
                        observed_ingress_monotonic_ms,
                        slot: inventory.slot,
                        tx_index: inventory.tx_index,
                        signature: bs58::encode(inventory.signature).into_string(),
                        bonding_curve: curve,
                        mint: mint.map(|value| value.to_string()),
                        initial_state: PumpExactStateCurveStateArtifactV2::from(state),
                    })?;
                }
            } else {
                counters.explicit_non_exact_mutation_count = counters
                    .explicit_non_exact_mutation_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 non-exact mutation count overflow"))?;
            }
        }

        writer.write_coverage(&PumpExactStateCoverageRecordV2 {
            schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
            source_run_id: raw.start_manifest.run_id.clone(),
            source_capture_sequence,
            observed_ingress_wall_ms,
            observed_ingress_monotonic_ms,
            slot: inventory.slot,
            tx_index: inventory.tx_index,
            signature: bs58::encode(inventory.signature).into_string(),
            rooted,
            success: inventory.success,
            occurrence_count: u32::try_from(inventory.occurrences.len()).unwrap_or(u32::MAX),
            candidate_count,
            exact_candidate_count,
            inventory_complete: !unknown_or_malformed,
            reason_codes: transaction_reasons.into_iter().collect(),
            candidates: candidate_coverage,
        })?;
    }

    counters.occurrence_ledger_reconciled = counters.successful_rooted_instruction_occurrence_count
        == counters
            .successful_rooted_proven_non_reserve_count
            .checked_add(counters.successful_rooted_validated_event_transport_count)
            .and_then(|value| value.checked_add(counters.successful_rooted_candidate_count))
            .and_then(|value| {
                value.checked_add(counters.successful_rooted_unknown_occurrence_count)
            })
            .unwrap_or(u64::MAX);
    counters.denominator_reconciled = counters.successful_rooted_mutation_denominator
        == counters
            .exact_rooted_mutation_count
            .checked_add(counters.explicit_non_exact_mutation_count)
            .unwrap_or(u64::MAX);
    counters.exact_rooted_coverage_ppm = coverage_ppm_v2(
        counters.exact_rooted_mutation_count,
        counters.successful_rooted_mutation_denominator,
    )?;
    counters.qualification_run_below_minimum = qualification_run_below_minimum_v2(
        raw.completion_receipt.cohort_capture_elapsed_ms,
        counters.successful_rooted_mutation_denominator,
    );

    let blockers = capability_blockers_v2(&counters);
    let status = if blockers.is_empty() {
        PumpExactStateCapabilityStatusV2::Qualified
    } else {
        PumpExactStateCapabilityStatusV2::Blocked
    };
    let raw_start_manifest_digest =
        digest_private_artifact_v2(&raw.raw_dir.join("run_start_manifest_v2.json"))?;
    let raw_completion_receipt_digest =
        digest_private_artifact_v2(&raw.raw_dir.join("run_completion_receipt_v2.json"))?;
    let semantics_manifest_digest = semantics_digest_to_runtime_v2(&semantics.manifest_digest);
    let vendored_idl_digest = semantics_digest_to_runtime_v2(&semantics.idl_digest);
    let source_run_id = raw.start_manifest.run_id.clone();
    let raw_segment_set_blake3 = raw.raw_segment_set_blake3.clone();
    let raw_storage_format_version = raw.start_manifest.storage_format_version;
    let raw_recheck_dir = raw_dir.to_path_buf();
    let semantics_recheck_path = semantics_manifest_path.to_path_buf();
    let expected_raw_segment_set_blake3 = raw_segment_set_blake3.clone();
    let expected_semantics_manifest_digest = semantics_manifest_digest.clone();
    let expected_vendored_idl_digest = vendored_idl_digest.clone();
    let expected_executable = running_executable.clone();
    let expected_raw_start_manifest_digest =
        artifact_digest_to_runtime_v2(&raw_start_manifest_digest);
    let expected_raw_completion_receipt_digest =
        artifact_digest_to_runtime_v2(&raw_completion_receipt_digest);
    let output_dir = output_dir.to_path_buf();

    let (receipt_path, _) = writer.finish(
        |births_artifact, trajectories_artifact, coverage_artifact| {
            let receipt = PumpExactStateCapabilityReceiptV2 {
                schema_version: PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2,
                kind: "pump_exact_state_capability_v2".to_owned(),
                source_run_id: source_run_id.clone(),
                status,
                blockers: blockers.clone(),
                source_storage_format_version: raw_storage_format_version,
                source_raw_segment_set_blake3: raw_segment_set_blake3.clone(),
                source_start_manifest_digest: artifact_digest_to_runtime_v2(
                    &raw_start_manifest_digest,
                ),
                source_completion_receipt_digest: artifact_digest_to_runtime_v2(
                    &raw_completion_receipt_digest,
                ),
                semantics_id: semantics.semantics_id.clone(),
                semantics_manifest_digest: semantics_manifest_digest.clone(),
                vendored_idl_digest: vendored_idl_digest.clone(),
                materializer_running_executable_digest: running_executable.clone(),
                cohort_slots_strictly_after,
                rooted_canonical_slot_count: counters.rooted_canonical_slot_count,
                filtered_pump_transaction_count: u64::try_from(raw.transactions.len())
                    .unwrap_or(u64::MAX),
                full_block_pump_transaction_count: raw.full_block_pump_transaction_count,
                pump_owned_account_update_count: u64::try_from(raw.account_updates.len())
                    .unwrap_or(u64::MAX),
                bonding_curve_account_count: counters.bonding_curve_account_count,
                bonding_curve_decoded_count: counters.bonding_curve_decoded_count,
                global_account_count: counters.global_account_count,
                global_validated_count: counters.global_validated_count,
                unknown_pump_owned_account_count: counters.unknown_pump_owned_account_count,
                account_decode_failure_count: counters.account_decode_failure_count,
                successful_rooted_instruction_occurrence_count: counters
                    .successful_rooted_instruction_occurrence_count,
                successful_rooted_proven_non_reserve_count: counters
                    .successful_rooted_proven_non_reserve_count,
                successful_rooted_validated_event_transport_count: counters
                    .successful_rooted_validated_event_transport_count,
                successful_rooted_candidate_count: counters.successful_rooted_candidate_count,
                successful_rooted_unknown_occurrence_count: counters
                    .successful_rooted_unknown_occurrence_count,
                successful_rooted_malformed_candidate_count: counters
                    .successful_rooted_malformed_candidate_count,
                occurrence_ledger_reconciled: counters.occurrence_ledger_reconciled,
                successful_rooted_mutation_denominator: counters
                    .successful_rooted_mutation_denominator,
                exact_rooted_mutation_count: counters.exact_rooted_mutation_count,
                explicit_non_exact_mutation_count: counters.explicit_non_exact_mutation_count,
                denominator_reconciled: counters.denominator_reconciled,
                exact_rooted_coverage_ppm: counters.exact_rooted_coverage_ppm,
                qualification_run_below_minimum: counters.qualification_run_below_minimum,
                required_exact_rooted_coverage_ppm: PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2,
                exact_trajectory_count: counters.exact_trajectory_count,
                successful_rooted_exact_trade_with_both_states_count: counters
                    .successful_rooted_exact_trade_with_both_states_count,
                exact_birth_count: counters.exact_birth_count,
                births_artifact: births_artifact.clone(),
                trajectories_artifact: trajectories_artifact.clone(),
                coverage_artifact: coverage_artifact.clone(),
            };
            Ok(receipt)
        },
        |receipt_artifact, births_artifact, trajectories_artifact, coverage_artifact| {
            Ok(PumpExactStateExactManifestV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                kind: "pump_exact_state_tape_v2".to_owned(),
                source_run_id: source_run_id.clone(),
                exact_state_capability_status: status,
                source_raw_segment_set_blake3: raw_segment_set_blake3.clone(),
                semantics_manifest_sha256: semantics_manifest_digest.sha256.clone(),
                semantics_manifest_blake3: semantics_manifest_digest.blake3.clone(),
                materializer_running_executable_sha256: running_executable.sha256.clone(),
                materializer_running_executable_blake3: running_executable.blake3.clone(),
                materializer_running_executable_bytes: running_executable.bytes,
                exact_state_capability_artifact: receipt_artifact,
                births_artifact,
                trajectories_artifact,
                coverage_artifact,
            })
        },
        || {
            let revalidated_raw = index_prospective_exact_state_raw_run_v2(&raw_recheck_dir)
                .context("revalidate V2 raw authority immediately before exact publish")?;
            if revalidated_raw.raw_segment_set_blake3 != expected_raw_segment_set_blake3 {
                bail!("V2 raw segment-set digest changed before exact publish");
            }
            if artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
                &raw_recheck_dir.join("run_start_manifest_v2.json"),
            )?) != expected_raw_start_manifest_digest
                || artifact_digest_to_runtime_v2(&digest_private_artifact_v2(
                    &raw_recheck_dir.join("run_completion_receipt_v2.json"),
                )?) != expected_raw_completion_receipt_digest
            {
                bail!("V2 raw control authority changed before exact publish");
            }
            let revalidated_semantics = load_pump_exact_state_semantics_authority_v2(
                &semantics_recheck_path,
            )
            .context("revalidate V2 semantics authority immediately before exact publish")?;
            validate_raw_semantics_binding_v2(
                &revalidated_raw.start_manifest,
                &revalidated_semantics,
            )?;
            revalidated_semantics
                .validate_program_data(&revalidated_raw.start_manifest.program_data_at_start)?;
            if semantics_digest_to_runtime_v2(&revalidated_semantics.manifest_digest)
                != expected_semantics_manifest_digest
                || semantics_digest_to_runtime_v2(&revalidated_semantics.idl_digest)
                    != expected_vendored_idl_digest
            {
                bail!("V2 semantics authority changed before exact publish");
            }
            if digest_v2_running_executable()? != expected_executable {
                bail!("V2 materializer running-image digest changed before exact publish");
            }
            validate_v2_exact_output_path(&raw_recheck_dir, &output_dir)?;
            Ok(())
        },
    )?;

    Ok(PumpExactStateQualificationSummaryV2 {
        source_run_id,
        status,
        blockers,
        output_dir,
        receipt_path,
        exact_rooted_coverage_ppm: counters.exact_rooted_coverage_ppm,
        exact_rooted_mutation_count: counters.exact_rooted_mutation_count,
        successful_rooted_mutation_denominator: counters.successful_rooted_mutation_denominator,
    })
}

#[derive(Default)]
struct PumpExactStateQualificationCountersV2 {
    rooted_canonical_slot_count: u64,
    bonding_curve_account_count: u64,
    bonding_curve_decoded_count: u64,
    global_account_count: u64,
    global_validated_count: u64,
    unknown_pump_owned_account_count: u64,
    account_decode_failure_count: u64,
    successful_rooted_instruction_occurrence_count: u64,
    successful_rooted_proven_non_reserve_count: u64,
    successful_rooted_validated_event_transport_count: u64,
    successful_rooted_candidate_count: u64,
    successful_rooted_unknown_occurrence_count: u64,
    successful_rooted_malformed_candidate_count: u64,
    occurrence_ledger_reconciled: bool,
    successful_rooted_mutation_denominator: u64,
    exact_rooted_mutation_count: u64,
    explicit_non_exact_mutation_count: u64,
    denominator_reconciled: bool,
    exact_rooted_coverage_ppm: u64,
    qualification_run_below_minimum: bool,
    exact_trajectory_count: u64,
    successful_rooted_exact_trade_with_both_states_count: u64,
    exact_birth_count: u64,
}

#[derive(Clone, Debug)]
struct PumpExactStateCandidateEvaluationV2 {
    exact: bool,
    non_exact_reason: Option<String>,
    state_before: Option<PumpExactStateCurveStateV2>,
    state_after: Option<PumpExactStateCurveStateV2>,
}

#[allow(clippy::too_many_arguments)]
fn evaluate_candidate_exactness_v2(
    effect: PumpExactStateInstructionEffectV2,
    instruction_payload_exact: bool,
    account_vector_exact: bool,
    bonding_curve: Option<Pubkey>,
    signature: [u8; 64],
    slot: u64,
    transaction_index: u32,
    transaction_candidate_count: u32,
    transaction_has_unknown_or_malformed: bool,
    anchors: &PumpExactStateAnchorIndexV2,
) -> PumpExactStateCandidateEvaluationV2 {
    let blocked = |reason: &str| PumpExactStateCandidateEvaluationV2 {
        exact: false,
        non_exact_reason: Some(reason.to_owned()),
        state_before: None,
        state_after: None,
    };
    if transaction_has_unknown_or_malformed {
        return blocked("transaction_mutation_inventory_incomplete");
    }
    if transaction_candidate_count != 1 {
        return blocked("transaction_has_multiple_reserve_or_dependency_candidates");
    }
    if !instruction_payload_exact {
        return blocked("instruction_payload_not_exact");
    }
    if !account_vector_exact {
        return blocked("instruction_account_vector_not_exact");
    }
    if !matches!(
        effect,
        PumpExactStateInstructionEffectV2::SupportedExactTrade
            | PumpExactStateInstructionEffectV2::SupportedExactCreate
    ) {
        return blocked("instruction_effect_not_supported_for_exact_state");
    }
    let Some(curve) = bonding_curve else {
        return blocked("bonding_curve_role_absent_from_exact_account_contract");
    };
    let Some(final_anchor) = anchors.unique_final_anchor(signature, curve, slot, transaction_index)
    else {
        return blocked("missing_or_ambiguous_same_signature_final_anchor");
    };
    if matches!(
        effect,
        PumpExactStateInstructionEffectV2::SupportedExactCreate
    ) {
        return PumpExactStateCandidateEvaluationV2 {
            exact: true,
            non_exact_reason: None,
            state_before: None,
            state_after: Some(final_anchor.state.clone()),
        };
    }
    let Some(pre_anchor) = anchors.unique_pre_anchor(curve, slot, transaction_index) else {
        return blocked("missing_exact_pre_anchor");
    };
    PumpExactStateCandidateEvaluationV2 {
        exact: true,
        non_exact_reason: None,
        state_before: Some(pre_anchor.state.clone()),
        state_after: Some(final_anchor.state.clone()),
    }
}

fn exact_state_effect_label_v2(effect: PumpExactStateInstructionEffectV2) -> &'static str {
    match effect {
        PumpExactStateInstructionEffectV2::SupportedExactTrade => "supported_exact_trade",
        PumpExactStateInstructionEffectV2::SupportedExactCreate => "supported_exact_create",
        PumpExactStateInstructionEffectV2::KnownReserveOrDependencyUnsupported => {
            "known_reserve_or_dependency_unsupported"
        }
        PumpExactStateInstructionEffectV2::GlobalDependencyMutation => "global_dependency_mutation",
        PumpExactStateInstructionEffectV2::ProvenNonReserve => "proven_non_reserve",
    }
}

fn insert_bounded_v2_reason(reasons: &mut BTreeSet<String>, reason: String) {
    const MAX_REASONS_PER_TRANSACTION: usize = 64;
    if reasons.len() < MAX_REASONS_PER_TRANSACTION {
        reasons.insert(reason);
    } else {
        reasons.insert("diagnostic_reason_overflow".to_owned());
    }
}

fn coverage_ppm_v2(numerator: u64, denominator: u64) -> Result<u64> {
    if denominator == 0 {
        return Ok(0);
    }
    numerator
        .checked_mul(1_000_000)
        .ok_or_else(|| anyhow::anyhow!("V2 coverage numerator overflow"))
        .map(|value| value / denominator)
}

/// A prospective run is usable only after a full V1.1 minimum cohort: either
/// thirty minutes of sealed cohort time or ten thousand successful rooted
/// mutations.  A missing elapsed value is deliberately below minimum.
fn qualification_run_below_minimum_v2(
    cohort_capture_elapsed_ms: Option<u64>,
    successful_rooted_mutation_denominator: u64,
) -> bool {
    cohort_capture_elapsed_ms.unwrap_or(0) < PUMP_EXACT_STATE_MIN_QUALIFICATION_COHORT_ELAPSED_MS_V2
        && successful_rooted_mutation_denominator
            < PUMP_EXACT_STATE_MIN_QUALIFICATION_MUTATION_DENOMINATOR_V2
}

fn capability_blockers_v2(
    counters: &PumpExactStateQualificationCountersV2,
) -> Vec<PumpExactStateCapabilityBlockerV2> {
    let mut blockers = BTreeSet::new();
    if counters.rooted_canonical_slot_count == 0 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::CanonicalSlotEvidenceMissing);
    }
    if counters.unknown_pump_owned_account_count != 0 || counters.account_decode_failure_count != 0
    {
        blockers.insert(PumpExactStateCapabilityBlockerV2::AccountDecodeIncomplete);
    }
    if counters.successful_rooted_unknown_occurrence_count != 0
        || counters.successful_rooted_malformed_candidate_count != 0
        || !counters.occurrence_ledger_reconciled
    {
        blockers.insert(PumpExactStateCapabilityBlockerV2::MutationInventoryIncomplete);
    }
    if counters.successful_rooted_mutation_denominator == 0 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::NoRootedCandidateMutation);
    }
    if counters.qualification_run_below_minimum {
        blockers.insert(PumpExactStateCapabilityBlockerV2::QualificationRunBelowMinimum);
    }
    if !counters.denominator_reconciled {
        blockers.insert(PumpExactStateCapabilityBlockerV2::MutationInventoryIncomplete);
    }
    if counters.exact_rooted_coverage_ppm < PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::ExactCoverageBelowThreshold);
    }
    if counters.exact_trajectory_count == 0 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::NoExactTrajectory);
    }
    if counters.successful_rooted_exact_trade_with_both_states_count == 0 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::NoSuccessfulRootedTradeWithBothStates);
    }
    if counters.exact_birth_count == 0 {
        blockers.insert(PumpExactStateCapabilityBlockerV2::NoExactBirth);
    }
    blockers.into_iter().collect()
}

fn semantics_digest_to_runtime_v2(
    digest: &crate::research_exact_tape_v2_semantics::PumpExactStateSemanticsDigestV2,
) -> PumpExactStateDigestV2 {
    PumpExactStateDigestV2 {
        sha256: digest.sha256.clone(),
        blake3: digest.blake3.clone(),
        bytes: digest.bytes,
    }
}

/// The semantics document must be selected before prospective capture, then
/// carried verbatim by the frozen raw start manifest.  A later qualifier may
/// re-read a file only to prove it is the same authority; it may never choose
/// a different IDL, ProgramData mapping, or layout after seeing the raw tape.
fn validate_raw_semantics_binding_v2(
    start: &PumpExactStateRunStartManifestV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
) -> Result<()> {
    if start.semantics_id != semantics.semantics_id
        || start.semantics_manifest_digest
            != semantics_digest_to_runtime_v2(&semantics.manifest_digest)
        || start.vendored_idl_digest != semantics_digest_to_runtime_v2(&semantics.idl_digest)
        || start.expected_program_data_hash_blake3
            != hex_bytes(&semantics.expected_program_data_hash_blake3())
    {
        bail!("V2 raw start manifest semantics authority differs from supplied manifest");
    }
    Ok(())
}

fn artifact_digest_to_runtime_v2(
    digest: &PumpExactStateArtifactDigestV2,
) -> PumpExactStateDigestV2 {
    PumpExactStateDigestV2 {
        sha256: digest.sha256.clone(),
        blake3: digest.blake3.clone(),
        bytes: digest.bytes,
    }
}

fn digest_v2_running_executable() -> Result<PumpExactStateDigestV2> {
    #[cfg(target_os = "linux")]
    {
        const MAX_EXECUTABLE_BYTES: u64 = 128 * 1024 * 1024;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
        let file = options
            .open("/proc/self/exe")
            .context("open kernel-bound /proc/self/exe for V2 qualification")?;
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.len() == 0 || metadata.len() > MAX_EXECUTABLE_BYTES {
            bail!("V2 qualification running executable is not a bounded regular file");
        }
        let length = usize::try_from(metadata.len())
            .context("V2 qualification executable length exceeds usize")?;
        let mut bytes = vec![0u8; length];
        file.read_exact_at(&mut bytes, 0)
            .context("read kernel-bound V2 qualification executable")?;
        let after = file.metadata()?;
        if after.len() != metadata.len()
            || after.dev() != metadata.dev()
            || after.ino() != metadata.ino()
        {
            bail!("V2 qualification running executable changed while digesting");
        }
        Ok(PumpExactStateDigestV2 {
            sha256: hex_bytes(&Sha256::digest(&bytes)),
            blake3: hex_bytes(blake3::hash(&bytes).as_bytes()),
            bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
        })
    }
    #[cfg(not(target_os = "linux"))]
    {
        bail!("V2 exact-state qualification requires Linux running-image authority")
    }
}

fn validate_v2_controls(
    start: &PumpExactStateRunStartManifestV2,
    completion: &PumpExactStateRunCompletionReceiptV2,
) -> Result<()> {
    if start.storage_format_version != PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2
        || completion.storage_format_version != PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2
        || start.schema_version != EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION
        || completion.schema_version != EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION
        || start.capture_config_schema_version != EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION
    {
        bail!("V2 raw control schema/version is not accepted");
    }
    if start.run_id != completion.run_id
        || !matches!(
            completion.status,
            PumpExactStateCaptureRunStatusV2::Complete
        )
        || !completion.clean_shutdown
        || !completion.readiness_completed
        || !completion.readiness_boundary_persisted
        || !completion.required_source_lanes_observed
        || !completion.storage_reserve_maintained
        || !completion.raw_byte_budget_respected
        || !completion.running_executable_unchanged
        || !completion.program_data_unchanged
        || completion.program_data_at_start != start.program_data_at_start
        // The start receipt is copied verbatim into completion for authority
        // binding. The independently observed completion receipt uses the
        // same semantic comparator as the recorder: its finalized RPC context
        // slot is an audit label and may legitimately advance during capture.
        || !completion
            .program_data_at_completion
            .as_ref()
            .is_some_and(|observed| {
                program_data_receipts_match_v2(&start.program_data_at_start, observed)
            })
        || !completion.source_lifecycle.stream_established
        || !completion.source_lifecycle.source_workers_cleanly_stopped
        || completion.source_lifecycle.source_updates_received == 0
        || completion.source_lifecycle.admitted_source_updates == 0
        || completion.source_lifecycle.dropped_source_updates != 0
        || completion.source_lifecycle.source_queue_bytes_at_close != 0
        || completion.source_lifecycle.source_readiness_status != "complete"
        || completion.source_lifecycle.fatal_capture_error.is_some()
        || completion.source_lifecycle.source_worker_error.is_some()
        || completion.writer.gap_count != 0
        || !completion.writer.clean_shutdown
        || completion.writer.error.is_some()
        || completion.writer.accepted_readiness_boundary_records != 1
        || completion.segment_list.is_empty()
    {
        bail!("V2 raw run does not satisfy the complete prospective capture contract");
    }
    let readiness = completion
        .source_readiness
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 complete receipt lacks source readiness"))?;
    let expected_readiness = [
        readiness.first_transaction_slot,
        readiness.first_account_update_slot,
        readiness.first_slot_update_slot,
        readiness.first_block_meta_slot,
        readiness.first_full_block_slot,
    ]
    .into_iter()
    .max()
    .ok_or_else(|| anyhow::anyhow!("V2 source readiness has no lane slots"))?;
    if readiness.source_readiness_slot != expected_readiness
        || completion.cohort_slots_strictly_after != Some(readiness.source_readiness_slot)
    {
        bail!("V2 stream-readiness boundary does not cover all required source lanes");
    }
    if completion
        .source_lifecycle
        .required_lane_first_slots
        .as_ref()
        != Some(readiness)
    {
        bail!("V2 source lifecycle readiness differs from stream boundary receipt");
    }
    let census = &completion.writer.required_lane_census;
    if census.transaction_messages == 0
        || census.account_updates == 0
        || census.slot_updates == 0
        || census.block_meta_updates == 0
        || census.full_blocks_started == 0
        || census.full_blocks_started != census.full_blocks_completed
        || census.incomplete_full_block_payloads != 0
        || census.unbound_full_block_chunks != 0
        || !census.full_block_payloads_reconciled
    {
        bail!("V2 completion receipt lacks reconciled required source lanes");
    }
    Ok(())
}

impl PumpExactStateRawRecordCollectorV2 {
    fn observe(&mut self, record: &PumpExactStateRawRecordV2) -> Result<()> {
        match record {
            PumpExactStateRawRecordV2::PrimaryTransaction(transaction) => {
                self.observe_source_sequence(
                    transaction.source.capture_sequence,
                    transaction.source.stream_epoch,
                )?;
                self.observe_source_ingress_time(
                    &transaction.event_time,
                    "filtered Pump transaction",
                )?;
                validate_source_payload(
                    transaction.source.capture_sequence,
                    &transaction.source.payload_hash_blake3,
                    &transaction.source_payload,
                )?;
                let (key, digest) = filtered_transaction_identity(transaction)?;
                if self
                    .filtered_transactions
                    .insert(key.clone(), digest)
                    .is_some()
                {
                    bail!(
                        "V2 filtered transaction {}:{} appears more than once",
                        key.slot,
                        key.tx_index
                    );
                }
            }
            PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) => {
                self.observe_source_sequence(
                    update.source.capture_sequence,
                    update.source.stream_epoch,
                )?;
                self.observe_source_ingress_time(&update.event_time, "Pump-owned account update")?;
                validate_source_payload(
                    update.source.capture_sequence,
                    &update.source.payload_hash_blake3,
                    &update.source_payload,
                )?;
                validate_pump_owned_account_update_projection_v2(
                    update,
                    self.expected_pump_program_id,
                )?;
                let data_hash = PumpResearchStorageHashV1::from(
                    *blake3::hash(&update.raw_account_data).as_bytes(),
                );
                if data_hash != update.raw_account_data_hash_blake3 {
                    bail!(
                        "V2 Pump-owned account update at capture sequence {} has data hash drift",
                        update.source.capture_sequence
                    );
                }
                self.pump_owned_account_update_count = self
                    .pump_owned_account_update_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 account update count overflow"))?;
            }
            PumpExactStateRawRecordV2::PrimarySlotUpdate(update) => {
                self.observe_source_sequence(
                    update.source.capture_sequence,
                    update.source.stream_epoch,
                )?;
                self.observe_source_ingress_time(&update.event_time, "Slot update")?;
                validate_source_payload(
                    update.source.capture_sequence,
                    &update.source.payload_hash_blake3,
                    &update.source_payload,
                )?;
                validate_slot_update_projection_v2(update)?;
                self.slot_update_count = self
                    .slot_update_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 slot update count overflow"))?;
            }
            PumpExactStateRawRecordV2::PrimaryBlockMeta(update) => {
                self.observe_source_sequence(
                    update.source.capture_sequence,
                    update.source.stream_epoch,
                )?;
                let ingress =
                    self.observe_source_ingress_time(&update.event_time, "BlockMeta update")?;
                validate_source_payload(
                    update.source.capture_sequence,
                    &update.source.payload_hash_blake3,
                    &update.source_payload,
                )?;
                validate_block_meta_projection_v2(update)?;
                self.block_meta_count = self
                    .block_meta_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 block-meta count overflow"))?;
                let ledger = self.block_lanes_by_slot.entry(update.slot).or_default();
                if ledger
                    .block_meta
                    .replace(PumpExactStateBlockMetaSlotEvidenceV2 {
                        parent_slot: update.parent_slot,
                        blockhash: update.blockhash.clone(),
                        parent_blockhash: update.parent_blockhash.clone(),
                        executed_transaction_count: update.executed_transaction_count,
                        ingress,
                        source_capture_sequence: update.source.capture_sequence,
                    })
                    .is_some()
                {
                    bail!("V2 raw run retains more than one BlockMeta payload for a slot");
                }
            }
            PumpExactStateRawRecordV2::FullBlockPayloadStarted(started) => {
                self.observe_source_sequence(
                    started.source.capture_sequence,
                    started.source.stream_epoch,
                )?;
                self.observe_source_ingress_time(&started.event_time, "full-block payload")?;
                if self.open_full_block.is_some()
                    || started.source_payload_bytes == 0
                    || started.source_payload_chunk_count == 0
                {
                    bail!("V2 full-block payload start is structurally invalid");
                }
                self.open_full_block = Some(PumpExactStateFullBlockAccumulatorV2 {
                    started: started.clone(),
                    next_chunk_index: 0,
                    bytes: Vec::new(),
                    sha256: Sha256::new(),
                    blake3: blake3::Hasher::new(),
                });
                self.full_block_started_count = self
                    .full_block_started_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full-block start count overflow"))?;
            }
            PumpExactStateRawRecordV2::FullBlockPayloadChunk(chunk) => {
                self.observe_full_block_chunk(chunk)?;
                self.full_block_chunk_count = self
                    .full_block_chunk_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full-block chunk count overflow"))?;
            }
            PumpExactStateRawRecordV2::FullBlockPayloadCompleted(completed) => {
                self.complete_full_block(completed)?;
            }
            PumpExactStateRawRecordV2::ProspectiveStreamBoundary(boundary) => {
                let readiness = &boundary.source_readiness;
                let expected_readiness_slot = [
                    readiness.first_transaction_slot,
                    readiness.first_account_update_slot,
                    readiness.first_slot_update_slot,
                    readiness.first_block_meta_slot,
                    readiness.first_full_block_slot,
                ]
                .into_iter()
                .max()
                .ok_or_else(|| anyhow::anyhow!("V2 stream readiness has no lane slots"))?;
                if boundary.source_stream_epoch == 0
                    || boundary.source_capture_sequence_exclusive == 0
                    || readiness.source_readiness_slot != expected_readiness_slot
                    || boundary.cohort_slots_strictly_after != expected_readiness_slot
                {
                    bail!("V2 prospective stream boundary is not self-consistent");
                }
                // The raw record order itself must prove the writer-side
                // guarantee: the boundary follows every admitted source
                // update whose capture sequence belongs to its exclusive
                // prefix. A completion receipt cannot repair a boundary
                // placed before that warm-up evidence, nor may a boundary
                // split the started/chunks/completed representation of one
                // full-block source update.
                let expected_last = boundary
                    .source_capture_sequence_exclusive
                    .checked_sub(1)
                    .ok_or_else(|| {
                        anyhow::anyhow!("V2 stream boundary exclusive source sequence underflowed")
                    })?;
                if self.open_full_block.is_some()
                    || u64::try_from(self.source_capture_sequences.len()).unwrap_or(u64::MAX)
                        != boundary.source_capture_sequence_exclusive
                    || self.source_capture_sequences.first().copied() != Some(0)
                    || self.source_capture_sequences.last().copied() != Some(expected_last)
                {
                    bail!(
                        "V2 stream-readiness boundary does not follow its complete exclusive source capture prefix"
                    );
                }
                if self.readiness_boundary.replace(boundary.clone()).is_some() {
                    bail!("V2 raw run retains more than one stream-readiness boundary");
                }
            }
            PumpExactStateRawRecordV2::CoverageGap(_) => {
                bail!("V2 complete raw run retains a coverage gap record");
            }
            PumpExactStateRawRecordV2::SegmentClosed(_) => {}
        }
        Ok(())
    }

    fn observe_source_sequence(&mut self, capture_sequence: u64, stream_epoch: u64) -> Result<()> {
        if stream_epoch == 0 {
            bail!("V2 source record has zero stream epoch");
        }
        match self.source_stream_epoch {
            Some(expected) if expected != stream_epoch => {
                bail!("V2 raw source stream epoch changed within one prospective run");
            }
            Some(_) => {}
            None => self.source_stream_epoch = Some(stream_epoch),
        }
        if let Some(boundary) = self.readiness_boundary.as_ref() {
            let previous = self.previous_source_capture_sequence.ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 source record followed a stream-readiness boundary without a warm-up prefix"
                )
            })?;
            let next = previous
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 source capture sequence overflow"))?;
            let expected_capture_sequence = if next == boundary.source_capture_sequence_exclusive {
                next.checked_add(1).ok_or_else(|| {
                    anyhow::anyhow!("V2 readiness boundary ordering marker overflow")
                })?
            } else {
                next
            };
            if capture_sequence != expected_capture_sequence {
                bail!(
                    "V2 source capture sequence {} is not contiguous with expected {} after the readiness boundary",
                    capture_sequence,
                    expected_capture_sequence
                );
            }
        } else if self
            .previous_source_capture_sequence
            .is_some_and(|previous| capture_sequence <= previous)
        {
            bail!("V2 source capture sequence is not strictly increasing");
        }
        if !self.source_capture_sequences.insert(capture_sequence) {
            bail!("V2 source capture sequence appears more than once");
        }
        self.previous_source_capture_sequence = Some(capture_sequence);
        Ok(())
    }

    /// Bind a source record to both timestamp domains.  Capture sequence and
    /// monotonic time must never move backwards; wall time is intentionally
    /// not compared because NTP/system-clock adjustments are audit metadata,
    /// not temporal authority.
    fn observe_source_ingress_time(
        &mut self,
        event_time: &PumpResearchEventTimeV1,
        label: &str,
    ) -> Result<PumpExactStateIngressTimestampV2> {
        let ingress = ingress_timestamp_v2(event_time, label)?;
        let monotonic_ms = ingress.monotonic_ms;
        if self
            .previous_source_ingress_monotonic_ms
            .is_some_and(|previous| monotonic_ms < previous)
        {
            bail!("V2 source ingress-monotonic time moved backwards");
        }
        if self.first_observed_ingress.is_none() {
            self.first_observed_ingress = Some(ingress);
        }
        self.previous_source_ingress_monotonic_ms = Some(monotonic_ms);
        Ok(ingress)
    }

    fn source_availability_bounds(
        &self,
        completion: &PumpExactStateRunCompletionReceiptV2,
        slots: &BTreeMap<u64, PumpExactStateSlotNodeV2>,
    ) -> Result<PumpExactStateSourceAvailabilityBoundsV2> {
        let first_observed_ingress = self.first_observed_ingress.ok_or_else(|| {
            anyhow::anyhow!("V2 raw record stream lacks any observed ingress timestamp pair")
        })?;
        let cohort_slots_strictly_after =
            completion.cohort_slots_strictly_after.ok_or_else(|| {
                anyhow::anyhow!("V2 completion receipt lacks stream-readiness cohort slot")
            })?;
        let (reconciled_full_block_frontier_slot, reconciled_full_block_frontier_ingress) =
            self.reconciled_full_block_frontier(cohort_slots_strictly_after, slots)?;
        if reconciled_full_block_frontier_ingress.monotonic_ms < first_observed_ingress.monotonic_ms
        {
            bail!("V2 reconciled full-block frontier precedes raw source availability start");
        }
        Ok(PumpExactStateSourceAvailabilityBoundsV2 {
            first_observed_ingress,
            reconciled_full_block_frontier_slot,
            reconciled_full_block_frontier_ingress,
        })
    }

    /// Require a bijective BlockMeta/full-block pair for every executed block
    /// record in the accepted cohort.  We deliberately do not require the
    /// converse for arbitrary Slot updates: the Slot stream itself cannot
    /// distinguish a skipped Solana slot from a provider omission.  It is
    /// therefore never the forward-completeness watermark.
    fn reconciled_full_block_frontier(
        &self,
        cohort_slots_strictly_after: u64,
        slots: &BTreeMap<u64, PumpExactStateSlotNodeV2>,
    ) -> Result<(u64, PumpExactStateIngressTimestampV2)> {
        // The first accepted produced block may have a parent at or before
        // the stream-readiness boundary because the new prospective stream
        // intentionally does not retain
        // a full historical blockhash chain.  Every later retained block must
        // extend the immediately preceding retained produced block.  This
        // deliberately uses `parent_slot`, never `slot - 1`: Solana can skip
        // numeric slots, but it cannot make a finalized child skip its actual
        // produced parent without leaving an evidence gap.
        let mut reconciled_blockhashes = BTreeMap::<u64, String>::new();
        let mut chain_tip_slot: Option<u64> = None;
        let mut chain_availability: Option<(PumpExactStateIngressTimestampV2, u64)> = None;
        for (slot, ledger) in &self.block_lanes_by_slot {
            if *slot <= cohort_slots_strictly_after {
                continue;
            }
            let (meta, full) = match (&ledger.block_meta, &ledger.full_block) {
                (Some(meta), Some(full)) => (meta, full),
                (Some(_), None) => {
                    bail!("V2 accepted cohort BlockMeta slot {slot} lacks matching full-block payload")
                }
                (None, Some(_)) => {
                    bail!("V2 accepted cohort full-block slot {slot} lacks matching BlockMeta payload")
                }
                (None, None) => continue,
            };
            if meta.parent_slot != full.parent_slot
                || meta.blockhash != full.blockhash
                || meta.parent_blockhash != full.parent_blockhash
                || meta.executed_transaction_count != full.executed_transaction_count
            {
                bail!("V2 BlockMeta/full-block identity differs for accepted cohort slot {slot}");
            }
            // We do not require a BlockMeta/full-block pair for every Slot:
            // a naked Slot can describe a legitimate skipped Solana slot.
            // The converse is different.  Once both block lanes prove that a
            // block executed, a matching finalized Slot is mandatory.  Without
            // it, a real Pump transaction could be silently excluded from the
            // rooted capability denominator merely because the Slot lane was
            // lost.
            let slot_node = slots.get(slot).ok_or_else(|| {
                anyhow::anyhow!(
                    "V2 accepted cohort BlockMeta/full-block slot {slot} lacks finalized Slot evidence"
                )
            })?;
            if slot_node.finalized_parents.len() != 1
                || !slot_node
                    .finalized_parents
                    .contains(&Some(meta.parent_slot))
            {
                bail!(
                    "V2 finalized Slot parent differs from BlockMeta/full-block identity for accepted cohort slot {slot}"
                );
            }
            if meta.parent_slot >= *slot {
                bail!(
                    "V2 accepted cohort BlockMeta/full-block slot {slot} has a non-earlier parent slot {}",
                    meta.parent_slot
                );
            }
            if meta.parent_slot > cohort_slots_strictly_after {
                let parent_blockhash = reconciled_blockhashes
                    .get(&meta.parent_slot)
                    .ok_or_else(|| {
                        anyhow::anyhow!(
                            "V2 accepted cohort BlockMeta/full-block slot {slot} references missing or unreconciled parent slot {}",
                            meta.parent_slot
                        )
                    })?;
                if meta.parent_blockhash != *parent_blockhash {
                    bail!(
                        "V2 accepted cohort BlockMeta/full-block slot {slot} parent blockhash differs from retained parent slot {}",
                        meta.parent_slot
                    );
                }
                if chain_tip_slot != Some(meta.parent_slot) {
                    bail!(
                        "V2 accepted cohort BlockMeta/full-block slot {slot} does not extend the complete parent-linked chain tip {:?}",
                        chain_tip_slot
                    );
                }
            } else if chain_tip_slot.is_some() {
                bail!(
                    "V2 accepted cohort BlockMeta/full-block slot {slot} reconnects below the stream-readiness boundary instead of extending the complete parent-linked chain"
                );
            }
            // The availability frontier begins only when the second member of
            // the reconciled pair has actually arrived.  The two lanes may be
            // delivered at different ingress times, so using the full-block
            // timestamp alone could claim coverage before its matching
            // BlockMeta was observed.  The raw collector has already checked
            // monotonic ordering, hence this deterministic max is the time at
            // which both completeness witnesses existed locally.
            let (pair_completed_ingress, pair_completed_sequence) =
                if (meta.ingress.monotonic_ms, meta.source_capture_sequence)
                    >= (full.ingress.monotonic_ms, full.source_capture_sequence)
                {
                    (meta.ingress, meta.source_capture_sequence)
                } else {
                    (full.ingress, full.source_capture_sequence)
                };
            if chain_availability.as_ref().is_none_or(|current| {
                (pair_completed_ingress.monotonic_ms, pair_completed_sequence)
                    > (current.0.monotonic_ms, current.1)
            }) {
                chain_availability = Some((pair_completed_ingress, pair_completed_sequence));
            }
            reconciled_blockhashes.insert(*slot, meta.blockhash.clone());
            chain_tip_slot = Some(*slot);
        }
        let slot = chain_tip_slot.ok_or_else(|| {
            anyhow::anyhow!("V2 accepted cohort has no reconciled BlockMeta/full-block frontier")
        })?;
        let (ingress, _) = chain_availability.ok_or_else(|| {
            anyhow::anyhow!("V2 accepted cohort parent-linked chain lacks availability evidence")
        })?;
        Ok((slot, ingress))
    }

    fn observe_full_block_chunk(
        &mut self,
        chunk: &PumpExactStateFullBlockPayloadChunkV2,
    ) -> Result<()> {
        let open = self
            .open_full_block
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("V2 full-block chunk has no open payload"))?;
        if chunk.source_capture_sequence != open.started.source.capture_sequence
            || chunk.chunk_index != open.next_chunk_index
        {
            bail!("V2 full-block chunk sequence/index mismatch");
        }
        let next_len = open
            .bytes
            .len()
            .checked_add(chunk.bytes.len())
            .ok_or_else(|| anyhow::anyhow!("V2 full-block byte count overflow"))?;
        if u64::try_from(next_len).unwrap_or(u64::MAX) > open.started.source_payload_bytes {
            bail!("V2 full-block chunks exceed declared source payload length");
        }
        open.sha256.update(&chunk.bytes);
        open.blake3.update(&chunk.bytes);
        open.bytes.extend_from_slice(&chunk.bytes);
        open.next_chunk_index = open
            .next_chunk_index
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("V2 full-block chunk index overflow"))?;
        Ok(())
    }

    fn complete_full_block(
        &mut self,
        completed: &PumpExactStateFullBlockPayloadCompletedV2,
    ) -> Result<()> {
        let open = self
            .open_full_block
            .take()
            .ok_or_else(|| anyhow::anyhow!("V2 full-block completion has no open payload"))?;
        if completed.source_capture_sequence != open.started.source.capture_sequence
            || completed.source_payload_bytes != open.started.source_payload_bytes
            || completed.source_payload_chunk_count != open.started.source_payload_chunk_count
            || completed.source_payload_chunk_count != open.next_chunk_index
            || u64::try_from(open.bytes.len()).unwrap_or(u64::MAX)
                != open.started.source_payload_bytes
        {
            bail!("V2 full-block completion does not match its started/chunk contract");
        }
        let sha256 = PumpResearchStorageHashV1::from(<[u8; 32]>::from(open.sha256.finalize()));
        let blake3 = PumpResearchStorageHashV1::from(*open.blake3.finalize().as_bytes());
        if sha256 != open.started.source_payload_sha256
            || sha256 != completed.source_payload_sha256
            || blake3 != open.started.source.payload_hash_blake3
            || blake3 != completed.source_payload_blake3
        {
            bail!("V2 full-block source payload digest mismatch");
        }
        let update = SubscribeUpdate::decode(open.bytes.as_slice())
            .context("decode reassembled V2 full-block SubscribeUpdate")?;
        let Some(UpdateOneof::Block(block)) = update.update_oneof else {
            bail!("V2 reassembled full-block payload is not a Block update");
        };
        if block.slot != open.started.slot
            || block.parent_slot != open.started.parent_slot
            || block.blockhash != open.started.blockhash
            || block.parent_blockhash != open.started.parent_blockhash
            || u64::try_from(block.transactions.len()).unwrap_or(u64::MAX)
                != open.started.executed_transaction_count
        {
            bail!("V2 decoded full block disagrees with its started identity");
        }
        let ingress = ingress_timestamp_v2(&open.started.event_time, "full-block payload")?;
        let slot_ledger = self.block_lanes_by_slot.entry(block.slot).or_default();
        if slot_ledger
            .full_block
            .replace(PumpExactStateFullBlockSlotEvidenceV2 {
                parent_slot: block.parent_slot,
                blockhash: block.blockhash.clone(),
                parent_blockhash: block.parent_blockhash.clone(),
                executed_transaction_count: u64::try_from(block.transactions.len())
                    .unwrap_or(u64::MAX),
                ingress,
                source_capture_sequence: open.started.source.capture_sequence,
            })
            .is_some()
        {
            bail!("V2 raw run retains more than one full-block payload for a slot");
        }
        for transaction in block.transactions {
            if transaction_invokes_pump(&transaction, self.expected_pump_program_id)? {
                let key = PumpExactStateTransactionKeyV2 {
                    slot: block.slot,
                    tx_index: transaction.index,
                    signature: fixed_signature(&transaction.signature)?,
                };
                let digest = *blake3::hash(&transaction.encode_to_vec()).as_bytes();
                if self
                    .full_block_pump_transactions
                    .insert(key.clone(), digest)
                    .is_some()
                {
                    bail!(
                        "V2 full-block Pump transaction {}:{} appears more than once",
                        key.slot,
                        key.tx_index
                    );
                }
            }
        }
        self.full_block_count = self
            .full_block_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("V2 full-block count overflow"))?;
        Ok(())
    }

    fn finish(
        &self,
        completion: &PumpExactStateRunCompletionReceiptV2,
        slots: &BTreeMap<u64, PumpExactStateSlotNodeV2>,
    ) -> Result<()> {
        let boundary = self
            .readiness_boundary
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("V2 raw run lacks its stream-readiness boundary"))?;
        if self.open_full_block.is_some() {
            bail!("V2 raw record stream has unfinished full block or invalid readiness boundary count");
        }
        if self.full_block_count != completion.writer.required_lane_census.full_blocks_completed
            || self.full_block_started_count
                != completion.writer.required_lane_census.full_blocks_started
            || self.full_block_chunk_count
                != completion.writer.required_lane_census.full_block_chunks
            || u64::try_from(self.source_capture_sequences.len()).unwrap_or(u64::MAX)
                != completion.writer.accepted_source_records
            || completion.writer.accepted_readiness_boundary_records != 1
            || u64::try_from(self.filtered_transactions.len()).unwrap_or(u64::MAX)
                != completion.writer.required_lane_census.transaction_messages
            || self.pump_owned_account_update_count
                != completion.writer.required_lane_census.account_updates
            || self.slot_update_count != completion.writer.required_lane_census.slot_updates
            || self.block_meta_count != completion.writer.required_lane_census.block_meta_updates
        {
            bail!("V2 raw record census differs from its completion receipt");
        }
        let source_count = u64::try_from(self.source_capture_sequences.len())
            .context("V2 source capture sequence count does not fit u64")?;
        let expected_last_source_sequence = if source_count
            == boundary.source_capture_sequence_exclusive
        {
            boundary
                .source_capture_sequence_exclusive
                .checked_sub(1)
                .ok_or_else(|| anyhow::anyhow!("V2 readiness boundary has empty source prefix"))?
        } else {
            // PRXTAPE3 reserves the boundary's own ordering position.  Once
            // a post-boundary source update exists, it starts immediately
            // after that marker, so the one omitted sequence value is both
            // deliberate and auditable rather than an unaccounted source
            // loss.
            source_count
        };
        if self.source_capture_sequences.first().copied() != Some(0)
            || self.source_capture_sequences.last().copied() != Some(expected_last_source_sequence)
            || boundary.source_capture_sequence_exclusive > source_count
            || self.source_stream_epoch != Some(boundary.source_stream_epoch)
        {
            bail!(
                "V2 stream-readiness boundary does not bind its complete source prefix and reserved ordering marker"
            );
        }
        reconcile_filtered_and_full_block_transactions(
            &self.filtered_transactions,
            &self.full_block_pump_transactions,
        )?;
        if completion.source_readiness.as_ref() != Some(&boundary.source_readiness)
            || completion.cohort_slots_strictly_after != Some(boundary.cohort_slots_strictly_after)
            || !completion.readiness_boundary_persisted
            || !completion.readiness_completed
        {
            bail!("V2 raw readiness boundary differs from completion receipt");
        }
        let _ = self.source_availability_bounds(completion, slots)?;
        Ok(())
    }
}

fn ingress_timestamp_v2(
    event_time: &PumpResearchEventTimeV1,
    label: &str,
) -> Result<PumpExactStateIngressTimestampV2> {
    let wall_ms = event_time
        .ingress_wall_ts_ms
        .ok_or_else(|| anyhow::anyhow!("V2 {label} lacks observed ingress-wall timestamp"))?;
    let monotonic_ms = event_time
        .ingress_monotonic_ts_ms
        .ok_or_else(|| anyhow::anyhow!("V2 {label} lacks observed ingress-monotonic timestamp"))?;
    Ok(PumpExactStateIngressTimestampV2 {
        wall_ms,
        monotonic_ms,
    })
}

impl PumpExactStateRawIndexBuilderV2 {
    fn new(expected_pump_program_id: [u8; 32]) -> Self {
        Self {
            expected_pump_program_id,
            collector: PumpExactStateRawRecordCollectorV2 {
                expected_pump_program_id,
                ..PumpExactStateRawRecordCollectorV2::default()
            },
            ..Self::default()
        }
    }

    fn observe(
        &mut self,
        segment_position: usize,
        frame_offset: u64,
        record: &PumpExactStateRawRecordV2,
    ) -> Result<()> {
        self.collector.observe(record)?;
        let pointer = PumpExactStateRawRecordPointerV2 {
            segment_position,
            frame_offset,
        };
        match record {
            PumpExactStateRawRecordV2::PrimaryTransaction(transaction) => {
                let tx_index = transaction.tx_index.ok_or_else(|| {
                    anyhow::anyhow!(
                        "V2 filtered Pump transaction lacks canonical transaction index"
                    )
                })?;
                self.transactions.push(PumpExactStateIndexedTransactionV2 {
                    pointer,
                    source_capture_sequence: transaction.source.capture_sequence,
                    slot: transaction.slot,
                    tx_index,
                    signature: transaction.signature.into_inner(),
                });
            }
            PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) => {
                if update.owner_program.into_inner() != self.expected_pump_program_id {
                    bail!("V2 Pump-owned account update has a non-Pump owner");
                }
                self.account_updates
                    .push(PumpExactStateIndexedAccountUpdateV2 {
                        pointer,
                        source_capture_sequence: update.source.capture_sequence,
                        slot: update.slot,
                        write_version: update.write_version,
                        account_pubkey: update.account_pubkey.into_inner(),
                        txn_signature: update.txn_signature.map(|value| value.into_inner()),
                    });
            }
            PumpExactStateRawRecordV2::PrimarySlotUpdate(update) => {
                if update.source_status == CommitmentLevel::Finalized as i32 {
                    self.slots
                        .entry(update.slot)
                        .or_default()
                        .finalized_parents
                        .insert(update.parent);
                }
            }
            // BlockMeta parentage belongs exclusively to the independently
            // reconciled BlockMeta/full-block ledger.  Do not let that lane
            // supply or repair the parent declared by a finalized Slot.
            PumpExactStateRawRecordV2::PrimaryBlockMeta(_) => {}
            _ => {}
        }
        Ok(())
    }
}

fn reconcile_filtered_and_full_block_transactions(
    filtered: &BTreeMap<PumpExactStateTransactionKeyV2, [u8; 32]>,
    full_block: &BTreeMap<PumpExactStateTransactionKeyV2, [u8; 32]>,
) -> Result<()> {
    if filtered.len() != full_block.len() || filtered != full_block {
        bail!("V2 filtered Pump transaction lane differs from full-block Pump inventory");
    }
    Ok(())
}

fn filtered_transaction_identity(
    transaction: &ghost_core::pump_research_exact_tape_v2::PumpExactStateTransactionEvidenceV2,
) -> Result<(PumpExactStateTransactionKeyV2, [u8; 32])> {
    let update = SubscribeUpdate::decode(transaction.source_payload.as_slice())
        .context("decode V2 filtered transaction SubscribeUpdate")?;
    let Some(UpdateOneof::Transaction(transaction_update)) = update.update_oneof else {
        bail!("V2 filtered transaction record does not retain a Transaction update");
    };
    let info = transaction_update
        .transaction
        .ok_or_else(|| anyhow::anyhow!("V2 filtered transaction update lacks transaction info"))?;
    if transaction_update.slot != transaction.slot
        || fixed_signature(&info.signature)? != transaction.signature.into_inner()
        || transaction.tx_index.map(u64::from) != Some(info.index)
    {
        bail!("V2 filtered transaction record identity differs from retained protobuf");
    }
    Ok((
        PumpExactStateTransactionKeyV2 {
            slot: transaction.slot,
            tx_index: info.index,
            signature: transaction.signature.into_inner(),
        },
        *blake3::hash(&info.encode_to_vec()).as_bytes(),
    ))
}

fn transaction_invokes_pump(
    transaction_info: &SubscribeUpdateTransactionInfo,
    expected_pump_program_id: [u8; 32],
) -> Result<bool> {
    let transaction = transaction_info
        .transaction
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 full-block transaction lacks transaction body"))?;
    let message = transaction
        .message
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 full-block transaction lacks message"))?;
    let mut account_keys = message.account_keys.clone();
    if let Some(meta) = &transaction_info.meta {
        account_keys.extend(meta.loaded_writable_addresses.iter().cloned());
        account_keys.extend(meta.loaded_readonly_addresses.iter().cloned());
    }
    let has_program = |index: u32| -> Result<bool> {
        let key = account_keys
            .get(usize::try_from(index).context("V2 program index does not fit usize")?)
            .ok_or_else(|| {
                anyhow::anyhow!("V2 transaction program index is out of account-key range")
            })?;
        Ok(key.as_slice() == expected_pump_program_id)
    };
    for instruction in &message.instructions {
        if has_program(instruction.program_id_index)? {
            return Ok(true);
        }
    }
    if let Some(meta) = &transaction_info.meta {
        for group in &meta.inner_instructions {
            for instruction in &group.instructions {
                if has_program(instruction.program_id_index)? {
                    return Ok(true);
                }
            }
        }
    }
    Ok(false)
}

fn decode_v2_transaction_context(
    transaction: &ghost_core::pump_research_exact_tape_v2::PumpExactStateTransactionEvidenceV2,
) -> Result<PumpExactStateTransactionContextV2> {
    let update = SubscribeUpdate::decode(transaction.source_payload.as_slice())
        .context("decode V2 filtered transaction SubscribeUpdate for semantics")?;
    let Some(UpdateOneof::Transaction(update)) = update.update_oneof else {
        bail!("V2 transaction record source payload is not a Transaction update");
    };
    if update.slot != transaction.slot {
        bail!("V2 transaction source payload slot differs from raw record");
    }
    let info = update
        .transaction
        .ok_or_else(|| anyhow::anyhow!("V2 transaction source payload lacks transaction info"))?;
    if fixed_signature(&info.signature)? != transaction.signature.into_inner()
        || transaction.tx_index.map(u64::from) != Some(info.index)
    {
        bail!("V2 transaction source payload identity differs from raw record");
    }
    let transaction_body = info
        .transaction
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 transaction lacks transaction body"))?;
    let message = transaction_body
        .message
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 transaction lacks message"))?;
    let header = message
        .header
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 transaction message lacks header"))?;
    let static_key_count = message.account_keys.len();
    let required_signatures = usize::try_from(header.num_required_signatures)
        .context("V2 transaction header signer count exceeds usize")?;
    let readonly_signed = usize::try_from(header.num_readonly_signed_accounts)
        .context("V2 transaction header readonly signer count exceeds usize")?;
    let readonly_unsigned = usize::try_from(header.num_readonly_unsigned_accounts)
        .context("V2 transaction header readonly unsigned count exceeds usize")?;
    if required_signatures < readonly_signed
        || required_signatures > static_key_count
        || readonly_unsigned > static_key_count.saturating_sub(required_signatures)
    {
        bail!("V2 transaction message header has impossible account partitions");
    }
    let mut accounts = Vec::with_capacity(static_key_count);
    for (index, key) in message.account_keys.iter().enumerate() {
        let pubkey =
            Pubkey::new_from_array(key.as_slice().try_into().map_err(|_| {
                anyhow::anyhow!("V2 transaction static account key is not 32 bytes")
            })?);
        let signer = index < required_signatures;
        let writable = if signer {
            index < required_signatures - readonly_signed
        } else {
            index < static_key_count - readonly_unsigned
        };
        accounts.push(PumpExactStateAccountMetaV2 {
            pubkey,
            signer,
            writable,
        });
    }
    let meta = info
        .meta
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("V2 transaction lacks transaction status metadata"))?;
    for key in &meta.loaded_writable_addresses {
        accounts.push(PumpExactStateAccountMetaV2 {
            pubkey: Pubkey::new_from_array(
                key.as_slice().try_into().map_err(|_| {
                    anyhow::anyhow!("V2 loaded writable account key is not 32 bytes")
                })?,
            ),
            signer: false,
            writable: true,
        });
    }
    for key in &meta.loaded_readonly_addresses {
        accounts.push(PumpExactStateAccountMetaV2 {
            pubkey: Pubkey::new_from_array(
                key.as_slice().try_into().map_err(|_| {
                    anyhow::anyhow!("V2 loaded readonly account key is not 32 bytes")
                })?,
            ),
            signer: false,
            writable: false,
        });
    }
    let mut inner = BTreeMap::new();
    for group in &meta.inner_instructions {
        if inner
            .insert(group.index, group.instructions.clone())
            .is_some()
        {
            bail!("V2 transaction has duplicate inner-instruction group index");
        }
    }
    Ok(PumpExactStateTransactionContextV2 {
        signature: transaction.signature.into_inner(),
        slot: transaction.slot,
        tx_index: u32::try_from(info.index).context("V2 transaction index exceeds u32")?,
        success: meta.err.is_none(),
        accounts,
        outer: message.instructions.clone(),
        inner,
    })
}

fn inventory_v2_from_transaction_context(
    context: &PumpExactStateTransactionContextV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    anchors: &PumpExactStateAnchorIndexV2,
) -> Result<PumpExactStateTransactionInventoryV2> {
    let mut occurrences = Vec::new();
    let mut keys = BTreeSet::new();
    for (outer_index, instruction) in context.outer.iter().enumerate() {
        let outer_index =
            u32::try_from(outer_index).context("V2 outer instruction index exceeds u32")?;
        if program_at_index(context, instruction.program_id_index)? == semantics.program_id {
            let occurrence = classify_pump_instruction_occurrence_v2(
                context,
                semantics,
                outer_index,
                Vec::new(),
                None,
                instruction.program_id_index,
                &instruction.accounts,
                &instruction.data,
                None,
                None,
            )?;
            insert_occurrence_v2(&mut occurrences, &mut keys, occurrence)?;
        }
        let Some(inner) = context.inner.get(&outer_index) else {
            continue;
        };
        for (inner_index, instruction) in inner.iter().enumerate() {
            if program_at_index(context, instruction.program_id_index)? != semantics.program_id {
                continue;
            }
            let inner_index =
                u16::try_from(inner_index).context("V2 inner instruction index exceeds u16")?;
            let immediate_parent = immediate_pump_parent_occurrence_key_v2(
                context,
                outer_index,
                inner,
                usize::from(inner_index),
                instruction.stack_height,
                semantics.program_id,
            )?;
            let occurrence = classify_pump_instruction_occurrence_v2(
                context,
                semantics,
                outer_index,
                vec![inner_index],
                instruction.stack_height,
                instruction.program_id_index,
                &instruction.accounts,
                &instruction.data,
                Some(outer_index),
                immediate_parent,
            )?;
            insert_occurrence_v2(&mut occurrences, &mut keys, occurrence)?;
        }
    }
    validate_event_transport_parent_links_v2(&mut occurrences);
    validate_event_transport_parent_semantics_v2(&mut occurrences, semantics, anchors, context);
    Ok(PumpExactStateTransactionInventoryV2 {
        slot: context.slot,
        tx_index: context.tx_index,
        signature: context.signature,
        success: context.success,
        occurrences,
    })
}

#[allow(clippy::too_many_arguments)]
fn classify_pump_instruction_occurrence_v2(
    context: &PumpExactStateTransactionContextV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    outer_instruction_index: u32,
    inner_instruction_path: Vec<u16>,
    stack_height: Option<u32>,
    program_id_index: u32,
    account_indices: &[u8],
    data: &[u8],
    outer_group: Option<u32>,
    immediate_parent: Option<PumpExactStateInstructionOccurrenceKeyV2>,
) -> Result<PumpExactStateInstructionOccurrenceV2> {
    let program_id = program_at_index(context, program_id_index)?;
    let discriminator = match data.get(..8).and_then(|value| value.try_into().ok()) {
        Some(discriminator) => discriminator,
        None => {
            return Ok(PumpExactStateInstructionOccurrenceV2 {
                key: PumpExactStateInstructionOccurrenceKeyV2 {
                    signature: context.signature,
                    outer_instruction_index,
                    inner_instruction_path,
                    stack_height,
                    program_id,
                    discriminator: [0; 8],
                },
                class: PumpExactStateOccurrenceClassV2::Unknown {
                    reason: "pump_instruction_missing_discriminator".to_owned(),
                },
            });
        }
    };
    let key = PumpExactStateInstructionOccurrenceKeyV2 {
        signature: context.signature,
        outer_instruction_index,
        inner_instruction_path,
        stack_height,
        program_id,
        discriminator,
    };
    if discriminator == ANCHOR_EVENT_CPI_WRAPPER_DISCRIMINATOR_V2 {
        return Ok(PumpExactStateInstructionOccurrenceV2 {
            key,
            class: classify_anchor_event_transport_v2(
                context,
                semantics,
                account_indices,
                data,
                outer_group,
                immediate_parent,
                stack_height,
            ),
        });
    }
    let Some(contract) = semantics.instruction(&discriminator) else {
        return Ok(PumpExactStateInstructionOccurrenceV2 {
            key,
            class: PumpExactStateOccurrenceClassV2::Unknown {
                reason: "unknown_pump_instruction_discriminator".to_owned(),
            },
        });
    };
    let accounts = validate_instruction_account_vector_v2(context, contract, account_indices);
    let payload = semantics.instruction_argument_fields(contract, &data[8..]);
    match contract.effect {
        PumpExactStateInstructionEffectV2::ProvenNonReserve => match (accounts, payload) {
            (Ok(account_roles), Ok(argument_fields)) => Ok(PumpExactStateInstructionOccurrenceV2 {
                key,
                class: PumpExactStateOccurrenceClassV2::ProvenNonReserve {
                    semantic_evidence: PumpExactStateInstructionSemanticEvidenceV2 {
                        discriminator: contract.discriminator,
                        account_roles,
                        argument_fields,
                    },
                },
            }),
            (account_error, payload_error) => Ok(PumpExactStateInstructionOccurrenceV2 {
                key,
                class: PumpExactStateOccurrenceClassV2::Unknown {
                    reason: {
                        let account_reason = account_error
                            .err()
                            .map_or_else(|| "none".to_owned(), |error| error.to_string());
                        let payload_reason = payload_error
                            .err()
                            .map_or_else(|| "none".to_owned(), |error| error.to_string());
                        format!(
                            "proven_non_reserve_contract_invalid:{account_reason}:{payload_reason}"
                        )
                    },
                },
            }),
        },
        effect => {
            let (bonding_curve, mint, account_vector_exact, account_reason, account_roles) =
                match accounts {
                    Ok(account_roles) => {
                        let (bonding_curve, mint) =
                            semantics.exact_state_account_pubkeys(contract, &account_roles)?;
                        (bonding_curve, mint, true, None, Some(account_roles))
                    }
                    Err(error) => (None, None, false, Some(error.to_string()), None),
                };
            let (instruction_payload_exact, payload_reason, argument_fields) = match payload {
                Ok(argument_fields) => (true, None, Some(argument_fields)),
                Err(error) => (false, Some(error.to_string()), None),
            };
            let failure_reason = account_reason.or(payload_reason);
            let semantic_evidence = match (account_roles, argument_fields) {
                (Some(account_roles), Some(argument_fields)) => {
                    Some(PumpExactStateInstructionSemanticEvidenceV2 {
                        discriminator: contract.discriminator,
                        account_roles,
                        argument_fields,
                    })
                }
                _ => None,
            };
            Ok(PumpExactStateInstructionOccurrenceV2 {
                key,
                class: PumpExactStateOccurrenceClassV2::Candidate {
                    effect,
                    instruction_payload_exact,
                    account_vector_exact,
                    bonding_curve,
                    mint,
                    failure_reason,
                    semantic_evidence,
                },
            })
        }
    }
}

fn validate_instruction_account_vector_v2(
    context: &PumpExactStateTransactionContextV2,
    contract: &crate::research_exact_tape_v2_semantics::PumpExactStateInstructionContractV2,
    account_indices: &[u8],
) -> Result<BTreeMap<String, Pubkey>> {
    if account_indices.len() != contract.accounts.len() {
        bail!(
            "V2 Pump instruction {} account count {} differs from pinned {}",
            contract.name,
            account_indices.len(),
            contract.accounts.len()
        );
    }
    let mut account_roles = BTreeMap::new();
    for (position, expected) in contract.accounts.iter().enumerate() {
        let account_index = usize::from(account_indices[position]);
        let actual = context.accounts.get(account_index).ok_or_else(|| {
            anyhow::anyhow!(
                "V2 Pump instruction {} account position {} references missing message account",
                contract.name,
                position
            )
        })?;
        if actual.signer != expected.signer || actual.writable != expected.writable {
            bail!(
                "V2 Pump instruction {} account {} signer/writable contract differs",
                contract.name,
                expected.name
            );
        }
        if expected
            .address
            .is_some_and(|address| address != actual.pubkey)
        {
            bail!(
                "V2 Pump instruction {} static account {} differs",
                contract.name,
                expected.name
            );
        }
        if account_roles
            .insert(expected.name.clone(), actual.pubkey)
            .is_some()
        {
            bail!(
                "V2 Pump instruction {} repeats pinned account role {}",
                contract.name,
                expected.name
            );
        }
    }
    Ok(account_roles)
}

fn classify_anchor_event_transport_v2(
    context: &PumpExactStateTransactionContextV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
    account_indices: &[u8],
    data: &[u8],
    outer_group: Option<u32>,
    immediate_parent: Option<PumpExactStateInstructionOccurrenceKeyV2>,
    stack_height: Option<u32>,
) -> PumpExactStateOccurrenceClassV2 {
    let Some(outer_group) = outer_group else {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "direct_anchor_event_transport_unsupported".to_owned(),
        };
    };
    let Some(immediate_parent) = immediate_parent else {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_parent_not_proven".to_owned(),
        };
    };
    if stack_height.is_none_or(|height| height < 2) {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_parent_not_proven".to_owned(),
        };
    }
    let Some(nested_discriminator) = data.get(8..16).and_then(|value| value.try_into().ok()) else {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_missing_nested_discriminator".to_owned(),
        };
    };
    let Some(event) = semantics.event(&nested_discriminator) else {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_unknown_nested_event".to_owned(),
        };
    };
    let event_fields = match semantics.event_semantic_fields(event, &data[16..]) {
        Ok(fields) => fields,
        Err(_) => {
            return PumpExactStateOccurrenceClassV2::Unknown {
                reason: "anchor_event_transport_payload_not_exact".to_owned(),
            }
        }
    };
    // An Anchor `emit_cpi!` self-CPI is encoded as a Pump instruction whose
    // program id is the Pump program and whose only account meta is the
    // readonly, non-signer `__event_authority` PDA.  The program itself is
    // the compiled instruction's program_id, not a second account meta.  Do
    // not accept a superset here: an arbitrary remaining account must not be
    // silently reclassified as event transport.
    if account_indices.len() != 1 || outer_group >= u32::MAX {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_account_vector_not_exact".to_owned(),
        };
    }
    let event_authority =
        Pubkey::find_program_address(&[b"__event_authority"], &semantics.program_id).0;
    let Some(account) = context.accounts.get(usize::from(account_indices[0])) else {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_account_index_invalid".to_owned(),
        };
    };
    if account.pubkey != event_authority || account.signer || account.writable {
        return PumpExactStateOccurrenceClassV2::Unknown {
            reason: "anchor_event_transport_authority_contract_invalid".to_owned(),
        };
    }
    PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
        immediate_parent,
        event_discriminator: nested_discriminator,
        event_fields,
        final_state_bindings: Vec::new(),
    }
}

fn immediate_pump_parent_occurrence_key_v2(
    context: &PumpExactStateTransactionContextV2,
    outer_index: u32,
    inner: &[yellowstone_grpc_proto::prelude::InnerInstruction],
    instruction_index: usize,
    stack_height: Option<u32>,
    pump_program_id: Pubkey,
) -> Result<Option<PumpExactStateInstructionOccurrenceKeyV2>> {
    let Some(height) = stack_height else {
        return Ok(None);
    };
    if height < 2 {
        return Ok(None);
    }
    for (previous_index, previous) in inner[..instruction_index].iter().enumerate().rev() {
        let Some(previous_height) = previous.stack_height else {
            continue;
        };
        if previous_height >= height {
            continue;
        }
        if previous_height + 1 != height
            || program_at_index(context, previous.program_id_index)? != pump_program_id
        {
            return Ok(None);
        }
        return Ok(Some(PumpExactStateInstructionOccurrenceKeyV2 {
            signature: context.signature,
            outer_instruction_index: outer_index,
            inner_instruction_path: vec![u16::try_from(previous_index)
                .context("V2 immediate Pump parent index exceeds u16")?],
            stack_height: Some(previous_height),
            program_id: pump_program_id,
            discriminator: instruction_discriminator_or_zero_v2(&previous.data),
        }));
    }
    let outer = context
        .outer
        .get(usize::try_from(outer_index).context("V2 outer index exceeds usize")?)
        .ok_or_else(|| {
            anyhow::anyhow!("V2 inner instruction group references missing outer instruction")
        })?;
    if height != 2 || program_at_index(context, outer.program_id_index)? != pump_program_id {
        return Ok(None);
    }
    Ok(Some(PumpExactStateInstructionOccurrenceKeyV2 {
        signature: context.signature,
        outer_instruction_index: outer_index,
        inner_instruction_path: Vec::new(),
        stack_height: None,
        program_id: pump_program_id,
        discriminator: instruction_discriminator_or_zero_v2(&outer.data),
    }))
}

fn instruction_discriminator_or_zero_v2(data: &[u8]) -> [u8; 8] {
    data.get(..8)
        .and_then(|value| value.try_into().ok())
        .unwrap_or([0; 8])
}

/// Validate the parent link after the full transaction ledger has been built.
/// The immediate-parent derivation is deliberately local to the inner
/// instruction stack, while this second pass proves that it points to one
/// actual non-event Pump occurrence from the same immutable transaction.
fn validate_event_transport_parent_links_v2(
    occurrences: &mut [PumpExactStateInstructionOccurrenceV2],
) {
    let classes_by_key = occurrences
        .iter()
        .map(|occurrence| {
            (
                occurrence.key.clone(),
                matches!(
                    occurrence.class,
                    PumpExactStateOccurrenceClassV2::ValidatedEventTransport { .. }
                ),
            )
        })
        .collect::<BTreeMap<_, _>>();
    for occurrence in occurrences {
        let PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
            immediate_parent, ..
        } = &occurrence.class
        else {
            continue;
        };
        let reason = match classes_by_key.get(immediate_parent) {
            None => Some("anchor_event_transport_parent_occurrence_missing"),
            Some(true) => Some("anchor_event_transport_parent_is_event_transport"),
            Some(false) => None,
        };
        if let Some(reason) = reason {
            occurrence.class = PumpExactStateOccurrenceClassV2::Unknown {
                reason: reason.to_owned(),
            };
        }
    }
}

/// Validate the semantic half of an Anchor Event-CPI only after the full
/// occurrence ledger exists.  The structural pass above proves the immediate
/// stack parent; this pass proves that the event variant, every declared field
/// relation, and every declared final-state value match that very parent.
/// A bad event becomes an `Unknown` occurrence, therefore taints the complete
/// successful rooted transaction and cannot be hidden by a high coverage
/// numerator.
fn validate_event_transport_parent_semantics_v2(
    occurrences: &mut [PumpExactStateInstructionOccurrenceV2],
    semantics: &PumpExactStateSemanticsAuthorityV2,
    anchors: &PumpExactStateAnchorIndexV2,
    context: &PumpExactStateTransactionContextV2,
) {
    let parent_evidence_by_key = occurrences
        .iter()
        .filter_map(|occurrence| {
            let evidence = match &occurrence.class {
                PumpExactStateOccurrenceClassV2::ProvenNonReserve { semantic_evidence } => {
                    Some(semantic_evidence)
                }
                PumpExactStateOccurrenceClassV2::Candidate {
                    semantic_evidence: Some(semantic_evidence),
                    ..
                } => Some(semantic_evidence),
                _ => None,
            }?;
            Some((occurrence.key.clone(), evidence.clone()))
        })
        .collect::<BTreeMap<_, _>>();

    for occurrence in occurrences {
        let (immediate_parent, event_discriminator, event_fields) = match &occurrence.class {
            PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                immediate_parent,
                event_discriminator,
                event_fields,
                ..
            } => (
                immediate_parent.clone(),
                *event_discriminator,
                event_fields.clone(),
            ),
            _ => continue,
        };
        let result = (|| -> Result<Vec<PumpExactStateEventFinalStateBindingV2>> {
            let parent = parent_evidence_by_key
                .get(&immediate_parent)
                .ok_or_else(|| {
                    anyhow::anyhow!("anchor_event_transport_parent_contract_not_exact")
                })?;
            if parent.discriminator != immediate_parent.discriminator {
                bail!("anchor_event_transport_parent_discriminator_mismatch");
            }
            let event = semantics
                .event(&event_discriminator)
                .ok_or_else(|| anyhow::anyhow!("anchor_event_transport_unknown_nested_event"))?;
            let bindings = semantics
                .validate_event_parent_semantics(event, &event_fields, parent)
                .map_err(|_| {
                    anyhow::anyhow!("anchor_event_transport_parent_semantics_not_exact")
                })?;
            if bindings.is_empty() {
                return Ok(bindings);
            }
            let parent_contract = semantics
                .instruction(&parent.discriminator)
                .ok_or_else(|| anyhow::anyhow!("anchor_event_transport_parent_contract_unknown"))?;
            let (curve, _) = semantics
                .exact_state_account_pubkeys(parent_contract, &parent.account_roles)
                .map_err(|_| anyhow::anyhow!("anchor_event_transport_parent_curve_role_absent"))?;
            let curve = curve.ok_or_else(|| {
                anyhow::anyhow!("anchor_event_transport_parent_curve_role_absent")
            })?;
            let final_anchor = anchors
                .unique_final_anchor(context.signature, curve, context.slot, context.tx_index)
                .ok_or_else(|| anyhow::anyhow!("anchor_event_transport_final_anchor_missing"))?;
            validate_event_final_state_bindings_v2(&final_anchor.state, &bindings)?;
            Ok(bindings)
        })();
        match result {
            Ok(final_state_bindings) => {
                if let PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                    final_state_bindings: stored,
                    ..
                } = &mut occurrence.class
                {
                    *stored = final_state_bindings;
                }
            }
            Err(error) => {
                occurrence.class = PumpExactStateOccurrenceClassV2::Unknown {
                    reason: error.to_string(),
                };
            }
        }
    }
}

fn validate_event_final_state_bindings_v2(
    state: &PumpExactStateCurveStateV2,
    bindings: &[PumpExactStateEventFinalStateBindingV2],
) -> Result<()> {
    for binding in bindings {
        if curve_state_field_borsh_bytes_v2(state, &binding.curve_state_field)?
            != binding.event_value_borsh
        {
            bail!("anchor_event_transport_final_state_mismatch");
        }
    }
    Ok(())
}

fn curve_state_field_borsh_bytes_v2(
    state: &PumpExactStateCurveStateV2,
    field: &str,
) -> Result<Vec<u8>> {
    let bytes = match field {
        "virtual_token_reserves" => state.virtual_token_reserves.to_le_bytes().to_vec(),
        "virtual_quote_reserves" => state.virtual_quote_reserves.to_le_bytes().to_vec(),
        "real_token_reserves" => state.real_token_reserves.to_le_bytes().to_vec(),
        "real_quote_reserves" => state.real_quote_reserves.to_le_bytes().to_vec(),
        "token_total_supply" => state.token_total_supply.to_le_bytes().to_vec(),
        "complete" => vec![u8::from(state.complete)],
        "creator" => state.creator.to_bytes().to_vec(),
        "is_mayhem_mode" => vec![u8::from(state.is_mayhem_mode)],
        "is_cashback_coin" => vec![u8::from(state.is_cashback_coin)],
        "quote_mint" => state.quote_mint.to_bytes().to_vec(),
        _ => bail!("unknown V2 final curve-state field {field:?}"),
    };
    Ok(bytes)
}

fn insert_occurrence_v2(
    occurrences: &mut Vec<PumpExactStateInstructionOccurrenceV2>,
    keys: &mut BTreeSet<PumpExactStateInstructionOccurrenceKeyV2>,
    occurrence: PumpExactStateInstructionOccurrenceV2,
) -> Result<()> {
    if !keys.insert(occurrence.key.clone()) {
        bail!("V2 Pump instruction occurrence locator appears more than once");
    }
    occurrences.push(occurrence);
    Ok(())
}

fn program_at_index(context: &PumpExactStateTransactionContextV2, index: u32) -> Result<Pubkey> {
    context
        .accounts
        .get(usize::try_from(index).context("V2 program account index exceeds usize")?)
        .map(|account| account.pubkey)
        .ok_or_else(|| anyhow::anyhow!("V2 program account index is out of account-key range"))
}

fn scan_v2_segment<F>(
    path: &Path,
    receipt: &PumpExactStateSegmentReceiptV2,
    expected_run_id: &str,
    expected_previous_prefix_hash: Option<PumpResearchStorageHashV1>,
    expected_clean_shutdown: bool,
    require_private_mode: bool,
    mut on_record: F,
) -> Result<PumpResearchStorageHashV1>
where
    F: FnMut(u64, &PumpExactStateRawRecordV2) -> Result<()>,
{
    let file = open_regular_nofollow(path, "V2 raw segment")?;
    if require_private_mode {
        require_private_open_authority_file_v2(&file, "V2 raw segment")?;
    }
    let expected_file_bytes = file.metadata()?.len();
    if expected_file_bytes != receipt.file_bytes {
        bail!("V2 raw segment receipt byte count mismatch");
    }
    let mut reader = BufReader::new(file.take(expected_file_bytes.saturating_add(1)));
    let mut offset = 0u64;
    let mut magic = [0u8; PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2.len()];
    reader.read_exact(&mut magic)?;
    offset = offset
        .checked_add(u64::try_from(magic.len())?)
        .context("V2 segment offset overflow")?;
    if magic != PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2 {
        bail!("V2 raw segment magic mismatch");
    }
    let header_frame = read_v2_frame(&mut reader, offset)?
        .ok_or_else(|| anyhow::anyhow!("V2 raw segment lacks a header frame"))?;
    offset = offset
        .checked_add(u64::try_from(header_frame.len())?)
        .context("V2 segment offset overflow")?;
    let mut header_bytes = magic.to_vec();
    header_bytes.extend_from_slice(&header_frame);
    let header = PumpExactStateRawCodecV2::decode_segment_header(&header_bytes)
        .map_err(anyhow::Error::msg)?;
    if header.run_id != expected_run_id
        || header.segment_index != receipt.segment_index
        || header.previous_segment_blake3 != expected_previous_prefix_hash
    {
        bail!("V2 raw segment header differs from control/chain authority");
    }
    let mut prefix_hasher = blake3::Hasher::new();
    prefix_hasher.update(&header_bytes);
    let mut file_blake3 = blake3::Hasher::new();
    file_blake3.update(&header_bytes);
    let mut file_sha256 = Sha256::new();
    file_sha256.update(&header_bytes);
    let mut accepted_record_count = 0u64;
    let mut data_bytes =
        u64::try_from(header_bytes.len()).context("V2 header bytes do not fit u64")?;
    let mut footer = None;
    loop {
        let frame_offset = offset;
        let Some(frame) = read_v2_frame(&mut reader, offset)? else {
            break;
        };
        offset = offset
            .checked_add(u64::try_from(frame.len())?)
            .context("V2 segment offset overflow")?;
        file_blake3.update(&frame);
        file_sha256.update(&frame);
        let record = PumpExactStateRawCodecV2::decode_record(&frame).map_err(anyhow::Error::msg)?;
        if record_stream_epoch(&record).is_some_and(|epoch| epoch != header.stream_epoch) {
            bail!("V2 raw segment record stream epoch differs from header");
        }
        match &record {
            PumpExactStateRawRecordV2::SegmentClosed(value) => {
                if footer.replace(value.clone()).is_some() {
                    bail!("V2 raw segment has more than one footer");
                }
                break;
            }
            _ => {
                prefix_hasher.update(&frame);
                accepted_record_count = accepted_record_count
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 raw accepted record count overflow"))?;
                data_bytes = data_bytes
                    .checked_add(u64::try_from(frame.len())?)
                    .ok_or_else(|| anyhow::anyhow!("V2 raw data byte count overflow"))?;
                on_record(frame_offset, &record)?;
            }
        }
    }
    let footer = footer.ok_or_else(|| anyhow::anyhow!("V2 raw segment lacks terminal footer"))?;
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 || offset != expected_file_bytes {
        bail!("V2 raw segment has trailing or size-drift bytes");
    }
    let metadata = reader.get_ref().get_ref().metadata()?;
    if !metadata.is_file() || metadata.len() != expected_file_bytes {
        bail!("V2 raw segment changed while being scanned");
    }
    let prefix_hash = PumpResearchStorageHashV1::from(*prefix_hasher.finalize().as_bytes());
    if footer.storage_format_version != PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2
        || footer.segment_index != receipt.segment_index
        || footer.clean_shutdown != expected_clean_shutdown
        || footer.accepted_record_count != accepted_record_count
        || footer.data_bytes != data_bytes
        || footer.segment_blake3 != prefix_hash
        || receipt.accepted_record_count != accepted_record_count
    {
        bail!("V2 raw segment footer/receipt does not match frozen contents");
    }
    let file_sha256 = PumpResearchStorageHashV1::from(<[u8; 32]>::from(file_sha256.finalize()));
    let file_blake3 = PumpResearchStorageHashV1::from(*file_blake3.finalize().as_bytes());
    if receipt.file_sha256 != file_sha256 || receipt.file_blake3 != file_blake3 {
        bail!("V2 raw segment whole-file digest mismatch");
    }
    Ok(prefix_hash)
}

fn read_v2_frame<R: Read>(reader: &mut R, _offset: u64) -> Result<Option<Vec<u8>>> {
    let mut length = [0u8; 4];
    if reader.read(&mut length[..1])? == 0 {
        return Ok(None);
    }
    reader.read_exact(&mut length[1..])?;
    let payload_length = usize::try_from(u32::from_le_bytes(length))
        .context("V2 frame length does not fit usize")?;
    if payload_length > PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2 {
        bail!("V2 frame payload exceeds frozen record limit");
    }
    let total = 4usize
        .checked_add(payload_length)
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| anyhow::anyhow!("V2 frame length overflow"))?;
    let mut frame = vec![0u8; total];
    frame[..4].copy_from_slice(&length);
    reader.read_exact(&mut frame[4..])?;
    Ok(Some(frame))
}

fn record_stream_epoch(record: &PumpExactStateRawRecordV2) -> Option<u64> {
    match record {
        PumpExactStateRawRecordV2::PrimaryTransaction(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PrimarySlotUpdate(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PrimaryBlockMeta(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::FullBlockPayloadStarted(value) => {
            Some(value.source.stream_epoch)
        }
        PumpExactStateRawRecordV2::CoverageGap(value) => Some(value.stream_epoch),
        _ => None,
    }
}

fn validate_source_payload(
    capture_sequence: u64,
    expected: &PumpResearchStorageHashV1,
    payload: &[u8],
) -> Result<()> {
    let actual = PumpResearchStorageHashV1::from(*blake3::hash(payload).as_bytes());
    if &actual != expected {
        bail!("V2 source payload hash mismatch at capture sequence {capture_sequence}");
    }
    Ok(())
}

/// The raw projection fields are convenient index inputs, never an
/// independent authority.  Re-decode the retained complete protobuf before
/// accepting them so an internally self-hashed raw record cannot redirect an
/// exact anchor, canonicality edge, or BlockMeta identity away from the bytes
/// actually received from Yellowstone.
fn decode_retained_source_update_v2(payload: &[u8], label: &str) -> Result<SubscribeUpdate> {
    SubscribeUpdate::decode(payload)
        .with_context(|| format!("decode retained V2 {label} SubscribeUpdate"))
}

fn validate_pump_owned_account_update_projection_v2(
    update: &PumpExactStatePumpOwnedAccountUpdateV2,
    expected_pump_program_id: [u8; 32],
) -> Result<()> {
    let source = decode_retained_source_update_v2(&update.source_payload, "Pump-owned account")?;
    let Some(UpdateOneof::Account(source_update)) = source.update_oneof else {
        bail!(
            "V2 Pump-owned account update source payload is not a SubscribeUpdate::Account at capture sequence {}",
            update.source.capture_sequence
        );
    };
    let account = source_update.account.ok_or_else(|| {
        anyhow::anyhow!(
            "V2 retained Pump-owned account protobuf lacks account payload at capture sequence {}",
            update.source.capture_sequence
        )
    })?;
    let source_pubkey: [u8; 32] = account.pubkey.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!(
            "V2 retained Pump-owned account protobuf has non-32-byte pubkey at capture sequence {}",
            update.source.capture_sequence
        )
    })?;
    let source_owner: [u8; 32] = account.owner.as_slice().try_into().map_err(|_| {
        anyhow::anyhow!(
            "V2 retained Pump-owned account protobuf has non-32-byte owner at capture sequence {}",
            update.source.capture_sequence
        )
    })?;
    if source_owner != expected_pump_program_id {
        bail!(
            "V2 retained Pump-owned account protobuf has a non-Pump owner at capture sequence {}",
            update.source.capture_sequence
        );
    }
    let source_txn_signature = account
        .txn_signature
        .as_deref()
        .map(fixed_signature)
        .transpose()
        .context("V2 retained Pump-owned account protobuf has non-64-byte txn_signature")?;
    let source_evidence_class =
        classify_pump_owned_account_evidence_v2(source_pubkey, &account.data)?;
    if source_update.slot != update.slot
        || source_update.is_startup != update.is_startup
        || source_pubkey != update.account_pubkey.into_inner()
        || source_owner != update.owner_program.into_inner()
        || account.data != update.raw_account_data
        || account.write_version != update.write_version
        || source_txn_signature != update.txn_signature.map(|signature| signature.into_inner())
        || source_evidence_class != update.evidence_class
    {
        bail!(
            "V2 Pump-owned account update projection differs from retained protobuf at capture sequence {}",
            update.source.capture_sequence
        );
    }
    Ok(())
}

fn validate_slot_update_projection_v2(update: &PumpExactStateSlotEvidenceV2) -> Result<()> {
    let source = decode_retained_source_update_v2(&update.source_payload, "Slot")?;
    let Some(UpdateOneof::Slot(source_update)) = source.update_oneof else {
        bail!(
            "V2 Slot update source payload is not a SubscribeUpdate::Slot at capture sequence {}",
            update.source.capture_sequence
        );
    };
    if source_update.slot != update.slot
        || source_update.parent != update.parent
        || source_update.status != update.source_status
    {
        bail!(
            "V2 Slot update projection differs from retained protobuf at capture sequence {}",
            update.source.capture_sequence
        );
    }
    Ok(())
}

fn validate_block_meta_projection_v2(update: &PumpExactStateBlockMetaEvidenceV2) -> Result<()> {
    let source = decode_retained_source_update_v2(&update.source_payload, "BlockMeta")?;
    let Some(UpdateOneof::BlockMeta(source_update)) = source.update_oneof else {
        bail!(
            "V2 BlockMeta source payload is not a SubscribeUpdate::BlockMeta at capture sequence {}",
            update.source.capture_sequence
        );
    };
    let source_block_time = source_update.block_time.as_ref().map(|time| time.timestamp);
    if source_update.slot != update.slot
        || source_update.parent_slot != update.parent_slot
        || source_update.blockhash != update.blockhash
        || source_update.parent_blockhash != update.parent_blockhash
        || source_update.executed_transaction_count != update.executed_transaction_count
        || source_block_time != update.block_time
    {
        bail!(
            "V2 BlockMeta projection differs from retained protobuf at capture sequence {}",
            update.source.capture_sequence
        );
    }
    Ok(())
}

fn fixed_signature(bytes: &[u8]) -> Result<[u8; 64]> {
    bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("V2 transaction signature is not 64 bytes"))
}

fn safe_v2_segment_filename(filename: &str) -> Result<&str> {
    if filename.is_empty()
        || filename.contains('/')
        || filename.contains('\\')
        || filename == "."
        || filename == ".."
        || !filename.starts_with("segment_")
        || !filename.ends_with(".bin")
    {
        bail!("V2 raw receipt has unsafe segment filename");
    }
    Ok(filename)
}

fn open_regular_nofollow(path: &Path, label: &str) -> Result<File> {
    #[cfg(unix)]
    {
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
        let after = file.metadata()?;
        if !after.is_file() {
            bail!("opened {label} {} is not regular", path.display());
        }
        Ok(file)
    }
    #[cfg(not(unix))]
    {
        let _ = (path, label);
        bail!("V2 raw inspection requires Unix no-follow/nonblocking file handling")
    }
}

#[cfg(target_os = "linux")]
fn create_anonymous_v2_raw_snapshot(snapshot_parent: &Path) -> Result<File> {
    let metadata = fs::symlink_metadata(snapshot_parent).with_context(|| {
        format!(
            "inspect V2 anonymous raw snapshot parent {}",
            snapshot_parent.display()
        )
    })?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!("V2 anonymous raw snapshot parent must be a real directory");
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_TMPFILE | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(snapshot_parent).with_context(|| {
        format!(
            "create anonymous V2 raw snapshot under {}",
            snapshot_parent.display()
        )
    })?;
    let metadata = file.metadata()?;
    if !metadata.is_file() || metadata.nlink() != 0 {
        bail!("V2 raw snapshot is not an anonymous regular inode");
    }
    Ok(file)
}

#[cfg(not(target_os = "linux"))]
fn create_anonymous_v2_raw_snapshot(_snapshot_parent: &Path) -> Result<File> {
    bail!("V2 exact-state qualification requires Linux O_TMPFILE snapshot authority")
}

#[cfg(target_os = "linux")]
fn reopen_anonymous_v2_raw_snapshot_read_only(snapshot: &File) -> Result<File> {
    let before = snapshot.metadata()?;
    if !before.is_file() || before.nlink() != 0 {
        bail!("V2 writable raw snapshot is not an anonymous regular inode");
    }
    let proc_path = PathBuf::from(format!("/proc/self/fd/{}", snapshot.as_raw_fd()));
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    let read_only = options
        .open(&proc_path)
        .context("reopen V2 anonymous raw snapshot read-only")?;
    let after = read_only.metadata()?;
    if !after.is_file()
        || after.nlink() != 0
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.len() != before.len()
    {
        bail!("V2 read-only raw snapshot descriptor does not bind copied anonymous inode");
    }
    Ok(read_only)
}

#[cfg(not(target_os = "linux"))]
fn reopen_anonymous_v2_raw_snapshot_read_only(_snapshot: &File) -> Result<File> {
    bail!("V2 exact-state qualification requires Linux anonymous snapshot reopen")
}

fn copy_segment_to_anonymous_snapshot_v2(
    source_path: &Path,
    snapshot_parent: &Path,
    receipt: &PumpExactStateSegmentReceiptV2,
) -> Result<Arc<File>> {
    let source = open_regular_nofollow(source_path, "V2 raw segment snapshot source")?;
    let before = source.metadata()?;
    if !before.is_file() || before.len() != receipt.file_bytes {
        bail!(
            "V2 source segment {} does not match expected snapshot length",
            source_path.display()
        );
    }
    let mut snapshot = create_anonymous_v2_raw_snapshot(snapshot_parent)?;
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    while offset < receipt.file_bytes {
        let remaining = receipt.file_bytes.saturating_sub(offset);
        let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))
            .context("V2 snapshot chunk length does not fit usize")?;
        #[cfg(unix)]
        source
            .read_exact_at(&mut buffer[..chunk_len], offset)
            .with_context(|| {
                format!(
                    "read V2 source segment {} at offset {offset}",
                    source_path.display()
                )
            })?;
        snapshot
            .write_all(&buffer[..chunk_len])
            .context("write V2 anonymous raw snapshot")?;
        sha256.update(&buffer[..chunk_len]);
        blake3.update(&buffer[..chunk_len]);
        offset = offset
            .checked_add(u64::try_from(chunk_len).unwrap_or(u64::MAX))
            .ok_or_else(|| anyhow::anyhow!("V2 snapshot offset overflow"))?;
    }
    let after = source.metadata()?;
    if !after.is_file()
        || after.len() != receipt.file_bytes
        || after.dev() != before.dev()
        || after.ino() != before.ino()
    {
        bail!("V2 source segment changed during anonymous snapshot copy");
    }
    let copied_sha256 = PumpResearchStorageHashV1::from(<[u8; 32]>::from(sha256.finalize()));
    let copied_blake3 = PumpResearchStorageHashV1::from(*blake3.finalize().as_bytes());
    if copied_sha256 != receipt.file_sha256 || copied_blake3 != receipt.file_blake3 {
        bail!("V2 source segment hash differs while creating anonymous snapshot");
    }
    snapshot
        .sync_all()
        .context("sync V2 anonymous raw snapshot")?;
    #[cfg(unix)]
    snapshot.set_permissions(fs::Permissions::from_mode(0o400))?;
    let snapshot_metadata = snapshot.metadata()?;
    if !snapshot_metadata.is_file()
        || snapshot_metadata.len() != receipt.file_bytes
        || snapshot_metadata.nlink() != 0
    {
        bail!("V2 anonymous raw snapshot metadata is invalid");
    }
    let pinned = reopen_anonymous_v2_raw_snapshot_read_only(&snapshot)?;
    drop(snapshot);
    Ok(Arc::new(pinned))
}

fn read_v2_frame_from_open_file(file: &File, offset: u64) -> Result<Vec<u8>> {
    let mut length = [0u8; 4];
    #[cfg(unix)]
    file.read_exact_at(&mut length, offset)
        .with_context(|| format!("read V2 frozen frame length at {offset}"))?;
    let payload_length = usize::try_from(u32::from_le_bytes(length))
        .context("V2 frozen frame payload length does not fit usize")?;
    if payload_length > PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2 {
        bail!("V2 frozen frame payload exceeds record limit");
    }
    let total = 4usize
        .checked_add(payload_length)
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| anyhow::anyhow!("V2 frozen frame length overflow"))?;
    let mut frame = vec![0u8; total];
    frame[..4].copy_from_slice(&length);
    #[cfg(unix)]
    file.read_exact_at(&mut frame[4..], offset.saturating_add(4))
        .with_context(|| format!("read V2 frozen frame at {offset}"))?;
    Ok(frame)
}

fn read_v2_json<T>(path: &Path, label: &str) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    let file = open_regular_nofollow(path, label)?;
    require_private_open_authority_file_v2(&file, label)?;
    let before = file.metadata()?;
    if before.len() > V2_RAW_CONTROL_MAX_BYTES {
        bail!("{label} exceeds V2 bounded control-file limit");
    }
    let length = usize::try_from(before.len()).context("V2 control length does not fit usize")?;
    let mut bytes = vec![0u8; length];
    #[cfg(unix)]
    file.read_exact_at(&mut bytes, 0)
        .with_context(|| format!("read V2 control {label}"))?;
    let after = file.metadata()?;
    #[cfg(unix)]
    if after.len() != before.len() || after.dev() != before.dev() || after.ino() != before.ino() {
        bail!("V2 control {label} changed while being read");
    }
    serde_json::from_slice(&bytes).with_context(|| format!("parse V2 control {label}"))
}

fn require_private_open_authority_file_v2(file: &File, label: &str) -> Result<()> {
    #[cfg(unix)]
    {
        let mode = file.metadata()?.permissions().mode() & 0o777;
        if mode & 0o077 != 0 || mode & 0o600 != 0o600 {
            bail!(
                "opened {label} must be owner-private and readable (mode {:o})",
                mode
            );
        }
    }
    #[cfg(not(unix))]
    {
        let _ = (file, label);
        bail!("V2 opened authority files require Unix permission checks");
    }
    Ok(())
}

fn hex_bytes(bytes: &[u8]) -> String {
    let mut output = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(output, "{byte:02x}");
    }
    output
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::{
        pump_research_exact_tape_v2::{
            PumpExactStateProspectiveStreamBoundaryV2, PumpExactStateProviderRoleV2,
            PumpExactStateSegmentClosedV2, PumpExactStateSegmentHeaderV2,
            PumpExactStateSourceEnvelopeV2, PumpExactStateSourceReadinessV2,
            PumpExactStateTransactionEvidenceV2,
        },
        pump_research_tape::{PumpResearchEventTimeV1, PumpResearchStorageSignatureV1},
    };
    use serde_json::Value;
    use yellowstone_grpc_proto::prelude::{
        CompiledInstruction, InnerInstruction, InnerInstructions, Message as ProtoMessage,
        SubscribeUpdateBlock, SubscribeUpdateTransaction, Transaction, TransactionStatusMeta,
    };

    fn storage_hash(bytes: &[u8]) -> PumpResearchStorageHashV1 {
        PumpResearchStorageHashV1::from(*blake3::hash(bytes).as_bytes())
    }

    fn sha256_hash(bytes: &[u8]) -> PumpResearchStorageHashV1 {
        PumpResearchStorageHashV1::from(<[u8; 32]>::from(Sha256::digest(bytes)))
    }

    fn pump_key() -> Vec<u8> {
        solana_sdk::pubkey::Pubkey::from_str(
            ghost_core::pump_research_tape::PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1,
        )
        .expect("pinned Pump program id")
        .to_bytes()
        .to_vec()
    }

    fn transaction_info(signature_byte: u8, index: u64) -> SubscribeUpdateTransactionInfo {
        SubscribeUpdateTransactionInfo {
            signature: vec![signature_byte; 64],
            is_vote: false,
            transaction: Some(Transaction {
                signatures: vec![vec![signature_byte; 64]],
                message: Some(ProtoMessage {
                    header: None,
                    account_keys: vec![pump_key()],
                    recent_blockhash: vec![],
                    instructions: vec![CompiledInstruction {
                        program_id_index: 0,
                        accounts: vec![],
                        data: vec![1],
                    }],
                    versioned: false,
                    address_table_lookups: vec![],
                }),
            }),
            meta: Some(TransactionStatusMeta::default()),
            index,
        }
    }

    fn source(sequence: u64, payload: &[u8]) -> PumpExactStateSourceEnvelopeV2 {
        PumpExactStateSourceEnvelopeV2 {
            provider_id: "fixture".to_owned(),
            provider_role: PumpExactStateProviderRoleV2::PrimaryAuthority,
            stream_epoch: 1,
            capture_sequence: sequence,
            payload_hash_blake3: storage_hash(payload),
        }
    }

    fn finalized_slot_node(parent_slot: u64) -> PumpExactStateSlotNodeV2 {
        let mut node = PumpExactStateSlotNodeV2::default();
        node.finalized_parents.insert(Some(parent_slot));
        node
    }

    fn reconciled_block_pair(
        parent_slot: u64,
        blockhash: &str,
        parent_blockhash: &str,
        ingress_ms: u64,
        source_capture_sequence: u64,
    ) -> PumpExactStateBlockLaneSlotLedgerV2 {
        let ingress = PumpExactStateIngressTimestampV2 {
            wall_ms: ingress_ms,
            monotonic_ms: ingress_ms,
        };
        PumpExactStateBlockLaneSlotLedgerV2 {
            block_meta: Some(PumpExactStateBlockMetaSlotEvidenceV2 {
                parent_slot,
                blockhash: blockhash.to_owned(),
                parent_blockhash: parent_blockhash.to_owned(),
                executed_transaction_count: 0,
                ingress,
                source_capture_sequence,
            }),
            full_block: Some(PumpExactStateFullBlockSlotEvidenceV2 {
                parent_slot,
                blockhash: blockhash.to_owned(),
                parent_blockhash: parent_blockhash.to_owned(),
                executed_transaction_count: 0,
                ingress,
                source_capture_sequence,
            }),
        }
    }

    #[test]
    fn reconciled_frontier_uses_complete_chain_tip_when_parent_evidence_arrives_last() {
        let mut collector = PumpExactStateRawRecordCollectorV2::default();
        let mut slots = BTreeMap::new();
        slots.insert(103, finalized_slot_node(102));
        slots.insert(104, finalized_slot_node(103));
        slots.insert(105, finalized_slot_node(104));

        collector.block_lanes_by_slot.insert(
            103,
            reconciled_block_pair(102, "hash-103", "hash-102", 1_000, 1),
        );
        // The 105 child pair is retained before the parent pair is fully
        // delivered.  At final raw inspection the linked chain is valid, but
        // its availability begins only once the late parent pair arrives.
        collector.block_lanes_by_slot.insert(
            104,
            reconciled_block_pair(103, "hash-104", "hash-103", 3_000, 4),
        );
        collector.block_lanes_by_slot.insert(
            105,
            reconciled_block_pair(104, "hash-105", "hash-104", 2_000, 3),
        );

        let (frontier_slot, frontier_ingress) = collector
            .reconciled_full_block_frontier(102, &slots)
            .expect("a complete parent-linked chain must reconcile");
        assert_eq!(frontier_slot, 105);
        assert_eq!(frontier_ingress.monotonic_ms, 3_000);
        assert_eq!(frontier_ingress.wall_ms, 3_000);
    }

    #[test]
    fn full_block_pump_inventory_reconciles_identical_filtered_transaction() {
        let info = transaction_info(7, 3);
        let block = SubscribeUpdateBlock {
            slot: 42,
            parent_slot: 41,
            blockhash: "blockhash".to_owned(),
            parent_blockhash: "parent".to_owned(),
            executed_transaction_count: 1,
            transactions: vec![info.clone()],
            ..SubscribeUpdateBlock::default()
        };
        let block_payload = SubscribeUpdate {
            filters: vec!["full-block".to_owned()],
            update_oneof: Some(UpdateOneof::Block(block)),
        }
        .encode_to_vec();
        let mut collector = PumpExactStateRawRecordCollectorV2 {
            expected_pump_program_id: pump_key()
                .try_into()
                .expect("Pump pubkey must have 32 bytes"),
            ..PumpExactStateRawRecordCollectorV2::default()
        };
        collector
            .observe(&PumpExactStateRawRecordV2::FullBlockPayloadStarted(
                PumpExactStateFullBlockPayloadStartedV2 {
                    source: source(1, &block_payload),
                    slot: 42,
                    parent_slot: 41,
                    blockhash: "blockhash".to_owned(),
                    parent_blockhash: "parent".to_owned(),
                    executed_transaction_count: 1,
                    event_time: PumpResearchEventTimeV1 {
                        chain_event_ts_ms: None,
                        ingress_wall_ts_ms: Some(1_000),
                        ingress_monotonic_ts_ms: Some(1_000),
                    },
                    source_payload_sha256: sha256_hash(&block_payload),
                    source_payload_bytes: u64::try_from(block_payload.len()).expect("length"),
                    source_payload_chunk_count: 1,
                },
            ))
            .expect("open full block");
        collector
            .observe(&PumpExactStateRawRecordV2::FullBlockPayloadChunk(
                PumpExactStateFullBlockPayloadChunkV2 {
                    source_capture_sequence: 1,
                    chunk_index: 0,
                    bytes: block_payload.clone(),
                },
            ))
            .expect("full block chunk");
        collector
            .observe(&PumpExactStateRawRecordV2::FullBlockPayloadCompleted(
                PumpExactStateFullBlockPayloadCompletedV2 {
                    source_capture_sequence: 1,
                    source_payload_blake3: storage_hash(&block_payload),
                    source_payload_sha256: sha256_hash(&block_payload),
                    source_payload_bytes: u64::try_from(block_payload.len()).expect("length"),
                    source_payload_chunk_count: 1,
                },
            ))
            .expect("complete full block");

        let filtered_payload = SubscribeUpdate {
            filters: vec!["pump-filtered".to_owned()],
            update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
                transaction: Some(info),
                slot: 42,
            })),
        }
        .encode_to_vec();
        collector
            .observe(&PumpExactStateRawRecordV2::PrimaryTransaction(
                PumpExactStateTransactionEvidenceV2 {
                    source: source(2, &filtered_payload),
                    slot: 42,
                    tx_index: Some(3),
                    signature: PumpResearchStorageSignatureV1::from([7; 64]),
                    event_time: PumpResearchEventTimeV1 {
                        chain_event_ts_ms: None,
                        ingress_wall_ts_ms: Some(1_001),
                        ingress_monotonic_ts_ms: Some(1_001),
                    },
                    block_time: None,
                    source_payload: filtered_payload,
                },
            ))
            .expect("filtered transaction");
        assert_eq!(
            collector.full_block_pump_transactions,
            collector.filtered_transactions
        );
    }

    #[test]
    fn full_block_pump_detection_covers_loaded_and_inner_program_indices() {
        let mut loaded = transaction_info(8, 4);
        let loaded_body = loaded.transaction.as_mut().expect("transaction body");
        let loaded_message = loaded_body.message.as_mut().expect("message");
        loaded_message.account_keys.clear();
        loaded_message.instructions = vec![CompiledInstruction {
            program_id_index: 0,
            accounts: vec![],
            data: vec![],
        }];
        loaded.meta = Some(TransactionStatusMeta {
            loaded_writable_addresses: vec![pump_key()],
            ..TransactionStatusMeta::default()
        });
        assert!(transaction_invokes_pump(
            &loaded,
            pump_key()
                .try_into()
                .expect("Pump pubkey must have 32 bytes"),
        )
        .expect("loaded Pump program"));

        let mut inner = transaction_info(9, 5);
        let inner_body = inner.transaction.as_mut().expect("transaction body");
        let inner_message = inner_body.message.as_mut().expect("message");
        inner_message.instructions.clear();
        inner.meta = Some(TransactionStatusMeta {
            inner_instructions: vec![InnerInstructions {
                index: 0,
                instructions: vec![InnerInstruction {
                    program_id_index: 0,
                    accounts: vec![],
                    data: vec![],
                    stack_height: Some(2),
                }],
            }],
            ..TransactionStatusMeta::default()
        });
        assert!(transaction_invokes_pump(
            &inner,
            pump_key()
                .try_into()
                .expect("Pump pubkey must have 32 bytes"),
        )
        .expect("inner Pump program"));
    }

    #[test]
    fn full_block_reconciliation_rejects_same_locator_with_different_transaction_evidence() {
        let key = PumpExactStateTransactionKeyV2 {
            slot: 77,
            tx_index: 5,
            signature: [4; 64],
        };
        let filtered = BTreeMap::from([(key.clone(), [1; 32])]);
        let full_block = BTreeMap::from([(key, [2; 32])]);
        assert!(reconcile_filtered_and_full_block_transactions(&filtered, &full_block).is_err());
    }

    #[test]
    fn full_block_reconciliation_rejects_missing_extra_and_locator_drift() {
        let key = PumpExactStateTransactionKeyV2 {
            slot: 77,
            tx_index: 5,
            signature: [4; 64],
        };
        let filtered = BTreeMap::from([(key.clone(), [1; 32])]);
        assert!(
            reconcile_filtered_and_full_block_transactions(&filtered, &BTreeMap::new()).is_err(),
            "a missing filtered Pump transaction must fail closed"
        );
        assert!(
            reconcile_filtered_and_full_block_transactions(&BTreeMap::new(), &filtered).is_err(),
            "an extra full-block Pump transaction must fail closed"
        );
        let signature_drift = BTreeMap::from([(
            PumpExactStateTransactionKeyV2 {
                signature: [5; 64],
                ..key.clone()
            },
            [1; 32],
        )]);
        assert!(
            reconcile_filtered_and_full_block_transactions(&filtered, &signature_drift).is_err(),
            "signature drift must fail closed"
        );
        let tx_index_drift = BTreeMap::from([(
            PumpExactStateTransactionKeyV2 { tx_index: 6, ..key },
            [1; 32],
        )]);
        assert!(
            reconcile_filtered_and_full_block_transactions(&filtered, &tx_index_drift).is_err(),
            "transaction-index drift must fail closed"
        );
    }

    #[test]
    fn frozen_v2_segment_scan_requires_positioned_footer_and_whole_file_receipt_match() {
        let temporary = tempfile::tempdir().expect("temporary raw directory");
        let path = temporary.path().join("segment_00000.bin");
        let header = PumpExactStateSegmentHeaderV2 {
            storage_format_version: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            run_id: "fixture-v2".to_owned(),
            segment_index: 0,
            stream_epoch: 1,
            opened_wall_ts_ms: 1,
            opened_monotonic_ts_ms: 1,
            capture_contract_sha256: PumpResearchStorageHashV1::from([3; 32]),
            previous_segment_blake3: None,
        };
        let header_bytes =
            PumpExactStateRawCodecV2::encode_segment_header(&header).expect("encode V2 header");
        let prefix = PumpResearchStorageHashV1::from(*blake3::hash(&header_bytes).as_bytes());
        let footer = PumpExactStateRawCodecV2::encode_record(
            &PumpExactStateRawRecordV2::SegmentClosed(PumpExactStateSegmentClosedV2 {
                storage_format_version: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                segment_index: 0,
                accepted_record_count: 0,
                data_bytes: u64::try_from(header_bytes.len()).expect("header length"),
                segment_blake3: prefix,
                closed_wall_ts_ms: 2,
                clean_shutdown: true,
            }),
        )
        .expect("encode V2 footer");
        let mut bytes = header_bytes.clone();
        bytes.extend_from_slice(&footer);
        fs::write(&path, &bytes).expect("write frozen V2 fixture segment");
        let receipt = PumpExactStateSegmentReceiptV2 {
            segment_index: 0,
            filename: "segment_00000.bin".to_owned(),
            file_bytes: u64::try_from(bytes.len()).expect("file length"),
            file_sha256: sha256_hash(&bytes),
            file_blake3: storage_hash(&bytes),
            first_capture_sequence: None,
            last_capture_sequence: None,
            accepted_record_count: 0,
        };
        assert_eq!(
            scan_v2_segment(&path, &receipt, "fixture-v2", None, true, false, |_, _| Ok(
                ()
            ))
            .expect("scan terminal V2 fixture"),
            prefix
        );
        assert!(
            scan_v2_segment(
                &path,
                &receipt,
                "fixture-v2",
                None,
                false,
                false,
                |_, _| Ok(())
            )
            .is_err(),
            "a terminal footer must not masquerade as an intermediate rollover"
        );

        let rollover_footer = PumpExactStateRawCodecV2::encode_record(
            &PumpExactStateRawRecordV2::SegmentClosed(PumpExactStateSegmentClosedV2 {
                storage_format_version: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                segment_index: 0,
                accepted_record_count: 0,
                data_bytes: u64::try_from(header_bytes.len()).expect("header length"),
                segment_blake3: prefix,
                closed_wall_ts_ms: 2,
                clean_shutdown: false,
            }),
        )
        .expect("encode intermediate rollover footer");
        let mut rollover_bytes = header_bytes;
        rollover_bytes.extend_from_slice(&rollover_footer);
        fs::write(&path, &rollover_bytes).expect("write intermediate V2 fixture segment");
        let rollover_receipt = PumpExactStateSegmentReceiptV2 {
            file_bytes: u64::try_from(rollover_bytes.len()).expect("rollover file length"),
            file_sha256: sha256_hash(&rollover_bytes),
            file_blake3: storage_hash(&rollover_bytes),
            ..receipt.clone()
        };
        assert_eq!(
            scan_v2_segment(
                &path,
                &rollover_receipt,
                "fixture-v2",
                None,
                false,
                false,
                |_, _| Ok(()),
            )
            .expect("scan intermediate rollover V2 fixture"),
            prefix
        );
        assert!(
            scan_v2_segment(
                &path,
                &rollover_receipt,
                "fixture-v2",
                None,
                true,
                false,
                |_, _| Ok(()),
            )
            .is_err(),
            "an intermediate rollover footer must not masquerade as the terminal segment"
        );
        let mut corrupted = rollover_receipt.clone();
        corrupted.file_sha256 = PumpResearchStorageHashV1::from([9; 32]);
        assert!(scan_v2_segment(
            &path,
            &corrupted,
            "fixture-v2",
            None,
            false,
            false,
            |_, _| Ok(())
        )
        .is_err());
    }

    fn test_digest() -> PumpExactStateDigestV2 {
        PumpExactStateDigestV2 {
            sha256: "11".repeat(32),
            blake3: "22".repeat(32),
            bytes: 1,
        }
    }

    fn test_curve_state() -> PumpExactStateCurveStateV2 {
        PumpExactStateCurveStateV2 {
            virtual_token_reserves: 10,
            virtual_quote_reserves: 11,
            real_token_reserves: 12,
            real_quote_reserves: 13,
            token_total_supply: 14,
            complete: false,
            creator: Pubkey::new_from_array([3; 32]),
            is_mayhem_mode: false,
            is_cashback_coin: false,
            quote_mint: Pubkey::default(),
        }
    }

    #[test]
    fn anchored_single_candidate_requires_pre_and_unique_final_state() {
        let curve = Pubkey::new_from_array([7; 32]);
        let signature = [9; 64];
        let pre = PumpExactStateCurveAnchorV2 {
            curve,
            state: test_curve_state(),
            slot: 10,
            write_version: 1,
            transaction_index: None,
            source_capture_sequence: Some(1),
            canonical: true,
        };
        let final_anchor = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 14,
                ..test_curve_state()
            },
            slot: 11,
            write_version: 2,
            transaction_index: Some(0),
            source_capture_sequence: Some(2),
            canonical: true,
        };
        let anchors = PumpExactStateAnchorIndexV2 {
            by_curve: BTreeMap::from([(curve, vec![pre, final_anchor.clone()])]),
            final_by_signature: BTreeMap::from([((signature, curve), vec![final_anchor])]),
            ..PumpExactStateAnchorIndexV2::default()
        };
        let exact = evaluate_candidate_exactness_v2(
            PumpExactStateInstructionEffectV2::SupportedExactTrade,
            true,
            true,
            Some(curve),
            signature,
            11,
            0,
            1,
            false,
            &anchors,
        );
        assert!(exact.exact);
        assert!(exact.state_before.is_some());
        assert!(exact.state_after.is_some());
        assert!(
            !evaluate_candidate_exactness_v2(
                PumpExactStateInstructionEffectV2::SupportedExactTrade,
                true,
                true,
                Some(curve),
                signature,
                11,
                0,
                2,
                false,
                &anchors,
            )
            .exact
        );
    }

    #[test]
    fn first_streamed_trade_without_predecessor_is_typed_non_exact_without_repair() {
        let curve = Pubkey::new_from_array([7; 32]);
        let signature = [9; 64];
        let final_anchor = PumpExactStateCurveAnchorV2 {
            curve,
            state: test_curve_state(),
            slot: 11,
            write_version: 2,
            transaction_index: Some(0),
            source_capture_sequence: Some(2),
            canonical: true,
        };
        let anchors = PumpExactStateAnchorIndexV2 {
            by_curve: BTreeMap::from([(curve, vec![final_anchor.clone()])]),
            final_by_signature: BTreeMap::from([((signature, curve), vec![final_anchor])]),
            ..PumpExactStateAnchorIndexV2::default()
        };
        let evaluation = evaluate_candidate_exactness_v2(
            PumpExactStateInstructionEffectV2::SupportedExactTrade,
            true,
            true,
            Some(curve),
            signature,
            11,
            0,
            1,
            false,
            &anchors,
        );
        assert!(!evaluation.exact);
        assert_eq!(
            evaluation.non_exact_reason.as_deref(),
            Some("missing_exact_pre_anchor"),
            "the first observed trade of an older curve cannot borrow state from RPC or an implicit baseline"
        );
        assert!(evaluation.state_before.is_none());
        assert!(evaluation.state_after.is_none());
    }

    #[test]
    fn coverage_threshold_is_literal_and_does_not_round_up() {
        assert_eq!(
            coverage_ppm_v2(998_999, 1_000_000).expect("coverage"),
            998_999
        );
        assert_eq!(coverage_ppm_v2(999, 1_000).expect("coverage"), 999_000);
    }

    #[test]
    fn qualification_run_minimum_is_literal_for_time_or_mutation_denominator() {
        assert!(
            qualification_run_below_minimum_v2(Some(1_799_999), 9_999),
            "9,999 mutations below thirty minutes must remain blocked"
        );
        assert!(
            !qualification_run_below_minimum_v2(Some(1_799_999), 10_000),
            "10,000 mutations satisfies the alternative V1.1 minimum"
        );
        assert!(
            !qualification_run_below_minimum_v2(Some(1_800_000), 0),
            "thirty minutes satisfies the alternative V1.1 minimum"
        );
        assert!(
            qualification_run_below_minimum_v2(None, 9_999),
            "missing elapsed cohort evidence cannot silently satisfy the minimum"
        );
    }

    #[test]
    fn stream_boundary_cannot_precede_its_exclusive_source_prefix() {
        let mut collector = PumpExactStateRawRecordCollectorV2::default();
        let boundary = PumpExactStateProspectiveStreamBoundaryV2 {
            source_readiness: PumpExactStateSourceReadinessV2 {
                first_transaction_slot: 101,
                first_account_update_slot: 102,
                first_slot_update_slot: 103,
                first_block_meta_slot: 104,
                first_full_block_slot: 105,
                source_readiness_slot: 105,
            },
            source_stream_epoch: 1,
            source_capture_sequence_exclusive: 5,
            cohort_slots_strictly_after: 105,
            sealed_wall_ts_ms: 10,
            sealed_monotonic_ts_ms: 10,
        };
        let error = collector
            .observe(&PumpExactStateRawRecordV2::ProspectiveStreamBoundary(
                boundary,
            ))
            .expect_err("a raw boundary cannot claim an unwritten warm-up prefix");
        assert!(error
            .to_string()
            .contains("complete exclusive source capture prefix"));
    }

    #[test]
    fn qualified_capability_requires_an_exact_trade_with_both_states() {
        let mut counters = PumpExactStateQualificationCountersV2 {
            rooted_canonical_slot_count: 1,
            successful_rooted_instruction_occurrence_count: 1_000,
            successful_rooted_candidate_count: 1_000,
            occurrence_ledger_reconciled: true,
            successful_rooted_mutation_denominator: 1_000,
            exact_rooted_mutation_count: 999,
            explicit_non_exact_mutation_count: 1,
            denominator_reconciled: true,
            exact_rooted_coverage_ppm: PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2,
            exact_trajectory_count: 999,
            successful_rooted_exact_trade_with_both_states_count: 1,
            exact_birth_count: 1,
            ..PumpExactStateQualificationCountersV2::default()
        };
        assert!(
            !capability_blockers_v2(&counters).contains(
                &PumpExactStateCapabilityBlockerV2::NoSuccessfulRootedTradeWithBothStates
            ),
            "an exact create must not be the only way past the trade-state gate"
        );
        counters.exact_rooted_coverage_ppm = 998_999;
        assert!(
            capability_blockers_v2(&counters)
                .contains(&PumpExactStateCapabilityBlockerV2::ExactCoverageBelowThreshold),
            "998999 ppm is a literal blocked result and may not round up"
        );
        counters.exact_rooted_coverage_ppm = PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2;
        counters.successful_rooted_exact_trade_with_both_states_count = 0;
        assert!(
            capability_blockers_v2(&counters).contains(
                &PumpExactStateCapabilityBlockerV2::NoSuccessfulRootedTradeWithBothStates
            ),
            "a mathematically high coverage result with no two-state trade is blocked"
        );
        counters.successful_rooted_exact_trade_with_both_states_count = 1;
        counters.successful_rooted_unknown_occurrence_count = 1;
        assert!(
            capability_blockers_v2(&counters)
                .contains(&PumpExactStateCapabilityBlockerV2::MutationInventoryIncomplete),
            "one unknown occurrence blocks Qualified even when coverage and trade-state gates pass"
        );
    }

    #[test]
    fn qualified_receipt_allows_999000_ppm_with_exact_only_trajectory_rows() {
        let artifact = |line_count| PumpExactStateArtifactDigestV2 {
            sha256: "11".repeat(32),
            blake3: "22".repeat(32),
            bytes: 1,
            line_count,
            newline_complete: true,
        };
        let births = artifact(1);
        let trajectories = artifact(999);
        let coverage = artifact(2);
        let receipt = PumpExactStateCapabilityReceiptV2 {
            schema_version: PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2,
            kind: "pump_exact_state_capability_v2".to_owned(),
            source_run_id: "fixture".to_owned(),
            status: PumpExactStateCapabilityStatusV2::Qualified,
            blockers: Vec::new(),
            source_storage_format_version: PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            source_raw_segment_set_blake3: "33".repeat(32),
            source_start_manifest_digest: test_digest(),
            source_completion_receipt_digest: test_digest(),
            semantics_id: "fixture-semantics".to_owned(),
            semantics_manifest_digest: test_digest(),
            vendored_idl_digest: test_digest(),
            materializer_running_executable_digest: test_digest(),
            cohort_slots_strictly_after: 0,
            rooted_canonical_slot_count: 1,
            filtered_pump_transaction_count: 2,
            full_block_pump_transaction_count: 2,
            pump_owned_account_update_count: 1,
            qualification_run_below_minimum: false,
            bonding_curve_account_count: 1,
            bonding_curve_decoded_count: 1,
            global_account_count: 1,
            global_validated_count: 1,
            unknown_pump_owned_account_count: 0,
            account_decode_failure_count: 0,
            successful_rooted_instruction_occurrence_count: 1_000,
            successful_rooted_proven_non_reserve_count: 0,
            successful_rooted_validated_event_transport_count: 0,
            successful_rooted_candidate_count: 1_000,
            successful_rooted_unknown_occurrence_count: 0,
            successful_rooted_malformed_candidate_count: 0,
            occurrence_ledger_reconciled: true,
            successful_rooted_mutation_denominator: 1_000,
            exact_rooted_mutation_count: 999,
            explicit_non_exact_mutation_count: 1,
            denominator_reconciled: true,
            exact_rooted_coverage_ppm: 999_000,
            required_exact_rooted_coverage_ppm: PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2,
            exact_trajectory_count: 999,
            successful_rooted_exact_trade_with_both_states_count: 999,
            exact_birth_count: 1,
            births_artifact: births.clone(),
            trajectories_artifact: trajectories.clone(),
            coverage_artifact: coverage.clone(),
        };
        let receipt_digest = artifact(1);
        let manifest_digest = artifact(1);
        let manifest = PumpExactStateExactManifestV2 {
            schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
            kind: "pump_exact_state_tape_v2".to_owned(),
            source_run_id: "fixture".to_owned(),
            exact_state_capability_status: PumpExactStateCapabilityStatusV2::Qualified,
            source_raw_segment_set_blake3: "33".repeat(32),
            semantics_manifest_sha256: "11".repeat(32),
            semantics_manifest_blake3: "22".repeat(32),
            materializer_running_executable_sha256: "11".repeat(32),
            materializer_running_executable_blake3: "22".repeat(32),
            materializer_running_executable_bytes: 1,
            exact_state_capability_artifact: receipt_digest.clone(),
            births_artifact: births.clone(),
            trajectories_artifact: trajectories.clone(),
            coverage_artifact: coverage.clone(),
        };
        validate_exact_output_receipt_v2(
            &receipt,
            &manifest,
            &receipt_digest,
            &manifest_digest,
            &births,
            &trajectories,
            &coverage,
        )
        .expect("999000 ppm may contain one explicit non-exact candidate without adding it to exact trajectories");

        let wrong_trajectory_count = artifact(1_000);
        assert!(
            validate_exact_output_receipt_v2(
                &receipt,
                &manifest,
                &receipt_digest,
                &manifest_digest,
                &births,
                &wrong_trajectory_count,
                &coverage,
            )
            .is_err(),
            "a non-exact candidate must not be smuggled into the exact trajectory stream"
        );
    }

    #[test]
    fn same_slot_predecessor_requires_a_signature_bound_lower_transaction_index() {
        let curve = Pubkey::new_from_array([7; 32]);
        let signature = [9; 64];
        let predecessor = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 15,
                ..test_curve_state()
            },
            slot: 11,
            write_version: 2,
            transaction_index: Some(3),
            source_capture_sequence: Some(3),
            canonical: true,
        };
        let unbound_same_slot = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 16,
                ..test_curve_state()
            },
            slot: 11,
            write_version: 3,
            transaction_index: None,
            source_capture_sequence: Some(4),
            canonical: true,
        };
        let later = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 17,
                ..test_curve_state()
            },
            slot: 11,
            write_version: 4,
            transaction_index: Some(5),
            source_capture_sequence: Some(5),
            canonical: true,
        };
        let noncanonical_lower_slot = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 99,
                ..test_curve_state()
            },
            slot: 10,
            write_version: u64::MAX,
            transaction_index: Some(0),
            source_capture_sequence: Some(u64::MAX),
            canonical: false,
        };
        let final_anchor = PumpExactStateCurveAnchorV2 {
            curve,
            state: PumpExactStateCurveStateV2 {
                real_quote_reserves: 18,
                ..test_curve_state()
            },
            slot: 11,
            write_version: 5,
            transaction_index: Some(4),
            source_capture_sequence: Some(6),
            canonical: true,
        };
        let anchors = PumpExactStateAnchorIndexV2 {
            by_curve: BTreeMap::from([(
                curve,
                vec![
                    predecessor.clone(),
                    unbound_same_slot,
                    later,
                    noncanonical_lower_slot,
                    final_anchor.clone(),
                ],
            )]),
            final_by_signature: BTreeMap::from([((signature, curve), vec![final_anchor])]),
            ..PumpExactStateAnchorIndexV2::default()
        };
        let evaluation = evaluate_candidate_exactness_v2(
            PumpExactStateInstructionEffectV2::SupportedExactTrade,
            true,
            true,
            Some(curve),
            signature,
            11,
            4,
            1,
            false,
            &anchors,
        );
        assert!(evaluation.exact);
        assert_eq!(
            evaluation
                .state_before
                .as_ref()
                .expect("signature-bound predecessor")
                .real_quote_reserves,
            predecessor.state.real_quote_reserves
        );
    }

    #[test]
    fn outcome_blind_window_status_is_cutoff_safe_and_never_opens_outcomes() {
        let birth = 1_000u64;
        let last = birth
            + PUMP_EXACT_STATE_WINDOW_OBSERVATION_MS_V2
            + PUMP_EXACT_STATE_WINDOW_FORWARD_MS_V2;
        assert_eq!(
            outcome_blind_window_status_v2(Some(birth), birth, last, 0),
            PumpExactStateOutcomeBlindWindowStatusV2::Complete
        );
        assert_eq!(
            outcome_blind_window_status_v2(Some(birth), birth, last, 1),
            PumpExactStateOutcomeBlindWindowStatusV2::ObservationContainsNonExactMutation
        );
        assert_eq!(
            outcome_blind_window_status_v2(None, birth, last, 0),
            PumpExactStateOutcomeBlindWindowStatusV2::MissingBirthObservedTimestamp
        );
        assert_eq!(
            outcome_blind_window_status_v2(Some(birth - 1), birth, last, 0),
            PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunStart
        );
        assert_eq!(
            outcome_blind_window_status_v2(Some(birth), birth, last - 1, 0),
            PumpExactStateOutcomeBlindWindowStatusV2::TruncatedAtRunEnd
        );
    }

    #[test]
    fn outcome_blind_export_refuses_an_unscoped_candidate() {
        let curve = Pubkey::new_from_array([7; 32]).to_string();
        let scoped = PumpExactStateCandidateCoverageRecordV2 {
            bonding_curve: Some(curve.clone()),
            mint: None,
            effect: "known_reserve_or_dependency_unsupported".to_owned(),
            exact: false,
            non_exact_reason: Some("fixture".to_owned()),
        };
        assert_eq!(
            outcome_blind_candidate_curve_v2(&scoped).expect("scoped candidate"),
            curve.as_str()
        );
        let unscoped = PumpExactStateCandidateCoverageRecordV2 {
            bonding_curve: None,
            ..scoped
        };
        assert!(
            outcome_blind_candidate_curve_v2(&unscoped).is_err(),
            "an unscoped non-exact candidate must block window export rather than disappear"
        );
    }

    #[test]
    fn event_final_state_binding_rejects_a_structurally_valid_wrong_reserve() {
        let state = test_curve_state();
        let matching = PumpExactStateEventFinalStateBindingV2 {
            curve_state_field: "virtual_quote_reserves".to_owned(),
            event_value_borsh: state.virtual_quote_reserves.to_le_bytes().to_vec(),
        };
        validate_event_final_state_bindings_v2(&state, &[matching.clone()])
            .expect("matching anchored reserve state");
        let mismatched = PumpExactStateEventFinalStateBindingV2 {
            event_value_borsh: state
                .virtual_quote_reserves
                .checked_add(1)
                .expect("fixture increment")
                .to_le_bytes()
                .to_vec(),
            ..matching
        };
        assert!(
            validate_event_final_state_bindings_v2(&state, &[mismatched]).is_err(),
            "a valid wrapper/event payload with wrong post-state is not transport authority"
        );
    }

    #[test]
    fn qualification_snapshot_storage_budget_preserves_raw_reserve_and_overflow_fails_closed() {
        assert_eq!(
            required_v2_qualification_storage_bytes(14_000, 16_000)
                .expect("bounded qualification storage budget"),
            30_000 + V2_QUALIFICATION_METADATA_ALLOWANCE_BYTES
        );
        assert!(required_v2_qualification_storage_bytes(u64::MAX, 1).is_err());
        assert!(required_v2_qualification_storage_bytes(1, u64::MAX).is_err());
    }

    #[test]
    fn event_transport_requires_a_real_non_transport_parent_occurrence() {
        let parent_key = PumpExactStateInstructionOccurrenceKeyV2 {
            signature: [1; 64],
            outer_instruction_index: 2,
            inner_instruction_path: Vec::new(),
            stack_height: None,
            program_id: Pubkey::new_from_array([3; 32]),
            discriminator: [4; 8],
        };
        let event_key = PumpExactStateInstructionOccurrenceKeyV2 {
            signature: [1; 64],
            outer_instruction_index: 2,
            inner_instruction_path: vec![0],
            stack_height: Some(2),
            program_id: Pubkey::new_from_array([3; 32]),
            discriminator: ANCHOR_EVENT_CPI_WRAPPER_DISCRIMINATOR_V2,
        };
        let candidate = PumpExactStateInstructionOccurrenceV2 {
            key: parent_key.clone(),
            class: PumpExactStateOccurrenceClassV2::Candidate {
                effect: PumpExactStateInstructionEffectV2::SupportedExactTrade,
                instruction_payload_exact: true,
                account_vector_exact: true,
                bonding_curve: None,
                mint: None,
                failure_reason: None,
                semantic_evidence: None,
            },
        };
        let event = PumpExactStateInstructionOccurrenceV2 {
            key: event_key.clone(),
            class: PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                immediate_parent: parent_key.clone(),
                event_discriminator: [5; 8],
                event_fields: BTreeMap::new(),
                final_state_bindings: Vec::new(),
            },
        };
        let mut linked = vec![candidate, event];
        validate_event_transport_parent_links_v2(&mut linked);
        assert!(matches!(
            linked[1].class,
            PumpExactStateOccurrenceClassV2::ValidatedEventTransport { .. }
        ));

        let missing_parent = PumpExactStateInstructionOccurrenceKeyV2 {
            inner_instruction_path: vec![9],
            ..parent_key
        };
        let mut unlinked = vec![PumpExactStateInstructionOccurrenceV2 {
            key: event_key,
            class: PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                immediate_parent: missing_parent,
                event_discriminator: [5; 8],
                event_fields: BTreeMap::new(),
                final_state_bindings: Vec::new(),
            },
        }];
        validate_event_transport_parent_links_v2(&mut unlinked);
        assert!(matches!(
            &unlinked[0].class,
            PumpExactStateOccurrenceClassV2::Unknown { reason }
                if reason == "anchor_event_transport_parent_occurrence_missing"
        ));

        let mut nested_transport = vec![
            PumpExactStateInstructionOccurrenceV2 {
                key: parent_key.clone(),
                class: PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                    immediate_parent: parent_key.clone(),
                    event_discriminator: [5; 8],
                    event_fields: BTreeMap::new(),
                    final_state_bindings: Vec::new(),
                },
            },
            PumpExactStateInstructionOccurrenceV2 {
                key: PumpExactStateInstructionOccurrenceKeyV2 {
                    signature: [1; 64],
                    outer_instruction_index: 2,
                    inner_instruction_path: vec![1],
                    stack_height: Some(2),
                    program_id: Pubkey::new_from_array([3; 32]),
                    discriminator: ANCHOR_EVENT_CPI_WRAPPER_DISCRIMINATOR_V2,
                },
                class: PumpExactStateOccurrenceClassV2::ValidatedEventTransport {
                    immediate_parent: parent_key,
                    event_discriminator: [5; 8],
                    event_fields: BTreeMap::new(),
                    final_state_bindings: Vec::new(),
                },
            },
        ];
        validate_event_transport_parent_links_v2(&mut nested_transport);
        assert!(matches!(
            &nested_transport[1].class,
            PumpExactStateOccurrenceClassV2::Unknown { reason }
                if reason == "anchor_event_transport_parent_is_event_transport"
        ));
    }

    #[test]
    fn event_transport_parent_key_follows_the_immediate_pump_stack_frame() {
        let pump = Pubkey::new_from_array([3; 32]);
        let parent_discriminator = [4; 8];
        let context = PumpExactStateTransactionContextV2 {
            signature: [1; 64],
            slot: 9,
            tx_index: 2,
            success: true,
            accounts: vec![PumpExactStateAccountMetaV2 {
                pubkey: pump,
                signer: false,
                writable: false,
            }],
            outer: vec![yellowstone_grpc_proto::prelude::CompiledInstruction {
                program_id_index: 0,
                accounts: Vec::new(),
                data: parent_discriminator.to_vec(),
            }],
            inner: BTreeMap::new(),
        };
        let inner = vec![
            yellowstone_grpc_proto::prelude::InnerInstruction {
                program_id_index: 0,
                accounts: Vec::new(),
                data: parent_discriminator.to_vec(),
                stack_height: Some(2),
            },
            yellowstone_grpc_proto::prelude::InnerInstruction {
                program_id_index: 0,
                accounts: Vec::new(),
                data: ANCHOR_EVENT_CPI_WRAPPER_DISCRIMINATOR_V2.to_vec(),
                stack_height: Some(3),
            },
        ];
        let inner_parent =
            immediate_pump_parent_occurrence_key_v2(&context, 0, &inner, 1, Some(3), pump)
                .expect("derive inner event parent")
                .expect("inner Pump parent");
        assert_eq!(inner_parent.inner_instruction_path, vec![0]);
        assert_eq!(inner_parent.stack_height, Some(2));
        assert_eq!(inner_parent.discriminator, parent_discriminator);

        let outer_parent =
            immediate_pump_parent_occurrence_key_v2(&context, 0, &inner[..1], 0, Some(2), pump)
                .expect("derive outer event parent")
                .expect("outer Pump parent");
        assert!(outer_parent.inner_instruction_path.is_empty());
        assert_eq!(outer_parent.stack_height, None);
        assert_eq!(outer_parent.discriminator, parent_discriminator);
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn anonymous_snapshot_survives_source_path_removal_without_a_named_staging_path() {
        let temporary = tempfile::tempdir().expect("temporary snapshot root");
        let source = temporary.path().join("segment_00000.bin");
        let bytes = b"frozen-v2-segment-snapshot";
        fs::write(&source, bytes).expect("write source segment");
        let receipt = PumpExactStateSegmentReceiptV2 {
            segment_index: 0,
            filename: "segment_00000.bin".to_owned(),
            file_bytes: u64::try_from(bytes.len()).expect("length"),
            file_sha256: sha256_hash(bytes),
            file_blake3: storage_hash(bytes),
            first_capture_sequence: None,
            last_capture_sequence: None,
            accepted_record_count: 0,
        };
        let snapshot = copy_segment_to_anonymous_snapshot_v2(&source, temporary.path(), &receipt)
            .expect("anonymous snapshot");
        assert_eq!(snapshot.metadata().expect("metadata").nlink(), 0);
        fs::remove_file(&source).expect("remove source path");
        let mut observed = vec![0u8; bytes.len()];
        snapshot
            .read_exact_at(&mut observed, 0)
            .expect("read anonymous snapshot");
        assert_eq!(observed, bytes);
        assert!(
            fs::read_dir(temporary.path())
                .expect("temporary directory")
                .next()
                .is_none(),
            "snapshot must not create a named staging path"
        );
    }

    #[test]
    fn exact_output_is_atomic_and_manifest_binds_capability_receipt() {
        let temporary = tempfile::tempdir().expect("temporary output root");
        let raw_dir = temporary.path().join("raw-v2");
        let output_dir = temporary.path().join("exact-v2");
        fs::create_dir(&raw_dir).expect("raw directory");
        let mut writer =
            PumpExactStateExactOutputWriterV2::create(&raw_dir, &output_dir).expect("writer");
        writer
            .write_coverage(&PumpExactStateCoverageRecordV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                source_run_id: "fixture".to_owned(),
                source_capture_sequence: 1,
                observed_ingress_wall_ms: Some(1_000),
                observed_ingress_monotonic_ms: Some(1_000),
                slot: 1,
                tx_index: 0,
                signature: "fixture".to_owned(),
                rooted: true,
                success: true,
                occurrence_count: 1,
                candidate_count: 1,
                exact_candidate_count: 0,
                inventory_complete: false,
                reason_codes: vec!["fixture".to_owned()],
                candidates: Vec::new(),
            })
            .expect("coverage");
        let (receipt_path, receipt_digest) = writer
            .finish(
                |births, trajectories, coverage| {
                    Ok(PumpExactStateCapabilityReceiptV2 {
                        schema_version: PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2,
                        kind: "fixture".to_owned(),
                        source_run_id: "fixture".to_owned(),
                        status: PumpExactStateCapabilityStatusV2::Blocked,
                        blockers: vec![
                            PumpExactStateCapabilityBlockerV2::ExactCoverageBelowThreshold,
                        ],
                        source_storage_format_version: 2,
                        source_raw_segment_set_blake3: "33".repeat(32),
                        source_start_manifest_digest: test_digest(),
                        source_completion_receipt_digest: test_digest(),
                        semantics_id: "fixture".to_owned(),
                        semantics_manifest_digest: test_digest(),
                        vendored_idl_digest: test_digest(),
                        materializer_running_executable_digest: test_digest(),
                        cohort_slots_strictly_after: 0,
                        rooted_canonical_slot_count: 1,
                        filtered_pump_transaction_count: 1,
                        full_block_pump_transaction_count: 1,
                        pump_owned_account_update_count: 0,
                        qualification_run_below_minimum: false,
                        bonding_curve_account_count: 0,
                        bonding_curve_decoded_count: 0,
                        global_account_count: 0,
                        global_validated_count: 0,
                        unknown_pump_owned_account_count: 0,
                        account_decode_failure_count: 0,
                        successful_rooted_instruction_occurrence_count: 1,
                        successful_rooted_proven_non_reserve_count: 0,
                        successful_rooted_validated_event_transport_count: 0,
                        successful_rooted_candidate_count: 1,
                        successful_rooted_unknown_occurrence_count: 0,
                        successful_rooted_malformed_candidate_count: 0,
                        occurrence_ledger_reconciled: true,
                        successful_rooted_mutation_denominator: 1,
                        exact_rooted_mutation_count: 0,
                        explicit_non_exact_mutation_count: 1,
                        denominator_reconciled: true,
                        exact_rooted_coverage_ppm: 0,
                        required_exact_rooted_coverage_ppm:
                            PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2,
                        exact_trajectory_count: 0,
                        successful_rooted_exact_trade_with_both_states_count: 0,
                        exact_birth_count: 0,
                        births_artifact: births,
                        trajectories_artifact: trajectories,
                        coverage_artifact: coverage,
                    })
                },
                |receipt, births, trajectories, coverage| {
                    Ok(PumpExactStateExactManifestV2 {
                        schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                        kind: "fixture".to_owned(),
                        source_run_id: "fixture".to_owned(),
                        exact_state_capability_status: PumpExactStateCapabilityStatusV2::Blocked,
                        source_raw_segment_set_blake3: "33".repeat(32),
                        semantics_manifest_sha256: "11".repeat(32),
                        semantics_manifest_blake3: "22".repeat(32),
                        materializer_running_executable_sha256: "44".repeat(32),
                        materializer_running_executable_blake3: "55".repeat(32),
                        materializer_running_executable_bytes: 1,
                        exact_state_capability_artifact: receipt,
                        births_artifact: births,
                        trajectories_artifact: trajectories,
                        coverage_artifact: coverage,
                    })
                },
                || Ok(()),
            )
            .expect("publish fixture output");
        assert_eq!(
            receipt_path,
            output_dir.join("exact_state_capability_v2.json")
        );
        assert!(!temporary.path().join(".exact-v2.partial").exists());
        assert_eq!(
            digest_private_artifact_v2(&receipt_path)
                .expect("digest receipt")
                .sha256,
            receipt_digest.sha256
        );
        let manifest: Value = serde_json::from_slice(
            &fs::read(output_dir.join("manifest_v2.json")).expect("read manifest"),
        )
        .expect("parse manifest");
        assert_eq!(
            manifest["exact_state_capability_artifact"]["sha256"],
            Value::String(receipt_digest.sha256)
        );
        assert!(
            validate_exact_output_artifacts_v2(&output_dir).is_err(),
            "a blocked diagnostic output must never become strategy-input authority"
        );
        #[cfg(unix)]
        {
            assert_eq!(
                fs::metadata(&output_dir).expect("output metadata").mode() & 0o777,
                0o700
            );
            assert_eq!(
                fs::metadata(&receipt_path)
                    .expect("receipt metadata")
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }

    #[test]
    fn qualified_exact_output_adapter_pins_artifacts_and_detects_late_drift() {
        let temporary = tempfile::tempdir().expect("temporary output root");
        let raw_dir = temporary.path().join("raw-v2");
        let output_dir = temporary.path().join("exact-v2");
        fs::create_dir(&raw_dir).expect("raw directory");
        let mut writer =
            PumpExactStateExactOutputWriterV2::create(&raw_dir, &output_dir).expect("writer");
        let state = PumpExactStateCurveStateArtifactV2::from(&test_curve_state());
        writer
            .write_birth(&PumpExactStateBirthRecordV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                source_run_id: "fixture".to_owned(),
                source_capture_sequence: 1,
                observed_ingress_wall_ms: Some(1_000),
                observed_ingress_monotonic_ms: Some(1_000),
                slot: 1,
                tx_index: 0,
                signature: "fixture-signature".to_owned(),
                bonding_curve: Pubkey::new_from_array([7; 32]).to_string(),
                mint: None,
                initial_state: state.clone(),
            })
            .expect("birth");
        writer
            .write_trajectory(&PumpExactStateTrajectoryRecordV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                source_run_id: "fixture".to_owned(),
                source_capture_sequence: 1,
                observed_ingress_wall_ms: Some(1_000),
                observed_ingress_monotonic_ms: Some(1_000),
                slot: 1,
                tx_index: 0,
                signature: "fixture-signature".to_owned(),
                bonding_curve: Pubkey::new_from_array([7; 32]).to_string(),
                mint: None,
                effect: "supported_exact_trade".to_owned(),
                state_before: Some(state.clone()),
                state_after: Some(state),
            })
            .expect("trajectory");
        writer
            .write_coverage(&PumpExactStateCoverageRecordV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                source_run_id: "fixture".to_owned(),
                source_capture_sequence: 1,
                observed_ingress_wall_ms: Some(1_000),
                observed_ingress_monotonic_ms: Some(1_000),
                slot: 1,
                tx_index: 0,
                signature: "fixture-signature".to_owned(),
                rooted: true,
                success: true,
                occurrence_count: 1,
                candidate_count: 1,
                exact_candidate_count: 1,
                inventory_complete: true,
                reason_codes: Vec::new(),
                candidates: vec![PumpExactStateCandidateCoverageRecordV2 {
                    bonding_curve: Some(Pubkey::new_from_array([7; 32]).to_string()),
                    mint: None,
                    effect: "supported_exact_trade".to_owned(),
                    exact: true,
                    non_exact_reason: None,
                }],
            })
            .expect("coverage");
        // This row is intentionally outside the rooted capability universe.
        // It proves that coverage remains the complete diagnostic stream while
        // trajectories remain exact-only and therefore retain the literal
        // receipt line-count binding needed by a later strategy-input check.
        writer
            .write_coverage(&PumpExactStateCoverageRecordV2 {
                schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                source_run_id: "fixture".to_owned(),
                source_capture_sequence: 2,
                observed_ingress_wall_ms: Some(2_000),
                observed_ingress_monotonic_ms: Some(2_000),
                slot: 2,
                tx_index: 0,
                signature: "fixture-non-capability".to_owned(),
                rooted: false,
                success: true,
                occurrence_count: 1,
                candidate_count: 1,
                exact_candidate_count: 0,
                inventory_complete: false,
                reason_codes: vec!["outside_rooted_capability_universe".to_owned()],
                candidates: vec![PumpExactStateCandidateCoverageRecordV2 {
                    bonding_curve: Some(Pubkey::new_from_array([7; 32]).to_string()),
                    mint: None,
                    effect: "known_reserve_or_dependency_unsupported".to_owned(),
                    exact: false,
                    non_exact_reason: Some("outside_rooted_capability_universe".to_owned()),
                }],
            })
            .expect("non-capability coverage");
        writer
            .finish(
                |births, trajectories, coverage| {
                    Ok(PumpExactStateCapabilityReceiptV2 {
                        schema_version: PUMP_EXACT_STATE_CAPABILITY_SCHEMA_VERSION_V2,
                        kind: "pump_exact_state_capability_v2".to_owned(),
                        source_run_id: "fixture".to_owned(),
                        status: PumpExactStateCapabilityStatusV2::Qualified,
                        blockers: Vec::new(),
                        source_storage_format_version:
                            PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
                        source_raw_segment_set_blake3: "33".repeat(32),
                        source_start_manifest_digest: test_digest(),
                        source_completion_receipt_digest: test_digest(),
                        semantics_id: "fixture-semantics".to_owned(),
                        semantics_manifest_digest: test_digest(),
                        vendored_idl_digest: test_digest(),
                        materializer_running_executable_digest: test_digest(),
                        cohort_slots_strictly_after: 0,
                        rooted_canonical_slot_count: 1,
                        filtered_pump_transaction_count: 2,
                        full_block_pump_transaction_count: 1,
                        pump_owned_account_update_count: 1,
                        qualification_run_below_minimum: false,
                        bonding_curve_account_count: 1,
                        bonding_curve_decoded_count: 1,
                        global_account_count: 1,
                        global_validated_count: 1,
                        unknown_pump_owned_account_count: 0,
                        account_decode_failure_count: 0,
                        successful_rooted_instruction_occurrence_count: 1,
                        successful_rooted_proven_non_reserve_count: 0,
                        successful_rooted_validated_event_transport_count: 0,
                        successful_rooted_candidate_count: 1,
                        successful_rooted_unknown_occurrence_count: 0,
                        successful_rooted_malformed_candidate_count: 0,
                        occurrence_ledger_reconciled: true,
                        successful_rooted_mutation_denominator: 1,
                        exact_rooted_mutation_count: 1,
                        explicit_non_exact_mutation_count: 0,
                        denominator_reconciled: true,
                        exact_rooted_coverage_ppm: 1_000_000,
                        required_exact_rooted_coverage_ppm:
                            PUMP_EXACT_STATE_REQUIRED_COVERAGE_PPM_V2,
                        exact_trajectory_count: 1,
                        successful_rooted_exact_trade_with_both_states_count: 1,
                        exact_birth_count: 1,
                        births_artifact: births,
                        trajectories_artifact: trajectories,
                        coverage_artifact: coverage,
                    })
                },
                |receipt, births, trajectories, coverage| {
                    Ok(PumpExactStateExactManifestV2 {
                        schema_version: PUMP_EXACT_STATE_EXACT_OUTPUT_SCHEMA_VERSION_V2,
                        kind: "pump_exact_state_tape_v2".to_owned(),
                        source_run_id: "fixture".to_owned(),
                        exact_state_capability_status: PumpExactStateCapabilityStatusV2::Qualified,
                        source_raw_segment_set_blake3: "33".repeat(32),
                        semantics_manifest_sha256: "11".repeat(32),
                        semantics_manifest_blake3: "22".repeat(32),
                        materializer_running_executable_sha256: "11".repeat(32),
                        materializer_running_executable_blake3: "22".repeat(32),
                        materializer_running_executable_bytes: 1,
                        exact_state_capability_artifact: receipt,
                        births_artifact: births,
                        trajectories_artifact: trajectories,
                        coverage_artifact: coverage,
                    })
                },
                || Ok(()),
            )
            .expect("publish Qualified output");

        let authority = validate_exact_output_artifacts_v2(&output_dir)
            .expect("Qualified output must pass exact artifact authority");
        revalidate_open_exact_artifact_v2(
            &authority.trajectories_artifact.file,
            &authority.trajectories_artifact.digest,
            "V2 exact trajectories JSONL",
        )
        .expect("unchanged descriptor must retain authority");
        fs::write(output_dir.join("trajectories_v2.jsonl"), b"{}\n")
            .expect("mutate only temporary fixture");
        assert!(
            revalidate_open_exact_artifact_v2(
                &authority.trajectories_artifact.file,
                &authority.trajectories_artifact.digest,
                "V2 exact trajectories JSONL",
            )
            .is_err(),
            "late in-place JSONL replacement must invalidate the retained descriptor authority"
        );
    }
}
