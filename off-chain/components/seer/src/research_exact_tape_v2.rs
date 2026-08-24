//! Standalone prospective Exact-State Pump Research Tape V2 capture helpers.
//!
//! This module is intentionally separate from `research_tape` (the immutable
//! GO-D V1 capture path).  V2 records a wider raw source universe for a future
//! prospective cohort and does not reinterpret, overwrite, or certify the
//! historical tape.
//!
//! The first boundary in this module converts a decoded Yellowstone source
//! update into the V2 raw contract.  It performs no semantic Pump instruction
//! parsing, no account-state reconstruction, no RPC backfill, and no active
//! Seer runtime work.

use crate::{
    grpc_connection::{
        pump_research_exact_state_v2_subscription_request_fingerprint_blake3,
        PumpResearchSourceConnectionV1, PumpResearchSourceSinkV1, PumpResearchSourceUpdateV1,
        BONDING_CURVE_DISC, PUMP_FUN_PROGRAM_ID,
        PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES,
    },
    local_gap::LocalGapTracker,
    research_exact_tape_v2_semantics::{
        load_pump_exact_state_semantics_authority_v2, PumpExactStateSemanticsAuthorityV2,
        PumpExactStateSemanticsDigestV2,
    },
};
use anyhow::{bail, Context, Result};
use ghost_core::{
    pump_research_exact_tape_v2::{
        PumpExactStateAccountEvidenceClassV2, PumpExactStateBlockMetaEvidenceV2,
        PumpExactStateCoverageBoundaryV2, PumpExactStateCoverageGapReasonV2,
        PumpExactStateCoverageGapV2, PumpExactStateFullBlockPayloadChunkV2,
        PumpExactStateFullBlockPayloadCompletedV2, PumpExactStateFullBlockPayloadStartedV2,
        PumpExactStateProspectiveStreamBoundaryV2, PumpExactStateProviderRoleV2,
        PumpExactStatePumpOwnedAccountUpdateV2, PumpExactStateRawRecordV2,
        PumpExactStateSlotEvidenceV2, PumpExactStateSourceEnvelopeV2,
        PumpExactStateSourceReadinessV2, PumpExactStateTransactionEvidenceV2,
    },
    pump_research_tape::{
        PumpProgramDataReceiptV1, PumpResearchEventTimeV1, PumpResearchStoragePubkeyV1,
        PumpResearchStorageSignatureV1, PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1,
        PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1,
    },
    LocalCoverageBoundaryV1, LocalCoverageGapReasonV1, LocalCoverageGapV1,
};
use parking_lot::Mutex;
use prost::Message;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_account_decoder::UiAccountEncoding;
use solana_client::rpc_config::RpcAccountInfoConfig;
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::Signature,
};
use std::{
    collections::BTreeMap,
    env,
    fs::{self, File, OpenOptions},
    io::{BufWriter, Write},
    path::{Path, PathBuf},
    process::Command,
    str::FromStr,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tempfile::Builder as TempDirBuilder;
use tokio_util::sync::CancellationToken;
use yellowstone_grpc_proto::prelude::{subscribe_update::UpdateOneof, SubscribeUpdate};

#[cfg(unix)]
use std::os::unix::{
    ffi::OsStrExt,
    fs::FileExt,
    fs::{DirBuilderExt, MetadataExt, OpenOptionsExt, PermissionsExt},
};

pub(crate) const EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION: u16 = 3;
pub(crate) const EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION: u16 = 3;
const DEFAULT_V2_SOURCE_QUEUE_CAPACITY: usize = 8_192;
const DEFAULT_V2_SOURCE_QUEUE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MIN_V2_SOURCE_QUEUE_MAX_BYTES: u64 =
    PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES as u64 + 1024;
/// A V2 cohort must have a finite, explicitly recorded prospective window.
/// This is deliberately a capture boundary rather than a qualification
/// threshold: an otherwise clean raw run remains only raw evidence.
const DEFAULT_V2_COHORT_CAPTURE_WALL_MS: u64 = 3_600_000;
const MIN_V2_COHORT_CAPTURE_WALL_MS: u64 = 60_000;
const MAX_V2_COHORT_CAPTURE_WALL_MS: u64 = 86_400_000;
/// Reserve retained on the filesystem containing the prospective V2 output.
/// The operator may raise it in the hash-pinned capture configuration.
const DEFAULT_V2_MIN_FREE_BYTES: u64 = 14_000_000_000;
const MIN_V2_MIN_FREE_BYTES: u64 = 1_000_000_000;
/// Total raw V2 bytes that the writer is permitted to persist.  Together with
/// `min_free_bytes`, this lets preflight prove enough local storage before a
/// provider connection is opened.
const DEFAULT_V2_MAX_RAW_BYTES: u64 = 16_000_000_000;
const MIN_V2_MAX_RAW_BYTES: u64 = 1_000_000_000;
const MAX_V2_MAX_RAW_BYTES: u64 = 512_000_000_000;
/// Bounded room for start/completion JSON plus the segment receipt list.  It
/// is reserved before provider I/O, so reaching the raw byte cap cannot make
/// a otherwise-valid run lose its final authority receipts to ENOSPC.
const V2_CAPTURE_METADATA_ALLOWANCE_BYTES: u64 = 64 * 1024 * 1024;
/// Reserve inside `segment_max_bytes` for the terminal V2 footer frame.  The
/// frozen V2 footer is materially smaller than this value; keeping an
/// explicit cap means a segment limit remains a physical-file limit rather
/// than a pre-footer advisory target.
const V2_SEGMENT_FOOTER_RESERVE_BYTES: u64 = 4 * 1024;
const MIN_V2_SEGMENT_MAX_BYTES: u64 =
    ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_RECORD_MAX_BYTES_V2 as u64
        + 64 * 1024;
const V2_STORAGE_FLOOR_CHECK_INTERVAL_MS: u64 = 1_000;
/// A writer turn drains a finite number of source/control entries before it
/// gives the stream-readiness control plane a chance to persist one record.
/// Without this fairness bound a permanently busy source channel could starve
/// the readiness boundary indefinitely.
const V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE: usize = 256;
const DEFAULT_V2_SOURCE_READINESS_TIMEOUT_MS: u64 = 30_000;
const MAX_V2_SOURCE_READINESS_TIMEOUT_MS: u64 = 600_000;
const MAX_V2_SOURCE_QUEUE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_V2_FLUSH_INTERVAL_MS: u64 = 1_000;
const DEFAULT_V2_SEGMENT_MAX_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_V2_SEGMENT_MAX_DURATION_MS: u64 = 300_000;
const V2_CAPTURE_CONFIG_MAX_BYTES: u64 = 128 * 1024;
/// Atomic sentinel for a required source lane that has not yet admitted an
/// update.  Slot `u64::MAX` is rejected as malformed source evidence, so the
/// sentinel cannot be confused with an observed lane slot.
const V2_REQUIRED_LANE_SLOT_UNSET: u64 = u64::MAX;
/// High-bit lock used only while the control plane reserves the one
/// stream-readiness marker in the shared capture-order domain.  Normal source
/// sequence values are always below this bit, so the writer can distinguish a
/// transient reservation from a real source sequence without a mutex on the
/// Yellowstone receive path.
const V2_CAPTURE_SEQUENCE_RESERVING_BIT: u64 = 1_u64 << 63;
const V2_READINESS_BOUNDARY_SEQUENCE_UNSET: u64 = u64::MAX;

/// Dedicated configuration for a prospective Exact-State Tape V2 run.
///
/// It is deliberately not embedded in `SeerConfig`: no active Ghost runtime
/// source, candidate, or execution behavior can select it.  The small
/// ProgramData RPC is used only for start/end program-data receipts; it is
/// never an account-state snapshot, a backfill source, or a repair path.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PumpExactStateCaptureConfigV2 {
    pub primary_provider_id: String,
    pub grpc_endpoint: String,
    #[serde(default)]
    pub grpc_auth_token_env: Option<String>,
    #[serde(default = "default_v2_grpc_auth_header")]
    pub grpc_auth_header: String,
    pub program_data_rpc_endpoint: String,
    #[serde(default)]
    pub program_data_rpc_auth_token_env: Option<String>,
    #[serde(default = "default_v2_rpc_auth_header")]
    pub program_data_rpc_auth_header: String,
    #[serde(default = "default_v2_pump_program_id")]
    pub pump_program_id: String,
    /// Create-time semantic authority for this prospective capture.  The
    /// preflight hashes this manifest and its vendored IDL, and capture checks
    /// its ProgramData selection before it creates a raw V2 directory.  It is
    /// deliberately outside the active runtime and contains no credentials.
    pub semantics_manifest_path: PathBuf,
    /// Dedicated parent directory for create-new V2 runs.  It must not be a
    /// raw directory from GO-D or any other frozen tape.
    pub output_dir: PathBuf,
    #[serde(default = "default_v2_required_for_run")]
    pub required_for_run: bool,
    #[serde(default = "default_v2_source_queue_capacity")]
    pub source_queue_capacity: usize,
    /// Independent byte ceiling for deterministic serialized `SubscribeUpdate`
    /// payloads awaiting the writer.  V2 captures full blocks, so a bounded
    /// item count alone is not a memory-safety contract.
    #[serde(default = "default_v2_source_queue_max_bytes")]
    pub source_queue_max_bytes: u64,
    /// Maximum duration of the prospective cohort after the stream-readiness
    /// boundary has been durably sealed.  A wall-deadline is a
    /// deliberate clean stop; an unexpected source exit is not.
    #[serde(default = "default_v2_cohort_capture_wall_ms")]
    pub cohort_capture_wall_ms: u64,
    /// Bytes that must remain free on the output filesystem throughout V2
    /// capture.  Falling below this reserve is a typed fail-closed writer
    /// failure, never an implicit invitation to keep filling the volume.
    #[serde(default = "default_v2_min_free_bytes")]
    pub min_free_bytes: u64,
    /// Absolute raw V2 byte budget for this one prospective run.  It is
    /// checked before the provider is contacted and enforced by the writer.
    #[serde(default = "default_v2_max_raw_bytes")]
    pub max_raw_bytes: u64,
    /// Bound on waiting for all five persisted source lanes before the one
    /// stream-only cohort boundary is accepted.
    #[serde(default = "default_v2_source_readiness_timeout_ms")]
    pub source_readiness_timeout_ms: u64,
    #[serde(default = "default_v2_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_v2_segment_max_bytes")]
    pub segment_max_bytes: u64,
    #[serde(default = "default_v2_segment_max_duration_ms")]
    pub segment_max_duration_ms: u64,
}

impl PumpExactStateCaptureConfigV2 {
    pub fn load(path: &Path) -> Result<(Self, Vec<u8>)> {
        let bytes = read_private_regular_file_v2(
            path,
            "prospective V2 capture config",
            V2_CAPTURE_CONFIG_MAX_BYTES,
        )?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("V2 capture config {} is not UTF-8 TOML", path.display()))?;
        let config: Self = toml::from_str(text)
            .with_context(|| format!("parse prospective V2 capture config {}", path.display()))?;
        config.validate()?;
        Ok((config, bytes))
    }

    pub fn validate(&self) -> Result<()> {
        validate_v2_identifier("primary_provider_id", &self.primary_provider_id, 256)?;
        validate_v2_endpoint("grpc_endpoint", &self.grpc_endpoint)?;
        validate_v2_trimmed("grpc_auth_header", &self.grpc_auth_header)?;
        validate_v2_endpoint("program_data_rpc_endpoint", &self.program_data_rpc_endpoint)?;
        validate_v2_trimmed(
            "program_data_rpc_auth_header",
            &self.program_data_rpc_auth_header,
        )?;
        if let Some(name) = &self.grpc_auth_token_env {
            validate_v2_trimmed("grpc_auth_token_env", name)?;
        }
        if let Some(name) = &self.program_data_rpc_auth_token_env {
            validate_v2_trimmed("program_data_rpc_auth_token_env", name)?;
        }
        if self.output_dir.as_os_str().is_empty() {
            bail!("V2 output_dir must not be empty");
        }
        if self.semantics_manifest_path.as_os_str().is_empty() {
            bail!("V2 semantics_manifest_path must not be empty");
        }
        if self.output_dir.components().any(|component| {
            matches!(component, std::path::Component::Normal(name) if name == "raw" || name == "raw-v2")
        }) {
            bail!(
                "V2 output_dir must be a dedicated parent, never an existing raw/raw-v2 tape directory"
            );
        }
        validate_v2_output_root_isolated(&self.output_dir)?;
        if self.source_queue_capacity == 0 {
            bail!("V2 source_queue_capacity must be greater than zero");
        }
        if self.source_queue_max_bytes < MIN_V2_SOURCE_QUEUE_MAX_BYTES
            || self.source_queue_max_bytes > MAX_V2_SOURCE_QUEUE_MAX_BYTES
        {
            bail!(
                "V2 source_queue_max_bytes must be in {}..={}, got {}",
                MIN_V2_SOURCE_QUEUE_MAX_BYTES,
                MAX_V2_SOURCE_QUEUE_MAX_BYTES,
                self.source_queue_max_bytes
            );
        }
        if !(MIN_V2_COHORT_CAPTURE_WALL_MS..=MAX_V2_COHORT_CAPTURE_WALL_MS)
            .contains(&self.cohort_capture_wall_ms)
        {
            bail!(
                "V2 cohort_capture_wall_ms must be in {}..={}, got {}",
                MIN_V2_COHORT_CAPTURE_WALL_MS,
                MAX_V2_COHORT_CAPTURE_WALL_MS,
                self.cohort_capture_wall_ms
            );
        }
        if self.min_free_bytes < MIN_V2_MIN_FREE_BYTES {
            bail!(
                "V2 min_free_bytes must be at least {}, got {}",
                MIN_V2_MIN_FREE_BYTES,
                self.min_free_bytes
            );
        }
        if !(MIN_V2_MAX_RAW_BYTES..=MAX_V2_MAX_RAW_BYTES).contains(&self.max_raw_bytes) {
            bail!(
                "V2 max_raw_bytes must be in {}..={}, got {}",
                MIN_V2_MAX_RAW_BYTES,
                MAX_V2_MAX_RAW_BYTES,
                self.max_raw_bytes
            );
        }
        self.min_free_bytes
            .checked_add(self.max_raw_bytes)
            .ok_or_else(|| anyhow::anyhow!("V2 min_free_bytes + max_raw_bytes overflows u64"))?;
        if !self.required_for_run {
            bail!("V2 required_for_run must remain true; prospective exact evidence cannot be optional");
        }
        for (name, value) in [
            (
                "source_readiness_timeout_ms",
                self.source_readiness_timeout_ms,
            ),
            ("flush_interval_ms", self.flush_interval_ms),
            ("segment_max_duration_ms", self.segment_max_duration_ms),
        ] {
            if value == 0 {
                bail!("V2 {name} must be greater than zero");
            }
        }
        if self.source_readiness_timeout_ms > MAX_V2_SOURCE_READINESS_TIMEOUT_MS {
            bail!(
                "V2 source_readiness_timeout_ms {} exceeds hard maximum {}",
                self.source_readiness_timeout_ms,
                MAX_V2_SOURCE_READINESS_TIMEOUT_MS
            );
        }
        if self.segment_max_bytes < MIN_V2_SEGMENT_MAX_BYTES
            || self.segment_max_bytes > self.max_raw_bytes
        {
            bail!(
                "V2 segment_max_bytes must be in {}..={} for this V2 raw budget, got {}",
                MIN_V2_SEGMENT_MAX_BYTES,
                self.max_raw_bytes,
                self.segment_max_bytes,
            );
        }
        let configured_program = self
            .pump_program_id
            .parse::<Pubkey>()
            .context("V2 pump_program_id is not a valid Solana pubkey")?;
        let expected_program = PUMP_FUN_PROGRAM_ID
            .parse::<Pubkey>()
            .context("compiled Pump program ID is invalid")?;
        if configured_program != expected_program {
            bail!(
                "V2 pump_program_id {} differs from the declared Pump program {}",
                configured_program,
                expected_program
            );
        }
        Ok(())
    }

    fn resolve_grpc_auth_token(&self) -> Result<Option<String>> {
        resolve_v2_optional_env("gRPC", self.grpc_auth_token_env.as_deref())
    }

    fn resolve_program_data_rpc_auth_token(&self) -> Result<Option<String>> {
        resolve_v2_optional_env(
            "ProgramData RPC",
            self.program_data_rpc_auth_token_env.as_deref(),
        )
    }
}

const V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION: u16 = 4;
const V2_OPERATOR_PREFLIGHT_RECEIPT_FILE: &str = "operator_preflight_receipt_v2.json";
const V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES: u64 = 64 * 1024;
const V2_OPERATOR_PREFLIGHT_RELEASE_DIR: &str = "release";
const V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE: &str = "release/pump-exact-state-tape-v2";
const V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE: &str = "release/build.log";
const V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE: &str = "release/build_receipt_v2.json";
const V2_OPERATOR_PREFLIGHT_BUILD_LOG_MAX_BYTES: u64 = 64 * 1024 * 1024;
const V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_MAX_BYTES: u64 = 128 * 1024 * 1024;
const V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS: &str =
    "fresh_locked_offline_release_from_clean_commit_with_isolated_target_and_operator_credentials_removed_v1";
const V2_OPERATOR_PREFLIGHT_DENIED_BUILD_ENVIRONMENT: &[&str] = &[
    "RUSTC",
    "RUSTC_WRAPPER",
    "RUSTC_WORKSPACE_WRAPPER",
    "CARGO_BUILD_RUSTC",
    "CARGO_BUILD_RUSTC_WRAPPER",
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "CARGO_BUILD_JOBS",
    "CARGO_INCREMENTAL",
];

/// Local-only inspected preflight material.  It does not open gRPC, invoke
/// JSON-RPC, resolve credential values, create an output directory, or touch
/// GO-D.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateCapturePreflightSummaryV2 {
    pub capture_config_digest: PumpExactStateDigestV2,
    pub running_executable_digest: PumpExactStateDigestV2,
    pub source_request_fingerprint_blake3: String,
    pub source_capture_semantics: String,
    pub semantics_id: String,
    pub semantics_manifest_digest: PumpExactStateDigestV2,
    pub vendored_idl_digest: PumpExactStateDigestV2,
    pub expected_program_data_hash_blake3: String,
}

pub fn preflight_prospective_exact_state_capture_v2_from_config_path(
    config_path: &Path,
) -> Result<PumpExactStateCapturePreflightSummaryV2> {
    let (config, config_bytes) = PumpExactStateCaptureConfigV2::load(config_path)?;
    let semantics = load_pump_exact_state_semantics_authority_v2(&config.semantics_manifest_path)
        .context("load V2 semantics authority during local-only preflight")?;
    require_v2_capture_storage_budget(
        &config.output_dir,
        config.min_free_bytes,
        config.max_raw_bytes,
    )?;
    Ok(PumpExactStateCapturePreflightSummaryV2 {
        capture_config_digest: digest_bytes_v2(&config_bytes),
        running_executable_digest: digest_running_executable_v2()?,
        source_request_fingerprint_blake3: hex_bytes_v2(
            &pump_research_exact_state_v2_subscription_request_fingerprint_blake3(),
        ),
        source_capture_semantics:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2
                .to_owned(),
        semantics_id: semantics.semantics_id.clone(),
        semantics_manifest_digest: semantics_digest_to_capture_v2(&semantics.manifest_digest),
        vendored_idl_digest: semantics_digest_to_capture_v2(&semantics.idl_digest),
        expected_program_data_hash_blake3: hex_bytes_v2(
            &semantics.expected_program_data_hash_blake3(),
        ),
    })
}

/// Fresh offline build evidence retained inside one create-new preflight
/// bundle.  It intentionally claims a clean-commit locked offline build, not
/// a broader sealed-source contract owned by the historical V1 flow.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateFreshBuildReceiptV2 {
    schema_version: u16,
    build_semantics: String,
    repository_commit: String,
    cargo_command: Vec<String>,
    build_log_digest: PumpExactStateDigestV2,
    release_binary_digest: PumpExactStateDigestV2,
    build_started_wall_ms: u64,
    build_completed_wall_ms: u64,
}

/// Hash-pinned local authority required before a V2 capture may start.  It
/// binds the clean repository commit, config bytes, concrete V2 request and a
/// fresh offline release binary copied into this immutable preflight bundle.
/// Capture must execute that copied binary; the preflight executable that
/// created the bundle has no capture authority by itself.  No field contains
/// endpoint or credential bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpExactStateOperatorPreflightReceiptV2 {
    schema_version: u16,
    receipt_kind: String,
    created_wall_ms: u64,
    repository_commit: String,
    git_status_digest: PumpExactStateDigestV2,
    git_status_entry_count: u64,
    capture_config_digest: PumpExactStateDigestV2,
    preflight_executable_digest: PumpExactStateDigestV2,
    release_binary_file: String,
    release_binary_digest: PumpExactStateDigestV2,
    build_log_file: String,
    build_log_digest: PumpExactStateDigestV2,
    build_receipt_file: String,
    build_receipt_digest: PumpExactStateDigestV2,
    build_semantics: String,
    source_request_fingerprint_blake3: String,
    source_capture_semantics: String,
    source_max_decoded_message_bytes: u64,
    semantics_id: String,
    semantics_manifest_digest: PumpExactStateDigestV2,
    vendored_idl_digest: PumpExactStateDigestV2,
    expected_program_data_hash_blake3: String,
    cohort_capture_wall_ms: u64,
    min_free_bytes: u64,
    max_raw_bytes: u64,
    required_storage_bytes: u64,
    output_filesystem_available_bytes_at_preflight: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpExactStateOperatorPreflightSummaryV2 {
    pub bundle_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub receipt_digest: PumpExactStateDigestV2,
    pub sealed_release_binary_digest: PumpExactStateDigestV2,
}

/// Authority retained only after every file in the preflight bundle has been
/// read through the bounded no-follow reader and tied to the kernel-bound
/// image of this process.  A capture consumes this value rather than reading
/// Git state or `target/release` again.
#[derive(Clone, Debug)]
struct PumpExactStateValidatedOperatorPreflightV2 {
    receipt: PumpExactStateOperatorPreflightReceiptV2,
    receipt_digest: PumpExactStateDigestV2,
    semantics: PumpExactStateSemanticsAuthorityV2,
}

/// Create a new local V2 preflight authority bundle.  A prospective capture
/// cannot use a debug executable or a dirty worktree: those are explicit
/// operator failures, not information the raw run may paper over later.
pub fn create_operator_preflight_v2_from_config_path(
    config_path: &Path,
    output_dir: &Path,
) -> Result<PumpExactStateOperatorPreflightSummaryV2> {
    require_release_v2_operator_binary()?;
    let (config, config_bytes) = PumpExactStateCaptureConfigV2::load(config_path)?;
    let semantics = load_pump_exact_state_semantics_authority_v2(&config.semantics_manifest_path)
        .context("load V2 semantics authority for operator preflight")?;
    let repository_root = repository_root_v2()?;
    let git_status = require_clean_repository_at_v2(&repository_root)?;
    let output_filesystem_available_bytes_at_preflight = require_v2_capture_storage_budget(
        &config.output_dir,
        config.min_free_bytes,
        config.max_raw_bytes,
    )?;
    let repository_commit = repository_commit_at_v2(&repository_root)?;
    let preflight_executable_digest = digest_running_executable_v2()?;
    validate_v2_new_operator_artifact_path(output_dir, "V2 operator preflight output")?;
    let bundle_dir = output_dir;
    create_private_directory_v2(bundle_dir).with_context(|| {
        format!(
            "create new V2 operator preflight directory {}",
            bundle_dir.display()
        )
    })?;
    let fresh_build = build_fresh_release_into_preflight_bundle_v2(
        bundle_dir,
        &config,
        &repository_root,
        &repository_commit,
    )?;
    let receipt = PumpExactStateOperatorPreflightReceiptV2 {
        schema_version: V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION,
        receipt_kind: "pump_exact_state_operator_preflight_v2".to_owned(),
        created_wall_ms: wall_clock_ms_v2(),
        repository_commit,
        git_status_entry_count: git_status_entry_count_v2(&git_status),
        git_status_digest: digest_bytes_v2(&git_status),
        capture_config_digest: digest_bytes_v2(&config_bytes),
        preflight_executable_digest,
        release_binary_file: V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE.to_owned(),
        release_binary_digest: fresh_build.release_binary_digest.clone(),
        build_log_file: V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE.to_owned(),
        build_log_digest: fresh_build.build_log_digest.clone(),
        build_receipt_file: V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE.to_owned(),
        build_receipt_digest: fresh_build.build_receipt_digest.clone(),
        build_semantics: V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS.to_owned(),
        source_request_fingerprint_blake3: hex_bytes_v2(
            &pump_research_exact_state_v2_subscription_request_fingerprint_blake3(),
        ),
        source_capture_semantics:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2
                .to_owned(),
        source_max_decoded_message_bytes:
            PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES as u64,
        semantics_id: semantics.semantics_id.clone(),
        semantics_manifest_digest: semantics_digest_to_capture_v2(&semantics.manifest_digest),
        vendored_idl_digest: semantics_digest_to_capture_v2(&semantics.idl_digest),
        expected_program_data_hash_blake3: hex_bytes_v2(
            &semantics.expected_program_data_hash_blake3(),
        ),
        cohort_capture_wall_ms: config.cohort_capture_wall_ms,
        min_free_bytes: config.min_free_bytes,
        max_raw_bytes: config.max_raw_bytes,
        required_storage_bytes: required_v2_storage_bytes(
            config.min_free_bytes,
            config.max_raw_bytes,
        )?,
        output_filesystem_available_bytes_at_preflight,
    };
    let receipt_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_RECEIPT_FILE);
    write_json_create_new_v2(&receipt_path, &receipt)?;
    let receipt_bytes = read_bounded_regular_file_v2(
        &receipt_path,
        "V2 operator preflight receipt",
        V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES,
    )?;
    Ok(PumpExactStateOperatorPreflightSummaryV2 {
        bundle_dir: bundle_dir.to_path_buf(),
        receipt_path,
        receipt_digest: digest_bytes_v2(&receipt_bytes),
        sealed_release_binary_digest: receipt.release_binary_digest,
    })
}

fn require_release_v2_operator_binary() -> Result<()> {
    if cfg!(debug_assertions) {
        bail!(
            "V2 operator preflight/capture requires a release binary; invoke target/release/pump-exact-state-tape-v2"
        );
    }
    Ok(())
}

fn require_clean_repository_at_v2(repository_root: &Path) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .current_dir(repository_root)
        .args(["status", "--porcelain=v1", "-z"])
        .output()
        .context("invoke git status for V2 operator preflight")?;
    if !output.status.success() {
        bail!("git status failed during V2 operator preflight");
    }
    if !output.stdout.is_empty() {
        bail!(
            "V2 operator preflight requires a clean worktree; commit or isolate the prospective capture implementation first"
        );
    }
    Ok(output.stdout)
}

fn git_status_entry_count_v2(status: &[u8]) -> u64 {
    status.iter().filter(|byte| **byte == 0).count() as u64
}

fn validate_v2_fresh_build_environment() -> Result<()> {
    let present: Vec<&str> = V2_OPERATOR_PREFLIGHT_DENIED_BUILD_ENVIRONMENT
        .iter()
        .copied()
        .filter(|name| env::var_os(name).is_some())
        .collect();
    if !present.is_empty() {
        bail!(
            "V2 operator preflight rejects unsealed compiler/build environment overrides: {}",
            present.join(", ")
        );
    }
    Ok(())
}

fn write_bytes_create_new_v2(path: &Path, bytes: &[u8], mode: u32, label: &str) -> Result<()> {
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(mode);
    let mut file = options
        .open(path)
        .with_context(|| format!("create {label} {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {label} {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {label} {}", path.display()))?;
    sync_directory_v2(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("{label} path has no parent"))?,
    )?;
    Ok(())
}

fn copy_fresh_release_binary_into_bundle_v2(
    source: &Path,
    destination: &Path,
) -> Result<PumpExactStateDigestV2> {
    #[cfg(unix)]
    {
        let metadata = fs::symlink_metadata(source)
            .with_context(|| format!("inspect fresh V2 release binary {}", source.display()))?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.permissions().mode() & 0o111 == 0
        {
            bail!(
                "fresh V2 release binary {} must be an executable regular non-symlink file",
                source.display()
            );
        }
    }
    let bytes = read_bounded_regular_file_v2(
        source,
        "fresh V2 release binary",
        V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_MAX_BYTES,
    )?;
    if bytes.is_empty() {
        bail!("fresh V2 release binary is empty");
    }
    write_bytes_create_new_v2(destination, &bytes, 0o700, "copied fresh V2 release binary")?;
    Ok(digest_bytes_v2(&bytes))
}

struct PumpExactStateFreshBuildOutputV2 {
    release_binary_digest: PumpExactStateDigestV2,
    build_log_digest: PumpExactStateDigestV2,
    build_receipt_digest: PumpExactStateDigestV2,
}

/// Build a clean-commit V2 binary into a fresh target directory, then copy its
/// bytes into the preflight bundle.  The temporary target avoids accepting a
/// stale `target/release` artifact as capture authority.  The clean Git commit
/// is rechecked after build so the command cannot race a local source change.
fn build_fresh_release_into_preflight_bundle_v2(
    bundle_dir: &Path,
    config: &PumpExactStateCaptureConfigV2,
    repository_root: &Path,
    repository_commit: &str,
) -> Result<PumpExactStateFreshBuildOutputV2> {
    validate_v2_fresh_build_environment()?;
    let release_dir = bundle_dir.join(V2_OPERATOR_PREFLIGHT_RELEASE_DIR);
    create_private_directory_v2(&release_dir).with_context(|| {
        format!(
            "create V2 preflight release directory {}",
            release_dir.display()
        )
    })?;
    let staging = TempDirBuilder::new()
        .prefix("pump-exact-state-tape-v2-build-")
        .tempdir()
        .context("create isolated V2 fresh-build target directory")?;
    let target_dir = staging.path().join("target");
    let command = fresh_build_cargo_command_v2();
    let build_started_wall_ms = wall_clock_ms_v2();
    let mut cargo = Command::new("cargo");
    cargo
        .current_dir(repository_root)
        .args(&command[1..])
        .env("CARGO_TARGET_DIR", &target_dir);
    for name in [
        config.grpc_auth_token_env.as_deref(),
        config.program_data_rpc_auth_token_env.as_deref(),
        Some("GHOST_PUMP_RESEARCH_GRPC_TOKEN"),
        Some("GHOST_PUMP_RESEARCH_RPC_TOKEN"),
        Some("GHOST_PUMP_RESEARCH_AUDIT_RPC_TOKEN"),
        Some("GHOST_SEER_GRPC_X_TOKEN"),
        Some("GHOST_RPC_AUTH_TOKEN"),
    ]
    .into_iter()
    .flatten()
    {
        cargo.env_remove(name);
    }
    let output = cargo
        .output()
        .context("run fresh locked offline V2 release build")?;
    let build_completed_wall_ms = wall_clock_ms_v2();
    let mut build_log = Vec::new();
    build_log.extend_from_slice(b"[stdout]\n");
    build_log.extend_from_slice(&output.stdout);
    build_log.extend_from_slice(b"\n[stderr]\n");
    build_log.extend_from_slice(&output.stderr);
    let build_log_len =
        u64::try_from(build_log.len()).context("V2 fresh-build log length does not fit u64")?;
    if build_log_len > V2_OPERATOR_PREFLIGHT_BUILD_LOG_MAX_BYTES {
        bail!(
            "V2 fresh-build log exceeded bounded maximum {} bytes",
            V2_OPERATOR_PREFLIGHT_BUILD_LOG_MAX_BYTES
        );
    }
    let build_log_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE);
    write_bytes_create_new_v2(&build_log_path, &build_log, 0o600, "V2 fresh-build log")?;
    let build_log_digest = digest_bytes_v2(&build_log);
    if !output.status.success() {
        bail!(
            "fresh locked offline V2 release build failed with status {}",
            output.status
        );
    }
    let git_status = require_clean_repository_at_v2(repository_root)?;
    if !git_status.is_empty() || repository_commit_at_v2(repository_root)? != repository_commit {
        bail!("V2 repository changed while the fresh release binary was being built");
    }
    let built_binary = target_dir.join("release/pump-exact-state-tape-v2");
    let release_binary_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE);
    let release_binary_digest =
        copy_fresh_release_binary_into_bundle_v2(&built_binary, &release_binary_path)?;
    let build_receipt = PumpExactStateFreshBuildReceiptV2 {
        schema_version: V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION,
        build_semantics: V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS.to_owned(),
        repository_commit: repository_commit.to_owned(),
        cargo_command: command,
        build_log_digest: build_log_digest.clone(),
        release_binary_digest: release_binary_digest.clone(),
        build_started_wall_ms,
        build_completed_wall_ms,
    };
    let build_receipt_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE);
    write_json_create_new_v2(&build_receipt_path, &build_receipt)?;
    let build_receipt_bytes = read_bounded_regular_file_v2(
        &build_receipt_path,
        "V2 fresh-build receipt",
        V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES,
    )?;
    Ok(PumpExactStateFreshBuildOutputV2 {
        release_binary_digest,
        build_log_digest,
        build_receipt_digest: digest_bytes_v2(&build_receipt_bytes),
    })
}

fn fresh_build_cargo_command_v2() -> Vec<String> {
    vec![
        "cargo".to_owned(),
        "build".to_owned(),
        "--locked".to_owned(),
        "--offline".to_owned(),
        "--release".to_owned(),
        "-p".to_owned(),
        "seer".to_owned(),
        "--bin".to_owned(),
        "pump-exact-state-tape-v2".to_owned(),
    ]
}

fn validate_operator_preflight_v2(
    config: &PumpExactStateCaptureConfigV2,
    config_bytes: &[u8],
    receipt_path: &Path,
) -> Result<PumpExactStateValidatedOperatorPreflightV2> {
    require_release_v2_operator_binary()?;
    if receipt_path.file_name().and_then(|name| name.to_str())
        != Some(V2_OPERATOR_PREFLIGHT_RECEIPT_FILE)
    {
        bail!("V2 operator preflight receipt must be named {V2_OPERATOR_PREFLIGHT_RECEIPT_FILE}");
    }
    let bundle_dir = receipt_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("V2 operator preflight receipt has no parent bundle"))?;
    require_existing_private_directory_v2(bundle_dir, "V2 operator preflight bundle")?;
    validate_v2_existing_directory_outside_raw_authority(
        bundle_dir,
        "V2 operator preflight bundle",
    )?;
    let receipt_bytes = read_bounded_regular_file_v2(
        receipt_path,
        "V2 operator preflight receipt",
        V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES,
    )?;
    let receipt: PumpExactStateOperatorPreflightReceiptV2 = serde_json::from_slice(&receipt_bytes)
        .with_context(|| {
            format!(
                "parse V2 operator preflight receipt {}",
                receipt_path.display()
            )
        })?;
    if receipt.schema_version != V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION
        || receipt.receipt_kind != "pump_exact_state_operator_preflight_v2"
    {
        bail!("V2 operator preflight receipt schema/kind is not accepted");
    }
    validate_operator_preflight_bundle_contents_v2(bundle_dir, &receipt)?;
    if receipt.capture_config_digest != digest_bytes_v2(config_bytes) {
        bail!("V2 operator preflight capture config digest does not match current config bytes");
    }
    let semantics = load_pump_exact_state_semantics_authority_v2(&config.semantics_manifest_path)
        .context("reload V2 semantics authority before capture")?;
    validate_preflight_semantics_binding_v2(&receipt, &semantics)?;
    let required_storage_bytes =
        required_v2_storage_bytes(config.min_free_bytes, config.max_raw_bytes)?;
    if receipt.cohort_capture_wall_ms != config.cohort_capture_wall_ms
        || receipt.min_free_bytes != config.min_free_bytes
        || receipt.max_raw_bytes != config.max_raw_bytes
        || receipt.required_storage_bytes != required_storage_bytes
    {
        bail!("V2 operator preflight storage/time contract differs from the current config");
    }
    require_v2_capture_storage_budget(
        &config.output_dir,
        config.min_free_bytes,
        config.max_raw_bytes,
    )?;
    if receipt.release_binary_digest != digest_running_executable_v2()? {
        bail!("V2 capture must execute the sealed fresh release binary from its preflight bundle");
    }
    let request_fingerprint =
        hex_bytes_v2(&pump_research_exact_state_v2_subscription_request_fingerprint_blake3());
    if receipt.source_request_fingerprint_blake3 != request_fingerprint
        || receipt.source_capture_semantics
            != ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2
        || receipt.source_max_decoded_message_bytes
            != PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES as u64
    {
        bail!("V2 operator preflight source contract differs from the current binary");
    }
    Ok(PumpExactStateValidatedOperatorPreflightV2 {
        receipt,
        receipt_digest: digest_bytes_v2(&receipt_bytes),
        semantics,
    })
}

/// Compare the one semantics authority loaded from the operator config with
/// the values sealed into a preflight receipt.  Capture retains that validated
/// authority object through the ProgramData gate instead of resolving a
/// mutable manifest path a second time after provider I/O has begun.
fn validate_preflight_semantics_binding_v2(
    receipt: &PumpExactStateOperatorPreflightReceiptV2,
    semantics: &PumpExactStateSemanticsAuthorityV2,
) -> Result<()> {
    if receipt.semantics_id != semantics.semantics_id
        || receipt.semantics_manifest_digest
            != semantics_digest_to_capture_v2(&semantics.manifest_digest)
        || receipt.vendored_idl_digest != semantics_digest_to_capture_v2(&semantics.idl_digest)
        || receipt.expected_program_data_hash_blake3
            != hex_bytes_v2(&semantics.expected_program_data_hash_blake3())
    {
        bail!("V2 operator preflight semantics authority differs from the current config manifest");
    }
    Ok(())
}

fn semantics_digest_to_capture_v2(
    digest: &PumpExactStateSemanticsDigestV2,
) -> PumpExactStateDigestV2 {
    PumpExactStateDigestV2 {
        sha256: digest.sha256.clone(),
        blake3: digest.blake3.clone(),
        bytes: digest.bytes,
    }
}

/// Validate every immutable file retained in the preflight bundle.  It is
/// deliberately independent of current Git state: capture authority is the
/// sealed build bundle and its kernel-bound process image, not whichever
/// checkout happens to be present when the cohort is later started.
fn validate_operator_preflight_bundle_contents_v2(
    bundle_dir: &Path,
    receipt: &PumpExactStateOperatorPreflightReceiptV2,
) -> Result<()> {
    if receipt.release_binary_file != V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE
        || receipt.build_log_file != V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE
        || receipt.build_receipt_file != V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE
        || receipt.build_semantics != V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS
    {
        bail!("V2 operator preflight bundle layout or fresh-build semantics are not accepted");
    }
    validate_v2_trimmed("preflight repository_commit", &receipt.repository_commit)?;
    let release_binary_path = bundle_dir.join(&receipt.release_binary_file);
    let release_binary_bytes = read_private_executable_regular_file_v2(
        &release_binary_path,
        "V2 sealed fresh release binary",
        V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_MAX_BYTES,
    )?;
    if receipt.release_binary_digest != digest_bytes_v2(&release_binary_bytes) {
        bail!("V2 sealed fresh release binary digest does not match preflight receipt");
    }
    let build_log_path = bundle_dir.join(&receipt.build_log_file);
    let build_log_bytes = read_bounded_regular_file_v2(
        &build_log_path,
        "V2 sealed fresh-build log",
        V2_OPERATOR_PREFLIGHT_BUILD_LOG_MAX_BYTES,
    )?;
    if receipt.build_log_digest != digest_bytes_v2(&build_log_bytes) {
        bail!("V2 sealed fresh-build log digest does not match preflight receipt");
    }
    let build_receipt_path = bundle_dir.join(&receipt.build_receipt_file);
    let build_receipt_bytes = read_bounded_regular_file_v2(
        &build_receipt_path,
        "V2 sealed fresh-build receipt",
        V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES,
    )?;
    if receipt.build_receipt_digest != digest_bytes_v2(&build_receipt_bytes) {
        bail!("V2 sealed fresh-build receipt digest does not match preflight receipt");
    }
    let build_receipt: PumpExactStateFreshBuildReceiptV2 =
        serde_json::from_slice(&build_receipt_bytes)
            .context("parse V2 sealed fresh-build receipt")?;
    if build_receipt.schema_version != V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION
        || build_receipt.build_semantics != V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS
        || build_receipt.repository_commit != receipt.repository_commit
        || build_receipt.cargo_command != fresh_build_cargo_command_v2()
        || build_receipt.build_log_digest != receipt.build_log_digest
        || build_receipt.release_binary_digest != receipt.release_binary_digest
        || build_receipt.build_started_wall_ms == 0
        || build_receipt.build_completed_wall_ms < build_receipt.build_started_wall_ms
    {
        bail!("V2 sealed fresh-build receipt does not prove the accepted release binary");
    }
    Ok(())
}

fn read_private_executable_regular_file_v2(
    path: &Path,
    label: &str,
    max_bytes: u64,
) -> Result<Vec<u8>> {
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
        if mode & 0o077 != 0 || mode & 0o111 == 0 {
            bail!(
                "{label} {} must be private and executable (mode {:o})",
                path.display(),
                mode
            );
        }
    }
    read_bounded_regular_file_v2(path, label, max_bytes)
}

fn read_private_regular_file_v2(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>> {
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
        if mode & 0o077 != 0 {
            bail!(
                "{label} {} must not grant group/other access (mode {:o})",
                path.display(),
                mode
            );
        }
    }
    read_bounded_regular_file_v2(path, label, max_bytes)
}

fn read_bounded_regular_file_v2(path: &Path, label: &str, max_bytes: u64) -> Result<Vec<u8>> {
    read_bounded_regular_file_v2_after_precheck(path, label, max_bytes, || {})
}

fn read_bounded_regular_file_v2_after_precheck<F>(
    path: &Path,
    label: &str,
    max_bytes: u64,
    before_open: F,
) -> Result<Vec<u8>>
where
    F: FnOnce(),
{
    #[cfg(unix)]
    {
        let before_path = fs::symlink_metadata(path)
            .with_context(|| format!("inspect {label} {}", path.display()))?;
        if before_path.file_type().is_symlink() || !before_path.is_file() {
            bail!(
                "{label} {} must be a regular non-symlink file",
                path.display()
            );
        }
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
        before_open();
        let file = options
            .open(path)
            .with_context(|| format!("open bounded {label} {}", path.display()))?;
        read_open_regular_file_exact_v2(&file, label, max_bytes)
            .with_context(|| format!("read bounded {label} {}", path.display()))
    }
    #[cfg(not(unix))]
    {
        let _ = (path, label, max_bytes, before_open);
        bail!("V2 authority reads require Unix no-follow/nonblocking file handling")
    }
}

/// Read exactly the size frozen by an already-open regular descriptor.  This
/// is the common authority primitive for V2 config/receipt inputs and the
/// kernel-bound running executable.  It never reads a mutable descriptor
/// until EOF and verifies the descriptor identity after the positional read.
#[cfg(unix)]
fn read_open_regular_file_exact_v2(file: &File, label: &str, max_bytes: u64) -> Result<Vec<u8>> {
    let before = file
        .metadata()
        .with_context(|| format!("fstat opened {label}"))?;
    if !before.is_file() || before.len() > max_bytes {
        bail!(
            "opened {label} is not a bounded regular file ({} bytes, max {})",
            before.len(),
            max_bytes
        );
    }
    let length =
        usize::try_from(before.len()).context("bounded V2 authority length does not fit usize")?;
    let mut bytes = vec![0_u8; length];
    if !bytes.is_empty() {
        file.read_exact_at(&mut bytes, 0)
            .with_context(|| format!("read exactly opened {label}"))?;
    }
    let after = file
        .metadata()
        .with_context(|| format!("re-fstat opened {label}"))?;
    if !after.is_file()
        || after.len() != before.len()
        || after.dev() != before.dev()
        || after.ino() != before.ino()
    {
        bail!("opened {label} changed while being read");
    }
    Ok(bytes)
}

fn validate_v2_trimmed(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim() != value {
        bail!("V2 {name} must be non-empty and have no surrounding whitespace");
    }
    Ok(())
}

fn validate_v2_identifier(name: &str, value: &str, max_bytes: usize) -> Result<()> {
    validate_v2_trimmed(name, value)?;
    if value.len() > max_bytes {
        bail!(
            "V2 {name} must be at most {max_bytes} UTF-8 bytes, got {}",
            value.len()
        );
    }
    Ok(())
}

fn validate_v2_endpoint(name: &str, value: &str) -> Result<()> {
    validate_v2_trimmed(name, value)?;
    let parsed = Url::parse(value).with_context(|| format!("V2 {name} is not an absolute URL"))?;
    if parsed.host_str().is_none() {
        bail!("V2 {name} has no hostname");
    }
    if !parsed.username().is_empty() || parsed.password().is_some() {
        bail!("V2 {name} must not embed credentials");
    }
    Ok(())
}

/// A V2 run root must already be a private operator directory and cannot sit
/// inside a completed V1/V2 raw run.  This check happens before any writer,
/// snapshot, or provider action so a mistaken output path cannot create even
/// a temporary directory next to immutable historical raw evidence.
fn validate_v2_output_root_isolated(output_dir: &Path) -> Result<()> {
    require_existing_private_directory_v2(output_dir, "V2 output_dir")?;
    validate_v2_existing_directory_outside_raw_authority(output_dir, "V2 output_dir")
}

/// Available bytes on the filesystem that owns `path`.  This remains a
/// preflight and periodic fail-closed check, not a claim that a mutable shared
/// filesystem cannot change between two syscalls.  The raw writer separately
/// enforces its immutable per-run byte budget, so either a storage-floor
/// breach or an I/O error produces an incomplete run.
#[cfg(target_os = "linux")]
fn available_filesystem_bytes_v2(path: &Path) -> Result<u64> {
    let path = std::ffi::CString::new(path.as_os_str().as_bytes()).with_context(|| {
        format!(
            "V2 storage path {} contains an interior NUL",
            path.display()
        )
    })?;
    // SAFETY: `path` is NUL-terminated and points to an existing operator
    // directory validated before this function is called.  `statvfs` only
    // writes the initialized local `stats` structure.
    let mut stats = std::mem::MaybeUninit::<libc::statvfs>::zeroed();
    let result = unsafe { libc::statvfs(path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return Err(std::io::Error::last_os_error())
            .with_context(|| format!("statvfs V2 output filesystem {}", path.to_string_lossy()));
    }
    // SAFETY: a zero return from `statvfs` initializes `stats` in full.
    let stats = unsafe { stats.assume_init() };
    let bytes = (stats.f_bavail as u128)
        .checked_mul(stats.f_frsize as u128)
        .ok_or_else(|| anyhow::anyhow!("V2 available filesystem byte count overflow"))?;
    u64::try_from(bytes).context("V2 available filesystem bytes exceed u64")
}

#[cfg(not(target_os = "linux"))]
fn available_filesystem_bytes_v2(_path: &Path) -> Result<u64> {
    bail!("V2 prospective capture requires Linux statvfs storage authority")
}

fn required_v2_storage_bytes(min_free_bytes: u64, max_raw_bytes: u64) -> Result<u64> {
    min_free_bytes
        .checked_add(max_raw_bytes)
        .and_then(|bytes| bytes.checked_add(V2_CAPTURE_METADATA_ALLOWANCE_BYTES))
        .ok_or_else(|| anyhow::anyhow!("V2 minimum free and raw byte budgets overflow u64"))
}

fn require_v2_capture_storage_budget(
    output_dir: &Path,
    min_free_bytes: u64,
    max_raw_bytes: u64,
) -> Result<u64> {
    let available = available_filesystem_bytes_v2(output_dir)?;
    let required = required_v2_storage_bytes(min_free_bytes, max_raw_bytes)?;
    if available < required {
        bail!(
            "V2 output filesystem {} has {} bytes available but this hash-pinned run requires at least {} ({} raw budget + {} retained reserve + {} metadata allowance)",
            output_dir.display(),
            available,
            required,
            max_raw_bytes,
            min_free_bytes,
            V2_CAPTURE_METADATA_ALLOWANCE_BYTES,
        );
    }
    Ok(available)
}

fn validate_v2_new_operator_artifact_path(path: &Path, label: &str) -> Result<()> {
    if path.as_os_str().is_empty() {
        bail!("{label} must not be empty");
    }
    if fs::symlink_metadata(path).is_ok() {
        bail!("{label} {} must be create-new", path.display());
    }
    let parent = path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("{label} {} has no parent", path.display()))?;
    require_existing_private_directory_v2(parent, &format!("{label} parent"))?;
    validate_v2_existing_directory_outside_raw_authority(parent, &format!("{label} parent"))
}

fn validate_v2_existing_directory_outside_raw_authority(path: &Path, label: &str) -> Result<()> {
    let canonical = fs::canonicalize(path)
        .with_context(|| format!("canonicalize {label} {}", path.display()))?;
    for ancestor in canonical.ancestors() {
        let is_existing_raw_root = ancestor.join("run_completion_receipt.json").is_file()
            || ancestor.join("run_completion_receipt_v2.json").is_file();
        let is_existing_run_parent = ancestor
            .join("raw")
            .join("run_completion_receipt.json")
            .is_file()
            || ancestor
                .join("raw-v2")
                .join("run_completion_receipt_v2.json")
                .is_file();
        if is_existing_raw_root || is_existing_run_parent {
            bail!(
                "{label} {} is inside existing raw/run authority {}; choose a separate private V2 root",
                path.display(),
                ancestor.display()
            );
        }
    }
    Ok(())
}

fn resolve_v2_optional_env(surface: &str, variable: Option<&str>) -> Result<Option<String>> {
    variable
        .map(|name| {
            env::var(name).with_context(|| {
                format!("read V2 {surface} credential from environment variable {name}")
            })
        })
        .transpose()
}

fn default_v2_grpc_auth_header() -> String {
    "x-token".to_owned()
}

fn default_v2_rpc_auth_header() -> String {
    "x-api-key".to_owned()
}

fn default_v2_pump_program_id() -> String {
    PUMP_FUN_PROGRAM_ID.to_owned()
}

const fn default_v2_required_for_run() -> bool {
    true
}

const fn default_v2_source_queue_capacity() -> usize {
    DEFAULT_V2_SOURCE_QUEUE_CAPACITY
}

const fn default_v2_source_queue_max_bytes() -> u64 {
    DEFAULT_V2_SOURCE_QUEUE_MAX_BYTES
}

const fn default_v2_cohort_capture_wall_ms() -> u64 {
    DEFAULT_V2_COHORT_CAPTURE_WALL_MS
}

const fn default_v2_min_free_bytes() -> u64 {
    DEFAULT_V2_MIN_FREE_BYTES
}

const fn default_v2_max_raw_bytes() -> u64 {
    DEFAULT_V2_MAX_RAW_BYTES
}

const fn default_v2_source_readiness_timeout_ms() -> u64 {
    DEFAULT_V2_SOURCE_READINESS_TIMEOUT_MS
}

const fn default_v2_flush_interval_ms() -> u64 {
    DEFAULT_V2_FLUSH_INTERVAL_MS
}

const fn default_v2_segment_max_bytes() -> u64 {
    DEFAULT_V2_SEGMENT_MAX_BYTES
}

const fn default_v2_segment_max_duration_ms() -> u64 {
    DEFAULT_V2_SEGMENT_MAX_DURATION_MS
}

/// Safe-to-publish digest.  It never contains endpoint or credential bytes.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpExactStateDigestV2 {
    pub sha256: String,
    pub blake3: String,
    pub bytes: u64,
}

fn digest_bytes_v2(bytes: &[u8]) -> PumpExactStateDigestV2 {
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    let blake3 = blake3::hash(bytes);
    PumpExactStateDigestV2 {
        sha256: hex_bytes_v2(&sha256),
        blake3: hex_bytes_v2(blake3.as_bytes()),
        bytes: u64::try_from(bytes.len()).unwrap_or(u64::MAX),
    }
}

fn sha256_storage_hash_v2(
    bytes: &[u8],
) -> ghost_core::pump_research_tape::PumpResearchStorageHashV1 {
    let sha256: [u8; 32] = Sha256::digest(bytes).into();
    ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(sha256)
}

fn hex_bytes_v2(bytes: &[u8]) -> String {
    let mut value = String::with_capacity(bytes.len().saturating_mul(2));
    for byte in bytes {
        use std::fmt::Write as _;
        let _ = write!(value, "{byte:02x}");
    }
    value
}

#[derive(Clone, Debug)]
struct PumpExactStateReadinessSealV2 {
    source_readiness: PumpExactStateSourceReadinessV2,
    cohort_slots_strictly_after: u64,
}

/// Persist the only prospective cohort boundary.  It is deliberately derived
/// exclusively from accepted Yellowstone evidence: neither an RPC account
/// snapshot nor any historical account universe participates in this proof.
async fn persist_stream_readiness_boundary_v2(
    coordinator: &PumpExactStateCaptureCoordinatorV2,
    timeout: Duration,
) -> Result<PumpExactStateReadinessSealV2> {
    let source_readiness = coordinator.wait_for_required_source_lanes(timeout).await?;
    validate_source_readiness_v2(&source_readiness)?;
    let source_stream_epoch = coordinator.established_stream_epoch().ok_or_else(|| {
        anyhow::anyhow!("V2 source stream disappeared before stream readiness could be sealed")
    })?;
    let source_capture_sequence_exclusive = coordinator.arm_stream_boundary()?;
    let cohort_slots_strictly_after = source_readiness.source_readiness_slot;
    let boundary = PumpExactStateProspectiveStreamBoundaryV2 {
        source_readiness: source_readiness.clone(),
        source_stream_epoch,
        source_capture_sequence_exclusive,
        cohort_slots_strictly_after,
        sealed_wall_ts_ms: wall_clock_ms_v2(),
        sealed_monotonic_ts_ms: crate::types::arrival_time_ms(),
    };
    coordinator.persist_armed_stream_boundary(
        source_capture_sequence_exclusive,
        boundary,
        timeout,
    )?;
    if coordinator.established_stream_epoch() != Some(source_stream_epoch) {
        bail!("V2 source stream changed epoch before the readiness boundary was durably confirmed");
    }
    Ok(PumpExactStateReadinessSealV2 {
        source_readiness,
        cohort_slots_strictly_after,
    })
}

/// Defensive control-plane validation for the source-readiness payload before
/// it becomes frozen raw evidence.  The normal ingress path constructs this
/// value from atomics, but treating the derived maximum as a checked invariant
/// here prevents any future caller from lowering the declared readiness slot
/// and admitting a cohort before all five lanes are represented.
fn validate_source_readiness_v2(readiness: &PumpExactStateSourceReadinessV2) -> Result<()> {
    let slots = [
        readiness.first_transaction_slot,
        readiness.first_account_update_slot,
        readiness.first_slot_update_slot,
        readiness.first_block_meta_slot,
        readiness.first_full_block_slot,
    ];
    if slots
        .iter()
        .any(|slot| *slot == V2_REQUIRED_LANE_SLOT_UNSET)
    {
        bail!("V2 source readiness contains an unobserved required lane slot");
    }
    let expected = slots
        .into_iter()
        .max()
        .ok_or_else(|| anyhow::anyhow!("V2 source readiness has no lane slots"))?;
    if readiness.source_readiness_slot != expected {
        bail!(
            "V2 source readiness slot {} differs from required-lane maximum {}",
            readiness.source_readiness_slot,
            expected,
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpExactStateCaptureRunStatusV2 {
    Complete,
    Incomplete,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateRunStartManifestV2 {
    pub(crate) storage_format_version: u16,
    pub(crate) schema_version: u16,
    pub(crate) capture_config_schema_version: u16,
    pub(crate) run_id: String,
    pub(crate) repository_commit: String,
    pub(crate) running_executable_digest: PumpExactStateDigestV2,
    pub(crate) operator_preflight_receipt_digest: PumpExactStateDigestV2,
    pub(crate) sealed_release_binary_digest: PumpExactStateDigestV2,
    pub(crate) sealed_fresh_build_receipt_digest: PumpExactStateDigestV2,
    pub(crate) sealed_build_semantics: String,
    pub(crate) capture_config_digest: PumpExactStateDigestV2,
    pub(crate) capture_contract_sha256: String,
    pub(crate) source_request_fingerprint_blake3: String,
    pub(crate) source_capture_semantics: String,
    pub(crate) source_max_decoded_message_bytes: u64,
    pub(crate) semantics_id: String,
    pub(crate) semantics_manifest_digest: PumpExactStateDigestV2,
    pub(crate) vendored_idl_digest: PumpExactStateDigestV2,
    pub(crate) expected_program_data_hash_blake3: String,
    pub(crate) primary_provider_id: String,
    pub(crate) grpc_endpoint_digest: PumpExactStateDigestV2,
    pub(crate) program_data_rpc_endpoint_digest: PumpExactStateDigestV2,
    pub(crate) program_data_rpc_auth_mode: String,
    pub(crate) pump_program_id: String,
    pub(crate) program_data_at_start: PumpProgramDataReceiptV1,
    pub(crate) cohort_capture_wall_ms: u64,
    pub(crate) min_free_bytes: u64,
    pub(crate) max_raw_bytes: u64,
    pub(crate) required_storage_bytes: u64,
    pub(crate) output_filesystem_available_bytes_at_start: u64,
    pub(crate) capture_started_wall_ms: u64,
    pub(crate) capture_started_monotonic_ms: u64,
    pub(crate) required_for_run: bool,
}

/// An explicit normal boundary for the raw prospective cohort.  A raw V2 run
/// may finish only after the operator requested shutdown or the hash-pinned
/// cohort wall deadline elapsed.  A source task that simply disappears never
/// acquires one of these normal termination values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum PumpExactStateCaptureTerminationV2 {
    OperatorSignal,
    CohortWallDeadline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateRunCompletionReceiptV2 {
    pub(crate) storage_format_version: u16,
    pub(crate) schema_version: u16,
    pub(crate) run_id: String,
    pub(crate) status: PumpExactStateCaptureRunStatusV2,
    pub(crate) clean_shutdown: bool,
    pub(crate) source_readiness: Option<PumpExactStateSourceReadinessV2>,
    pub(crate) readiness_boundary_persisted: bool,
    pub(crate) cohort_slots_strictly_after: Option<u64>,
    pub(crate) readiness_completed: bool,
    pub(crate) running_executable_at_completion: Option<PumpExactStateDigestV2>,
    pub(crate) running_executable_unchanged: bool,
    pub(crate) program_data_at_start: PumpProgramDataReceiptV1,
    pub(crate) program_data_at_completion: Option<PumpProgramDataReceiptV1>,
    pub(crate) program_data_unchanged: bool,
    pub(crate) cohort_capture_termination: Option<PumpExactStateCaptureTerminationV2>,
    pub(crate) cohort_capture_elapsed_ms: Option<u64>,
    pub(crate) min_free_bytes: u64,
    pub(crate) max_raw_bytes: u64,
    pub(crate) output_filesystem_available_bytes_at_completion: Option<u64>,
    pub(crate) storage_reserve_maintained: bool,
    pub(crate) raw_byte_budget_respected: bool,
    pub(crate) required_source_lanes_observed: bool,
    pub(crate) source_lifecycle: PumpExactStateCaptureSourceLifecycleV2,
    pub(crate) writer: PumpExactStateWriterSummaryV2,
    pub(crate) segment_list: Vec<PumpExactStateSegmentReceiptV2>,
    pub(crate) completion_wall_ms: u64,
}

#[derive(Clone, Debug)]
pub struct PumpExactStateCaptureRunSummaryV2 {
    pub run_id: String,
    pub raw_dir: PathBuf,
    pub status: PumpExactStateCaptureRunStatusV2,
    pub clean_shutdown: bool,
    pub gap_count: u64,
    pub source_error: Option<String>,
    pub writer_error: Option<String>,
    pub completion_receipt_error: Option<String>,
}

impl PumpExactStateCaptureRunSummaryV2 {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.status, PumpExactStateCaptureRunStatusV2::Complete)
    }
}

struct PumpExactStateCapturePathsV2 {
    run_id: String,
    raw_dir: PathBuf,
    start_manifest_path: PathBuf,
    completion_receipt_path: PathBuf,
}

impl PumpExactStateCapturePathsV2 {
    fn create(output_dir: &Path) -> Result<Self> {
        create_private_directory_all_v2(output_dir)?;
        let base = format!(
            "pump-exact-state-v2-{}-{}",
            wall_clock_ms_v2(),
            std::process::id()
        );
        for suffix in 0u32..10_000 {
            let run_id = if suffix == 0 {
                base.clone()
            } else {
                format!("{base}-{suffix}")
            };
            let run_dir = output_dir.join(&run_id);
            match create_private_directory_v2(&run_dir) {
                Ok(()) => {
                    let raw_dir = run_dir.join("raw-v2");
                    if let Err(error) = create_private_directory_v2(&raw_dir) {
                        let _ = fs::remove_dir(&run_dir);
                        return Err(error).with_context(|| {
                            format!("create V2 raw directory {}", raw_dir.display())
                        });
                    }
                    return Ok(Self {
                        run_id,
                        start_manifest_path: raw_dir.join("run_start_manifest_v2.json"),
                        completion_receipt_path: raw_dir.join("run_completion_receipt_v2.json"),
                        raw_dir,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error)
                        .with_context(|| format!("create V2 run directory {}", run_dir.display()));
                }
            }
        }
        bail!("could not allocate a unique prospective V2 run ID")
    }
}

fn create_private_directory_all_v2(path: &Path) -> Result<()> {
    require_existing_private_directory_v2(path, "V2 output parent")
}

fn create_private_directory_v2(path: &Path) -> std::io::Result<()> {
    let mut builder = fs::DirBuilder::new();
    builder.recursive(false);
    #[cfg(unix)]
    builder.mode(0o700);
    builder.create(path)
}

fn require_existing_private_directory_v2(path: &Path, label: &str) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "{label} {} must be an existing non-symlink directory",
            path.display()
        );
    }
    #[cfg(unix)]
    {
        if metadata.permissions().mode() & 0o077 != 0 {
            bail!(
                "{label} {} must not grant group/other access (mode {:o})",
                path.display(),
                metadata.permissions().mode() & 0o777
            );
        }
    }
    Ok(())
}

fn write_json_create_new_v2<T: Serialize>(path: &Path, value: &T) -> Result<()> {
    let mut bytes = serde_json::to_vec_pretty(value)
        .with_context(|| format!("serialize V2 JSON {}", path.display()))?;
    bytes.push(b'\n');
    let mut options = OpenOptions::new();
    options.write(true).create_new(true);
    #[cfg(unix)]
    options.mode(0o600);
    let mut file = options
        .open(path)
        .with_context(|| format!("create V2 JSON {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write V2 JSON {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync V2 JSON {}", path.display()))?;
    sync_directory_v2(
        path.parent()
            .ok_or_else(|| anyhow::anyhow!("V2 JSON path has no parent"))?,
    )?;
    Ok(())
}

fn repository_root_v2() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("invoke git rev-parse --show-toplevel for V2 preflight")?;
    if !output.status.success() {
        bail!("git rev-parse --show-toplevel failed during V2 operator preflight");
    }
    let root = String::from_utf8(output.stdout)
        .context("git rev-parse --show-toplevel returned invalid UTF-8")?;
    let root = PathBuf::from(root.trim());
    let metadata = fs::symlink_metadata(&root)
        .with_context(|| format!("inspect V2 repository root {}", root.display()))?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() {
        bail!(
            "V2 repository root {} must be a non-symlink directory",
            root.display()
        );
    }
    Ok(root)
}

fn repository_commit_at_v2(repository_root: &Path) -> Result<String> {
    let output = Command::new("git")
        .current_dir(repository_root)
        .args(["rev-parse", "HEAD"])
        .output()
        .context("invoke git rev-parse HEAD for V2 run manifest")?;
    if !output.status.success() {
        bail!("git rev-parse HEAD failed while creating V2 run manifest");
    }
    let commit = String::from_utf8(output.stdout)
        .context("git rev-parse HEAD returned invalid UTF-8")?
        .trim()
        .to_owned();
    validate_v2_trimmed("repository_commit", &commit)?;
    Ok(commit)
}

fn digest_running_executable_v2() -> Result<PumpExactStateDigestV2> {
    #[cfg(target_os = "linux")]
    {
        const MAX_EXECUTABLE_BYTES: u64 = 128 * 1024 * 1024;
        let mut options = OpenOptions::new();
        options
            .read(true)
            .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
        let file = options
            .open("/proc/self/exe")
            .context("open kernel-bound /proc/self/exe for V2 running-image digest")?;
        let bytes = read_open_regular_file_exact_v2(
            &file,
            "kernel-bound V2 running executable descriptor",
            MAX_EXECUTABLE_BYTES,
        )?;
        if bytes.is_empty() {
            bail!("V2 running executable descriptor is empty");
        }
        Ok(digest_bytes_v2(&bytes))
    }
    #[cfg(not(target_os = "linux"))]
    {
        bail!("V2 capture requires Linux /proc/self/exe running-image authority")
    }
}

/// Compares the immutable ProgramData authority observed at capture start and
/// completion. `observed_context_slot` deliberately remains an audit label:
/// a later finalized RPC observation normally has a later context slot without
/// implying any Program or ProgramData change.
pub(crate) fn program_data_receipts_match_v2(
    start: &PumpProgramDataReceiptV1,
    completion: &PumpProgramDataReceiptV1,
) -> bool {
    start.pump_program_id == completion.pump_program_id
        && start.pump_program_account_owner == completion.pump_program_account_owner
        && start.pump_programdata_pubkey == completion.pump_programdata_pubkey
        && start.program_data_owner == completion.program_data_owner
        && start.program_data_hash_algorithm == completion.program_data_hash_algorithm
        && start.program_data_hash_blake3 == completion.program_data_hash_blake3
        && start.program_deployment_slot == completion.program_deployment_slot
        && start.commitment == completion.commitment
}

/// Read the two finalized accounts needed to pin V2 semantics, without the
/// Solana client's compatibility `getVersion` probe.  The V2 operator RPC
/// contract is intentionally literal: its only account reads are the Pump
/// Program and the ProgramData account named by that Program.  This local
/// reader is separate from V1 so GO-D's historical capture path is unchanged.
async fn observe_program_data_receipt_v2(
    rpc_endpoint: &str,
    rpc_auth_token: Option<&str>,
    rpc_auth_header: &str,
    pump_program_id: Pubkey,
) -> Result<PumpProgramDataReceiptV1> {
    let client = match rpc_auth_token {
        Some(token) => crate::rpc_http_client::new_async_rpc_client_with_explicit_auth(
            rpc_endpoint.to_owned(),
            rpc_auth_header,
            token,
        )
        .map_err(anyhow::Error::msg)?,
        None => crate::rpc_http_client::new_async_rpc_client_without_legacy_auth(
            rpc_endpoint.to_owned(),
        )
        .map_err(anyhow::Error::msg)?,
    };
    let account_config = RpcAccountInfoConfig {
        // ProgramData is larger than the JSON-RPC Base58 account-data limit.
        // Use one explicit binary encoding for both of the only permitted
        // ProgramData authority reads rather than relying on client defaults.
        encoding: Some(UiAccountEncoding::Base64),
        commitment: Some(CommitmentConfig::finalized()),
        ..RpcAccountInfoConfig::default()
    };
    let program_response = client
        .get_account_with_config(&pump_program_id, account_config.clone())
        .await
        .context("read finalized Pump Program account")?;
    let program_account = program_response
        .value
        .ok_or_else(|| anyhow::anyhow!("Pump Program account {pump_program_id} is missing"))?;
    if program_account.owner != bpf_loader_upgradeable::id() {
        bail!(
            "Pump Program account owner {} is not upgradeable loader {}",
            program_account.owner,
            bpf_loader_upgradeable::id()
        );
    }
    let program_state: UpgradeableLoaderState = bincode::deserialize(&program_account.data)
        .context("decode UpgradeableLoaderState for Pump Program")?;
    let programdata_pubkey = match program_state {
        UpgradeableLoaderState::Program {
            programdata_address,
        } => programdata_address,
        other => bail!("Pump Program account has unexpected upgradeable-loader state {other:?}"),
    };
    let programdata_response = client
        .get_account_with_config(&programdata_pubkey, account_config)
        .await
        .context("read finalized Pump ProgramData account")?;
    let programdata_account = programdata_response.value.ok_or_else(|| {
        anyhow::anyhow!("Pump ProgramData account {programdata_pubkey} is missing")
    })?;
    if programdata_account.owner != bpf_loader_upgradeable::id() {
        bail!(
            "Pump ProgramData owner {} is not upgradeable loader {}",
            programdata_account.owner,
            bpf_loader_upgradeable::id()
        );
    }
    let programdata_state: UpgradeableLoaderState = bincode::deserialize(&programdata_account.data)
        .context("decode UpgradeableLoaderState for Pump ProgramData")?;
    let deployment_slot = match programdata_state {
        UpgradeableLoaderState::ProgramData { slot, .. } => Some(slot),
        other => bail!("Pump ProgramData account has unexpected state {other:?}"),
    };

    Ok(PumpProgramDataReceiptV1 {
        pump_program_id: PumpResearchStoragePubkeyV1::from(pump_program_id.to_bytes()),
        pump_program_account_owner: PumpResearchStoragePubkeyV1::from(
            program_account.owner.to_bytes(),
        ),
        pump_programdata_pubkey: PumpResearchStoragePubkeyV1::from(programdata_pubkey.to_bytes()),
        program_data_owner: PumpResearchStoragePubkeyV1::from(programdata_account.owner.to_bytes()),
        program_data_hash_algorithm: PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1.to_owned(),
        program_data_hash_blake3: hash_bytes_v2(&programdata_account.data),
        program_deployment_slot: deployment_slot,
        observed_context_slot: programdata_response.context.slot,
        commitment: "finalized".to_owned(),
    })
}

/// Run a standalone prospective Exact-State Tape V2 capture from an operator
/// TOML.  This entry point is intentionally separate from the V1 CLI: it can
/// create only a new `raw-v2` run and does not reinterpret or touch GO-D.
///
/// The sequence is deliberately strict:
///
/// ```text
/// local config validation
/// -> finalized ProgramData start receipt
/// -> create-new raw-v2 run
/// -> established stream-only Yellowstone source
/// -> durable five-lane stream-readiness boundary
/// -> prospective source capture until explicit shutdown
/// -> ProgramData completion receipt + immutable completion receipt
/// ```
///
/// A failure after the create-new raw run is allocated produces an incomplete
/// run; an earlier local/ProgramData failure creates no raw path at all.  In
/// neither case can this entry point make a raw-complete or exact-state-
/// qualified claim.
pub async fn run_prospective_exact_state_capture_v2_from_config_path(
    config_path: &Path,
    preflight_receipt_path: &Path,
) -> Result<PumpExactStateCaptureRunSummaryV2> {
    let (config, config_bytes) = PumpExactStateCaptureConfigV2::load(config_path)?;
    let preflight = validate_operator_preflight_v2(&config, &config_bytes, preflight_receipt_path)?;
    run_prospective_exact_state_capture_v2(config, config_bytes, preflight).await
}

async fn run_prospective_exact_state_capture_v2(
    config: PumpExactStateCaptureConfigV2,
    config_bytes: Vec<u8>,
    preflight: PumpExactStateValidatedOperatorPreflightV2,
) -> Result<PumpExactStateCaptureRunSummaryV2> {
    config.validate()?;
    let output_filesystem_available_bytes_at_start = require_v2_capture_storage_budget(
        &config.output_dir,
        config.min_free_bytes,
        config.max_raw_bytes,
    )?;
    let pump_program_id = config
        .pump_program_id
        .parse::<Pubkey>()
        .context("validated V2 pump_program_id unexpectedly failed to parse")?;
    let grpc_auth_token = config.resolve_grpc_auth_token()?;
    let program_data_rpc_auth_token = config.resolve_program_data_rpc_auth_token()?;
    let program_data_at_start = observe_program_data_receipt_v2(
        &config.program_data_rpc_endpoint,
        program_data_rpc_auth_token.as_deref(),
        &config.program_data_rpc_auth_header,
        pump_program_id,
    )
    .await
    .context("V2 requires finalized Pump ProgramData before opening its source stream")?;
    preflight
        .semantics
        .validate_program_data(&program_data_at_start)
        .context(
            "V2 finalized Pump ProgramData does not match the semantics manifest pinned before capture",
        )?;
    let running_executable_digest = digest_running_executable_v2()?;
    if running_executable_digest != preflight.receipt.release_binary_digest {
        bail!("V2 running executable drifted after preflight validation and before run allocation");
    }
    let capture_config_digest = digest_bytes_v2(&config_bytes);
    let capture_contract_sha256 = sha256_storage_hash_v2(&config_bytes);
    let paths = PumpExactStateCapturePathsV2::create(&config.output_dir)?;
    let start_manifest = PumpExactStateRunStartManifestV2 {
        storage_format_version:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
        schema_version: EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
        capture_config_schema_version: EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION,
        run_id: paths.run_id.clone(),
        repository_commit: preflight.receipt.repository_commit.clone(),
        running_executable_digest: running_executable_digest.clone(),
        operator_preflight_receipt_digest: preflight.receipt_digest,
        sealed_release_binary_digest: preflight.receipt.release_binary_digest.clone(),
        sealed_fresh_build_receipt_digest: preflight.receipt.build_receipt_digest.clone(),
        sealed_build_semantics: preflight.receipt.build_semantics.clone(),
        capture_config_digest: capture_config_digest.clone(),
        capture_contract_sha256: hex_bytes_v2(&capture_contract_sha256.into_inner()),
        source_request_fingerprint_blake3: hex_bytes_v2(
            &pump_research_exact_state_v2_subscription_request_fingerprint_blake3(),
        ),
        source_capture_semantics:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2
                .to_owned(),
        source_max_decoded_message_bytes:
            PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES as u64,
        semantics_id: preflight.receipt.semantics_id.clone(),
        semantics_manifest_digest: preflight.receipt.semantics_manifest_digest.clone(),
        vendored_idl_digest: preflight.receipt.vendored_idl_digest.clone(),
        expected_program_data_hash_blake3: preflight
            .receipt
            .expected_program_data_hash_blake3
            .clone(),
        primary_provider_id: config.primary_provider_id.clone(),
        grpc_endpoint_digest: digest_bytes_v2(config.grpc_endpoint.as_bytes()),
        program_data_rpc_endpoint_digest: digest_bytes_v2(
            config.program_data_rpc_endpoint.as_bytes(),
        ),
        program_data_rpc_auth_mode: if config.program_data_rpc_auth_token_env.is_some() {
            "explicit_standalone_auth".to_owned()
        } else {
            "standalone_no_auth_no_legacy_fallback".to_owned()
        },
        pump_program_id: config.pump_program_id.clone(),
        program_data_at_start: program_data_at_start.clone(),
        cohort_capture_wall_ms: config.cohort_capture_wall_ms,
        min_free_bytes: config.min_free_bytes,
        max_raw_bytes: config.max_raw_bytes,
        required_storage_bytes: required_v2_storage_bytes(
            config.min_free_bytes,
            config.max_raw_bytes,
        )?,
        output_filesystem_available_bytes_at_start,
        capture_started_wall_ms: wall_clock_ms_v2(),
        capture_started_monotonic_ms: crate::types::arrival_time_ms(),
        required_for_run: config.required_for_run,
    };
    write_json_create_new_v2(&paths.start_manifest_path, &start_manifest)?;

    let coordinator = match PumpExactStateCaptureCoordinatorV2::start(
        &paths.raw_dir,
        paths.run_id.clone(),
        capture_contract_sha256,
        config.source_queue_capacity,
        config.source_queue_max_bytes,
        Duration::from_millis(config.flush_interval_ms),
        config.segment_max_bytes,
        Duration::from_millis(config.segment_max_duration_ms),
        config.max_raw_bytes,
        config.min_free_bytes,
    ) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            let summary = PumpExactStateWriterSummaryV2 {
                error: Some(format!("{error:#}")),
                ..PumpExactStateWriterSummaryV2::default()
            };
            let receipt = incomplete_completion_receipt_v2(
                &paths.run_id,
                &config,
                &program_data_at_start,
                coordinatorless_lifecycle_v2(),
                summary.clone(),
                None,
                None,
                false,
            );
            write_json_create_new_v2(&paths.completion_receipt_path, &receipt)?;
            return Ok(PumpExactStateCaptureRunSummaryV2 {
                run_id: paths.run_id,
                raw_dir: paths.raw_dir,
                status: PumpExactStateCaptureRunStatusV2::Incomplete,
                clean_shutdown: false,
                gap_count: 0,
                source_error: None,
                writer_error: summary.error,
                completion_receipt_error: None,
            });
        }
    };

    let source = match PumpResearchSourceConnectionV1::new_exact_state_v2(
        config.grpc_endpoint.clone(),
        grpc_auth_token,
        config.grpc_auth_header.clone(),
        config.primary_provider_id.clone(),
        config.source_queue_capacity,
        coordinator.source_sink(),
        coordinator.capture_abort(),
    ) {
        Ok(source) => Arc::new(source),
        Err(error) => {
            coordinator.fail_readiness();
            coordinator.finish_source();
            let writer = coordinator.finish_and_join();
            let receipt = incomplete_completion_receipt_v2(
                &paths.run_id,
                &config,
                &program_data_at_start,
                coordinator.source_lifecycle(),
                writer.clone(),
                None,
                None,
                false,
            );
            write_json_create_new_v2(&paths.completion_receipt_path, &receipt)?;
            return Ok(PumpExactStateCaptureRunSummaryV2 {
                run_id: paths.run_id,
                raw_dir: paths.raw_dir,
                status: PumpExactStateCaptureRunStatusV2::Incomplete,
                clean_shutdown: false,
                gap_count: writer.gap_count,
                source_error: Some(format!("{error:#}")),
                writer_error: writer.error,
                completion_receipt_error: None,
            });
        }
    };

    let source_for_task = Arc::clone(&source);
    let mut source_task = tokio::spawn(async move { source_for_task.run().await });
    let readiness_result = persist_stream_readiness_boundary_v2(
        &coordinator,
        Duration::from_millis(config.source_readiness_timeout_ms),
    )
    .await;

    let mut cohort_capture_termination = None;
    let mut cohort_capture_elapsed_ms = None;
    let (source_result, readiness_seal) = match readiness_result {
        Ok(seal) => {
            let cohort_started = Instant::now();
            let result = await_source_until_signal_v2(
                Arc::clone(&source),
                &mut source_task,
                Duration::from_millis(config.cohort_capture_wall_ms),
            )
            .await;
            cohort_capture_elapsed_ms =
                Some(u64::try_from(cohort_started.elapsed().as_millis()).unwrap_or(u64::MAX));
            match result {
                Ok(termination) => {
                    cohort_capture_termination = Some(termination);
                    (Ok(()), Some(seal))
                }
                Err(error) => (Err(error), Some(seal)),
            }
        }
        Err(error) => {
            coordinator.fail_readiness();
            source.request_shutdown();
            let task_result = source_task
                .await
                .map_err(|join| anyhow::anyhow!("V2 source task join failure: {join}"))
                .and_then(|result| result);
            let message = match task_result {
                Ok(()) => format!("V2 stream readiness failed: {error:#}"),
                Err(source_error) => {
                    format!(
                        "V2 stream readiness failed: {error:#}; source also ended: {source_error:#}"
                    )
                }
            };
            (Err(anyhow::anyhow!(message)), None)
        }
    };
    coordinator.finish_source();
    let writer = coordinator.finish_and_join();
    let source_lifecycle = coordinator.source_lifecycle();
    let program_data_completion_result = observe_program_data_receipt_v2(
        &config.program_data_rpc_endpoint,
        program_data_rpc_auth_token.as_deref(),
        &config.program_data_rpc_auth_header,
        pump_program_id,
    )
    .await;
    let completion_receipt_error = program_data_completion_result
        .as_ref()
        .err()
        .map(|error| format!("{error:#}"));
    let program_data_at_completion = program_data_completion_result.ok();
    let program_data_unchanged = program_data_at_completion
        .as_ref()
        .is_some_and(|completion| {
            program_data_receipts_match_v2(&program_data_at_start, completion)
        });
    let executable_completion = digest_running_executable_v2().ok();
    let running_executable_unchanged = executable_completion
        .as_ref()
        .is_some_and(|completion| completion == &running_executable_digest);
    let output_filesystem_available_bytes_at_completion =
        available_filesystem_bytes_v2(&config.output_dir).ok();
    let storage_reserve_maintained = output_filesystem_available_bytes_at_completion
        .is_some_and(|available| available >= config.min_free_bytes);
    let raw_byte_budget_respected = writer.raw_bytes_written <= config.max_raw_bytes;
    let required_source_lanes_observed = writer.required_lane_census.all_required_lanes_observed();
    let source_readiness = readiness_seal
        .as_ref()
        .map(|seal| seal.source_readiness.clone());
    let readiness_boundary_persisted = readiness_seal.is_some()
        && source_lifecycle.source_readiness_status == "complete"
        && writer.accepted_readiness_boundary_records == 1;
    let cohort_slots_strictly_after = readiness_seal
        .as_ref()
        .map(|seal| seal.cohort_slots_strictly_after);
    let readiness_completed = source_lifecycle.source_readiness_status == "complete";
    let clean_shutdown = source_result.is_ok()
        && writer.clean_shutdown
        && source_lifecycle.stream_established
        && source_lifecycle.source_workers_cleanly_stopped
        && source_lifecycle.dropped_source_updates == 0
        && source_lifecycle.source_queue_bytes_at_close == 0
        && source_lifecycle.fatal_capture_error.is_none()
        && source_lifecycle.source_worker_error.is_none()
        && readiness_completed
        && readiness_boundary_persisted
        && cohort_slots_strictly_after.is_some()
        && cohort_capture_termination.is_some()
        && storage_reserve_maintained
        && raw_byte_budget_respected
        && required_source_lanes_observed
        && program_data_unchanged
        && running_executable_unchanged;
    let status = if clean_shutdown && writer.gap_count == 0 {
        PumpExactStateCaptureRunStatusV2::Complete
    } else {
        PumpExactStateCaptureRunStatusV2::Incomplete
    };
    let receipt = PumpExactStateRunCompletionReceiptV2 {
        storage_format_version:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
        schema_version: EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
        run_id: paths.run_id.clone(),
        status,
        clean_shutdown,
        source_readiness,
        readiness_boundary_persisted,
        cohort_slots_strictly_after,
        readiness_completed,
        running_executable_at_completion: executable_completion,
        running_executable_unchanged,
        program_data_at_start,
        program_data_at_completion,
        program_data_unchanged,
        cohort_capture_termination,
        cohort_capture_elapsed_ms,
        min_free_bytes: config.min_free_bytes,
        max_raw_bytes: config.max_raw_bytes,
        output_filesystem_available_bytes_at_completion,
        storage_reserve_maintained,
        raw_byte_budget_respected,
        required_source_lanes_observed,
        source_lifecycle: source_lifecycle.clone(),
        writer: writer.clone(),
        segment_list: writer.segments.clone(),
        completion_wall_ms: wall_clock_ms_v2(),
    };
    write_json_create_new_v2(&paths.completion_receipt_path, &receipt)?;
    Ok(PumpExactStateCaptureRunSummaryV2 {
        run_id: paths.run_id,
        raw_dir: paths.raw_dir,
        status,
        clean_shutdown,
        gap_count: writer.gap_count,
        source_error: source_result.err().map(|error| format!("{error:#}")),
        writer_error: writer.error,
        completion_receipt_error,
    })
}

async fn await_source_until_signal_v2(
    source: Arc<PumpResearchSourceConnectionV1>,
    source_task: &mut tokio::task::JoinHandle<Result<()>>,
    cohort_capture_wall: Duration,
) -> Result<PumpExactStateCaptureTerminationV2> {
    if cohort_capture_wall.is_zero() {
        bail!("V2 cohort capture wall duration must be greater than zero");
    }
    tokio::select! {
        result = &mut *source_task => {
            result
                .map_err(|join| anyhow::anyhow!("V2 source task join failure: {join}"))??;
            bail!("V2 source task ended before an explicit operator signal or configured cohort wall deadline")
        }
        signal = tokio::signal::ctrl_c() => {
            signal.context("wait for V2 capture shutdown signal")?;
            source.request_shutdown();
            (&mut *source_task).await
                .map_err(|join| anyhow::anyhow!("V2 source task join failure after shutdown: {join}"))??;
            Ok(PumpExactStateCaptureTerminationV2::OperatorSignal)
        }
        _ = tokio::time::sleep(cohort_capture_wall) => {
            source.request_shutdown();
            (&mut *source_task).await
                .map_err(|join| anyhow::anyhow!("V2 source task join failure after cohort wall deadline: {join}"))??;
            Ok(PumpExactStateCaptureTerminationV2::CohortWallDeadline)
        }
    }
}

fn coordinatorless_lifecycle_v2() -> PumpExactStateCaptureSourceLifecycleV2 {
    PumpExactStateCaptureSourceLifecycleV2 {
        source_readiness_status: "failed".to_owned(),
        ..PumpExactStateCaptureSourceLifecycleV2::default()
    }
}

fn incomplete_completion_receipt_v2(
    run_id: &str,
    config: &PumpExactStateCaptureConfigV2,
    program_data_at_start: &PumpProgramDataReceiptV1,
    source_lifecycle: PumpExactStateCaptureSourceLifecycleV2,
    writer: PumpExactStateWriterSummaryV2,
    program_data_at_completion: Option<PumpProgramDataReceiptV1>,
    running_executable_at_completion: Option<PumpExactStateDigestV2>,
    running_executable_unchanged: bool,
) -> PumpExactStateRunCompletionReceiptV2 {
    let program_data_unchanged = program_data_at_completion
        .as_ref()
        .is_some_and(|completion| {
            program_data_receipts_match_v2(program_data_at_start, completion)
        });
    PumpExactStateRunCompletionReceiptV2 {
        storage_format_version:
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
        schema_version: EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
        run_id: run_id.to_owned(),
        status: PumpExactStateCaptureRunStatusV2::Incomplete,
        clean_shutdown: false,
        source_readiness: None,
        readiness_boundary_persisted: false,
        cohort_slots_strictly_after: None,
        readiness_completed: false,
        running_executable_at_completion,
        running_executable_unchanged,
        program_data_at_start: program_data_at_start.clone(),
        program_data_at_completion,
        program_data_unchanged,
        cohort_capture_termination: None,
        cohort_capture_elapsed_ms: None,
        min_free_bytes: config.min_free_bytes,
        max_raw_bytes: config.max_raw_bytes,
        output_filesystem_available_bytes_at_completion: available_filesystem_bytes_v2(
            &config.output_dir,
        )
        .ok(),
        storage_reserve_maintained: available_filesystem_bytes_v2(&config.output_dir)
            .is_ok_and(|available| available >= config.min_free_bytes),
        raw_byte_budget_respected: writer.raw_bytes_written <= config.max_raw_bytes,
        required_source_lanes_observed: false,
        source_lifecycle,
        segment_list: writer.segments.clone(),
        writer,
        completion_wall_ms: wall_clock_ms_v2(),
    }
}

/// Convert exactly one admitted decoded Yellowstone update into its V2 raw
/// record sequence.  Most source updates produce one record.  An unfiltered
/// full block is preserved as a bounded started/chunks/completed sequence so
/// V2 can prove filtered Pump-transaction completeness without allowing a
/// large block payload to bypass the frozen per-frame ceiling.
///
/// The writer owns durable framing; this conversion owns the narrow
/// source-schema contract.  Any unexpected update or account-owner drift is
/// an error so a prospective run fails closed instead of silently claiming
/// full Pump-owned coverage.
#[cfg(test)]
fn raw_records_from_source_v2(
    capture_sequence: u64,
    update: PumpResearchSourceUpdateV1,
) -> Result<Vec<PumpExactStateRawRecordV2>> {
    let source_payload = update.update.encode_to_vec();
    raw_records_from_source_payload_v2(capture_sequence, update, source_payload)
}

/// Convert a decoded source update while preserving the exact deterministic
/// protobuf byte vector already owned by V2 ingress.  The writer calls this
/// form so source provenance is not re-encoded after the receive boundary.
fn raw_records_from_source_payload_v2(
    capture_sequence: u64,
    update: PumpResearchSourceUpdateV1,
    source_payload: Vec<u8>,
) -> Result<Vec<PumpExactStateRawRecordV2>> {
    let PumpResearchSourceUpdateV1 {
        provider_id,
        stream_epoch,
        ingress_wall_ts_ms,
        ingress_monotonic_ts_ms,
        update,
    } = update;
    // Preserve the whole decoded `SubscribeUpdate`, not just its inner
    // variant.  The outer payload carries provider filter labels (and any
    // future protobuf envelope fields), so serializing only Transaction /
    // Account / Block would silently weaken V2 source provenance.
    let event_time = PumpResearchEventTimeV1 {
        chain_event_ts_ms: None,
        ingress_wall_ts_ms: Some(ingress_wall_ts_ms),
        ingress_monotonic_ts_ms: Some(ingress_monotonic_ts_ms),
    };
    let source = |source_payload: &[u8]| PumpExactStateSourceEnvelopeV2 {
        provider_id: provider_id.clone(),
        provider_role: PumpExactStateProviderRoleV2::PrimaryAuthority,
        stream_epoch,
        capture_sequence,
        payload_hash_blake3: hash_bytes_v2(source_payload),
    };

    match update.update_oneof {
        Some(UpdateOneof::Transaction(transaction)) => {
            let transaction_info = transaction.transaction.ok_or_else(|| {
                anyhow::anyhow!("V2 source SubscribeUpdateTransaction lacks transaction info")
            })?;
            let signature = fixed_signature_v2(&transaction_info.signature)
                .context("V2 source SubscribeUpdateTransaction has non-64-byte signature")?;
            let tx_index = u32::try_from(transaction_info.index).ok();
            Ok(vec![PumpExactStateRawRecordV2::PrimaryTransaction(
                PumpExactStateTransactionEvidenceV2 {
                    source: source(&source_payload),
                    slot: transaction.slot,
                    tx_index,
                    signature,
                    event_time,
                    block_time: None,
                    source_payload,
                },
            )])
        }
        Some(UpdateOneof::Account(account_update)) => {
            let account = account_update.account.ok_or_else(|| {
                anyhow::anyhow!("V2 source SubscribeUpdateAccount lacks account payload")
            })?;
            let account_pubkey = fixed_pubkey_v2(&account.pubkey)
                .context("V2 source AccountUpdate has non-32-byte pubkey")?;
            let owner_program = fixed_pubkey_v2(&account.owner)
                .context("V2 source AccountUpdate has non-32-byte owner")?;
            let expected_owner = Pubkey::from_str(PUMP_FUN_PROGRAM_ID)
                .context("compile-time Pump program ID is invalid")?;
            if owner_program.into_inner() != expected_owner.to_bytes() {
                bail!(
                    "V2 stream-only account source emitted account {} with non-Pump owner {}",
                    Pubkey::new_from_array(account_pubkey.into_inner()),
                    Pubkey::new_from_array(owner_program.into_inner()),
                );
            }
            let txn_signature = match account.txn_signature {
                Some(signature) => Some(
                    fixed_signature_v2(&signature)
                        .context("V2 source AccountUpdate has non-64-byte txn_signature")?,
                ),
                None => None,
            };
            let evidence_class = classify_pump_owned_account_evidence_v2(
                account_pubkey.into_inner(),
                &account.data,
            )?;
            let raw_account_data_hash_blake3 = hash_bytes_v2(&account.data);
            Ok(vec![PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(
                PumpExactStatePumpOwnedAccountUpdateV2 {
                    source: source(&source_payload),
                    evidence_class,
                    is_startup: account_update.is_startup,
                    account_pubkey,
                    owner_program,
                    raw_account_data: account.data,
                    raw_account_data_hash_blake3,
                    slot: account_update.slot,
                    write_version: account.write_version,
                    txn_signature,
                    event_time,
                    source_payload,
                },
            )])
        }
        Some(UpdateOneof::Slot(slot_update)) => {
            Ok(vec![PumpExactStateRawRecordV2::PrimarySlotUpdate(
                PumpExactStateSlotEvidenceV2 {
                    source: source(&source_payload),
                    slot: slot_update.slot,
                    parent: slot_update.parent,
                    source_status: slot_update.status,
                    event_time,
                    source_payload,
                },
            )])
        }
        Some(UpdateOneof::BlockMeta(block_meta)) => {
            Ok(vec![PumpExactStateRawRecordV2::PrimaryBlockMeta(
                PumpExactStateBlockMetaEvidenceV2 {
                    source: source(&source_payload),
                    slot: block_meta.slot,
                    parent_slot: block_meta.parent_slot,
                    blockhash: block_meta.blockhash,
                    parent_blockhash: block_meta.parent_blockhash,
                    executed_transaction_count: block_meta.executed_transaction_count,
                    block_time: block_meta.block_time.map(|time| time.timestamp),
                    event_time,
                    source_payload,
                },
            )])
        }
        Some(UpdateOneof::TransactionStatus(_)) => {
            bail!("V2 source profile received unsupported TransactionStatus update")
        }
        Some(UpdateOneof::Block(block)) => {
            let source_payload_sha256 = sha256_storage_hash_v2(&source_payload);
            let source_payload_blake3 = hash_bytes_v2(&source_payload);
            let source_payload_chunk_count = u64::try_from(
                source_payload
                    .chunks(
                        ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2,
                    )
                    .len(),
            )
            .map_err(|_| anyhow::anyhow!("V2 full block source chunk count overflow"))?;
            let source_payload_bytes = u64::try_from(source_payload.len()).unwrap_or(u64::MAX);
            let mut records = Vec::with_capacity(
                usize::try_from(source_payload_chunk_count)
                    .unwrap_or(usize::MAX)
                    .saturating_add(2),
            );
            records.push(PumpExactStateRawRecordV2::FullBlockPayloadStarted(
                PumpExactStateFullBlockPayloadStartedV2 {
                    source: source(&source_payload),
                    slot: block.slot,
                    parent_slot: block.parent_slot,
                    blockhash: block.blockhash,
                    parent_blockhash: block.parent_blockhash,
                    executed_transaction_count: block.executed_transaction_count,
                    event_time,
                    source_payload_sha256,
                    source_payload_bytes,
                    source_payload_chunk_count,
                },
            ));
            for (chunk_index, bytes) in source_payload
                .chunks(
                    ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2,
                )
                .enumerate()
            {
                records.push(PumpExactStateRawRecordV2::FullBlockPayloadChunk(
                    PumpExactStateFullBlockPayloadChunkV2 {
                        source_capture_sequence: capture_sequence,
                        chunk_index: u64::try_from(chunk_index)
                            .map_err(|_| anyhow::anyhow!("V2 full block chunk index overflow"))?,
                        bytes: bytes.to_vec(),
                    },
                ));
            }
            records.push(PumpExactStateRawRecordV2::FullBlockPayloadCompleted(
                PumpExactStateFullBlockPayloadCompletedV2 {
                    source_capture_sequence: capture_sequence,
                    source_payload_blake3,
                    source_payload_sha256,
                    source_payload_bytes,
                    source_payload_chunk_count,
                },
            ));
            Ok(records)
        }
        Some(UpdateOneof::Entry(_)) => {
            bail!("V2 source profile received Entry although Entry is disabled")
        }
        Some(UpdateOneof::Ping(_)) => bail!("V2 source profile received unsupported Ping update"),
        Some(UpdateOneof::Pong(_)) => {
            bail!("Pong must be handled by the gRPC receive loop before V2 raw capture")
        }
        None => bail!("V2 source message has no update_oneof payload"),
    }
}

fn fixed_pubkey_v2(bytes: &[u8]) -> Result<PumpResearchStoragePubkeyV1> {
    let fixed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 32 bytes, got {}", bytes.len()))?;
    Ok(PumpResearchStoragePubkeyV1::from(fixed))
}

fn fixed_signature_v2(bytes: &[u8]) -> Result<PumpResearchStorageSignatureV1> {
    let fixed: [u8; 64] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 64 bytes, got {}", bytes.len()))?;
    Ok(PumpResearchStorageSignatureV1::from(fixed))
}

/// Derive the closed stream-only account evidence class directly from the
/// retained Pump-owned account payload.  The offline raw validator reuses
/// this exact classifier when it binds a convenient raw projection back to
/// the complete retained `SubscribeUpdate`; any account outside the two
/// subscription classes is a source-contract failure, never retained noise.
pub(crate) fn classify_pump_owned_account_evidence_v2(
    account_pubkey: [u8; 32],
    raw_account_data: &[u8],
) -> Result<PumpExactStateAccountEvidenceClassV2> {
    let canonical_global = Pubkey::from_str(PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1)
        .context("canonical Pump Global pubkey is invalid")?;
    if account_pubkey == canonical_global.to_bytes() {
        return Ok(PumpExactStateAccountEvidenceClassV2::CanonicalGlobal);
    }
    if raw_account_data.starts_with(&BONDING_CURVE_DISC) {
        return Ok(PumpExactStateAccountEvidenceClassV2::CanonicalBondingCurve);
    }
    bail!(
        "V2 stream-only account source emitted a Pump account outside canonical Global/BondingCurve scope"
    )
}

fn hash_bytes_v2(bytes: &[u8]) -> ghost_core::pump_research_tape::PumpResearchStorageHashV1 {
    ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(*blake3::hash(bytes).as_bytes())
}

fn wall_clock_ms_v2() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
        .try_into()
        .unwrap_or(u64::MAX)
}

/// Per-segment whole-file receipt used by the prospective V2 run manifest.
/// The receipt is intentionally separate from the raw footer: it binds the
/// fully published file, including that footer, while the footer binds only
/// the prefix available at its own creation time.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateSegmentReceiptV2 {
    pub segment_index: u64,
    pub filename: String,
    pub file_bytes: u64,
    pub file_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    pub file_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    pub first_capture_sequence: Option<u64>,
    pub last_capture_sequence: Option<u64>,
    pub accepted_record_count: u64,
}

struct OpenSegmentV2 {
    index: u64,
    stream_epoch: u64,
    partial_path: PathBuf,
    final_path: PathBuf,
    writer: BufWriter<File>,
    prefix_hasher: blake3::Hasher,
    file_blake3_hasher: blake3::Hasher,
    file_sha256_hasher: Sha256,
    bytes_before_footer: u64,
    records_before_footer: u64,
    first_capture_sequence: Option<u64>,
    last_capture_sequence: Option<u64>,
    opened_at: Instant,
}

struct PumpExactStateOpenFullBlockPayloadV2 {
    expected_source_payload_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    expected_source_payload_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    expected_source_payload_bytes: u64,
    expected_source_payload_chunk_count: u64,
    next_chunk_index: u64,
    observed_source_payload_bytes: u64,
    source_payload_sha256: Sha256,
    source_payload_blake3: blake3::Hasher,
}

/// Writer-owned structural reconciliation for V2 full-block source payloads.
/// The source converter produces a started/chunk/completed triplet, but the
/// writer rechecks that contract as it durably frames records.  Consequently a
/// future change cannot leave a started block or orphaned chunk in a raw run
/// that later claims `Complete`.
#[derive(Default)]
struct PumpExactStateFullBlockReconciliationV2 {
    open: BTreeMap<u64, PumpExactStateOpenFullBlockPayloadV2>,
    started: u64,
    chunks: u64,
    completed: u64,
    unbound_chunks: u64,
}

impl PumpExactStateFullBlockReconciliationV2 {
    fn observe_written_record(&mut self, record: &PumpExactStateRawRecordV2) -> Result<()> {
        match record {
            PumpExactStateRawRecordV2::FullBlockPayloadStarted(value) => {
                let capture_sequence = value.source.capture_sequence;
                if value.source_payload_bytes == 0 || value.source_payload_chunk_count == 0 {
                    bail!(
                        "V2 full block capture sequence {} has an empty payload or zero chunks",
                        capture_sequence
                    );
                }
                if self
                    .open
                    .insert(
                        capture_sequence,
                        PumpExactStateOpenFullBlockPayloadV2 {
                            expected_source_payload_sha256: value.source_payload_sha256,
                            expected_source_payload_blake3: value.source.payload_hash_blake3,
                            expected_source_payload_bytes: value.source_payload_bytes,
                            expected_source_payload_chunk_count: value.source_payload_chunk_count,
                            next_chunk_index: 0,
                            observed_source_payload_bytes: 0,
                            source_payload_sha256: Sha256::new(),
                            source_payload_blake3: blake3::Hasher::new(),
                        },
                    )
                    .is_some()
                {
                    bail!(
                        "V2 full block capture sequence {} started more than once",
                        capture_sequence
                    );
                }
                self.started = self
                    .started
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full block start counter overflow"))?;
            }
            PumpExactStateRawRecordV2::FullBlockPayloadChunk(value) => {
                let Some(open) = self.open.get_mut(&value.source_capture_sequence) else {
                    self.unbound_chunks = self.unbound_chunks.saturating_add(1);
                    bail!(
                        "V2 full block chunk {} has no preceding started record",
                        value.source_capture_sequence
                    );
                };
                if value.chunk_index != open.next_chunk_index {
                    bail!(
                        "V2 full block capture sequence {} expected chunk {} but received {}",
                        value.source_capture_sequence,
                        open.next_chunk_index,
                        value.chunk_index
                    );
                }
                if value.bytes.len()
                    > ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2
                {
                    bail!(
                        "V2 full block capture sequence {} chunk {} exceeds frozen chunk bound",
                        value.source_capture_sequence,
                        value.chunk_index
                    );
                }
                let bytes = u64::try_from(value.bytes.len())
                    .context("V2 full block chunk length does not fit u64")?;
                open.observed_source_payload_bytes = open
                    .observed_source_payload_bytes
                    .checked_add(bytes)
                    .ok_or_else(|| anyhow::anyhow!("V2 full block byte counter overflow"))?;
                if open.observed_source_payload_bytes > open.expected_source_payload_bytes {
                    bail!(
                        "V2 full block capture sequence {} exceeds declared payload bytes",
                        value.source_capture_sequence
                    );
                }
                open.source_payload_sha256.update(&value.bytes);
                open.source_payload_blake3.update(&value.bytes);
                open.next_chunk_index = open
                    .next_chunk_index
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full block chunk index overflow"))?;
                self.chunks = self
                    .chunks
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full block chunk counter overflow"))?;
            }
            PumpExactStateRawRecordV2::FullBlockPayloadCompleted(value) => {
                let Some(open) = self.open.remove(&value.source_capture_sequence) else {
                    bail!(
                        "V2 full block completion {} has no preceding started record",
                        value.source_capture_sequence
                    );
                };
                if open.next_chunk_index != open.expected_source_payload_chunk_count
                    || value.source_payload_chunk_count != open.expected_source_payload_chunk_count
                {
                    bail!(
                        "V2 full block capture sequence {} has incomplete chunk reconciliation",
                        value.source_capture_sequence
                    );
                }
                if open.observed_source_payload_bytes != open.expected_source_payload_bytes
                    || value.source_payload_bytes != open.expected_source_payload_bytes
                {
                    bail!(
                        "V2 full block capture sequence {} has payload byte-count drift",
                        value.source_capture_sequence
                    );
                }
                let sha256: [u8; 32] = open.source_payload_sha256.finalize().into();
                let blake3 = ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
                    *open.source_payload_blake3.finalize().as_bytes(),
                );
                let sha256 =
                    ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(sha256);
                if sha256 != open.expected_source_payload_sha256
                    || sha256 != value.source_payload_sha256
                    || blake3 != open.expected_source_payload_blake3
                    || blake3 != value.source_payload_blake3
                {
                    bail!(
                        "V2 full block capture sequence {} has payload digest drift",
                        value.source_capture_sequence
                    );
                }
                self.completed = self
                    .completed
                    .checked_add(1)
                    .ok_or_else(|| anyhow::anyhow!("V2 full block completion counter overflow"))?;
            }
            _ => {}
        }
        Ok(())
    }

    fn reconcile_at_clean_close(&self) -> Result<()> {
        if !self.open.is_empty() || self.started != self.completed {
            bail!(
                "V2 full block payload reconciliation is incomplete: started={} completed={} open={}",
                self.started,
                self.completed,
                self.open.len(),
            );
        }
        if self.unbound_chunks != 0 {
            bail!(
                "V2 full block payload reconciliation retained {} unbound chunks",
                self.unbound_chunks
            );
        }
        Ok(())
    }

    fn census(&self, fully_reconciled: bool) -> PumpExactStateRequiredLaneCensusV2 {
        PumpExactStateRequiredLaneCensusV2 {
            full_blocks_started: self.started,
            full_block_chunks: self.chunks,
            full_blocks_completed: self.completed,
            incomplete_full_block_payloads: u64::try_from(self.open.len()).unwrap_or(u64::MAX),
            unbound_full_block_chunks: self.unbound_chunks,
            full_block_payloads_reconciled: fully_reconciled,
            ..PumpExactStateRequiredLaneCensusV2::default()
        }
    }
}

/// Isolated V2 raw segment writer.  It is intentionally not a general
/// runtime writer: all callers are the standalone prospective capture path,
/// and all data must first satisfy `raw_records_from_source_v2` or the one
/// stream-readiness boundary contract.
struct PumpExactStateRawSegmentWriterV2 {
    raw_dir: PathBuf,
    run_id: String,
    capture_contract_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    flush_interval: Duration,
    segment_max_bytes: u64,
    segment_max_duration: Duration,
    max_raw_bytes: u64,
    min_free_bytes: u64,
    raw_bytes_written: u64,
    last_storage_floor_check: Instant,
    storage_floor_checked: bool,
    next_index: u64,
    highest_stream_epoch: Option<u64>,
    previous_segment_blake3: Option<ghost_core::pump_research_tape::PumpResearchStorageHashV1>,
    current: Option<OpenSegmentV2>,
    full_block_reconciliation: PumpExactStateFullBlockReconciliationV2,
    last_flush: Instant,
    receipts: Vec<PumpExactStateSegmentReceiptV2>,
}

impl PumpExactStateRawSegmentWriterV2 {
    fn new(
        raw_dir: PathBuf,
        run_id: String,
        capture_contract_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
        max_raw_bytes: u64,
        min_free_bytes: u64,
    ) -> Result<Self> {
        if flush_interval.is_zero() {
            bail!("V2 raw writer flush interval must be greater than zero");
        }
        if segment_max_bytes == 0 {
            bail!("V2 raw writer segment max bytes must be greater than zero");
        }
        if segment_max_duration.is_zero() {
            bail!("V2 raw writer segment max duration must be greater than zero");
        }
        if max_raw_bytes == 0 || min_free_bytes == 0 {
            bail!("V2 raw writer byte budget and storage reserve must be greater than zero");
        }
        fs::create_dir_all(&raw_dir)
            .with_context(|| format!("create V2 raw directory {}", raw_dir.display()))?;
        Ok(Self {
            raw_dir,
            run_id,
            capture_contract_sha256,
            flush_interval,
            segment_max_bytes,
            segment_max_duration,
            max_raw_bytes,
            min_free_bytes,
            raw_bytes_written: 0,
            last_storage_floor_check: Instant::now(),
            storage_floor_checked: false,
            next_index: 0,
            highest_stream_epoch: None,
            previous_segment_blake3: None,
            current: None,
            full_block_reconciliation: PumpExactStateFullBlockReconciliationV2::default(),
            last_flush: Instant::now(),
            receipts: Vec::new(),
        })
    }

    fn receipts(&self) -> &[PumpExactStateSegmentReceiptV2] {
        &self.receipts
    }

    fn raw_bytes_written(&self) -> u64 {
        self.raw_bytes_written
    }

    fn ensure_storage_floor(&mut self) -> Result<()> {
        if self.storage_floor_checked
            && self.last_storage_floor_check.elapsed()
                < Duration::from_millis(V2_STORAGE_FLOOR_CHECK_INTERVAL_MS)
        {
            return Ok(());
        }
        let available = available_filesystem_bytes_v2(&self.raw_dir)?;
        if available < self.min_free_bytes {
            bail!(
                "V2 storage reserve exhausted: {} has {} bytes available below configured floor {}",
                self.raw_dir.display(),
                available,
                self.min_free_bytes,
            );
        }
        self.last_storage_floor_check = Instant::now();
        self.storage_floor_checked = true;
        Ok(())
    }

    fn reserve_raw_bytes(&self, additional_bytes: u64, surface: &str) -> Result<()> {
        let next = self
            .raw_bytes_written
            .checked_add(additional_bytes)
            .ok_or_else(|| {
                anyhow::anyhow!("V2 raw byte counter overflow while writing {surface}")
            })?;
        if next > self.max_raw_bytes {
            bail!(
                "V2 raw byte budget exceeded while writing {surface}: {} would exceed configured {}",
                next,
                self.max_raw_bytes,
            );
        }
        Ok(())
    }

    fn account_raw_bytes_written(&mut self, bytes: usize, surface: &str) -> Result<()> {
        let bytes = u64::try_from(bytes).context("V2 raw frame byte length does not fit u64")?;
        self.raw_bytes_written = self.raw_bytes_written.checked_add(bytes).ok_or_else(|| {
            anyhow::anyhow!("V2 raw byte counter overflow after writing {surface}")
        })?;
        Ok(())
    }

    #[cfg(test)]
    fn write_source(
        &mut self,
        capture_sequence: u64,
        update: PumpResearchSourceUpdateV1,
    ) -> Result<()> {
        let source_payload = update.update.encode_to_vec();
        self.write_source_with_payload(capture_sequence, update, source_payload)
    }

    /// Persist a source update using the deterministic protobuf bytes that
    /// crossed V2 bounded ingress.  This avoids a second source re-encoding
    /// and keeps the stored evidence byte-identical to the queued payload.
    fn write_source_with_payload(
        &mut self,
        capture_sequence: u64,
        update: PumpResearchSourceUpdateV1,
        source_payload: Vec<u8>,
    ) -> Result<()> {
        let stream_epoch = update.stream_epoch;
        if stream_epoch == 0 {
            bail!("V2 source update has zero stream epoch");
        }
        let records = raw_records_from_source_payload_v2(capture_sequence, update, source_payload)?;
        if records.is_empty() {
            bail!("V2 source conversion produced no raw records");
        }
        for record in records {
            self.write_record(stream_epoch, Some(capture_sequence), &record)?;
            self.full_block_reconciliation
                .observe_written_record(&record)?;
        }
        Ok(())
    }

    /// The stream-readiness boundary is written through the same segment chain
    /// but deliberately has no gRPC capture sequence of its own.  It binds a
    /// single established epoch and an exclusive source sequence frontier.
    fn write_stream_boundary(
        &mut self,
        stream_epoch: u64,
        boundary: PumpExactStateProspectiveStreamBoundaryV2,
    ) -> Result<()> {
        if boundary.source_stream_epoch != stream_epoch {
            bail!(
                "V2 stream-readiness boundary epoch {} differs from writer epoch {}",
                boundary.source_stream_epoch,
                stream_epoch
            );
        }
        validate_source_readiness_v2(&boundary.source_readiness)?;
        if boundary.cohort_slots_strictly_after != boundary.source_readiness.source_readiness_slot {
            bail!(
                "V2 stream-readiness boundary cohort slot {} differs from source readiness slot {}",
                boundary.cohort_slots_strictly_after,
                boundary.source_readiness.source_readiness_slot
            );
        }
        self.write_record(
            stream_epoch,
            None,
            &PumpExactStateRawRecordV2::ProspectiveStreamBoundary(boundary),
        )
    }

    /// The readiness control-plane receives its acknowledgement only after
    /// the boundary frame has reached the active segment's buffered writer and
    /// been synchronised to the filesystem.  Cohort timing must never begin
    /// before this succeeds.
    fn flush_active_and_sync(&mut self) -> Result<()> {
        let current = self
            .current
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("V2 stream boundary did not open a raw segment"))?;
        current
            .writer
            .flush()
            .with_context(|| format!("flush V2 raw segment {}", current.partial_path.display()))?;
        current
            .writer
            .get_ref()
            .sync_data()
            .with_context(|| format!("sync V2 raw segment {}", current.partial_path.display()))?;
        self.last_flush = Instant::now();
        Ok(())
    }

    fn full_block_census(&self, fully_reconciled: bool) -> PumpExactStateRequiredLaneCensusV2 {
        self.full_block_reconciliation.census(fully_reconciled)
    }

    fn reconcile_full_blocks_at_clean_close(&self) -> Result<()> {
        self.full_block_reconciliation.reconcile_at_clean_close()
    }

    fn write_record(
        &mut self,
        stream_epoch: u64,
        capture_sequence: Option<u64>,
        record: &PumpExactStateRawRecordV2,
    ) -> Result<()> {
        if matches!(record, PumpExactStateRawRecordV2::SegmentClosed(_)) {
            bail!("V2 raw callers cannot inject a segment footer");
        }
        if let Some(highest) = self.highest_stream_epoch {
            if stream_epoch < highest {
                bail!(
                    "V2 raw writer refuses stream epoch regression from {} to {}",
                    highest,
                    stream_epoch
                );
            }
        }
        self.highest_stream_epoch = Some(
            self.highest_stream_epoch
                .map_or(stream_epoch, |highest| highest.max(stream_epoch)),
        );
        self.ensure_storage_floor()?;
        let frame =
            ghost_core::pump_research_exact_tape_v2::PumpExactStateRawCodecV2::encode_record(
                record,
            )
            .map_err(anyhow::Error::msg)?;
        let frame_bytes =
            u64::try_from(frame.len()).context("V2 record length does not fit u64")?;
        self.reserve_raw_bytes(frame_bytes, "V2 raw record")?;
        self.rotate_before_record(stream_epoch, frame_bytes)?;
        self.ensure_current(stream_epoch)?;
        {
            let current = self
                .current
                .as_mut()
                .ok_or_else(|| anyhow::anyhow!("V2 raw writer lost its open segment"))?;
            if current.stream_epoch != stream_epoch {
                bail!(
                    "V2 raw writer stream epoch {} differs from open segment epoch {}",
                    stream_epoch,
                    current.stream_epoch
                );
            }
            let final_segment_bytes = current
                .bytes_before_footer
                .checked_add(frame_bytes)
                .and_then(|value| value.checked_add(V2_SEGMENT_FOOTER_RESERVE_BYTES))
                .ok_or_else(|| anyhow::anyhow!("V2 segment size calculation overflow"))?;
            if final_segment_bytes > self.segment_max_bytes {
                bail!(
                    "V2 source record plus terminal footer would exceed configured segment cap {}",
                    self.segment_max_bytes
                );
            }
            current.writer.write_all(&frame).with_context(|| {
                format!("write V2 raw segment {}", current.partial_path.display())
            })?;
            current.prefix_hasher.update(&frame);
            current.file_blake3_hasher.update(&frame);
            current.file_sha256_hasher.update(&frame);
            current.bytes_before_footer = current
                .bytes_before_footer
                .checked_add(u64::try_from(frame.len()).unwrap_or(u64::MAX))
                .ok_or_else(|| anyhow::anyhow!("V2 segment byte counter overflow"))?;
            current.records_before_footer = current
                .records_before_footer
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 segment record counter overflow"))?;
            if let Some(sequence) = capture_sequence {
                if current.first_capture_sequence.is_none() {
                    current.first_capture_sequence = Some(sequence);
                }
                current.last_capture_sequence = Some(sequence);
            }
        }
        self.account_raw_bytes_written(frame.len(), "V2 raw record")?;
        if self.last_flush.elapsed() >= self.flush_interval {
            let current = self.current.as_mut().ok_or_else(|| {
                anyhow::anyhow!("V2 raw writer lost its open segment before flush")
            })?;
            current.writer.flush().with_context(|| {
                format!("flush V2 raw segment {}", current.partial_path.display())
            })?;
            self.last_flush = Instant::now();
        }
        Ok(())
    }

    fn rotate_before_record(&mut self, stream_epoch: u64, next_frame_bytes: u64) -> Result<()> {
        let next_frame_with_footer = next_frame_bytes
            .checked_add(V2_SEGMENT_FOOTER_RESERVE_BYTES)
            .ok_or_else(|| anyhow::anyhow!("V2 segment size calculation overflow"))?;
        let should_rotate = self.current.as_ref().is_some_and(|current| {
            current.stream_epoch != stream_epoch
                || current.opened_at.elapsed() >= self.segment_max_duration
                || (current.records_before_footer > 0
                    && current
                        .bytes_before_footer
                        .saturating_add(next_frame_with_footer)
                        > self.segment_max_bytes)
        });
        if should_rotate {
            self.close_current(false)?;
        }
        Ok(())
    }

    fn ensure_current(&mut self, stream_epoch: u64) -> Result<()> {
        if self.current.is_some() {
            return Ok(());
        }
        let index = self.next_index;
        self.next_index = self
            .next_index
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("V2 raw segment index overflow"))?;
        let filename = format!("segment_{index:05}.bin");
        let final_path = self.raw_dir.join(&filename);
        let partial_path = self.raw_dir.join(format!("{filename}.partial"));
        self.ensure_storage_floor()?;
        let header = ghost_core::pump_research_exact_tape_v2::PumpExactStateSegmentHeaderV2 {
            storage_format_version: ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            run_id: self.run_id.clone(),
            segment_index: index,
            stream_epoch,
            opened_wall_ts_ms: wall_clock_ms_v2(),
            opened_monotonic_ts_ms: crate::types::arrival_time_ms(),
            capture_contract_sha256: self.capture_contract_sha256,
            previous_segment_blake3: self.previous_segment_blake3,
        };
        let header_bytes = ghost_core::pump_research_exact_tape_v2::PumpExactStateRawCodecV2::encode_segment_header(&header)
            .map_err(anyhow::Error::msg)?;
        let header_with_footer = u64::try_from(header_bytes.len())
            .context("V2 header length does not fit u64")?
            .checked_add(V2_SEGMENT_FOOTER_RESERVE_BYTES)
            .ok_or_else(|| anyhow::anyhow!("V2 header/footer size calculation overflow"))?;
        if header_with_footer > self.segment_max_bytes {
            bail!(
                "V2 segment max bytes {} cannot hold its header plus frozen footer reserve {}",
                self.segment_max_bytes,
                header_with_footer
            );
        }
        self.reserve_raw_bytes(
            u64::try_from(header_bytes.len()).context("V2 header length does not fit u64")?,
            "V2 raw segment header",
        )?;
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        options
            .mode(0o600)
            .custom_flags(libc::O_CLOEXEC | libc::O_NOFOLLOW);
        let file = options
            .open(&partial_path)
            .with_context(|| format!("create V2 raw segment {}", partial_path.display()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&header_bytes)
            .with_context(|| format!("write V2 raw header {}", partial_path.display()))?;
        self.account_raw_bytes_written(header_bytes.len(), "V2 raw segment header")?;
        let mut prefix_hasher = blake3::Hasher::new();
        prefix_hasher.update(&header_bytes);
        let mut file_blake3_hasher = blake3::Hasher::new();
        file_blake3_hasher.update(&header_bytes);
        let mut file_sha256_hasher = Sha256::new();
        file_sha256_hasher.update(&header_bytes);
        self.current = Some(OpenSegmentV2 {
            index,
            stream_epoch,
            partial_path,
            final_path,
            writer,
            prefix_hasher,
            file_blake3_hasher,
            file_sha256_hasher,
            bytes_before_footer: u64::try_from(header_bytes.len()).unwrap_or(u64::MAX),
            records_before_footer: 0,
            first_capture_sequence: None,
            last_capture_sequence: None,
            opened_at: Instant::now(),
        });
        self.last_flush = Instant::now();
        Ok(())
    }

    fn close_current(
        &mut self,
        clean_shutdown: bool,
    ) -> Result<Option<PumpExactStateSegmentReceiptV2>> {
        if clean_shutdown {
            self.reconcile_full_blocks_at_clean_close()?;
        }
        let Some(mut current) = self.current.take() else {
            return Ok(None);
        };
        let prefix_hash = ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
            *current.prefix_hasher.finalize().as_bytes(),
        );
        let footer = ghost_core::pump_research_exact_tape_v2::PumpExactStateSegmentClosedV2 {
            storage_format_version: ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            segment_index: current.index,
            accepted_record_count: current.records_before_footer,
            data_bytes: current.bytes_before_footer,
            segment_blake3: prefix_hash,
            closed_wall_ts_ms: wall_clock_ms_v2(),
            clean_shutdown,
        };
        let footer_frame =
            ghost_core::pump_research_exact_tape_v2::PumpExactStateRawCodecV2::encode_record(
                &PumpExactStateRawRecordV2::SegmentClosed(footer),
            )
            .map_err(anyhow::Error::msg)?;
        let footer_bytes =
            u64::try_from(footer_frame.len()).context("V2 footer length does not fit u64")?;
        if footer_bytes > V2_SEGMENT_FOOTER_RESERVE_BYTES {
            bail!(
                "V2 frozen footer is {} bytes above configured {}-byte segment footer reserve",
                footer_bytes,
                V2_SEGMENT_FOOTER_RESERVE_BYTES
            );
        }
        let final_segment_bytes = current
            .bytes_before_footer
            .checked_add(footer_bytes)
            .ok_or_else(|| anyhow::anyhow!("V2 segment final byte counter overflow"))?;
        if final_segment_bytes > self.segment_max_bytes {
            bail!(
                "V2 final segment {} bytes exceeds configured cap {}",
                final_segment_bytes,
                self.segment_max_bytes
            );
        }
        self.ensure_storage_floor()?;
        self.reserve_raw_bytes(footer_bytes, "V2 raw segment footer")?;
        current
            .writer
            .write_all(&footer_frame)
            .with_context(|| format!("write V2 raw footer {}", current.partial_path.display()))?;
        current.file_blake3_hasher.update(&footer_frame);
        current.file_sha256_hasher.update(&footer_frame);
        self.account_raw_bytes_written(footer_frame.len(), "V2 raw segment footer")?;
        current
            .writer
            .flush()
            .with_context(|| format!("flush V2 raw segment {}", current.partial_path.display()))?;
        current
            .writer
            .get_ref()
            .sync_all()
            .with_context(|| format!("sync V2 raw segment {}", current.partial_path.display()))?;
        drop(current.writer);
        fs::rename(&current.partial_path, &current.final_path).with_context(|| {
            format!(
                "atomically publish V2 raw segment {} -> {}",
                current.partial_path.display(),
                current.final_path.display()
            )
        })?;
        sync_directory_v2(&self.raw_dir)?;
        let file_bytes = current
            .bytes_before_footer
            .checked_add(u64::try_from(footer_frame.len()).unwrap_or(u64::MAX))
            .ok_or_else(|| anyhow::anyhow!("V2 segment file byte counter overflow"))?;
        let receipt = PumpExactStateSegmentReceiptV2 {
            segment_index: current.index,
            filename: current
                .final_path
                .file_name()
                .and_then(|name| name.to_str())
                .ok_or_else(|| anyhow::anyhow!("V2 segment filename is not UTF-8"))?
                .to_owned(),
            file_bytes,
            file_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
                <[u8; 32]>::from(current.file_sha256_hasher.finalize()),
            ),
            file_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
                *current.file_blake3_hasher.finalize().as_bytes(),
            ),
            first_capture_sequence: current.first_capture_sequence,
            last_capture_sequence: current.last_capture_sequence,
            accepted_record_count: current.records_before_footer,
        };
        self.previous_segment_blake3 = Some(prefix_hash);
        self.receipts.push(receipt.clone());
        Ok(Some(receipt))
    }
}

fn sync_directory_v2(path: &Path) -> Result<()> {
    let directory = File::open(path)
        .with_context(|| format!("open V2 raw directory {} for sync", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("sync V2 raw directory {}", path.display()))
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum PumpExactStateCaptureFatalReasonV2 {
    None = 0,
    DropControlLaneSaturated = 1,
    DropControlLaneDisconnected = 2,
    DataLaneDisconnected = 3,
    ReadinessQueueDisconnected = 4,
    ReadinessQueueTimeout = 5,
    WriterFailure = 6,
    WriterPanic = 7,
    WriterJoinPanic = 8,
    ReadinessNotSealed = 9,
    SourceStreamEpochChanged = 10,
    SourceUpdateEpochMismatch = 11,
    SourceQueueByteBudgetExceeded = 12,
    SourceStreamInterrupted = 13,
    SourceRequiredLaneMalformed = 14,
}

impl PumpExactStateCaptureFatalReasonV2 {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::None,
            1 => Self::DropControlLaneSaturated,
            2 => Self::DropControlLaneDisconnected,
            3 => Self::DataLaneDisconnected,
            4 => Self::ReadinessQueueDisconnected,
            5 => Self::ReadinessQueueTimeout,
            6 => Self::WriterFailure,
            7 => Self::WriterPanic,
            8 => Self::WriterJoinPanic,
            9 => Self::ReadinessNotSealed,
            10 => Self::SourceStreamEpochChanged,
            11 => Self::SourceUpdateEpochMismatch,
            12 => Self::SourceQueueByteBudgetExceeded,
            13 => Self::SourceStreamInterrupted,
            14 => Self::SourceRequiredLaneMalformed,
            _ => Self::WriterFailure,
        }
    }

    const fn message(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::DropControlLaneSaturated => Some(
                "V2 drop-control lane saturated before a typed coverage gap could be persisted",
            ),
            Self::DropControlLaneDisconnected => Some(
                "V2 drop-control lane disconnected before a typed coverage gap could be persisted",
            ),
            Self::DataLaneDisconnected => {
                Some("V2 source data lane disconnected while source updates were arriving")
            }
            Self::ReadinessQueueDisconnected => {
                Some("V2 readiness queue disconnected before the stream boundary was persisted")
            }
            Self::ReadinessQueueTimeout => {
                Some("V2 readiness queue did not drain within its bounded persistence deadline")
            }
            Self::WriterFailure => Some("V2 raw writer failed"),
            Self::WriterPanic => Some("V2 raw writer thread panicked"),
            Self::WriterJoinPanic => {
                Some("V2 raw writer thread panicked before reporting a failure")
            }
            Self::ReadinessNotSealed => {
                Some("V2 capture ended without exactly one durable stream-readiness boundary")
            }
            Self::SourceStreamEpochChanged => Some(
                "V2 Yellowstone source reconnected or changed stream epoch; a prospective run cannot claim continuous source coverage across that boundary",
            ),
            Self::SourceUpdateEpochMismatch => Some(
                "V2 source update epoch did not match the one established prospective source epoch",
            ),
            Self::SourceQueueByteBudgetExceeded => Some(
                "V2 decoded Yellowstone source backlog exceeded its configured byte budget",
            ),
            Self::SourceStreamInterrupted => Some(
                "V2 Yellowstone source stream was interrupted after establishment; a prospective run cannot claim continuity across that boundary",
            ),
            Self::SourceRequiredLaneMalformed => Some(
                "V2 required Yellowstone source lane carried malformed or unsupported slot evidence",
            ),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
enum PumpExactStateReadinessStatusV2 {
    Pending = 0,
    Persisting = 1,
    Complete = 2,
    Failed = 3,
}

impl PumpExactStateReadinessStatusV2 {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Pending,
            1 => Self::Persisting,
            2 => Self::Complete,
            3 => Self::Failed,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateCaptureSourceLifecycleV2 {
    pub(crate) stream_established: bool,
    pub(crate) established_stream_epoch: Option<u64>,
    pub(crate) source_updates_received: u64,
    pub(crate) admitted_source_updates: u64,
    pub(crate) dropped_source_updates: u64,
    pub(crate) source_queue_peak_bytes: u64,
    pub(crate) source_queue_bytes_at_close: u64,
    pub(crate) source_workers_cleanly_stopped: bool,
    pub(crate) required_lane_first_slots: Option<PumpExactStateSourceReadinessV2>,
    pub(crate) source_readiness_status: String,
    pub(crate) fatal_capture_error: Option<String>,
    pub(crate) source_worker_error: Option<String>,
}

/// Persisted source-lane census.  This is writer-owned: a lane is counted
/// only after its decoded `SubscribeUpdate` has been converted and durably
/// written into the V2 segment chain.  It is therefore independent from the
/// ingress admission counters used to establish the stream boundary.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateRequiredLaneCensusV2 {
    pub(crate) transaction_messages: u64,
    pub(crate) account_updates: u64,
    pub(crate) slot_updates: u64,
    pub(crate) block_meta_updates: u64,
    pub(crate) full_blocks_started: u64,
    pub(crate) full_block_chunks: u64,
    pub(crate) full_blocks_completed: u64,
    pub(crate) incomplete_full_block_payloads: u64,
    pub(crate) unbound_full_block_chunks: u64,
    pub(crate) full_block_payloads_reconciled: bool,
}

impl PumpExactStateRequiredLaneCensusV2 {
    fn record_source_lane(&mut self, lane: PumpExactStateRequiredSourceLaneV2) -> Result<()> {
        let (counter, name) = match lane {
            PumpExactStateRequiredSourceLaneV2::Transaction => {
                (&mut self.transaction_messages, "transaction")
            }
            PumpExactStateRequiredSourceLaneV2::Account => (&mut self.account_updates, "account"),
            PumpExactStateRequiredSourceLaneV2::Slot => (&mut self.slot_updates, "slot"),
            PumpExactStateRequiredSourceLaneV2::BlockMeta => {
                (&mut self.block_meta_updates, "block-meta")
            }
            PumpExactStateRequiredSourceLaneV2::FullBlock => return Ok(()),
        };
        *counter = counter
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("V2 {name} required-lane census overflow"))?;
        Ok(())
    }

    fn all_required_lanes_observed(&self) -> bool {
        self.transaction_messages > 0
            && self.account_updates > 0
            && self.slot_updates > 0
            && self.block_meta_updates > 0
            && self.full_blocks_started > 0
            && self.full_blocks_started == self.full_blocks_completed
            && self.incomplete_full_block_payloads == 0
            && self.unbound_full_block_chunks == 0
            && self.full_block_payloads_reconciled
    }

    fn apply_full_block_reconciliation(&mut self, full_blocks: &Self) {
        self.full_blocks_started = full_blocks.full_blocks_started;
        self.full_block_chunks = full_blocks.full_block_chunks;
        self.full_blocks_completed = full_blocks.full_blocks_completed;
        self.incomplete_full_block_payloads = full_blocks.incomplete_full_block_payloads;
        self.unbound_full_block_chunks = full_blocks.unbound_full_block_chunks;
        self.full_block_payloads_reconciled = full_blocks.full_block_payloads_reconciled;
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateWriterSummaryV2 {
    pub(crate) segments: Vec<PumpExactStateSegmentReceiptV2>,
    pub(crate) raw_bytes_written: u64,
    pub(crate) accepted_source_records: u64,
    pub(crate) accepted_readiness_boundary_records: u64,
    pub(crate) required_lane_census: PumpExactStateRequiredLaneCensusV2,
    pub(crate) persisted_ingress_gap_missing_events: u64,
    pub(crate) persisted_ingress_gap_episodes: u64,
    pub(crate) gap_count: u64,
    pub(crate) clean_shutdown: bool,
    pub(crate) error: Option<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpExactStateRequiredSourceLaneV2 {
    Transaction,
    Account,
    Slot,
    BlockMeta,
    FullBlock,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpExactStateRequiredLaneObservationV2 {
    lane: PumpExactStateRequiredSourceLaneV2,
    slot: u64,
}

struct QueuedSourceUpdateV2 {
    capture_sequence: u64,
    byte_cost: u64,
    provider_id: String,
    stream_epoch: u64,
    ingress_wall_ts_ms: u64,
    ingress_monotonic_ts_ms: u64,
    required_lane: PumpExactStateRequiredLaneObservationV2,
    boundary: PumpExactStateCoverageBoundaryV2,
    /// The queue owns one deterministic protobuf encoding rather than the
    /// decoded Yellowstone object graph.  This makes its byte budget an exact
    /// bound on the retained source payload plus fixed envelope headroom.
    encoded_update: Vec<u8>,
}

impl QueuedSourceUpdateV2 {
    fn decode_update(&self) -> Result<SubscribeUpdate> {
        SubscribeUpdate::decode(self.encoded_update.as_slice())
            .context("decode queued V2 source protobuf payload")
    }

    fn into_dropped_source(self, queue_high_water: usize) -> DroppedSourceUpdateV2 {
        DroppedSourceUpdateV2 {
            capture_sequence: self.capture_sequence,
            provider_id: self.provider_id,
            stream_epoch: self.stream_epoch,
            boundary: self.boundary,
            queue_high_water,
        }
    }
}

struct DroppedSourceUpdateV2 {
    capture_sequence: u64,
    provider_id: String,
    stream_epoch: u64,
    boundary: PumpExactStateCoverageBoundaryV2,
    queue_high_water: usize,
}

enum CaptureControlV2 {
    DroppedSource(DroppedSourceUpdateV2),
}

struct QueuedStreamBoundaryV2 {
    /// A reserved position in the global ordered-ingress sequence.  It is not
    /// a Yellowstone source update: every source update before it belongs to
    /// the warm-up prefix, and the next source update is assigned the
    /// following sequence value.  Reserving this position prevents a writer
    /// race in which a post-boundary source frame could otherwise be written
    /// before the durable boundary control frame arrives on its own lane.
    ordering_sequence: u64,
    stream_epoch: u64,
    boundary: PumpExactStateProspectiveStreamBoundaryV2,
    acknowledgement: crossbeam_channel::Sender<std::result::Result<(), String>>,
}

enum OrderedIngressEventV2 {
    Source(QueuedSourceUpdateV2),
    Dropped(DroppedSourceUpdateV2),
    ReadinessBoundary(QueuedStreamBoundaryV2),
}

/// Bounded capture ingress used directly by the standalone V2 Yellowstone
/// receive task.  The receive path does atomic accounting and `try_send`
/// only; typed gaps and cancellation are writer/control-plane work.
struct PumpExactStateCaptureIngressV2 {
    data_tx: crossbeam_channel::Sender<QueuedSourceUpdateV2>,
    control_tx: crossbeam_channel::Sender<CaptureControlV2>,
    data_capacity: usize,
    data_byte_capacity: u64,
    queued_source_bytes: AtomicU64,
    peak_queued_source_bytes: AtomicU64,
    next_capture_sequence: AtomicU64,
    readiness_boundary_sequence: AtomicU64,
    final_capture_sequence: AtomicU64,
    accepting: AtomicBool,
    active_capture_calls: AtomicUsize,
    finish_started: AtomicBool,
    source_finished: AtomicBool,
    stream_established: AtomicBool,
    established_stream_epoch: AtomicU64,
    stream_ready: tokio::sync::Notify,
    first_transaction_slot: AtomicU64,
    first_account_update_slot: AtomicU64,
    first_slot_update_slot: AtomicU64,
    first_block_meta_slot: AtomicU64,
    first_full_block_slot: AtomicU64,
    source_updates_received: AtomicU64,
    admitted_source_updates: AtomicU64,
    source_workers_cleanly_stopped: AtomicBool,
    dropped_data_records: AtomicU64,
    fatal_capture_reason: AtomicU8,
    fatal_source_cancel_dispatched: AtomicBool,
    source_worker_error: Mutex<Option<String>>,
    capture_abort: CancellationToken,
}

struct PumpExactStateCaptureAdmissionGuardV2<'a> {
    active_capture_calls: &'a AtomicUsize,
}

impl Drop for PumpExactStateCaptureAdmissionGuardV2<'_> {
    fn drop(&mut self) {
        self.active_capture_calls.fetch_sub(1, Ordering::Release);
    }
}

impl PumpExactStateCaptureIngressV2 {
    fn new(
        data_tx: crossbeam_channel::Sender<QueuedSourceUpdateV2>,
        control_tx: crossbeam_channel::Sender<CaptureControlV2>,
        data_capacity: usize,
        data_byte_capacity: u64,
        capture_abort: CancellationToken,
    ) -> Self {
        Self {
            data_tx,
            control_tx,
            data_capacity,
            data_byte_capacity,
            queued_source_bytes: AtomicU64::new(0),
            peak_queued_source_bytes: AtomicU64::new(0),
            next_capture_sequence: AtomicU64::new(0),
            readiness_boundary_sequence: AtomicU64::new(V2_READINESS_BOUNDARY_SEQUENCE_UNSET),
            final_capture_sequence: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            active_capture_calls: AtomicUsize::new(0),
            finish_started: AtomicBool::new(false),
            source_finished: AtomicBool::new(false),
            stream_established: AtomicBool::new(false),
            established_stream_epoch: AtomicU64::new(0),
            stream_ready: tokio::sync::Notify::new(),
            first_transaction_slot: AtomicU64::new(V2_REQUIRED_LANE_SLOT_UNSET),
            first_account_update_slot: AtomicU64::new(V2_REQUIRED_LANE_SLOT_UNSET),
            first_slot_update_slot: AtomicU64::new(V2_REQUIRED_LANE_SLOT_UNSET),
            first_block_meta_slot: AtomicU64::new(V2_REQUIRED_LANE_SLOT_UNSET),
            first_full_block_slot: AtomicU64::new(V2_REQUIRED_LANE_SLOT_UNSET),
            source_updates_received: AtomicU64::new(0),
            admitted_source_updates: AtomicU64::new(0),
            source_workers_cleanly_stopped: AtomicBool::new(false),
            dropped_data_records: AtomicU64::new(0),
            fatal_capture_reason: AtomicU8::new(PumpExactStateCaptureFatalReasonV2::None as u8),
            fatal_source_cancel_dispatched: AtomicBool::new(false),
            source_worker_error: Mutex::new(None),
            capture_abort,
        }
    }

    fn capture_abort(&self) -> CancellationToken {
        self.capture_abort.clone()
    }

    fn established_stream_epoch(&self) -> Option<u64> {
        let epoch = self.established_stream_epoch.load(Ordering::Acquire);
        (self.stream_established.load(Ordering::Acquire) && epoch != 0).then_some(epoch)
    }

    fn record_fatal_capture_error(&self, reason: PumpExactStateCaptureFatalReasonV2) -> bool {
        self.fatal_capture_reason
            .compare_exchange(
                PumpExactStateCaptureFatalReasonV2::None as u8,
                reason as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn fatal_capture_reason(&self) -> PumpExactStateCaptureFatalReasonV2 {
        PumpExactStateCaptureFatalReasonV2::from_raw(
            self.fatal_capture_reason.load(Ordering::Acquire),
        )
    }

    fn cancel_source_from_writer_if_fatal(&self) -> bool {
        if matches!(
            self.fatal_capture_reason(),
            PumpExactStateCaptureFatalReasonV2::None
        ) {
            return false;
        }
        if self
            .fatal_source_cancel_dispatched
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.capture_abort.cancel();
            return true;
        }
        false
    }

    fn try_reserve_source_bytes(&self, byte_cost: u64) -> bool {
        loop {
            let current = self.queued_source_bytes.load(Ordering::Acquire);
            let Some(next) = current.checked_add(byte_cost) else {
                return false;
            };
            if next > self.data_byte_capacity {
                return false;
            }
            if self
                .queued_source_bytes
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                self.peak_queued_source_bytes
                    .fetch_max(next, Ordering::AcqRel);
                return true;
            }
        }
    }

    fn release_source_bytes(&self, byte_cost: u64) -> Result<()> {
        loop {
            let current = self.queued_source_bytes.load(Ordering::Acquire);
            let Some(next) = current.checked_sub(byte_cost) else {
                self.record_fatal_capture_error(PumpExactStateCaptureFatalReasonV2::WriterFailure);
                self.cancel_source_from_writer_if_fatal();
                bail!(
                    "V2 source byte accounting underflow: release {} from queued {}",
                    byte_cost,
                    current
                );
            };
            if self
                .queued_source_bytes
                .compare_exchange(current, next, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Ok(());
            }
        }
    }

    fn record_dropped_source(&self, dropped: DroppedSourceUpdateV2) {
        self.dropped_data_records.fetch_add(1, Ordering::Relaxed);
        match self
            .control_tx
            .try_send(CaptureControlV2::DroppedSource(dropped))
        {
            Ok(()) => {}
            Err(crossbeam_channel::TrySendError::Full(_)) => {
                self.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::DropControlLaneSaturated,
                );
                self.cancel_source_from_writer_if_fatal();
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                self.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::DropControlLaneDisconnected,
                );
                self.cancel_source_from_writer_if_fatal();
            }
        }
    }

    fn required_lane_slot_cell(&self, lane: PumpExactStateRequiredSourceLaneV2) -> &AtomicU64 {
        match lane {
            PumpExactStateRequiredSourceLaneV2::Transaction => &self.first_transaction_slot,
            PumpExactStateRequiredSourceLaneV2::Account => &self.first_account_update_slot,
            PumpExactStateRequiredSourceLaneV2::Slot => &self.first_slot_update_slot,
            PumpExactStateRequiredSourceLaneV2::BlockMeta => &self.first_block_meta_slot,
            PumpExactStateRequiredSourceLaneV2::FullBlock => &self.first_full_block_slot,
        }
    }

    fn record_required_lane_admission(
        &self,
        observation: PumpExactStateRequiredLaneObservationV2,
    ) -> Result<()> {
        if observation.slot == V2_REQUIRED_LANE_SLOT_UNSET {
            bail!(
                "V2 required source lane {:?} reported reserved slot value {}",
                observation.lane,
                observation.slot
            );
        }
        let cell = self.required_lane_slot_cell(observation.lane);
        let _ = cell.compare_exchange(
            V2_REQUIRED_LANE_SLOT_UNSET,
            observation.slot,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.stream_ready.notify_waiters();
        Ok(())
    }

    fn required_lane_readiness(&self) -> Option<PumpExactStateSourceReadinessV2> {
        let first_transaction_slot = self.first_transaction_slot.load(Ordering::Acquire);
        let first_account_update_slot = self.first_account_update_slot.load(Ordering::Acquire);
        let first_slot_update_slot = self.first_slot_update_slot.load(Ordering::Acquire);
        let first_block_meta_slot = self.first_block_meta_slot.load(Ordering::Acquire);
        let first_full_block_slot = self.first_full_block_slot.load(Ordering::Acquire);
        let slots = [
            first_transaction_slot,
            first_account_update_slot,
            first_slot_update_slot,
            first_block_meta_slot,
            first_full_block_slot,
        ];
        if slots
            .iter()
            .any(|slot| *slot == V2_REQUIRED_LANE_SLOT_UNSET)
        {
            return None;
        }
        let source_readiness_slot = slots.into_iter().max()?;
        Some(PumpExactStateSourceReadinessV2 {
            first_transaction_slot,
            first_account_update_slot,
            first_slot_update_slot,
            first_block_meta_slot,
            first_full_block_slot,
            source_readiness_slot,
        })
    }

    fn lifecycle(
        &self,
        readiness_status: PumpExactStateReadinessStatusV2,
    ) -> PumpExactStateCaptureSourceLifecycleV2 {
        let epoch = self.established_stream_epoch.load(Ordering::Acquire);
        PumpExactStateCaptureSourceLifecycleV2 {
            stream_established: self.stream_established.load(Ordering::Acquire),
            established_stream_epoch: (epoch != 0).then_some(epoch),
            source_updates_received: self.source_updates_received.load(Ordering::Acquire),
            admitted_source_updates: self.admitted_source_updates.load(Ordering::Acquire),
            dropped_source_updates: self.dropped_data_records.load(Ordering::Acquire),
            source_queue_peak_bytes: self.peak_queued_source_bytes.load(Ordering::Acquire),
            source_queue_bytes_at_close: self.queued_source_bytes.load(Ordering::Acquire),
            source_workers_cleanly_stopped: self
                .source_workers_cleanly_stopped
                .load(Ordering::Acquire),
            required_lane_first_slots: self.required_lane_readiness(),
            source_readiness_status: match readiness_status {
                PumpExactStateReadinessStatusV2::Pending => "pending",
                PumpExactStateReadinessStatusV2::Persisting => "persisting",
                PumpExactStateReadinessStatusV2::Complete => "complete",
                PumpExactStateReadinessStatusV2::Failed => "failed",
            }
            .to_owned(),
            fatal_capture_error: self.fatal_capture_reason().message().map(str::to_owned),
            source_worker_error: self.source_worker_error.lock().clone(),
        }
    }

    fn finish(&self) {
        if self.finish_started.swap(true, Ordering::AcqRel) {
            return;
        }
        self.accepting.store(false, Ordering::Release);
        while self.active_capture_calls.load(Ordering::Acquire) != 0 {
            std::thread::yield_now();
        }
        self.final_capture_sequence
            .store(self.current_capture_sequence_exclusive(), Ordering::Release);
        self.source_finished.store(true, Ordering::Release);
    }

    fn current_capture_sequence_exclusive(&self) -> u64 {
        loop {
            let current = self.next_capture_sequence.load(Ordering::Acquire);
            if current & V2_CAPTURE_SEQUENCE_RESERVING_BIT == 0 {
                return current;
            }
            std::hint::spin_loop();
        }
    }

    fn allocate_source_capture_sequence(&self) -> Option<u64> {
        loop {
            let current = self.next_capture_sequence.load(Ordering::Acquire);
            if current & V2_CAPTURE_SEQUENCE_RESERVING_BIT != 0 {
                // The one-time control-plane reservation is only a pair of
                // atomic stores.  Waiting here preserves lossless ordering;
                // silently dropping an update during that interval would
                // instead violate the prospective source contract.
                std::hint::spin_loop();
                continue;
            }
            if current >= V2_CAPTURE_SEQUENCE_RESERVING_BIT - 1 {
                return None;
            }
            if self
                .next_capture_sequence
                .compare_exchange(current, current + 1, Ordering::AcqRel, Ordering::Acquire)
                .is_ok()
            {
                return Some(current);
            }
        }
    }

    /// Reserve one position in the globally ordered ingress sequence for the
    /// durable stream-readiness control record.  The returned value is the
    /// exclusive source prefix bound into the raw record; source updates after
    /// it receive strictly larger sequence numbers.
    fn reserve_readiness_boundary_sequence(&self) -> u64 {
        loop {
            let current = self.next_capture_sequence.load(Ordering::Acquire);
            if current & V2_CAPTURE_SEQUENCE_RESERVING_BIT != 0 {
                std::hint::spin_loop();
                continue;
            }
            if current >= V2_CAPTURE_SEQUENCE_RESERVING_BIT - 1 {
                // This cannot recover into a valid ordering domain.  The
                // caller treats a zero/invalid boundary as a fail-closed
                // capture error before a receipt can claim Complete.
                return V2_READINESS_BOUNDARY_SEQUENCE_UNSET;
            }
            if self
                .next_capture_sequence
                .compare_exchange(
                    current,
                    current | V2_CAPTURE_SEQUENCE_RESERVING_BIT,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                self.readiness_boundary_sequence
                    .store(current, Ordering::Release);
                self.next_capture_sequence
                    .store(current + 1, Ordering::Release);
                return current;
            }
        }
    }

    fn readiness_boundary_sequence(&self) -> Option<u64> {
        let sequence = self.readiness_boundary_sequence.load(Ordering::Acquire);
        (sequence != V2_READINESS_BOUNDARY_SEQUENCE_UNSET).then_some(sequence)
    }

    fn try_begin_capture(&self) -> Option<PumpExactStateCaptureAdmissionGuardV2<'_>> {
        if !self.accepting.load(Ordering::Acquire) {
            return None;
        }
        self.active_capture_calls.fetch_add(1, Ordering::AcqRel);
        if !self.accepting.load(Ordering::Acquire) {
            self.active_capture_calls.fetch_sub(1, Ordering::Release);
            return None;
        }
        Some(PumpExactStateCaptureAdmissionGuardV2 {
            active_capture_calls: &self.active_capture_calls,
        })
    }

    async fn wait_for_required_source_lanes(
        &self,
        timeout: Duration,
    ) -> Result<PumpExactStateSourceReadinessV2> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let notified = self.stream_ready.notified();
            if let Some(readiness) = self.required_lane_readiness() {
                return Ok(readiness);
            }
            if let Some(reason) = self.fatal_capture_reason().message() {
                bail!("V2 source readiness cannot be established: {reason}");
            }
            if self.capture_abort.is_cancelled() {
                bail!("V2 source readiness cancelled before every required lane was observed");
            }
            tokio::time::timeout_at(deadline, notified)
                .await
                .map_err(|_| {
                    anyhow::anyhow!(
                        "V2 source did not admit transaction/account/slot/block-meta/full-block evidence before readiness deadline"
                    )
                })?;
        }
    }
}

impl PumpResearchSourceSinkV1 for PumpExactStateCaptureIngressV2 {
    fn source_stream_established(&self, stream_epoch: u64) {
        if stream_epoch == 0 {
            self.record_fatal_capture_error(PumpExactStateCaptureFatalReasonV2::WriterFailure);
            self.cancel_source_from_writer_if_fatal();
            return;
        }
        let previous_epoch = self.established_stream_epoch.load(Ordering::Acquire);
        if previous_epoch != 0 && previous_epoch != stream_epoch {
            self.record_fatal_capture_error(
                PumpExactStateCaptureFatalReasonV2::SourceStreamEpochChanged,
            );
            self.cancel_source_from_writer_if_fatal();
            self.stream_ready.notify_waiters();
            return;
        }
        self.established_stream_epoch
            .store(stream_epoch, Ordering::Release);
        self.stream_established.store(true, Ordering::Release);
        self.stream_ready.notify_waiters();
    }

    fn source_stream_interrupted(&self, stream_epoch: u64, error: String) {
        let established_epoch = self.established_stream_epoch.load(Ordering::Acquire);
        let reason = if established_epoch == 0 || stream_epoch != established_epoch {
            PumpExactStateCaptureFatalReasonV2::SourceUpdateEpochMismatch
        } else {
            PumpExactStateCaptureFatalReasonV2::SourceStreamInterrupted
        };
        self.record_fatal_capture_error(reason);
        self.source_worker_failed(format!(
            "V2 established source stream epoch {stream_epoch} interrupted: {error}"
        ));
        self.cancel_source_from_writer_if_fatal();
        self.stream_ready.notify_waiters();
    }

    fn try_capture(&self, update: PumpResearchSourceUpdateV1) {
        let established_epoch = self.established_stream_epoch.load(Ordering::Acquire);
        if !self.stream_established.load(Ordering::Acquire)
            || established_epoch == 0
            || update.stream_epoch != established_epoch
        {
            self.record_fatal_capture_error(
                PumpExactStateCaptureFatalReasonV2::SourceUpdateEpochMismatch,
            );
            self.cancel_source_from_writer_if_fatal();
            return;
        }
        let Some(_admission) = self.try_begin_capture() else {
            return;
        };
        let required_lane = match required_source_lane_observation_v2(&update) {
            Ok(observation) => observation,
            Err(error) => {
                self.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::SourceRequiredLaneMalformed,
                );
                self.source_worker_failed(format!(
                    "V2 required source lane cannot establish readiness: {error:#}"
                ));
                self.cancel_source_from_writer_if_fatal();
                return;
            }
        };
        let Some(capture_sequence) = self.allocate_source_capture_sequence() else {
            self.record_fatal_capture_error(PumpExactStateCaptureFatalReasonV2::WriterFailure);
            self.source_worker_failed(
                "V2 source capture sequence exhausted its reserved ordering domain".to_owned(),
            );
            self.cancel_source_from_writer_if_fatal();
            return;
        };
        self.source_updates_received.fetch_add(1, Ordering::Relaxed);
        let dropped = DroppedSourceUpdateV2 {
            capture_sequence,
            provider_id: update.provider_id.clone(),
            stream_epoch: update.stream_epoch,
            boundary: source_raw_boundary_v2(&update),
            queue_high_water: self.data_capacity,
        };
        let Some(queued) = queued_source_update_v2(capture_sequence, update, required_lane) else {
            self.record_fatal_capture_error(
                PumpExactStateCaptureFatalReasonV2::SourceQueueByteBudgetExceeded,
            );
            self.record_dropped_source(dropped);
            self.cancel_source_from_writer_if_fatal();
            return;
        };
        let byte_cost = queued.byte_cost;
        let admitted_lane = queued.required_lane;
        if !self.try_reserve_source_bytes(byte_cost) {
            self.record_fatal_capture_error(
                PumpExactStateCaptureFatalReasonV2::SourceQueueByteBudgetExceeded,
            );
            self.record_dropped_source(dropped);
            self.cancel_source_from_writer_if_fatal();
            return;
        }
        match self.data_tx.try_send(queued) {
            Ok(()) => {
                self.admitted_source_updates.fetch_add(1, Ordering::Relaxed);
                if let Err(error) = self.record_required_lane_admission(admitted_lane) {
                    self.record_fatal_capture_error(
                        PumpExactStateCaptureFatalReasonV2::SourceRequiredLaneMalformed,
                    );
                    self.source_worker_failed(format!(
                        "V2 required source lane admission could not be recorded: {error:#}"
                    ));
                    self.cancel_source_from_writer_if_fatal();
                }
            }
            Err(crossbeam_channel::TrySendError::Full(queued)) => {
                if let Err(error) = self.release_source_bytes(queued.byte_cost) {
                    self.source_worker_failed(format!(
                        "V2 source byte release after full queue failed: {error:#}"
                    ));
                }
                self.record_dropped_source(queued.into_dropped_source(self.data_capacity));
            }
            Err(crossbeam_channel::TrySendError::Disconnected(queued)) => {
                if let Err(error) = self.release_source_bytes(queued.byte_cost) {
                    self.source_worker_failed(format!(
                        "V2 source byte release after disconnected queue failed: {error:#}"
                    ));
                }
                self.record_dropped_source(queued.into_dropped_source(self.data_capacity));
                self.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::DataLaneDisconnected,
                );
                self.cancel_source_from_writer_if_fatal();
            }
        }
    }

    fn source_worker_failed(&self, error: String) {
        let mut state = self.source_worker_error.lock();
        if state.is_none() {
            *state = Some(error);
        }
    }

    fn source_workers_stopped_cleanly(&self) {
        self.source_workers_cleanly_stopped
            .store(true, Ordering::Release);
    }

    fn finish_source(&self) {
        self.finish();
    }
}

/// V2 writer/coordinator.  The stream-readiness boundary has a one-item
/// control lane so the Yellowstone receive task never blocks on persistence.
/// The control plane waits for a durable acknowledgement before it starts the
/// cohort timer; any queue failure, timeout, second boundary, or missing seal
/// fails the capture closed.
pub(crate) struct PumpExactStateCaptureCoordinatorV2 {
    ingress: Arc<PumpExactStateCaptureIngressV2>,
    readiness_tx: crossbeam_channel::Sender<QueuedStreamBoundaryV2>,
    readiness_status: Arc<AtomicU8>,
    join: Mutex<Option<JoinHandle<()>>>,
    progress: Arc<Mutex<PumpExactStateWriterSummaryV2>>,
}

impl PumpExactStateCaptureCoordinatorV2 {
    pub(crate) fn start(
        raw_dir: &Path,
        run_id: String,
        capture_contract_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
        queue_capacity: usize,
        source_queue_max_bytes: u64,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
        max_raw_bytes: u64,
        min_free_bytes: u64,
    ) -> Result<Self> {
        if queue_capacity == 0 || source_queue_max_bytes == 0 {
            bail!("V2 capture queue capacities and source byte capacity must be greater than zero");
        }
        if max_raw_bytes == 0 || min_free_bytes == 0 {
            bail!("V2 capture raw byte budget and storage reserve must be greater than zero");
        }
        let (data_tx, data_rx) = crossbeam_channel::bounded(queue_capacity);
        let (control_tx, control_rx) = crossbeam_channel::bounded(queue_capacity);
        let (readiness_tx, readiness_rx) = crossbeam_channel::bounded(1);
        let capture_abort = CancellationToken::new();
        let ingress = Arc::new(PumpExactStateCaptureIngressV2::new(
            data_tx,
            control_tx,
            queue_capacity,
            source_queue_max_bytes,
            capture_abort,
        ));
        let progress = Arc::new(Mutex::new(PumpExactStateWriterSummaryV2::default()));
        let readiness_status = Arc::new(AtomicU8::new(
            PumpExactStateReadinessStatusV2::Pending as u8,
        ));
        let writer_ingress = Arc::clone(&ingress);
        let writer_progress = Arc::clone(&progress);
        let writer_readiness_status = Arc::clone(&readiness_status);
        let writer_raw_dir = raw_dir.to_path_buf();
        let join = thread::Builder::new()
            .name("pump-exact-state-tape-v2-writer".to_owned())
            .spawn(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    raw_writer_main_v2(
                        &writer_raw_dir,
                        &run_id,
                        capture_contract_sha256,
                        data_rx,
                        control_rx,
                        readiness_rx,
                        Arc::clone(&writer_ingress),
                        Arc::clone(&writer_progress),
                        Arc::clone(&writer_readiness_status),
                        flush_interval,
                        segment_max_bytes,
                        segment_max_duration,
                        max_raw_bytes,
                        min_free_bytes,
                    )
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        let message = format!("{error:#}");
                        {
                            let mut progress = writer_progress.lock();
                            progress.error = Some(message);
                            progress.clean_shutdown = false;
                        }
                        writer_ingress.record_fatal_capture_error(
                            PumpExactStateCaptureFatalReasonV2::WriterFailure,
                        );
                        writer_ingress.cancel_source_from_writer_if_fatal();
                    }
                    Err(_) => {
                        {
                            let mut progress = writer_progress.lock();
                            progress.error = Some("V2 raw writer thread panicked".to_owned());
                            progress.clean_shutdown = false;
                        }
                        writer_ingress.record_fatal_capture_error(
                            PumpExactStateCaptureFatalReasonV2::WriterPanic,
                        );
                        writer_ingress.cancel_source_from_writer_if_fatal();
                    }
                }
            })
            .context("spawn bounded V2 raw writer thread")?;
        Ok(Self {
            ingress,
            readiness_tx,
            readiness_status,
            join: Mutex::new(Some(join)),
            progress,
        })
    }

    pub(crate) fn source_sink(&self) -> Arc<dyn PumpResearchSourceSinkV1> {
        Arc::clone(&self.ingress) as Arc<dyn PumpResearchSourceSinkV1>
    }

    pub(crate) fn capture_abort(&self) -> CancellationToken {
        self.ingress.capture_abort()
    }

    pub(crate) fn established_stream_epoch(&self) -> Option<u64> {
        self.ingress.established_stream_epoch()
    }

    pub(crate) async fn wait_for_required_source_lanes(
        &self,
        timeout: Duration,
    ) -> Result<PumpExactStateSourceReadinessV2> {
        self.ingress.wait_for_required_source_lanes(timeout).await
    }

    /// Persist the exactly-one source boundary and wait until the writer has
    /// flushed and synchronised it.  This is control-plane work and is never
    /// called on the gRPC receive task.
    /// Arm the exactly-once readiness barrier before reserving its ordered
    /// control position.  This keeps the marker out of the source-record
    /// census while making its placement deterministic relative to every
    /// source update admitted before or after it.
    pub(crate) fn arm_stream_boundary(&self) -> Result<u64> {
        self.readiness_status
            .compare_exchange(
                PumpExactStateReadinessStatusV2::Pending as u8,
                PumpExactStateReadinessStatusV2::Persisting as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| anyhow::anyhow!("V2 stream-readiness boundary must occur exactly once"))?;
        let sequence = self.ingress.reserve_readiness_boundary_sequence();
        if sequence == V2_READINESS_BOUNDARY_SEQUENCE_UNSET {
            self.fail_readiness();
            bail!("V2 stream-readiness boundary exhausted its ordering domain");
        }
        Ok(sequence)
    }

    /// Queue an already armed stream boundary at its reserved global ordering
    /// position and wait until it is durably written.  The caller must bind
    /// that same position as `source_capture_sequence_exclusive`.
    pub(crate) fn persist_armed_stream_boundary(
        &self,
        ordering_sequence: u64,
        boundary: PumpExactStateProspectiveStreamBoundaryV2,
        timeout: Duration,
    ) -> Result<()> {
        if timeout.is_zero() {
            bail!("V2 stream-readiness persistence timeout must be greater than zero");
        }
        if PumpExactStateReadinessStatusV2::from_raw(self.readiness_status.load(Ordering::Acquire))
            != PumpExactStateReadinessStatusV2::Persisting
        {
            bail!("V2 stream-readiness boundary was not armed before persistence");
        }
        if ordering_sequence != boundary.source_capture_sequence_exclusive {
            self.fail_readiness();
            bail!(
                "V2 stream-readiness ordering sequence {} differs from exclusive source prefix {}",
                ordering_sequence,
                boundary.source_capture_sequence_exclusive
            );
        }
        let (acknowledgement_tx, acknowledgement_rx) = crossbeam_channel::bounded(1);
        self.readiness_tx
            .send_timeout(
                QueuedStreamBoundaryV2 {
                    ordering_sequence,
                    stream_epoch: boundary.source_stream_epoch,
                    boundary,
                    acknowledgement: acknowledgement_tx,
                },
                timeout,
            )
            .map_err(|error| {
                let reason = match error {
                    crossbeam_channel::SendTimeoutError::Timeout(_) => {
                        PumpExactStateCaptureFatalReasonV2::ReadinessQueueTimeout
                    }
                    crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                        PumpExactStateCaptureFatalReasonV2::ReadinessQueueDisconnected
                    }
                };
                self.fail_readiness();
                anyhow::anyhow!("V2 stream-readiness boundary enqueue failed: {reason:?}")
            })?;
        match acknowledgement_rx.recv_timeout(timeout) {
            Ok(Ok(())) => Ok(()),
            Ok(Err(error)) => {
                self.fail_readiness();
                bail!("V2 stream-readiness boundary persistence failed: {error}");
            }
            Err(crossbeam_channel::RecvTimeoutError::Timeout) => {
                self.ingress.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::ReadinessQueueTimeout,
                );
                self.fail_readiness();
                bail!("V2 stream-readiness boundary was not durably confirmed before timeout");
            }
            Err(crossbeam_channel::RecvTimeoutError::Disconnected) => {
                self.ingress.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::ReadinessQueueDisconnected,
                );
                self.fail_readiness();
                bail!("V2 stream-readiness acknowledgement lane disconnected");
            }
        }
    }

    pub(crate) fn fail_readiness(&self) {
        loop {
            let current = PumpExactStateReadinessStatusV2::from_raw(
                self.readiness_status.load(Ordering::Acquire),
            );
            if matches!(
                current,
                PumpExactStateReadinessStatusV2::Complete | PumpExactStateReadinessStatusV2::Failed
            ) {
                break;
            }
            if self
                .readiness_status
                .compare_exchange(
                    current as u8,
                    PumpExactStateReadinessStatusV2::Failed as u8,
                    Ordering::AcqRel,
                    Ordering::Acquire,
                )
                .is_ok()
            {
                self.ingress.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::ReadinessNotSealed,
                );
                self.ingress.cancel_source_from_writer_if_fatal();
                break;
            }
        }
    }

    pub(crate) fn finish_source(&self) {
        self.ingress.finish();
    }

    pub(crate) fn source_lifecycle(&self) -> PumpExactStateCaptureSourceLifecycleV2 {
        self.ingress
            .lifecycle(PumpExactStateReadinessStatusV2::from_raw(
                self.readiness_status.load(Ordering::Acquire),
            ))
    }

    #[must_use]
    pub(crate) fn finish_and_join(&self) -> PumpExactStateWriterSummaryV2 {
        if matches!(
            PumpExactStateReadinessStatusV2::from_raw(
                self.readiness_status.load(Ordering::Acquire)
            ),
            PumpExactStateReadinessStatusV2::Pending | PumpExactStateReadinessStatusV2::Persisting
        ) {
            self.fail_readiness();
        }
        self.ingress.finish();
        if let Some(handle) = self.join.lock().take() {
            if handle.join().is_err() {
                let mut progress = self.progress.lock();
                progress.error = Some("V2 raw writer thread panicked".to_owned());
                progress.clean_shutdown = false;
                self.ingress.record_fatal_capture_error(
                    PumpExactStateCaptureFatalReasonV2::WriterJoinPanic,
                );
            }
        }
        let mut summary = self.progress.lock().clone();
        let lifecycle = self.source_lifecycle();
        if let Some(error) = lifecycle.fatal_capture_error {
            summary
                .error
                .get_or_insert_with(|| format!("V2 capture failed closed: {error}"));
            summary.clean_shutdown = false;
        }
        if lifecycle.dropped_source_updates != summary.persisted_ingress_gap_missing_events {
            summary.error.get_or_insert_with(|| {
                format!(
                    "V2 source queue dropped {} updates but persisted ingress gaps account for {}",
                    lifecycle.dropped_source_updates, summary.persisted_ingress_gap_missing_events
                )
            });
            summary.clean_shutdown = false;
        }
        if lifecycle.source_queue_bytes_at_close != 0 {
            summary.error.get_or_insert_with(|| {
                format!(
                    "V2 source queue retained {} bytes after writer shutdown",
                    lifecycle.source_queue_bytes_at_close
                )
            });
            summary.clean_shutdown = false;
        }
        summary
    }
}

fn raw_writer_main_v2(
    raw_dir: &Path,
    run_id: &str,
    capture_contract_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    data_rx: crossbeam_channel::Receiver<QueuedSourceUpdateV2>,
    control_rx: crossbeam_channel::Receiver<CaptureControlV2>,
    readiness_rx: crossbeam_channel::Receiver<QueuedStreamBoundaryV2>,
    ingress: Arc<PumpExactStateCaptureIngressV2>,
    progress: Arc<Mutex<PumpExactStateWriterSummaryV2>>,
    readiness_status: Arc<AtomicU8>,
    flush_interval: Duration,
    segment_max_bytes: u64,
    segment_max_duration: Duration,
    max_raw_bytes: u64,
    min_free_bytes: u64,
) -> Result<()> {
    let mut writer = PumpExactStateRawSegmentWriterV2::new(
        raw_dir.to_path_buf(),
        run_id.to_owned(),
        capture_contract_sha256,
        flush_interval,
        segment_max_bytes,
        segment_max_duration,
        max_raw_bytes,
        min_free_bytes,
    )?;
    let mut local_gap_tracker =
        LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
    let mut pending = BTreeMap::<u64, OrderedIngressEventV2>::new();
    let mut next_capture_sequence = 0u64;
    let mut received_stream_boundary = false;

    loop {
        ingress.cancel_source_from_writer_if_fatal();
        let mut made_progress = false;
        for _ in 0..V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE {
            let Ok(CaptureControlV2::DroppedSource(dropped)) = control_rx.try_recv() else {
                break;
            };
            insert_ordered_ingress_event_v2(
                &mut pending,
                dropped.capture_sequence,
                OrderedIngressEventV2::Dropped(dropped),
            )?;
            made_progress = true;
        }
        for _ in 0..V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE {
            let Ok(queued) = data_rx.try_recv() else {
                break;
            };
            let sequence = queued.capture_sequence;
            insert_ordered_ingress_event_v2(
                &mut pending,
                sequence,
                OrderedIngressEventV2::Source(queued),
            )?;
            made_progress = true;
        }
        while let Some(event) = pending.remove(&next_capture_sequence) {
            match event {
                OrderedIngressEventV2::Source(source) => {
                    process_ordered_ingress_event_v2(
                        OrderedIngressEventV2::Source(source),
                        &mut writer,
                        &mut local_gap_tracker,
                        &progress,
                        &ingress,
                    )?;
                }
                OrderedIngressEventV2::Dropped(dropped) => {
                    process_ordered_ingress_event_v2(
                        OrderedIngressEventV2::Dropped(dropped),
                        &mut writer,
                        &mut local_gap_tracker,
                        &progress,
                        &ingress,
                    )?;
                }
                OrderedIngressEventV2::ReadinessBoundary(boundary) => {
                    let result = (|| -> Result<()> {
                        if boundary.ordering_sequence != next_capture_sequence
                            || boundary.ordering_sequence
                                != boundary.boundary.source_capture_sequence_exclusive
                            || ingress.readiness_boundary_sequence()
                                != Some(boundary.ordering_sequence)
                        {
                            bail!(
                                "V2 stream-readiness boundary ordering marker does not match its exclusive source prefix"
                            );
                        }
                        if ingress.established_stream_epoch() != Some(boundary.stream_epoch) {
                            bail!(
                                "V2 source stream epoch changed before stream-readiness boundary persistence"
                            );
                        }
                        writer.write_stream_boundary(
                            boundary.stream_epoch,
                            boundary.boundary.clone(),
                        )?;
                        writer.flush_active_and_sync()?;
                        readiness_status
                            .compare_exchange(
                                PumpExactStateReadinessStatusV2::Persisting as u8,
                                PumpExactStateReadinessStatusV2::Complete as u8,
                                Ordering::AcqRel,
                                Ordering::Acquire,
                            )
                            .map_err(|_| {
                                anyhow::anyhow!(
                                    "V2 writer cannot complete an unarmed stream-readiness boundary"
                                )
                            })?;
                        let mut summary = progress.lock();
                        summary.raw_bytes_written = writer.raw_bytes_written();
                        summary.accepted_readiness_boundary_records = summary
                            .accepted_readiness_boundary_records
                            .checked_add(1)
                            .ok_or_else(|| {
                                anyhow::anyhow!("V2 readiness boundary counter overflow")
                            })?;
                        if summary.accepted_readiness_boundary_records != 1 {
                            bail!("V2 writer accepted more than one stream-readiness boundary");
                        }
                        Ok(())
                    })();
                    match result {
                        Ok(()) => {
                            let _ = boundary.acknowledgement.send(Ok(()));
                        }
                        Err(error) => {
                            readiness_status.store(
                                PumpExactStateReadinessStatusV2::Failed as u8,
                                Ordering::Release,
                            );
                            ingress.record_fatal_capture_error(
                                PumpExactStateCaptureFatalReasonV2::ReadinessNotSealed,
                            );
                            ingress.cancel_source_from_writer_if_fatal();
                            let message = format!("{error:#}");
                            let _ = boundary.acknowledgement.send(Err(message.clone()));
                            return Err(anyhow::anyhow!(message));
                        }
                    }
                }
            }
            next_capture_sequence = next_capture_sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 capture sequence overflow"))?;
            made_progress = true;
        }
        if let Ok(boundary) = readiness_rx.try_recv() {
            if received_stream_boundary {
                bail!("V2 writer received more than one stream-readiness boundary");
            }
            if boundary.ordering_sequence != boundary.boundary.source_capture_sequence_exclusive {
                bail!(
                    "V2 queued stream-readiness boundary ordering marker differs from its exclusive source prefix"
                );
            }
            insert_ordered_ingress_event_v2(
                &mut pending,
                boundary.ordering_sequence,
                OrderedIngressEventV2::ReadinessBoundary(boundary),
            )?;
            received_stream_boundary = true;
            made_progress = true;
        }

        if ingress.source_finished.load(Ordering::Acquire) {
            let status =
                PumpExactStateReadinessStatusV2::from_raw(readiness_status.load(Ordering::Acquire));
            if matches!(status, PumpExactStateReadinessStatusV2::Failed) {
                bail!("V2 stream readiness failed before source closure");
            }
            if matches!(status, PumpExactStateReadinessStatusV2::Complete)
                && next_capture_sequence == ingress.final_capture_sequence.load(Ordering::Acquire)
                && pending.is_empty()
                && data_rx.is_empty()
                && control_rx.is_empty()
                && readiness_rx.is_empty()
            {
                local_gap_tracker.close_open_without_after();
                persist_completed_local_gaps_v2(&mut local_gap_tracker, &mut writer, &progress)?;
                writer.reconcile_full_blocks_at_clean_close()?;
                let lifecycle = ingress.lifecycle(status);
                let summary = progress.lock().clone();
                if lifecycle.dropped_source_updates != summary.persisted_ingress_gap_missing_events
                {
                    bail!(
                        "V2 dropped {} source updates but persisted ingress gaps account for {}",
                        lifecycle.dropped_source_updates,
                        summary.persisted_ingress_gap_missing_events
                    );
                }
                let full_block_census = writer.full_block_census(true);
                let mut required_lane_census = summary.required_lane_census.clone();
                required_lane_census.apply_full_block_reconciliation(&full_block_census);
                if !required_lane_census.all_required_lanes_observed() {
                    // The writer may have drained every retained frame, but a
                    // semantically silent or partially subscribed source is
                    // not a complete prospective capture.  Publish the
                    // structurally valid raw evidence with an explicitly
                    // unclean terminal footer and let the completion receipt
                    // report the per-lane census; never allow this state to
                    // look like a clean raw run.
                    writer.close_current(false)?;
                    let mut summary = progress.lock();
                    summary.segments = writer.receipts().to_vec();
                    summary.raw_bytes_written = writer.raw_bytes_written();
                    summary.required_lane_census = required_lane_census;
                    summary.clean_shutdown = false;
                    summary.error.get_or_insert_with(|| {
                        "V2 required source lane census is incomplete; transaction/account/slot/block-meta/full-block evidence must each be durably present and full blocks must reconcile".to_owned()
                    });
                    return Ok(());
                }
                writer.close_current(true)?;
                let mut summary = progress.lock();
                summary.segments = writer.receipts().to_vec();
                summary.raw_bytes_written = writer.raw_bytes_written();
                summary.required_lane_census = required_lane_census;
                summary.clean_shutdown = true;
                return Ok(());
            }
            if data_rx.is_empty() && control_rx.is_empty() && pending.is_empty() {
                match status {
                    PumpExactStateReadinessStatusV2::Pending
                    | PumpExactStateReadinessStatusV2::Persisting => {
                        // The coordinator will confirm or fail the one
                        // readiness boundary before joining.  Do not spin.
                    }
                    PumpExactStateReadinessStatusV2::Complete if readiness_rx.is_empty() => {
                        bail!(
                            "V2 source finished at capture sequence {} but writer did not reach sequence {}",
                            ingress.final_capture_sequence.load(Ordering::Acquire),
                            next_capture_sequence
                        );
                    }
                    PumpExactStateReadinessStatusV2::Complete
                    | PumpExactStateReadinessStatusV2::Failed => {}
                }
            }
        }

        if !made_progress {
            crossbeam_channel::select! {
                recv(control_rx) -> control => match control {
                    Ok(CaptureControlV2::DroppedSource(dropped)) => {
                        insert_ordered_ingress_event_v2(
                            &mut pending,
                            dropped.capture_sequence,
                            OrderedIngressEventV2::Dropped(dropped),
                        )?;
                    }
                    Err(_) if !ingress.source_finished.load(Ordering::Acquire) => {
                        bail!("V2 drop-control lane disconnected before source lifecycle finished");
                    }
                    Err(_) => {}
                },
                recv(data_rx) -> source => match source {
                    Ok(queued) => {
                        let sequence = queued.capture_sequence;
                        insert_ordered_ingress_event_v2(
                            &mut pending,
                            sequence,
                            OrderedIngressEventV2::Source(queued),
                        )?;
                    }
                    Err(_) if !ingress.source_finished.load(Ordering::Acquire) => {
                        bail!("V2 source data queue disconnected before source lifecycle finished");
                    }
                    Err(_) => {}
                },
                recv(readiness_rx) -> boundary => match boundary {
                    Ok(boundary) => {
                        if received_stream_boundary {
                            bail!("V2 writer received more than one stream-readiness boundary");
                        }
                        if boundary.ordering_sequence
                            != boundary.boundary.source_capture_sequence_exclusive
                        {
                            bail!(
                                "V2 queued stream-readiness boundary ordering marker differs from its exclusive source prefix"
                            );
                        }
                        insert_ordered_ingress_event_v2(
                            &mut pending,
                            boundary.ordering_sequence,
                            OrderedIngressEventV2::ReadinessBoundary(boundary),
                        )?;
                        received_stream_boundary = true;
                    }
                    Err(_) => {}
                },
                default(Duration::from_millis(5)) => {}
            }
        }
    }
}

fn insert_ordered_ingress_event_v2(
    pending: &mut BTreeMap<u64, OrderedIngressEventV2>,
    capture_sequence: u64,
    event: OrderedIngressEventV2,
) -> Result<()> {
    if pending.insert(capture_sequence, event).is_some() {
        bail!("V2 received duplicate ingress outcome for capture sequence {capture_sequence}");
    }
    Ok(())
}

fn process_ordered_ingress_event_v2(
    event: OrderedIngressEventV2,
    writer: &mut PumpExactStateRawSegmentWriterV2,
    local_gap_tracker: &mut LocalGapTracker,
    progress: &Arc<Mutex<PumpExactStateWriterSummaryV2>>,
    ingress: &PumpExactStateCaptureIngressV2,
) -> Result<()> {
    match event {
        OrderedIngressEventV2::Source(queued) => {
            let byte_cost = queued.byte_cost;
            let required_lane = queued.required_lane.lane;
            let result = (|| -> Result<()> {
                let decoded_update = queued.decode_update()?;
                let capture_sequence = queued.capture_sequence;
                let update = PumpResearchSourceUpdateV1 {
                    provider_id: queued.provider_id.clone(),
                    stream_epoch: queued.stream_epoch,
                    ingress_wall_ts_ms: queued.ingress_wall_ts_ms,
                    ingress_monotonic_ts_ms: queued.ingress_monotonic_ts_ms,
                    update: decoded_update,
                };
                local_gap_tracker.observe_admitted(source_boundary_v2(&update));
                persist_completed_local_gaps_v2(local_gap_tracker, writer, progress)?;
                writer.write_source_with_payload(capture_sequence, update, queued.encoded_update)
            })();
            // Keep the byte reservation through decode and durable write.  On
            // either success or a typed writer/decode failure it must be
            // released exactly once, otherwise completion could misreport a
            // retained ingress payload after the writer has already stopped.
            let release_result = ingress.release_source_bytes(byte_cost);
            result?;
            release_result?;
            let mut summary = progress.lock();
            summary.raw_bytes_written = writer.raw_bytes_written();
            summary.accepted_source_records = summary.accepted_source_records.saturating_add(1);
            summary
                .required_lane_census
                .record_source_lane(required_lane)?;
            let full_block_census = writer.full_block_census(false);
            summary
                .required_lane_census
                .apply_full_block_reconciliation(&full_block_census);
        }
        OrderedIngressEventV2::Dropped(dropped) => {
            local_gap_tracker.observe_saturation(
                dropped.provider_id,
                dropped.stream_epoch,
                local_boundary_from_raw_v2(&dropped.boundary),
                dropped.queue_high_water,
            );
        }
        OrderedIngressEventV2::ReadinessBoundary(_) => {
            bail!("V2 stream-readiness boundary reached the source-event writer path")
        }
    }
    Ok(())
}

fn persist_completed_local_gaps_v2(
    local_gap_tracker: &mut LocalGapTracker,
    writer: &mut PumpExactStateRawSegmentWriterV2,
    progress: &Arc<Mutex<PumpExactStateWriterSummaryV2>>,
) -> Result<()> {
    while let Some(gap) = local_gap_tracker.take_completed() {
        let missing = gap.missing_event_count;
        let stream_epoch = gap.stream_epoch;
        writer.write_record(
            stream_epoch,
            None,
            &PumpExactStateRawRecordV2::CoverageGap(raw_gap_from_local_v2(gap)),
        )?;
        let mut summary = progress.lock();
        summary.gap_count = summary.gap_count.saturating_add(1);
        summary.persisted_ingress_gap_episodes =
            summary.persisted_ingress_gap_episodes.saturating_add(1);
        summary.persisted_ingress_gap_missing_events = summary
            .persisted_ingress_gap_missing_events
            .saturating_add(missing);
    }
    if local_gap_tracker.completed_overflowed() {
        bail!("V2 local coverage-gap tracker overflowed");
    }
    Ok(())
}

fn source_boundary_v2(update: &PumpResearchSourceUpdateV1) -> LocalCoverageBoundaryV1 {
    let mut boundary = LocalCoverageBoundaryV1::default();
    match update.update.update_oneof.as_ref() {
        Some(UpdateOneof::Transaction(transaction)) => {
            boundary.slot = Some(transaction.slot);
            boundary.signature = transaction
                .transaction
                .as_ref()
                .and_then(|info| <[u8; 64]>::try_from(info.signature.as_slice()).ok())
                .map(Signature::from);
        }
        Some(UpdateOneof::Account(account)) => boundary.slot = Some(account.slot),
        Some(UpdateOneof::Slot(slot)) => boundary.slot = Some(slot.slot),
        Some(UpdateOneof::BlockMeta(meta)) => boundary.slot = Some(meta.slot),
        Some(UpdateOneof::TransactionStatus(status)) => {
            boundary.slot = Some(status.slot);
            boundary.signature = <[u8; 64]>::try_from(status.signature.as_slice())
                .ok()
                .map(Signature::from);
        }
        Some(UpdateOneof::Block(block)) => boundary.slot = Some(block.slot),
        Some(UpdateOneof::Entry(entry)) => boundary.slot = Some(entry.slot),
        Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_)) | None => {}
    }
    boundary
}

fn source_raw_boundary_v2(update: &PumpResearchSourceUpdateV1) -> PumpExactStateCoverageBoundaryV2 {
    let boundary = source_boundary_v2(update);
    PumpExactStateCoverageBoundaryV2 {
        slot: boundary.slot,
        signature: boundary
            .signature
            .as_ref()
            .and_then(|signature| fixed_signature_v2(signature.as_ref()).ok()),
    }
}

/// Every V2 source update must belong to exactly one required lane.  A
/// provider-side subscription that silently omits a lane is detected by the
/// stream-readiness/census gates; an unexpected message shape is rejected
/// immediately rather than being treated as a harmless control message.
fn required_source_lane_observation_v2(
    update: &PumpResearchSourceUpdateV1,
) -> Result<PumpExactStateRequiredLaneObservationV2> {
    let (lane, slot) = match update.update.update_oneof.as_ref() {
        Some(UpdateOneof::Transaction(value)) => {
            (PumpExactStateRequiredSourceLaneV2::Transaction, value.slot)
        }
        Some(UpdateOneof::Account(value)) => {
            (PumpExactStateRequiredSourceLaneV2::Account, value.slot)
        }
        Some(UpdateOneof::Slot(value)) => (PumpExactStateRequiredSourceLaneV2::Slot, value.slot),
        Some(UpdateOneof::BlockMeta(value)) => {
            (PumpExactStateRequiredSourceLaneV2::BlockMeta, value.slot)
        }
        Some(UpdateOneof::Block(value)) => {
            (PumpExactStateRequiredSourceLaneV2::FullBlock, value.slot)
        }
        Some(UpdateOneof::TransactionStatus(_)) => {
            bail!("V2 source received unsupported TransactionStatus update")
        }
        Some(UpdateOneof::Entry(_)) => bail!("V2 source received Entry although Entry is disabled"),
        Some(UpdateOneof::Ping(_)) => bail!("V2 source received unsupported Ping update"),
        Some(UpdateOneof::Pong(_)) => bail!("V2 source received Pong after gRPC control handling"),
        None => bail!("V2 source message has no update_oneof payload"),
    };
    if slot == V2_REQUIRED_LANE_SLOT_UNSET {
        bail!("V2 required source lane {lane:?} reported reserved slot value {slot}");
    }
    Ok(PumpExactStateRequiredLaneObservationV2 { lane, slot })
}

/// Convert one decoded source update into the exact bounded representation
/// retained by V2 ingress.  The queue intentionally owns this protobuf byte
/// vector, not a potentially over-allocated decoded object graph.
fn queued_source_update_v2(
    capture_sequence: u64,
    update: PumpResearchSourceUpdateV1,
    required_lane: PumpExactStateRequiredLaneObservationV2,
) -> Option<QueuedSourceUpdateV2> {
    let boundary = source_raw_boundary_v2(&update);
    let encoded_update = update.update.encode_to_vec();
    let encoded_bytes = u64::try_from(encoded_update.len()).ok()?;
    if encoded_bytes > PUMP_RESEARCH_EXACT_STATE_V2_MAX_DECODED_MESSAGE_BYTES as u64 {
        return None;
    }
    let byte_cost = encoded_bytes
        .checked_add(u64::try_from(update.provider_id.len()).ok()?)
        .and_then(|value| value.checked_add(256))?;
    Some(QueuedSourceUpdateV2 {
        capture_sequence,
        byte_cost,
        provider_id: update.provider_id,
        stream_epoch: update.stream_epoch,
        ingress_wall_ts_ms: update.ingress_wall_ts_ms,
        ingress_monotonic_ts_ms: update.ingress_monotonic_ts_ms,
        required_lane,
        boundary,
        encoded_update,
    })
}

fn local_boundary_from_raw_v2(
    boundary: &PumpExactStateCoverageBoundaryV2,
) -> LocalCoverageBoundaryV1 {
    LocalCoverageBoundaryV1 {
        slot: boundary.slot,
        signature: boundary
            .signature
            .map(|signature| Signature::from(signature.into_inner())),
    }
}

fn raw_gap_from_local_v2(gap: LocalCoverageGapV1) -> PumpExactStateCoverageGapV2 {
    PumpExactStateCoverageGapV2 {
        gap_id_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
            gap.gap_id_blake3,
        ),
        provider_id: gap.provider_id,
        stream_epoch: gap.stream_epoch,
        episode_sequence: gap.episode_sequence,
        reason: match gap.reason {
            LocalCoverageGapReasonV1::IngressQueueSaturated => {
                PumpExactStateCoverageGapReasonV2::IngressQueueSaturated
            }
            LocalCoverageGapReasonV1::WalQueueSaturated => {
                PumpExactStateCoverageGapReasonV2::WalQueueSaturated
            }
            LocalCoverageGapReasonV1::EvidenceQueueSaturated => {
                PumpExactStateCoverageGapReasonV2::EvidenceQueueSaturated
            }
            LocalCoverageGapReasonV1::IpcEgressQueueSaturated => {
                PumpExactStateCoverageGapReasonV2::IpcEgressQueueSaturated
            }
        },
        before: raw_boundary_from_local_v2(&gap.before),
        after: raw_boundary_from_local_v2(&gap.after),
        missing_event_count: gap.missing_event_count,
        first_dropped: raw_boundary_from_local_v2(&gap.first_dropped),
        last_dropped: raw_boundary_from_local_v2(&gap.last_dropped),
        queue_high_water: gap.queue_high_water as u64,
        started_at_wall_ms: gap.started_at_ms,
        ended_at_wall_ms: gap.ended_at_ms,
        recovered: gap.recovered,
    }
}

fn raw_boundary_from_local_v2(
    boundary: &LocalCoverageBoundaryV1,
) -> PumpExactStateCoverageBoundaryV2 {
    PumpExactStateCoverageBoundaryV2 {
        slot: boundary.slot,
        signature: boundary
            .signature
            .as_ref()
            .and_then(|signature| fixed_signature_v2(signature.as_ref()).ok()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::research_exact_tape_v2_materializer::{
        export_prospective_exact_state_outcome_blind_windows_v2,
        qualify_prospective_exact_state_raw_run_v2,
        validate_prospective_exact_state_strategy_input_v2, PumpExactStateCapabilityStatusV2,
    };
    use base64::{engine::general_purpose, Engine as _};
    use std::fs;
    #[cfg(unix)]
    use std::os::unix::fs::PermissionsExt;
    use tempfile::tempdir;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use yellowstone_grpc_proto::prelude::{
        subscribe_update::UpdateOneof, CommitmentLevel, CompiledInstruction, InnerInstruction,
        InnerInstructions, Message as ProtoMessage, SubscribeUpdate, SubscribeUpdateAccount,
        SubscribeUpdateAccountInfo, SubscribeUpdateBlock, SubscribeUpdateBlockMeta,
        SubscribeUpdateSlot, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
        Transaction, TransactionStatusMeta,
    };

    const TEST_V2_MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;
    const TEST_V2_MIN_FREE_BYTES: u64 = 1;

    fn v2_http_header_end(bytes: &[u8]) -> Option<usize> {
        bytes
            .windows(4)
            .position(|window| window == b"\r\n\r\n")
            .map(|position| position + 4)
    }

    async fn read_v2_mock_rpc_request(socket: &mut tokio::net::TcpStream) -> serde_json::Value {
        let mut bytes = Vec::new();
        let mut buffer = [0u8; 4096];
        loop {
            if let Some(header_end) = v2_http_header_end(&bytes) {
                let headers = std::str::from_utf8(&bytes[..header_end])
                    .expect("mock RPC request headers must be UTF-8");
                let content_length = headers
                    .lines()
                    .find_map(|line| {
                        let (name, value) = line.split_once(':')?;
                        name.eq_ignore_ascii_case("content-length")
                            .then(|| value.trim().parse::<usize>().expect("valid content length"))
                    })
                    .expect("mock RPC request must have Content-Length");
                if bytes.len() >= header_end + content_length {
                    return serde_json::from_slice(&bytes[header_end..header_end + content_length])
                        .expect("mock RPC body must be JSON");
                }
            }
            let read = socket
                .read(&mut buffer)
                .await
                .expect("read mock ProgramData RPC request");
            assert_ne!(read, 0, "mock ProgramData RPC peer closed before a request");
            bytes.extend_from_slice(&buffer[..read]);
        }
    }

    async fn write_v2_mock_rpc_response(
        socket: &mut tokio::net::TcpStream,
        request_id: serde_json::Value,
        context_slot: u64,
        account_data: &[u8],
    ) {
        let body = serde_json::json!({
            "jsonrpc": "2.0",
            "result": {
                "context": { "slot": context_slot },
                "value": {
                    "lamports": 1u64,
                    "data": [general_purpose::STANDARD.encode(account_data), "base64"],
                    "owner": bpf_loader_upgradeable::id().to_string(),
                    "executable": false,
                    "rentEpoch": 0u64,
                },
            },
            "id": request_id,
        })
        .to_string();
        let response = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        socket
            .write_all(response.as_bytes())
            .await
            .expect("write mock ProgramData RPC response");
        socket
            .shutdown()
            .await
            .expect("close mock ProgramData RPC response");
    }

    fn source_update(account: SubscribeUpdateAccount) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_bonding_curves".to_owned()],
                update_oneof: Some(UpdateOneof::Account(account)),
            },
        }
    }

    fn source_block_update(block: SubscribeUpdateBlock) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_full_blocks".to_owned()],
                update_oneof: Some(UpdateOneof::Block(block)),
            },
        }
    }

    fn source_full_block_update(slot: u64) -> PumpResearchSourceUpdateV1 {
        source_block_update(SubscribeUpdateBlock {
            slot,
            parent_slot: slot.saturating_sub(1),
            blockhash: format!("full-block-{slot}"),
            parent_blockhash: format!("full-block-parent-{}", slot.saturating_sub(1)),
            executed_transaction_count: 1,
            ..SubscribeUpdateBlock::default()
        })
    }

    fn source_readiness_for_test(
        transaction_slot: u64,
        account_slot: u64,
        slot_update_slot: u64,
        block_meta_slot: u64,
        full_block_slot: u64,
    ) -> PumpExactStateSourceReadinessV2 {
        PumpExactStateSourceReadinessV2 {
            first_transaction_slot: transaction_slot,
            first_account_update_slot: account_slot,
            first_slot_update_slot: slot_update_slot,
            first_block_meta_slot: block_meta_slot,
            first_full_block_slot: full_block_slot,
            source_readiness_slot: [
                transaction_slot,
                account_slot,
                slot_update_slot,
                block_meta_slot,
                full_block_slot,
            ]
            .into_iter()
            .max()
            .expect("non-empty required lane slot set"),
        }
    }

    fn source_transaction_update(slot: u64, signature_byte: u8) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_transactions".to_owned()],
                update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
                    transaction: Some(SubscribeUpdateTransactionInfo {
                        signature: vec![signature_byte; 64],
                        is_vote: false,
                        transaction: None,
                        meta: None,
                        index: 0,
                    }),
                    slot,
                })),
            },
        }
    }

    fn source_slot_update(slot: u64) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_slots".to_owned()],
                update_oneof: Some(UpdateOneof::Slot(SubscribeUpdateSlot {
                    slot,
                    parent: slot.checked_sub(1),
                    status: 2,
                })),
            },
        }
    }

    fn source_block_meta_update(slot: u64) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_blocks_meta".to_owned()],
                update_oneof: Some(UpdateOneof::BlockMeta(SubscribeUpdateBlockMeta {
                    slot,
                    blockhash: "blockhash-test".to_owned(),
                    rewards: None,
                    block_time: None,
                    block_height: None,
                    parent_slot: slot.saturating_sub(1),
                    parent_blockhash: "parent-blockhash-test".to_owned(),
                    executed_transaction_count: 1,
                    entries_count: 0,
                })),
            },
        }
    }

    fn one_source_record(
        capture_sequence: u64,
        update: PumpResearchSourceUpdateV1,
    ) -> Result<PumpExactStateRawRecordV2> {
        let mut records = raw_records_from_source_v2(capture_sequence, update)?;
        if records.len() != 1 {
            bail!(
                "test fixture expected one V2 source record, got {}",
                records.len()
            );
        }
        records
            .pop()
            .ok_or_else(|| anyhow::anyhow!("test fixture produced no V2 source records"))
    }

    fn pump_owned_account(pubkey: Pubkey, owner: Pubkey, data: Vec<u8>) -> SubscribeUpdateAccount {
        let data = if pubkey
            == Pubkey::from_str(PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1)
                .expect("canonical Pump Global test pubkey")
            || data.starts_with(&BONDING_CURVE_DISC)
        {
            data
        } else {
            let mut canonical = BONDING_CURVE_DISC.to_vec();
            canonical.extend_from_slice(&data);
            canonical
        };
        SubscribeUpdateAccount {
            account: Some(SubscribeUpdateAccountInfo {
                pubkey: pubkey.to_bytes().to_vec(),
                lamports: 123,
                owner: owner.to_bytes().to_vec(),
                executable: false,
                rent_epoch: 7,
                data,
                write_version: 9,
                txn_signature: None,
            }),
            slot: 88,
            is_startup: false,
        }
    }

    #[derive(Clone, Copy)]
    enum QualifiedExportFixtureVariantV2 {
        Qualified,
        BlockedWithoutTradeFinalAnchor,
        SlotOnlyForwardWatermark,
        BlockMetaWithoutForwardFullBlock,
        ReconciledBlockPairWithoutFinalizedSlot,
        OmitWholeBuyBlock,
        ParentBlockhashMismatch,
        SkippedNumericSlot,
        FinalizedSlotParentNone,
        ProcessedParentThenFinalizedParentNone,
        ConflictingFinalizedSlotParents,
        AccountProjectionMismatch,
        SlotProjectionMismatch,
        BlockMetaProjectionMismatch,
        WallClockStepCannotCreateWindow,
        WrongEventMint,
        WrongEventUser,
        WrongEventIxName,
        WrongEventQuoteMint,
        WrongEventCanonicalReserve,
        WarmupAndUnreconciledTailInvocationSkew,
        ProvisionalFinalityTail,
        QualifiedWithBuyRemainingAccount,
    }

    #[derive(Clone, Copy)]
    enum FixtureEventCorruptionV2 {
        Mint,
        User,
        IxName,
        QuoteMint,
        CanonicalReserve,
    }

    #[derive(Clone, Copy)]
    enum FixtureProjectionCorruptionV2 {
        AccountEvidenceClass,
        SlotParent,
        BlockMetaBlockhash,
    }

    fn fixture_event_corruption_for_variant_v2(
        variant: QualifiedExportFixtureVariantV2,
    ) -> Option<FixtureEventCorruptionV2> {
        match variant {
            QualifiedExportFixtureVariantV2::WrongEventMint => Some(FixtureEventCorruptionV2::Mint),
            QualifiedExportFixtureVariantV2::WrongEventUser => Some(FixtureEventCorruptionV2::User),
            QualifiedExportFixtureVariantV2::WrongEventIxName => {
                Some(FixtureEventCorruptionV2::IxName)
            }
            QualifiedExportFixtureVariantV2::WrongEventQuoteMint => {
                Some(FixtureEventCorruptionV2::QuoteMint)
            }
            QualifiedExportFixtureVariantV2::WrongEventCanonicalReserve => {
                Some(FixtureEventCorruptionV2::CanonicalReserve)
            }
            _ => None,
        }
    }

    fn fixture_projection_corruption_for_variant_v2(
        variant: QualifiedExportFixtureVariantV2,
    ) -> Option<FixtureProjectionCorruptionV2> {
        match variant {
            QualifiedExportFixtureVariantV2::AccountProjectionMismatch => {
                Some(FixtureProjectionCorruptionV2::AccountEvidenceClass)
            }
            QualifiedExportFixtureVariantV2::SlotProjectionMismatch => {
                Some(FixtureProjectionCorruptionV2::SlotParent)
            }
            QualifiedExportFixtureVariantV2::BlockMetaProjectionMismatch => {
                Some(FixtureProjectionCorruptionV2::BlockMetaBlockhash)
            }
            _ => None,
        }
    }

    fn prospective_v2_semantics_manifest_path_for_test() -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .ancestors()
            .nth(3)
            .expect("repository root from seer manifest directory")
            .join("configs/research/pump_exact_state_semantics_manifest_v2.json")
    }

    fn runtime_digest_from_semantics_manifest_for_test(
        digest: &PumpExactStateSemanticsDigestV2,
    ) -> PumpExactStateDigestV2 {
        PumpExactStateDigestV2 {
            sha256: digest.sha256.clone(),
            blake3: digest.blake3.clone(),
            bytes: digest.bytes,
        }
    }

    fn fixture_program_data_receipt_v2(
        semantics: &PumpExactStateSemanticsAuthorityV2,
        observed_context_slot: u64,
    ) -> PumpProgramDataReceiptV1 {
        let pump_program_id = PumpResearchStoragePubkeyV1::from(semantics.program_id.to_bytes());
        PumpProgramDataReceiptV1 {
            pump_program_id,
            pump_program_account_owner: PumpResearchStoragePubkeyV1::from([31; 32]),
            pump_programdata_pubkey: PumpResearchStoragePubkeyV1::from([32; 32]),
            program_data_owner: PumpResearchStoragePubkeyV1::from([33; 32]),
            program_data_hash_algorithm: "blake3-256".to_owned(),
            program_data_hash_blake3:
                ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
                    semantics.expected_program_data_hash_blake3(),
                ),
            program_deployment_slot: Some(1),
            observed_context_slot,
            commitment: "finalized".to_owned(),
        }
    }

    fn fixture_curve_account_data_v2(
        virtual_token_reserves: u64,
        virtual_quote_reserves: u64,
        real_token_reserves: u64,
        real_quote_reserves: u64,
        token_total_supply: u64,
        creator: Pubkey,
        quote_mint: Pubkey,
    ) -> Vec<u8> {
        let mut bytes = vec![0x17, 0xb7, 0xf8, 0x37, 0x60, 0xd8, 0xac, 0x60];
        for value in [
            virtual_token_reserves,
            virtual_quote_reserves,
            real_token_reserves,
            real_quote_reserves,
            token_total_supply,
        ] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        bytes.push(0); // complete = false
        bytes.extend_from_slice(&creator.to_bytes());
        bytes.push(0); // is_mayhem_mode = false
        bytes.push(0); // is_cashback_coin = false
        bytes.extend_from_slice(&quote_mint.to_bytes());
        assert_eq!(
            bytes.len(),
            115,
            "fixture must use the selected V2 curve layout"
        );
        // Real Pump-owned BondingCurve updates can retain a larger account
        // allocation than the 115-byte Borsh struct.  Exercise the same
        // zero-filled allocation padding through the public PRXTAPE3 writer
        // and offline qualifier rather than testing only an idealized prefix.
        bytes.resize(151, 0);
        bytes
    }

    fn fixture_source_update_v2(
        update: SubscribeUpdate,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "qualified-export-fixture".to_owned(),
            stream_epoch: 1,
            ingress_wall_ts_ms,
            ingress_monotonic_ts_ms: ingress_wall_ts_ms,
            update,
        }
    }

    fn fixture_with_ingress_monotonic_v2(
        mut update: PumpResearchSourceUpdateV1,
        ingress_monotonic_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        update.ingress_monotonic_ts_ms = ingress_monotonic_ts_ms;
        update
    }

    fn fixture_apply_time_variant_v2(
        update: PumpResearchSourceUpdateV1,
        variant: QualifiedExportFixtureVariantV2,
        source_capture_sequence: u64,
    ) -> PumpResearchSourceUpdateV1 {
        if matches!(
            variant,
            QualifiedExportFixtureVariantV2::WallClockStepCannotCreateWindow
        ) {
            // The wall labels still jump from 1s to 150999ms and 241000ms,
            // while the actual process duration remains only a few ms.
            // This fixture would be falsely Complete under wall-clock-only
            // cutoff authority.
            let monotonic_ms = match source_capture_sequence {
                0..=10 => 1_000,
                11..=15 => 1_001,
                _ => 1_002,
            };
            return fixture_with_ingress_monotonic_v2(update, monotonic_ms);
        }
        update
    }

    /// Test-only corruption path: retain the original complete protobuf and
    /// its source hash, then alter only the convenient raw projection before
    /// it is framed and receipted.  This proves the offline qualifier binds
    /// index/canonicality inputs to retained source bytes instead of merely
    /// trusting a self-consistent outer record hash.
    #[cfg(unix)]
    fn write_fixture_source_with_projection_corruption_v2(
        writer: &mut PumpExactStateRawSegmentWriterV2,
        capture_sequence: u64,
        source: PumpResearchSourceUpdateV1,
        variant: QualifiedExportFixtureVariantV2,
    ) -> Result<bool> {
        let source = fixture_apply_time_variant_v2(source, variant, capture_sequence);
        let Some(corruption) = fixture_projection_corruption_for_variant_v2(variant) else {
            writer.write_source(capture_sequence, source)?;
            return Ok(false);
        };
        let stream_epoch = source.stream_epoch;
        let mut records = raw_records_from_source_v2(capture_sequence, source)?;
        let mut applied = false;
        for record in &mut records {
            match (corruption, record) {
                (
                    FixtureProjectionCorruptionV2::AccountEvidenceClass,
                    PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update),
                ) if update.slot == 103 => {
                    update.evidence_class = PumpExactStateAccountEvidenceClassV2::CanonicalGlobal;
                    applied = true;
                }
                (
                    FixtureProjectionCorruptionV2::SlotParent,
                    PumpExactStateRawRecordV2::PrimarySlotUpdate(update),
                ) if update.slot == 103 => {
                    update.parent = Some(101);
                    applied = true;
                }
                (
                    FixtureProjectionCorruptionV2::BlockMetaBlockhash,
                    PumpExactStateRawRecordV2::PrimaryBlockMeta(update),
                ) if update.slot == 103 => {
                    update.blockhash = "fixture-projection-blockhash-drift".to_owned();
                    applied = true;
                }
                _ => {}
            }
        }
        for record in &records {
            writer.write_record(stream_epoch, Some(capture_sequence), record)?;
            writer
                .full_block_reconciliation
                .observe_written_record(record)?;
        }
        Ok(applied)
    }

    fn fixture_role_pubkey_v2(instruction: &str, role: &str, ordinal: usize) -> Pubkey {
        let digest = blake3::hash(format!("v2-fixture:{instruction}:{role}:{ordinal}").as_bytes());
        Pubkey::new_from_array(*digest.as_bytes())
    }

    fn fixture_exact_instruction_info_v2(
        semantics: &PumpExactStateSemanticsAuthorityV2,
        instruction_name: &str,
        discriminator: [u8; 8],
        argument_bytes: Vec<u8>,
        signature_byte: u8,
        tx_index: u64,
        bonding_curve: Pubkey,
        mint: Pubkey,
        user: Pubkey,
        include_remaining_account: bool,
    ) -> SubscribeUpdateTransactionInfo {
        let contract = semantics
            .instruction(&discriminator)
            .expect("fixture instruction must be present in the real vendored semantics");
        assert_eq!(contract.name, instruction_name);

        #[derive(Clone)]
        struct DeclaredAccount {
            original_position: usize,
            pubkey: Pubkey,
            signer: bool,
            writable: bool,
            name: String,
        }

        let pump_program = semantics.program_id;
        let declared = contract
            .accounts
            .iter()
            .enumerate()
            .map(|(ordinal, account)| {
                let pubkey = account
                    .address
                    .unwrap_or_else(|| match account.name.as_str() {
                        "bonding_curve" => bonding_curve,
                        "mint" | "base_mint" => mint,
                        "user" => user,
                        "program" => pump_program,
                        _ => fixture_role_pubkey_v2(instruction_name, &account.name, ordinal),
                    });
                DeclaredAccount {
                    original_position: ordinal,
                    pubkey,
                    signer: account.signer,
                    writable: account.writable,
                    name: account.name.clone(),
                }
            })
            .collect::<Vec<_>>();

        let mut signer_writable = Vec::new();
        let mut signer_readonly = Vec::new();
        let mut unsigned_writable = Vec::new();
        let mut unsigned_readonly = Vec::new();
        for account in &declared {
            match (account.signer, account.writable) {
                (true, true) => signer_writable.push(account.clone()),
                (true, false) => signer_readonly.push(account.clone()),
                (false, true) => unsigned_writable.push(account.clone()),
                (false, false) => unsigned_readonly.push(account.clone()),
            }
        }
        let required_signatures = signer_writable
            .len()
            .checked_add(signer_readonly.len())
            .expect("fixture signer count overflow");
        let mut ordered = signer_writable;
        ordered.extend(signer_readonly);
        ordered.extend(unsigned_writable);
        ordered.extend(unsigned_readonly);
        let mut instruction_account_indices = vec![0u8; declared.len()];
        let mut program_id_index = None;
        for (index, account) in ordered.iter().enumerate() {
            let index = u8::try_from(index).expect("fixture account index fits u8");
            instruction_account_indices[account.original_position] = index;
            if account.name == "program" {
                program_id_index = Some(u32::from(index));
            }
        }
        if include_remaining_account {
            // Pump documents variant-specific `remaining_accounts`. It is an
            // unsigned readonly message key deliberately appended after the
            // fully pinned account prefix; the public fixture proves that the
            // raw writer and qualifier retain it without granting it a role.
            let remaining_index =
                u8::try_from(ordered.len()).expect("fixture remaining account index fits u8");
            ordered.push(DeclaredAccount {
                original_position: usize::MAX,
                pubkey: fixture_role_pubkey_v2(instruction_name, "remaining_account", 255),
                signer: false,
                writable: false,
                name: "remaining_account".to_owned(),
            });
            instruction_account_indices.push(remaining_index);
        }
        let mut instruction_data = discriminator.to_vec();
        instruction_data.extend_from_slice(&argument_bytes);
        let mut message = ProtoMessage {
            header: Some(Default::default()),
            account_keys: ordered
                .iter()
                .map(|account| account.pubkey.to_bytes().to_vec())
                .collect(),
            recent_blockhash: Vec::new(),
            instructions: vec![CompiledInstruction {
                program_id_index: program_id_index.expect("fixture Pump program account"),
                accounts: instruction_account_indices,
                data: instruction_data,
            }],
            versioned: false,
            address_table_lookups: Vec::new(),
        };
        let header = message.header.as_mut().expect("fixture message header");
        header.num_required_signatures =
            u32::try_from(required_signatures).expect("fixture signer count fits u32");
        header.num_readonly_signed_accounts = u32::try_from(
            ordered[..required_signatures]
                .iter()
                .filter(|account| !account.writable)
                .count(),
        )
        .expect("fixture readonly signer count fits u32");
        header.num_readonly_unsigned_accounts = u32::try_from(
            ordered[required_signatures..]
                .iter()
                .filter(|account| !account.writable)
                .count(),
        )
        .expect("fixture readonly unsigned count fits u32");
        SubscribeUpdateTransactionInfo {
            signature: vec![signature_byte; 64],
            is_vote: false,
            transaction: Some(Transaction {
                signatures: vec![vec![signature_byte; 64]; required_signatures],
                message: Some(message),
            }),
            meta: Some(TransactionStatusMeta::default()),
            index: tx_index,
        }
    }

    fn fixture_create_payload_v2(creator: Pubkey) -> Vec<u8> {
        let mut payload = Vec::new();
        for value in ["fixture", "FX", "https://fixture.invalid"] {
            payload.extend_from_slice(
                &u32::try_from(value.len())
                    .expect("fixture string length fits u32")
                    .to_le_bytes(),
            );
            payload.extend_from_slice(value.as_bytes());
        }
        payload.extend_from_slice(&creator.to_bytes());
        payload
    }

    fn fixture_buy_payload_v2() -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&1u64.to_le_bytes());
        payload.extend_from_slice(&2u64.to_le_bytes());
        payload.push(0); // OptionBool { bool: false }
        payload
    }

    fn fixture_push_borsh_string_v2(bytes: &mut Vec<u8>, value: &str) {
        bytes.extend_from_slice(
            &u32::try_from(value.len())
                .expect("fixture Borsh string length fits u32")
                .to_le_bytes(),
        );
        bytes.extend_from_slice(value.as_bytes());
    }

    fn fixture_parent_account_role_pubkey_v2(
        transaction: &SubscribeUpdateTransactionInfo,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        parent_discriminator: [u8; 8],
        role: &str,
    ) -> Pubkey {
        let contract = semantics
            .instruction(&parent_discriminator)
            .expect("fixture parent instruction exists in pinned semantics");
        let position = contract
            .accounts
            .iter()
            .position(|account| account.name == role)
            .expect("fixture parent role exists in pinned instruction");
        let message = transaction
            .transaction
            .as_ref()
            .and_then(|body| body.message.as_ref())
            .expect("fixture parent has message");
        let account_index = message.instructions[0].accounts[position];
        Pubkey::new_from_array(
            message.account_keys[usize::from(account_index)]
                .as_slice()
                .try_into()
                .expect("fixture parent account key is a Pubkey"),
        )
    }

    fn fixture_create_event_payload_v2(
        mint: Pubkey,
        bonding_curve: Pubkey,
        user: Pubkey,
        creator: Pubkey,
        token_program: Pubkey,
        quote_mint: Pubkey,
        virtual_token_reserves: u64,
        real_token_reserves: u64,
        token_total_supply: u64,
        corruption: Option<FixtureEventCorruptionV2>,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        for value in ["fixture", "FX", "https://fixture.invalid"] {
            fixture_push_borsh_string_v2(&mut payload, value);
        }
        let event_mint = if matches!(corruption, Some(FixtureEventCorruptionV2::Mint)) {
            Pubkey::new_from_array([72; 32])
        } else {
            mint
        };
        let event_user = if matches!(corruption, Some(FixtureEventCorruptionV2::User)) {
            Pubkey::new_from_array([73; 32])
        } else {
            user
        };
        let event_quote_mint = if matches!(corruption, Some(FixtureEventCorruptionV2::QuoteMint)) {
            Pubkey::new_from_array([74; 32])
        } else {
            quote_mint
        };
        for key in [event_mint, bonding_curve, event_user, creator] {
            payload.extend_from_slice(&key.to_bytes());
        }
        payload.extend_from_slice(&17i64.to_le_bytes());
        payload.extend_from_slice(&virtual_token_reserves.to_le_bytes());
        payload.extend_from_slice(&777u64.to_le_bytes()); // legacy/native-SOL label only
        payload.extend_from_slice(&real_token_reserves.to_le_bytes());
        payload.extend_from_slice(&token_total_supply.to_le_bytes());
        payload.extend_from_slice(&token_program.to_bytes());
        payload.push(0); // is_mayhem_mode
        payload.push(0); // is_cashback_enabled
        payload.extend_from_slice(&event_quote_mint.to_bytes());
        payload.extend_from_slice(&100u64.to_le_bytes()); // deliberately != virtual_sol
        payload
    }

    fn fixture_trade_event_payload_v2(
        mint: Pubkey,
        user: Pubkey,
        quote_mint: Pubkey,
        virtual_token_reserves: u64,
        real_token_reserves: u64,
        corruption: Option<FixtureEventCorruptionV2>,
    ) -> Vec<u8> {
        let mut payload = Vec::new();
        payload.extend_from_slice(&mint.to_bytes());
        payload.extend_from_slice(&1u64.to_le_bytes()); // sol_amount
        payload.extend_from_slice(&2u64.to_le_bytes()); // token_amount
        payload.push(1); // is_buy
        payload.extend_from_slice(&user.to_bytes());
        payload.extend_from_slice(&18i64.to_le_bytes());
        payload.extend_from_slice(&777u64.to_le_bytes()); // virtual_sol_reserves
        let event_virtual_token_reserves =
            if matches!(corruption, Some(FixtureEventCorruptionV2::CanonicalReserve)) {
                virtual_token_reserves
                    .checked_add(1)
                    .expect("fixture canonical reserve does not overflow")
            } else {
                virtual_token_reserves
            };
        payload.extend_from_slice(&event_virtual_token_reserves.to_le_bytes());
        payload.extend_from_slice(&666u64.to_le_bytes()); // real_sol_reserves
        payload.extend_from_slice(&real_token_reserves.to_le_bytes());
        payload.extend_from_slice(&Pubkey::new_from_array([70; 32]).to_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes()); // fee basis points
        payload.extend_from_slice(&0u64.to_le_bytes()); // fee
        payload.extend_from_slice(&Pubkey::new_from_array([71; 32]).to_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes()); // creator fee basis points
        payload.extend_from_slice(&0u64.to_le_bytes()); // creator fee
        payload.push(0); // track_volume
        payload.extend_from_slice(&0u64.to_le_bytes()); // total_unclaimed_tokens
        payload.extend_from_slice(&0u64.to_le_bytes()); // total_claimed_tokens
        payload.extend_from_slice(&0u64.to_le_bytes()); // current_sol_volume
        payload.extend_from_slice(&19i64.to_le_bytes());
        fixture_push_borsh_string_v2(
            &mut payload,
            if matches!(corruption, Some(FixtureEventCorruptionV2::IxName)) {
                "sell"
            } else {
                "buy"
            },
        );
        payload.push(0); // mayhem_mode
        payload.extend_from_slice(&0u64.to_le_bytes()); // cashback fee basis points
        payload.extend_from_slice(&0u64.to_le_bytes()); // cashback
        payload.extend_from_slice(&0u64.to_le_bytes()); // buyback fee basis points
        payload.extend_from_slice(&0u64.to_le_bytes()); // buyback
        payload.extend_from_slice(&0u32.to_le_bytes()); // shareholders vec length
        payload.extend_from_slice(&quote_mint.to_bytes());
        payload.extend_from_slice(&0u64.to_le_bytes()); // quote amount
        payload.extend_from_slice(&100u64.to_le_bytes()); // deliberately != virtual_sol
        payload.extend_from_slice(&101u64.to_le_bytes()); // deliberately != real_sol
        payload
    }

    fn fixture_attach_anchor_event_cpi_v2(
        transaction: &mut SubscribeUpdateTransactionInfo,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        event_discriminator: [u8; 8],
        event_payload: Vec<u8>,
    ) {
        let transaction_body = transaction
            .transaction
            .as_mut()
            .expect("fixture Event-CPI parent has transaction body");
        let message = transaction_body
            .message
            .as_mut()
            .expect("fixture Event-CPI parent has message");
        let outer = message
            .instructions
            .first()
            .expect("fixture Event-CPI parent has outer Pump instruction");
        let pump_program_index = outer.program_id_index;
        assert_eq!(
            message.account_keys[usize::try_from(pump_program_index).expect("Pump index fits")],
            semantics.program_id.to_bytes().to_vec(),
            "fixture outer instruction must be a real Pump parent"
        );
        let event_authority =
            Pubkey::find_program_address(&[b"__event_authority"], &semantics.program_id).0;
        let event_authority_index = u8::try_from(message.account_keys.len())
            .expect("fixture Event-CPI account index fits u8");
        message
            .account_keys
            .push(event_authority.to_bytes().to_vec());
        let header = message
            .header
            .as_mut()
            .expect("fixture Event-CPI message header");
        header.num_readonly_unsigned_accounts = header
            .num_readonly_unsigned_accounts
            .checked_add(1)
            .expect("fixture readonly unsigned count fits u32");

        let mut data = vec![0xe4, 0x45, 0xa5, 0x2e, 0x51, 0xcb, 0x9a, 0x1d];
        data.extend_from_slice(&event_discriminator);
        data.extend_from_slice(&event_payload);
        let meta = transaction
            .meta
            .as_mut()
            .expect("fixture Event-CPI parent has metadata");
        assert!(
            meta.inner_instructions.is_empty(),
            "fixture Event-CPI must own the only inner-instruction group"
        );
        meta.inner_instructions.push(InnerInstructions {
            index: 0,
            instructions: vec![InnerInstruction {
                program_id_index: pump_program_index,
                accounts: vec![event_authority_index],
                data,
                stack_height: Some(2),
            }],
        });
    }

    fn fixture_transaction_source_v2(
        transaction: SubscribeUpdateTransactionInfo,
        slot: u64,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_source_update_v2(
            SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_transactions".to_owned()],
                update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
                    transaction: Some(transaction),
                    slot,
                })),
            },
            ingress_wall_ts_ms,
        )
    }

    fn fixture_account_include_only_transaction_v2(
        mut transaction: SubscribeUpdateTransactionInfo,
    ) -> SubscribeUpdateTransactionInfo {
        let message = transaction
            .transaction
            .as_mut()
            .and_then(|body| body.message.as_mut())
            .expect("fixture account-include transaction has a message");
        // Preserve Pump in the static account vector, which is sufficient for
        // Yellowstone's account-include predicate, but target the only outer
        // instruction at an unrelated program. This is source-lane evidence,
        // not an actual Pump invocation.
        let non_pump_program_index =
            u32::try_from(message.account_keys.len()).expect("fixture account index fits u32");
        message.account_keys.push(vec![0xa5; 32]);
        message.instructions[0].program_id_index = non_pump_program_index;
        transaction
    }

    fn fixture_account_source_v2(
        bonding_curve: Pubkey,
        data: Vec<u8>,
        slot: u64,
        write_version: u64,
        txn_signature: Option<u8>,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("pinned Pump program");
        fixture_source_update_v2(
            SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_bonding_curves".to_owned()],
                update_oneof: Some(UpdateOneof::Account(SubscribeUpdateAccount {
                    account: Some(SubscribeUpdateAccountInfo {
                        pubkey: bonding_curve.to_bytes().to_vec(),
                        lamports: 1,
                        owner: pump_program.to_bytes().to_vec(),
                        executable: false,
                        rent_epoch: 0,
                        data,
                        write_version,
                        txn_signature: txn_signature.map(|value| vec![value; 64]),
                    }),
                    slot,
                    is_startup: false,
                })),
            },
            ingress_wall_ts_ms,
        )
    }

    fn fixture_slot_source_v2(
        slot: u64,
        parent: u64,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_slot_source_with_parent_and_status_v2(
            slot,
            Some(parent),
            CommitmentLevel::Finalized as i32,
            ingress_wall_ts_ms,
        )
    }

    fn fixture_slot_source_with_parent_and_status_v2(
        slot: u64,
        parent: Option<u64>,
        status: i32,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_source_update_v2(
            SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_slots".to_owned()],
                update_oneof: Some(UpdateOneof::Slot(SubscribeUpdateSlot {
                    slot,
                    parent,
                    status,
                })),
            },
            ingress_wall_ts_ms,
        )
    }

    fn fixture_block_meta_source_v2(
        slot: u64,
        parent_slot: u64,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_block_meta_source_with_transaction_count_v2(
            slot,
            parent_slot,
            1,
            ingress_wall_ts_ms,
        )
    }

    fn fixture_parent_blockhash_v2(parent_slot: u64) -> String {
        format!("fixture-blockhash-{parent_slot}")
    }

    fn fixture_with_parent_blockhash_v2(
        mut source: PumpResearchSourceUpdateV1,
        parent_blockhash: &str,
    ) -> PumpResearchSourceUpdateV1 {
        match source.update.update_oneof.as_mut() {
            Some(UpdateOneof::BlockMeta(block_meta)) => {
                block_meta.parent_blockhash = parent_blockhash.to_owned();
            }
            Some(UpdateOneof::Block(block)) => {
                block.parent_blockhash = parent_blockhash.to_owned();
            }
            _ => panic!("fixture parent-blockhash override requires BlockMeta or Block"),
        }
        source
    }

    fn fixture_block_meta_source_with_transaction_count_v2(
        slot: u64,
        parent_slot: u64,
        executed_transaction_count: u64,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_source_update_v2(
            SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_blocks_meta".to_owned()],
                update_oneof: Some(UpdateOneof::BlockMeta(SubscribeUpdateBlockMeta {
                    slot,
                    blockhash: format!("fixture-blockhash-{slot}"),
                    rewards: None,
                    block_time: None,
                    block_height: None,
                    parent_slot,
                    parent_blockhash: fixture_parent_blockhash_v2(parent_slot),
                    executed_transaction_count,
                    entries_count: 0,
                })),
            },
            ingress_wall_ts_ms,
        )
    }

    fn fixture_full_block_source_v2(
        slot: u64,
        parent_slot: u64,
        transaction: SubscribeUpdateTransactionInfo,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_full_block_source_with_transactions_v2(
            slot,
            parent_slot,
            vec![transaction],
            ingress_wall_ts_ms,
        )
    }

    fn fixture_empty_full_block_source_v2(
        slot: u64,
        parent_slot: u64,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        fixture_full_block_source_with_transactions_v2(
            slot,
            parent_slot,
            Vec::new(),
            ingress_wall_ts_ms,
        )
    }

    fn fixture_full_block_source_with_transactions_v2(
        slot: u64,
        parent_slot: u64,
        transactions: Vec<SubscribeUpdateTransactionInfo>,
        ingress_wall_ts_ms: u64,
    ) -> PumpResearchSourceUpdateV1 {
        let executed_transaction_count =
            u64::try_from(transactions.len()).expect("fixture full-block count fits u64");
        fixture_source_update_v2(
            SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_full_blocks".to_owned()],
                update_oneof: Some(UpdateOneof::Block(SubscribeUpdateBlock {
                    slot,
                    parent_slot,
                    blockhash: format!("fixture-blockhash-{slot}"),
                    parent_blockhash: fixture_parent_blockhash_v2(parent_slot),
                    executed_transaction_count,
                    transactions,
                    ..SubscribeUpdateBlock::default()
                })),
            },
            ingress_wall_ts_ms,
        )
    }

    fn write_stream_boundary_for_qualified_export_fixture_v2(
        writer: &mut PumpExactStateRawSegmentWriterV2,
        readiness: PumpExactStateSourceReadinessV2,
        source_capture_sequence_exclusive: u64,
    ) {
        writer
            .write_stream_boundary(
                1,
                PumpExactStateProspectiveStreamBoundaryV2 {
                    cohort_slots_strictly_after: readiness.source_readiness_slot,
                    source_readiness: readiness,
                    source_stream_epoch: 1,
                    source_capture_sequence_exclusive,
                    sealed_wall_ts_ms: 1_002,
                    sealed_monotonic_ts_ms: 1_002,
                },
            )
            .expect("write fixture readiness boundary");
    }

    #[cfg(unix)]
    fn write_complete_raw_fixture_for_qualified_export_v2(
        raw_dir: &Path,
        run_id: &str,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        variant: QualifiedExportFixtureVariantV2,
    ) {
        write_complete_raw_fixture_for_qualified_export_with_intermediate_rollover_v2(
            raw_dir, run_id, semantics, variant, false,
        );
    }

    #[cfg(unix)]
    fn write_complete_raw_fixture_for_qualified_export_with_intermediate_rollover_v2(
        raw_dir: &Path,
        run_id: &str,
        semantics: &PumpExactStateSemanticsAuthorityV2,
        variant: QualifiedExportFixtureVariantV2,
        force_intermediate_rollover: bool,
    ) {
        let capture_contract =
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([41; 32]);
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.to_path_buf(),
            run_id.to_owned(),
            capture_contract,
            Duration::from_secs(60),
            16 * 1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create local V2 raw fixture writer");
        fs::set_permissions(raw_dir, fs::Permissions::from_mode(0o700))
            .expect("make local raw fixture authority-private");

        let curve = Pubkey::new_from_array([61; 32]);
        let mint = Pubkey::new_from_array([62; 32]);
        let user = Pubkey::new_from_array([63; 32]);
        let creator = Pubkey::new_from_array([64; 32]);
        let quote_mint = Pubkey::default();
        let pre_state =
            fixture_curve_account_data_v2(1_000, 1_001, 1_002, 1_003, 1_004, creator, quote_mint);
        let create_state =
            fixture_curve_account_data_v2(900, 1_100, 800, 1_090, 1_004, creator, quote_mint);
        let buy_state =
            fixture_curve_account_data_v2(800, 1_200, 700, 1_180, 1_004, creator, quote_mint);
        let create_discriminator = [24, 30, 200, 40, 5, 28, 7, 119];
        let buy_discriminator = [102, 6, 61, 18, 1, 218, 235, 234];
        let buy_has_remaining_account = matches!(
            variant,
            QualifiedExportFixtureVariantV2::QualifiedWithBuyRemainingAccount
        );
        let pre_cohort_buy = fixture_exact_instruction_info_v2(
            semantics,
            "buy",
            buy_discriminator,
            fixture_buy_payload_v2(),
            1,
            0,
            curve,
            mint,
            user,
            false,
        );
        let mut create = fixture_exact_instruction_info_v2(
            semantics,
            "create",
            create_discriminator,
            fixture_create_payload_v2(creator),
            2,
            0,
            curve,
            mint,
            user,
            false,
        );
        let mut buy = fixture_exact_instruction_info_v2(
            semantics,
            "buy",
            buy_discriminator,
            fixture_buy_payload_v2(),
            3,
            0,
            curve,
            mint,
            user,
            buy_has_remaining_account,
        );
        let skew_warmup_and_tail_invocations = matches!(
            variant,
            QualifiedExportFixtureVariantV2::WarmupAndUnreconciledTailInvocationSkew
        );
        let provisional_finality_tail = matches!(
            variant,
            QualifiedExportFixtureVariantV2::ProvisionalFinalityTail
        );
        let warmup_filtered_transaction = if skew_warmup_and_tail_invocations {
            fixture_account_include_only_transaction_v2(pre_cohort_buy.clone())
        } else {
            pre_cohort_buy.clone()
        };
        let tail_filtered_transaction = skew_warmup_and_tail_invocations.then(|| {
            fixture_exact_instruction_info_v2(
                semantics,
                "buy",
                buy_discriminator,
                fixture_buy_payload_v2(),
                4,
                0,
                curve,
                mint,
                user,
                false,
            )
        });
        let create_token_program = fixture_parent_account_role_pubkey_v2(
            &create,
            semantics,
            create_discriminator,
            "token_program",
        );
        let event_corruption = fixture_event_corruption_for_variant_v2(variant);
        fixture_attach_anchor_event_cpi_v2(
            &mut create,
            semantics,
            [27, 114, 169, 77, 222, 235, 99, 118],
            fixture_create_event_payload_v2(
                mint,
                curve,
                user,
                creator,
                create_token_program,
                quote_mint,
                900,
                800,
                1_004,
                event_corruption,
            ),
        );
        fixture_attach_anchor_event_cpi_v2(
            &mut buy,
            semantics,
            [189, 219, 127, 211, 78, 230, 97, 238],
            fixture_trade_event_payload_v2(mint, user, quote_mint, 800, 700, event_corruption),
        );

        writer
            .write_source(
                0,
                fixture_apply_time_variant_v2(fixture_slot_source_v2(102, 101, 1_000), variant, 0),
            )
            .expect("write fixture first Slot lane");
        writer
            .write_source(
                1,
                fixture_apply_time_variant_v2(
                    fixture_block_meta_source_v2(102, 101, 1_000),
                    variant,
                    1,
                ),
            )
            .expect("write fixture first BlockMeta lane");
        writer
            .write_source(
                2,
                fixture_apply_time_variant_v2(
                    fixture_full_block_source_v2(102, 101, pre_cohort_buy.clone(), 1_000),
                    variant,
                    2,
                ),
            )
            .expect("write fixture first full block");
        writer
            .write_source(
                3,
                fixture_apply_time_variant_v2(
                    fixture_account_source_v2(curve, pre_state, 102, 1, None, 1_000),
                    variant,
                    3,
                ),
            )
            .expect("write fixture first account lane");
        writer
            .write_source(
                4,
                fixture_apply_time_variant_v2(
                    fixture_transaction_source_v2(warmup_filtered_transaction, 102, 1_000),
                    variant,
                    4,
                ),
            )
            .expect("write fixture first transaction lane");

        let readiness = PumpExactStateSourceReadinessV2 {
            first_transaction_slot: 102,
            first_account_update_slot: 102,
            first_slot_update_slot: 102,
            first_block_meta_slot: 102,
            first_full_block_slot: 102,
            source_readiness_slot: 102,
        };
        write_stream_boundary_for_qualified_export_fixture_v2(&mut writer, readiness.clone(), 5);
        if force_intermediate_rollover {
            // The production writer uses this same false footer when it
            // rotates a bounded segment.  Keep it as a physical writer test:
            // the next source record opens the parent-linked successor, while
            // only the final close may assert clean shutdown.
            writer
                .close_current(false)
                .expect("close intermediate local V2 raw fixture segment");
        }

        let omit_whole_buy_block =
            matches!(variant, QualifiedExportFixtureVariantV2::OmitWholeBuyBlock);
        let omit_buy_final_anchor = matches!(
            variant,
            QualifiedExportFixtureVariantV2::BlockedWithoutTradeFinalAnchor
        );
        let skipped_numeric_slot =
            matches!(variant, QualifiedExportFixtureVariantV2::SkippedNumericSlot);
        let finalized_slot_parent_is_none = matches!(
            variant,
            QualifiedExportFixtureVariantV2::FinalizedSlotParentNone
                | QualifiedExportFixtureVariantV2::ProcessedParentThenFinalizedParentNone
                | QualifiedExportFixtureVariantV2::ConflictingFinalizedSlotParents
        );
        let processed_parent_precedes_finalized_none = matches!(
            variant,
            QualifiedExportFixtureVariantV2::ProcessedParentThenFinalizedParentNone
        );
        let conflicting_finalized_slot_parents = matches!(
            variant,
            QualifiedExportFixtureVariantV2::ConflictingFinalizedSlotParents
        );
        let (buy_slot, buy_parent_slot, forward_slot, forward_parent_slot) = if skipped_numeric_slot
        {
            // Numeric slot 104 is deliberately skipped.  The real
            // parent relation, not `slot - 1`, remains the authority.
            (105, 103, 106, 105)
        } else {
            (104, 103, 105, 104)
        };
        // PRXTAPE3 reserves the boundary's own ordering position (5), so the
        // first source record in the prospective cohort begins at 6.
        let mut next_sequence = 6u64;
        let mut projection_corruption_applied = false;
        macro_rules! write_fixture_source {
            ($source:expr, $label:literal) => {{
                projection_corruption_applied |=
                    write_fixture_source_with_projection_corruption_v2(
                        &mut writer,
                        next_sequence,
                        $source,
                        variant,
                    )
                    .expect($label);
                next_sequence = next_sequence
                    .checked_add(1)
                    .expect("fixture source sequence overflow");
            }};
        }

        write_fixture_source!(
            fixture_transaction_source_v2(create.clone(), 103, 1_000),
            "write fixture Create transaction"
        );
        write_fixture_source!(
            fixture_account_source_v2(curve, create_state, 103, 2, Some(2), 1_000),
            "write fixture Create final anchor"
        );
        write_fixture_source!(
            fixture_slot_source_v2(103, 102, 1_000),
            "write fixture Create Slot evidence"
        );
        write_fixture_source!(
            fixture_block_meta_source_v2(103, 102, 1_000),
            "write fixture Create BlockMeta evidence"
        );
        write_fixture_source!(
            fixture_full_block_source_v2(103, 102, create, 1_000),
            "write fixture Create full block"
        );
        if !omit_whole_buy_block {
            write_fixture_source!(
                fixture_transaction_source_v2(buy.clone(), buy_slot, 150_999),
                "write fixture Buy transaction"
            );
            if !omit_buy_final_anchor {
                write_fixture_source!(
                    fixture_account_source_v2(curve, buy_state, buy_slot, 3, Some(3), 150_999,),
                    "write fixture Buy final anchor"
                );
            }
            if processed_parent_precedes_finalized_none {
                // The retained protobuf is internally consistent: only a
                // nonfinal Slot reports the true parent.  The following
                // Finalized Slot deliberately reports None, so the qualifier
                // must reject instead of stitching fields across updates.
                write_fixture_source!(
                    fixture_slot_source_with_parent_and_status_v2(
                        buy_slot,
                        Some(buy_parent_slot),
                        CommitmentLevel::Processed as i32,
                        150_999,
                    ),
                    "write fixture Processed Slot parent before parentless Finalized Slot"
                );
            }
            if conflicting_finalized_slot_parents {
                write_fixture_source!(
                    fixture_slot_source_v2(buy_slot, buy_parent_slot, 150_999),
                    "write fixture first conflicting Finalized Slot parent"
                );
            }
            write_fixture_source!(
                if finalized_slot_parent_is_none {
                    fixture_slot_source_with_parent_and_status_v2(
                        buy_slot,
                        None,
                        CommitmentLevel::Finalized as i32,
                        150_999,
                    )
                } else {
                    fixture_slot_source_v2(buy_slot, buy_parent_slot, 150_999)
                },
                "write fixture Buy finalized Slot evidence"
            );
            write_fixture_source!(
                fixture_block_meta_source_v2(buy_slot, buy_parent_slot, 150_999),
                "write fixture Buy BlockMeta evidence"
            );
            write_fixture_source!(
                fixture_full_block_source_v2(buy_slot, buy_parent_slot, buy, 150_999),
                "write fixture Buy full block"
            );
        }

        match variant {
            QualifiedExportFixtureVariantV2::SlotOnlyForwardWatermark => {
                write_fixture_source!(
                    fixture_slot_source_v2(forward_slot, forward_parent_slot, 241_000),
                    "write fixture unpaired forward Slot evidence"
                );
            }
            QualifiedExportFixtureVariantV2::BlockMetaWithoutForwardFullBlock => {
                write_fixture_source!(
                    fixture_slot_source_v2(forward_slot, forward_parent_slot, 241_000),
                    "write fixture incomplete forward Slot evidence"
                );
                write_fixture_source!(
                    fixture_block_meta_source_with_transaction_count_v2(
                        forward_slot,
                        forward_parent_slot,
                        0,
                        241_000,
                    ),
                    "write fixture unmatched forward BlockMeta evidence"
                );
            }
            QualifiedExportFixtureVariantV2::ReconciledBlockPairWithoutFinalizedSlot => {
                write_fixture_source!(
                    fixture_block_meta_source_with_transaction_count_v2(
                        forward_slot,
                        forward_parent_slot,
                        0,
                        241_000,
                    ),
                    "write fixture reconciled BlockMeta without finalized Slot evidence"
                );
                write_fixture_source!(
                    fixture_empty_full_block_source_v2(forward_slot, forward_parent_slot, 241_000,),
                    "write fixture reconciled full block without finalized Slot evidence"
                );
            }
            _ => {
                let mut forward_block_meta = fixture_block_meta_source_with_transaction_count_v2(
                    forward_slot,
                    forward_parent_slot,
                    0,
                    241_000,
                );
                let mut forward_full_block =
                    fixture_empty_full_block_source_v2(forward_slot, forward_parent_slot, 241_000);
                if matches!(
                    variant,
                    QualifiedExportFixtureVariantV2::ParentBlockhashMismatch
                ) {
                    let drift = "fixture-parent-blockhash-drift";
                    forward_block_meta =
                        fixture_with_parent_blockhash_v2(forward_block_meta, drift);
                    forward_full_block =
                        fixture_with_parent_blockhash_v2(forward_full_block, drift);
                }
                write_fixture_source!(
                    fixture_slot_source_v2(forward_slot, forward_parent_slot, 241_000),
                    "write fixture reconciled forward Slot evidence"
                );
                write_fixture_source!(
                    forward_block_meta,
                    "write fixture reconciled forward BlockMeta evidence"
                );
                write_fixture_source!(
                    forward_full_block,
                    "write fixture reconciled empty forward full block"
                );
            }
        }
        if let Some(tail_filtered_transaction) = tail_filtered_transaction {
            // This source message arrived after the last complete block pair.
            // It remains raw evidence but cannot become a rooted capability
            // mutation until a later capture has a parent-linked block proof.
            write_fixture_source!(
                fixture_transaction_source_v2(
                    tail_filtered_transaction,
                    forward_slot
                        .checked_add(1)
                        .expect("fixture tail slot overflow"),
                    241_001,
                ),
                "write fixture unreconciled tail Pump transaction"
            );
        }
        if provisional_finality_tail {
            let tail_slot = forward_slot
                .checked_add(1)
                .expect("fixture provisional-finality tail slot overflow");
            // The Slot lane explicitly reports this produced block as
            // Processed, but the fixture closes before it becomes Finalized.
            // It is auditable tail evidence, never a rooted capability slot;
            // the preceding finalized full-block pair remains the frontier.
            write_fixture_source!(
                fixture_slot_source_with_parent_and_status_v2(
                    tail_slot,
                    Some(forward_slot),
                    CommitmentLevel::Processed as i32,
                    241_001,
                ),
                "write fixture provisional-finality tail Slot evidence"
            );
            write_fixture_source!(
                fixture_block_meta_source_with_transaction_count_v2(
                    tail_slot,
                    forward_slot,
                    0,
                    241_001,
                ),
                "write fixture provisional-finality tail BlockMeta evidence"
            );
            write_fixture_source!(
                fixture_empty_full_block_source_v2(tail_slot, forward_slot, 241_001),
                "write fixture provisional-finality tail full-block evidence"
            );
        }
        if fixture_projection_corruption_for_variant_v2(variant).is_some() {
            assert!(
                projection_corruption_applied,
                "requested fixture projection corruption was not applied"
            );
        }
        writer
            .close_current(true)
            .expect("close complete local V2 raw fixture");

        let source_update_count = next_sequence
            .checked_sub(1)
            .expect("fixture boundary ordering marker must be reserved");
        let mut required_lane_census = writer.full_block_census(true);
        required_lane_census.transaction_messages = if omit_whole_buy_block {
            2
        } else if skew_warmup_and_tail_invocations {
            4
        } else {
            3
        };
        required_lane_census.account_updates = if omit_whole_buy_block || omit_buy_final_anchor {
            2
        } else {
            3
        };
        required_lane_census.slot_updates = match variant {
            QualifiedExportFixtureVariantV2::ReconciledBlockPairWithoutFinalizedSlot
            | QualifiedExportFixtureVariantV2::OmitWholeBuyBlock => 3,
            QualifiedExportFixtureVariantV2::ProcessedParentThenFinalizedParentNone
            | QualifiedExportFixtureVariantV2::ConflictingFinalizedSlotParents => 5,
            QualifiedExportFixtureVariantV2::ProvisionalFinalityTail => 5,
            _ => 4,
        };
        required_lane_census.block_meta_updates = match variant {
            QualifiedExportFixtureVariantV2::SlotOnlyForwardWatermark
            | QualifiedExportFixtureVariantV2::OmitWholeBuyBlock => 3,
            QualifiedExportFixtureVariantV2::ProvisionalFinalityTail => 5,
            _ => 4,
        };
        let program_data = fixture_program_data_receipt_v2(semantics, 102);
        let running_executable = digest_running_executable_v2().expect("fixture running image");
        let semantics_manifest_digest =
            runtime_digest_from_semantics_manifest_for_test(&semantics.manifest_digest);
        let vendored_idl_digest =
            runtime_digest_from_semantics_manifest_for_test(&semantics.idl_digest);
        let start = PumpExactStateRunStartManifestV2 {
            storage_format_version: ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            schema_version: EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
            capture_config_schema_version: EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            repository_commit: "fixture-local-only".to_owned(),
            running_executable_digest: running_executable.clone(),
            operator_preflight_receipt_digest: digest_bytes_v2(b"fixture-preflight"),
            sealed_release_binary_digest: running_executable.clone(),
            sealed_fresh_build_receipt_digest: digest_bytes_v2(b"fixture-build"),
            sealed_build_semantics: "fixture-local-only".to_owned(),
            capture_config_digest: digest_bytes_v2(b"fixture-config"),
            capture_contract_sha256: hex_bytes_v2(&capture_contract.into_inner()),
            source_request_fingerprint_blake3: "42".repeat(32),
            source_capture_semantics: ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SOURCE_CAPTURE_SEMANTICS_V2.to_owned(),
            source_max_decoded_message_bytes: 16 * 1024 * 1024,
            semantics_id: semantics.semantics_id.clone(),
            semantics_manifest_digest,
            vendored_idl_digest,
            expected_program_data_hash_blake3: hex_bytes_v2(&semantics.expected_program_data_hash_blake3()),
            primary_provider_id: "qualified-export-fixture".to_owned(),
            grpc_endpoint_digest: digest_bytes_v2(b"fixture-grpc"),
            program_data_rpc_endpoint_digest: digest_bytes_v2(b"fixture-program-data-rpc"),
            program_data_rpc_auth_mode: "fixture-no-auth".to_owned(),
            pump_program_id: semantics.program_id.to_string(),
            program_data_at_start: program_data.clone(),
            cohort_capture_wall_ms: 1_800_000,
            min_free_bytes: TEST_V2_MIN_FREE_BYTES,
            max_raw_bytes: TEST_V2_MAX_RAW_BYTES,
            required_storage_bytes: TEST_V2_MIN_FREE_BYTES,
            output_filesystem_available_bytes_at_start: TEST_V2_MIN_FREE_BYTES,
            capture_started_wall_ms: 1_000,
            capture_started_monotonic_ms: 1_000,
            required_for_run: true,
        };
        write_json_create_new_v2(&raw_dir.join("run_start_manifest_v2.json"), &start)
            .expect("write local V2 start manifest");
        let completion = PumpExactStateRunCompletionReceiptV2 {
            storage_format_version: ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_STORAGE_FORMAT_VERSION_V2,
            schema_version: EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION,
            run_id: run_id.to_owned(),
            status: PumpExactStateCaptureRunStatusV2::Complete,
            clean_shutdown: true,
            source_readiness: Some(readiness.clone()),
            readiness_boundary_persisted: true,
            cohort_slots_strictly_after: Some(102),
            readiness_completed: true,
            running_executable_at_completion: Some(running_executable),
            running_executable_unchanged: true,
            program_data_at_start: program_data.clone(),
            program_data_at_completion: Some(program_data),
            program_data_unchanged: true,
            cohort_capture_termination: Some(PumpExactStateCaptureTerminationV2::CohortWallDeadline),
            cohort_capture_elapsed_ms: Some(1_800_000),
            min_free_bytes: TEST_V2_MIN_FREE_BYTES,
            max_raw_bytes: TEST_V2_MAX_RAW_BYTES,
            output_filesystem_available_bytes_at_completion: Some(TEST_V2_MIN_FREE_BYTES),
            storage_reserve_maintained: true,
            raw_byte_budget_respected: true,
            required_source_lanes_observed: true,
            source_lifecycle: PumpExactStateCaptureSourceLifecycleV2 {
                stream_established: true,
                established_stream_epoch: Some(1),
                source_updates_received: source_update_count,
                admitted_source_updates: source_update_count,
                dropped_source_updates: 0,
                source_queue_peak_bytes: 1,
                source_queue_bytes_at_close: 0,
                source_workers_cleanly_stopped: true,
                required_lane_first_slots: Some(readiness),
                source_readiness_status: "complete".to_owned(),
                fatal_capture_error: None,
                source_worker_error: None,
            },
            writer: PumpExactStateWriterSummaryV2 {
                segments: writer.receipts().to_vec(),
                raw_bytes_written: writer.raw_bytes_written(),
                accepted_source_records: source_update_count,
                accepted_readiness_boundary_records: 1,
                required_lane_census,
                persisted_ingress_gap_missing_events: 0,
                persisted_ingress_gap_episodes: 0,
                gap_count: 0,
                clean_shutdown: true,
                error: None,
            },
            segment_list: writer.receipts().to_vec(),
            completion_wall_ms: 1_801_000,
        };
        write_json_create_new_v2(&raw_dir.join("run_completion_receipt_v2.json"), &completion)
            .expect("write local V2 completion receipt");
    }

    #[cfg(unix)]
    fn fixture_exact_artifact_digest_json_v2(path: &Path) -> serde_json::Value {
        let bytes = fs::read(path).expect("read fixture exact artifact");
        let sha256: [u8; 32] = Sha256::digest(&bytes).into();
        serde_json::json!({
            "sha256": hex_bytes_v2(&sha256),
            "blake3": hex_bytes_v2(blake3::hash(&bytes).as_bytes()),
            "bytes": u64::try_from(bytes.len()).expect("fixture artifact byte count fits u64"),
            "line_count": u64::try_from(bytes.iter().filter(|byte| **byte == b'\n').count())
                .expect("fixture artifact line count fits u64"),
            "newline_complete": bytes.last() == Some(&b'\n'),
        })
    }

    #[cfg(unix)]
    fn rewrite_private_fixture_json_v2(path: &Path, value: &serde_json::Value) {
        let mut bytes = serde_json::to_vec_pretty(value).expect("serialize fixture JSON");
        bytes.push(b'\n');
        fs::write(path, bytes).expect("rewrite fixture JSON");
        fs::set_permissions(path, fs::Permissions::from_mode(0o600))
            .expect("restore fixture JSON private mode");
    }

    #[cfg(unix)]
    fn rewrite_completion_receipt_for_program_data_control_fixture_v2(
        raw_dir: &Path,
        rewrite: impl FnOnce(&mut serde_json::Value),
    ) {
        let completion_path = raw_dir.join("run_completion_receipt_v2.json");
        let mut completion: serde_json::Value = serde_json::from_slice(
            &fs::read(&completion_path).expect("read ProgramData control fixture receipt"),
        )
        .expect("parse ProgramData control fixture receipt");
        rewrite(&mut completion);
        rewrite_private_fixture_json_v2(&completion_path, &completion);
    }

    #[cfg(unix)]
    fn assert_v2_program_data_control_fails_raw_qualification(
        label: &str,
        rewrite: impl FnOnce(&mut serde_json::Value),
    ) {
        let temporary = tempdir().expect("temporary V2 ProgramData control fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for ProgramData control fixture");
        let raw_dir = temporary.path().join(format!("{label}-raw-v2"));
        let exact_dir = temporary.path().join(format!("{label}-exact-v2"));
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            &format!("program-data-control-{label}"),
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
        );
        rewrite_completion_receipt_for_program_data_control_fixture_v2(&raw_dir, rewrite);

        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                    "ProgramData completion control drift must fail before exact output creation",
                );
        assert!(
            error
                .to_string()
                .contains("V2 raw run does not satisfy the complete prospective capture contract"),
            "ProgramData control {label} must fail raw authority: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "ProgramData control {label} must not publish exact output"
        );
        assert!(
            !temporary
                .path()
                .join(format!(".{label}-exact-v2.partial"))
                .exists(),
            "ProgramData control {label} must fail before exact partial creation"
        );
    }

    #[cfg(unix)]
    fn corrupt_program_data_completion_identity_field_v2(
        completion: &mut serde_json::Value,
        field: &str,
    ) {
        let target = completion
            .get_mut("program_data_at_completion")
            .and_then(serde_json::Value::as_object_mut)
            .and_then(|receipt| receipt.get_mut(field))
            .unwrap_or_else(|| panic!("fixture ProgramData completion lacks {field}"));
        match target {
            serde_json::Value::Array(values) => {
                let first = values
                    .first()
                    .and_then(serde_json::Value::as_u64)
                    .expect("fixture ProgramData byte array has a first byte");
                values[0] = serde_json::json!(first ^ 1);
            }
            serde_json::Value::String(value) => value.push_str("-drift"),
            serde_json::Value::Number(value) => {
                let current = value
                    .as_u64()
                    .expect("fixture ProgramData numeric identity field is u64");
                *target = serde_json::json!(current.saturating_add(1));
            }
            other => {
                panic!("fixture ProgramData identity field {field} has unsupported shape {other}")
            }
        }
    }

    #[cfg(unix)]
    fn fixture_runtime_digest_json_v2(path: &Path) -> serde_json::Value {
        let digest = fixture_exact_artifact_digest_json_v2(path);
        serde_json::json!({
            "sha256": digest["sha256"].clone(),
            "blake3": digest["blake3"].clone(),
            "bytes": digest["bytes"].clone(),
        })
    }

    #[cfg(unix)]
    fn write_qualified_exact_fixture_for_strategy_input_v2(
        root: &Path,
        run_id: &str,
    ) -> (PathBuf, PathBuf, PathBuf) {
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for strategy-input fixture");
        let raw_dir = root.join("qualified-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            run_id,
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
        );
        let exact_dir = root.join("qualified-exact-v2");
        let summary =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect("Qualified raw fixture must materialize an exact artifact");
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        (raw_dir, semantics_path, exact_dir)
    }

    #[cfg(unix)]
    fn clone_and_inject_unscoped_candidate_into_qualified_exact_fixture_v2(
        source_dir: &Path,
        target_dir: &Path,
    ) {
        fs::create_dir(target_dir).expect("create cloned exact fixture directory");
        fs::set_permissions(target_dir, fs::Permissions::from_mode(0o700))
            .expect("make cloned exact fixture directory private");
        for filename in [
            "births_v2.jsonl",
            "trajectories_v2.jsonl",
            "coverage_v2.jsonl",
            "exact_state_capability_v2.json",
            "manifest_v2.json",
        ] {
            let target = target_dir.join(filename);
            fs::copy(source_dir.join(filename), &target).expect("clone exact fixture artifact");
            fs::set_permissions(&target, fs::Permissions::from_mode(0o600))
                .expect("make cloned exact fixture artifact private");
        }

        let coverage_path = target_dir.join("coverage_v2.jsonl");
        let mut coverage_rows = fs::read_to_string(&coverage_path)
            .expect("read cloned coverage JSONL")
            .lines()
            .map(|line| {
                serde_json::from_str::<serde_json::Value>(line).expect("parse coverage row")
            })
            .collect::<Vec<_>>();
        let pre_cohort = coverage_rows
            .first_mut()
            .expect("fixture coverage has a pre-cohort row");
        pre_cohort["candidate_count"] = serde_json::json!(1u32);
        pre_cohort["candidates"] = serde_json::json!([{
            "bonding_curve": null,
            "mint": null,
            "effect": "known_reserve_or_dependency_unsupported",
            "exact": false,
            "non_exact_reason": "fixture_unscoped_candidate"
        }]);
        let mut coverage_bytes = Vec::new();
        for row in &coverage_rows {
            coverage_bytes.extend_from_slice(
                &serde_json::to_vec(row).expect("serialize modified coverage row"),
            );
            coverage_bytes.push(b'\n');
        }
        fs::write(&coverage_path, coverage_bytes).expect("rewrite modified coverage JSONL");
        fs::set_permissions(&coverage_path, fs::Permissions::from_mode(0o600))
            .expect("restore modified coverage private mode");
        let coverage_digest = fixture_exact_artifact_digest_json_v2(&coverage_path);

        let receipt_path = target_dir.join("exact_state_capability_v2.json");
        let mut receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(&receipt_path).expect("read cloned capability receipt"),
        )
        .expect("parse cloned capability receipt");
        receipt["coverage_artifact"] = coverage_digest.clone();
        rewrite_private_fixture_json_v2(&receipt_path, &receipt);
        let receipt_digest = fixture_exact_artifact_digest_json_v2(&receipt_path);

        let manifest_path = target_dir.join("manifest_v2.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&fs::read(&manifest_path).expect("read cloned exact manifest"))
                .expect("parse cloned exact manifest");
        manifest["coverage_artifact"] = coverage_digest;
        manifest["exact_state_capability_artifact"] = receipt_digest;
        rewrite_private_fixture_json_v2(&manifest_path, &manifest);
    }

    #[cfg(unix)]
    #[test]
    fn public_prxtape3_complete_qualifies_and_exports_a_complete_outcome_blind_window() {
        let temporary = tempdir().expect("temporary V2 public-export fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for public fixture");

        let qualified_raw = temporary.path().join("qualified-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &qualified_raw,
            "qualified-public-export-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
        );
        let segment = fs::read(qualified_raw.join("segment_00000.bin"))
            .expect("read PRXTAPE3 public fixture segment");
        assert_eq!(
            &segment
                [..ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2
                    .len()],
            &ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_SEGMENT_MAGIC_V2,
            "public E2E must exercise the revised PRXTAPE3 storage contract"
        );
        let (_, raw_records) = decode_v2_segment(&qualified_raw.join("segment_00000.bin"));
        assert_eq!(
            raw_records
                .iter()
                .filter(|record| matches!(
                    record,
                    PumpExactStateRawRecordV2::ProspectiveStreamBoundary(_)
                ))
                .count(),
            1,
            "public E2E must retain exactly one five-lane readiness boundary"
        );
        let qualified_exact = temporary.path().join("qualified-exact");
        let qualified_summary = qualify_prospective_exact_state_raw_run_v2(
            &qualified_raw,
            &semantics_path,
            &qualified_exact,
        )
        .expect("public raw Complete fixture must qualify offline");
        assert_eq!(
            qualified_summary.status,
            PumpExactStateCapabilityStatusV2::Qualified,
            "Qualified raw fixture blockers: {:?}",
            qualified_summary.blockers
        );
        assert_eq!(qualified_summary.exact_rooted_mutation_count, 2);
        assert_eq!(qualified_summary.successful_rooted_mutation_denominator, 2);
        assert_eq!(qualified_summary.exact_rooted_coverage_ppm, 1_000_000);
        let qualified_receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(qualified_exact.join("exact_state_capability_v2.json"))
                .expect("read Qualified Event-CPI receipt"),
        )
        .expect("parse Qualified Event-CPI receipt");
        assert_eq!(
            qualified_receipt["successful_rooted_validated_event_transport_count"],
            serde_json::json!(2u64),
            "the public fixture must prove both real inner CreateEvent and TradeEvent transports"
        );
        assert_eq!(
            qualified_receipt["exact_birth_count"],
            serde_json::json!(1u64)
        );
        assert_eq!(
            qualified_receipt["successful_rooted_exact_trade_with_both_states_count"],
            serde_json::json!(1u64),
            "the post-boundary Create anchor must serve as the streamed predecessor for Buy"
        );

        let qualified_windows = temporary.path().join("qualified-windows");
        let exported = export_prospective_exact_state_outcome_blind_windows_v2(
            &qualified_raw,
            &semantics_path,
            &qualified_exact,
            &qualified_windows,
        )
        .expect("Qualified V2 fixture must export outcome-blind windows");
        assert_eq!(exported.exported_birth_count, 1);
        assert_eq!(exported.complete_window_count, 1);
        let windows = fs::read_to_string(qualified_windows.join("outcome_blind_windows_v2.jsonl"))
            .expect("read published V2 outcome-blind windows");
        let rows = windows
            .lines()
            .map(|line| serde_json::from_str::<serde_json::Value>(line).expect("parse window"))
            .collect::<Vec<_>>();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0]["status"], "complete");
        assert_eq!(
            rows[0]["time_axis"],
            serde_json::json!("observed_ingress_monotonic_ms"),
            "duration/cutoff authority must be monotonic ingress time"
        );
        assert_eq!(
            rows[0]["source_reconciled_full_block_frontier_ingress_monotonic_ms"],
            serde_json::json!(241_000u64),
            "only the reconciled empty BlockMeta/full-block slot, not a bare Slot update or the final filtered Pump transaction at 150999ms, proves the 90-second forward source availability"
        );
        assert_eq!(
            rows[0]["source_reconciled_full_block_frontier_slot"],
            serde_json::json!(105u64)
        );
        let export_manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(qualified_windows.join("manifest_v2.json"))
                .expect("read published V2 outcome-blind manifest"),
        )
        .expect("parse published V2 outcome-blind manifest");
        assert_eq!(export_manifest["complete_window_count"], 1);
        assert_eq!(export_manifest["windows_artifact"]["line_count"], 1);
        assert!(
            !temporary.path().join(".qualified-windows.partial").exists(),
            "successful outcome-blind export must atomically remove its partial directory"
        );

        let blocked_raw = temporary.path().join("blocked-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &blocked_raw,
            "blocked-public-export-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::BlockedWithoutTradeFinalAnchor,
        );
        let blocked_exact = temporary.path().join("blocked-exact");
        let blocked_summary = qualify_prospective_exact_state_raw_run_v2(
            &blocked_raw,
            &semantics_path,
            &blocked_exact,
        )
        .expect("a diagnostic V2 raw fixture may publish a Blocked receipt");
        assert_eq!(
            blocked_summary.status,
            PumpExactStateCapabilityStatusV2::Blocked
        );
        let blocked_windows = temporary.path().join("blocked-windows");
        assert!(
            export_prospective_exact_state_outcome_blind_windows_v2(
                &blocked_raw,
                &semantics_path,
                &blocked_exact,
                &blocked_windows,
            )
            .is_err(),
            "a Blocked exact artifact must never be exported as strategy-window input"
        );
        assert!(!blocked_windows.exists());
        assert!(!temporary.path().join(".blocked-windows.partial").exists());

        // A Qualified receipt may carry a bounded explicit non-exact candidate
        // in a real large denominator. The exporter must not silently scope
        // such a row to an arbitrary curve. This cloned artifact keeps every
        // raw/semantics/exact digest binding internally consistent while
        // injecting the one evidence shape the exporter must reject.
        let unscoped_exact = temporary.path().join("unscoped-exact");
        clone_and_inject_unscoped_candidate_into_qualified_exact_fixture_v2(
            &qualified_exact,
            &unscoped_exact,
        );
        let unscoped_windows = temporary.path().join("unscoped-windows");
        assert!(
            export_prospective_exact_state_outcome_blind_windows_v2(
                &qualified_raw,
                &semantics_path,
                &unscoped_exact,
                &unscoped_windows,
            )
            .is_err(),
            "an unscoped candidate in an otherwise Qualified exact artifact must fail closed before publication"
        );
        assert!(!unscoped_windows.exists());
        assert!(!temporary.path().join(".unscoped-windows.partial").exists());
    }

    #[cfg(unix)]
    #[test]
    fn public_prxtape3_qualifies_with_a_resolved_pump_remaining_account_after_the_pinned_prefix() {
        let temporary = tempdir().expect("temporary V2 remaining-account fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for remaining-account fixture");
        let raw_dir = temporary.path().join("remaining-account-raw-v2");
        let exact_dir = temporary.path().join("remaining-account-exact-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "remaining-account-public-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::QualifiedWithBuyRemainingAccount,
        );

        let summary =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect(
                    "a resolved Pump remaining account must not invalidate a PRXTAPE3 raw fixture",
                );
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        assert_eq!(summary.exact_rooted_mutation_count, 2);
        assert_eq!(summary.successful_rooted_mutation_denominator, 2);
        assert_eq!(summary.exact_rooted_coverage_ppm, 1_000_000);
        assert!(
            !temporary
                .path()
                .join(".remaining-account-exact-v2.partial")
                .exists(),
            "the public qualifier must atomically publish, not retain a partial output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_prxtape3_reconciles_only_actual_pump_invocations_inside_the_complete_cohort_chain() {
        let temporary = tempdir().expect("temporary V2 reconciliation-scope fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for reconciliation-scope fixture");
        let raw_dir = temporary.path().join("reconciliation-scope-raw-v2");
        let exact_dir = temporary.path().join("reconciliation-scope-exact-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "reconciliation-scope-public-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::WarmupAndUnreconciledTailInvocationSkew,
        );

        let summary = qualify_prospective_exact_state_raw_run_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        )
        .expect(
            "warm-up/full-only and trailing/filtered-only evidence outside the complete chain must not falsify in-chain reconciliation",
        );
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        assert_eq!(summary.successful_rooted_mutation_denominator, 2);
        assert!(exact_dir.join("exact_state_capability_v2.json").is_file());
        assert!(
            !temporary
                .path()
                .join(".reconciliation-scope-exact-v2.partial")
                .exists(),
            "public qualifier must not leave a partial exact artifact"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_prxtape3_uses_the_last_finalized_pair_before_a_provisional_tail() {
        let temporary = tempdir().expect("temporary V2 provisional-tail fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for provisional-tail fixture");
        let raw_dir = temporary.path().join("provisional-tail-raw-v2");
        let exact_dir = temporary.path().join("provisional-tail-exact-v2");
        let windows_dir = temporary.path().join("provisional-tail-windows-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "provisional-finality-tail-public-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::ProvisionalFinalityTail,
        );

        let summary =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect("a retained non-finalized tail must not invalidate the finalized prefix");
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        assert_eq!(summary.successful_rooted_mutation_denominator, 2);
        let exported = export_prospective_exact_state_outcome_blind_windows_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
            &windows_dir,
        )
        .expect("the preceding finalized frontier must remain eligible for outcome-blind export");
        assert_eq!(exported.complete_window_count, 1);
        assert!(exact_dir.join("exact_state_capability_v2.json").is_file());
        assert!(windows_dir.join("outcome_blind_windows_v2.jsonl").is_file());
        assert!(
            !temporary
                .path()
                .join(".provisional-tail-exact-v2.partial")
                .exists(),
            "public qualifier must not leave a partial exact artifact"
        );
        assert!(
            !temporary
                .path()
                .join(".provisional-tail-windows-v2.partial")
                .exists(),
            "public exporter must not leave a partial window artifact"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_prxtape3_intermediate_rollover_footer_qualifies_as_a_complete_chain() {
        let temporary = tempdir().expect("temporary V2 rollover-chain fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for rollover-chain fixture");
        let raw_dir = temporary.path().join("rollover-chain-raw-v2");
        let exact_dir = temporary.path().join("rollover-chain-exact-v2");
        write_complete_raw_fixture_for_qualified_export_with_intermediate_rollover_v2(
            &raw_dir,
            "rollover-chain-public-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
            true,
        );

        let completion: serde_json::Value = serde_json::from_slice(
            &fs::read(raw_dir.join("run_completion_receipt_v2.json"))
                .expect("read rollover-chain completion receipt"),
        )
        .expect("parse rollover-chain completion receipt");
        assert_eq!(
            completion["segment_list"].as_array().map(Vec::len),
            Some(2),
            "fixture must contain one normal rollover plus one terminal segment"
        );
        let (_, first_records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        let (_, terminal_records) = decode_v2_segment(&raw_dir.join("segment_00001.bin"));
        assert!(matches!(
            first_records.last(),
            Some(PumpExactStateRawRecordV2::SegmentClosed(footer)) if !footer.clean_shutdown
        ));
        assert!(matches!(
            terminal_records.last(),
            Some(PumpExactStateRawRecordV2::SegmentClosed(footer)) if footer.clean_shutdown
        ));

        let summary = qualify_prospective_exact_state_raw_run_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        )
        .expect(
            "a genuine intermediate rollover footer must be accepted before the terminal clean close",
        );
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        assert!(exact_dir.exists());
        assert!(
            !temporary
                .path()
                .join(".rollover-chain-exact-v2.partial")
                .exists(),
            "a valid multi-segment raw chain must atomically publish its exact artifact"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_later_finalized_program_data_context_slot_preserves_raw_authority() {
        let temporary = tempdir().expect("temporary V2 ProgramData context-slot fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for ProgramData context-slot fixture");
        let raw_dir = temporary.path().join("program-data-context-slot-raw-v2");
        let exact_dir = temporary.path().join("program-data-context-slot-exact-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "program-data-context-slot",
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
        );
        rewrite_completion_receipt_for_program_data_control_fixture_v2(&raw_dir, |completion| {
            completion["program_data_at_completion"]["observed_context_slot"] =
                serde_json::json!(103u64);
        });

        let summary = qualify_prospective_exact_state_raw_run_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        )
        .expect(
            "a later finalized ProgramData context slot is audit evidence, not immutable identity drift",
        );
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        assert!(exact_dir.exists());
        assert!(
            !temporary
                .path()
                .join(".program-data-context-slot-exact-v2.partial")
                .exists(),
            "a successful exact output must atomically remove its partial directory"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_program_data_completion_semantic_identity_drift_fails_raw_authority() {
        for field in [
            "pump_program_id",
            "pump_program_account_owner",
            "pump_programdata_pubkey",
            "program_data_owner",
            "program_data_hash_algorithm",
            "program_data_hash_blake3",
            "program_deployment_slot",
            "commitment",
        ] {
            let label = format!("program-data-completion-{field}");
            assert_v2_program_data_control_fails_raw_qualification(&label, |completion| {
                corrupt_program_data_completion_identity_field_v2(completion, field);
            });
        }
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_program_data_completion_control_fields_remain_fail_closed() {
        assert_v2_program_data_control_fails_raw_qualification(
            "program-data-start-copy-drift",
            |completion| {
                completion["program_data_at_start"]["observed_context_slot"] =
                    serde_json::json!(103u64);
            },
        );
        assert_v2_program_data_control_fails_raw_qualification(
            "program-data-completion-missing",
            |completion| {
                completion["program_data_at_completion"] = serde_json::Value::Null;
            },
        );
        assert_v2_program_data_control_fails_raw_qualification(
            "program-data-unchanged-false",
            |completion| {
                completion["program_data_unchanged"] = serde_json::json!(false);
            },
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_qualified_receipt_below_minimum_is_not_strategy_input_authority() {
        let temporary = tempdir().expect("temporary V2 minimum-flag fixture root");
        let (raw_dir, semantics_path, exact_dir) =
            write_qualified_exact_fixture_for_strategy_input_v2(
                temporary.path(),
                "qualified-minimum-flag-fixture",
            );

        let receipt_path = exact_dir.join("exact_state_capability_v2.json");
        let mut receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(&receipt_path).expect("read Qualified minimum-flag receipt"),
        )
        .expect("parse Qualified minimum-flag receipt");
        receipt["qualification_run_below_minimum"] = serde_json::json!(true);
        rewrite_private_fixture_json_v2(&receipt_path, &receipt);
        let receipt_digest = fixture_exact_artifact_digest_json_v2(&receipt_path);

        let manifest_path = exact_dir.join("manifest_v2.json");
        let mut manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(&manifest_path).expect("read Qualified minimum-flag manifest"),
        )
        .expect("parse Qualified minimum-flag manifest");
        manifest["exact_state_capability_artifact"] = receipt_digest;
        rewrite_private_fixture_json_v2(&manifest_path, &manifest);

        let error = match validate_prospective_exact_state_strategy_input_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        ) {
            Ok(_) => panic!(
                "a Qualified receipt marked below the qualification minimum is not authority"
            ),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("not a Qualified capability authority"),
            "unexpected minimum-flag authority error: {error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_strategy_input_recomputes_minimum_from_raw_cohort_authority() {
        let temporary = tempdir().expect("temporary V2 minimum-binding fixture root");
        let (raw_dir, semantics_path, exact_dir) =
            write_qualified_exact_fixture_for_strategy_input_v2(
                temporary.path(),
                "qualified-minimum-binding-fixture",
            );

        let completion_path = raw_dir.join("run_completion_receipt_v2.json");
        let mut completion: serde_json::Value = serde_json::from_slice(
            &fs::read(&completion_path).expect("read raw completion receipt"),
        )
        .expect("parse raw completion receipt");
        completion["cohort_capture_termination"] = serde_json::json!("operator_signal");
        completion["cohort_capture_elapsed_ms"] = serde_json::json!(1_799_999u64);
        rewrite_private_fixture_json_v2(&completion_path, &completion);
        let completion_digest = fixture_runtime_digest_json_v2(&completion_path);

        let receipt_path = exact_dir.join("exact_state_capability_v2.json");
        let mut receipt: serde_json::Value = serde_json::from_slice(
            &fs::read(&receipt_path).expect("read Qualified minimum-binding receipt"),
        )
        .expect("parse Qualified minimum-binding receipt");
        assert_eq!(
            receipt["qualification_run_below_minimum"],
            serde_json::json!(false),
            "fixture must begin with a Qualified minimum flag"
        );
        receipt["source_completion_receipt_digest"] = completion_digest;
        rewrite_private_fixture_json_v2(&receipt_path, &receipt);
        let receipt_digest = fixture_exact_artifact_digest_json_v2(&receipt_path);

        let manifest_path = exact_dir.join("manifest_v2.json");
        let mut manifest: serde_json::Value = serde_json::from_slice(
            &fs::read(&manifest_path).expect("read Qualified minimum-binding manifest"),
        )
        .expect("parse Qualified minimum-binding manifest");
        manifest["exact_state_capability_artifact"] = receipt_digest;
        rewrite_private_fixture_json_v2(&manifest_path, &manifest);

        let error = match validate_prospective_exact_state_strategy_input_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        ) {
            Ok(_) => panic!("strategy input must recompute the minimum gate from raw authority"),
            Err(error) => error,
        };
        assert!(
            error
                .to_string()
                .contains("qualification-run minimum flag differs from raw cohort authority"),
            "unexpected raw-minimum binding error: {error:#}"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_schema_two_incomplete_run_is_rejected_before_exact_output_creation() {
        let temporary = tempdir().expect("temporary schema-two raw fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for schema test");
        let raw_dir = temporary.path().join("schema-two-incomplete-raw");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "schema-two-incomplete-fixture",
            &semantics,
            QualifiedExportFixtureVariantV2::Qualified,
        );

        let start_path = raw_dir.join("run_start_manifest_v2.json");
        let mut start: serde_json::Value =
            serde_json::from_slice(&fs::read(&start_path).expect("read PRXTAPE3 start manifest"))
                .expect("parse PRXTAPE3 start manifest");
        start["storage_format_version"] = serde_json::json!(2u16);
        start["schema_version"] = serde_json::json!(2u16);
        rewrite_private_fixture_json_v2(&start_path, &start);

        let completion_path = raw_dir.join("run_completion_receipt_v2.json");
        let mut completion: serde_json::Value = serde_json::from_slice(
            &fs::read(&completion_path).expect("read PRXTAPE3 completion receipt"),
        )
        .expect("parse PRXTAPE3 completion receipt");
        completion["storage_format_version"] = serde_json::json!(2u16);
        completion["schema_version"] = serde_json::json!(2u16);
        completion["status"] = serde_json::json!("incomplete");
        rewrite_private_fixture_json_v2(&completion_path, &completion);

        let exact_dir = temporary.path().join("schema-two-exact");
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err("new qualifier must reject a historical PRXTAPE2 control contract");
        assert!(format!("{error:#}").contains("schema/version is not accepted"));
        assert!(!exact_dir.exists());
        assert!(!temporary.path().join(".schema-two-exact.partial").exists());
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_slot_only_forward_watermark_cannot_complete_outcome_window() {
        let temporary = tempdir().expect("temporary V2 Slot-only fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for Slot-only fixture");
        let raw_dir = temporary.path().join("slot-only-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "slot-only-forward-watermark",
            &semantics,
            QualifiedExportFixtureVariantV2::SlotOnlyForwardWatermark,
        );
        let exact_dir = temporary.path().join("slot-only-exact-v2");
        let summary = qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
            .expect("a late Slot without a BlockMeta/full-block pair does not rewrite the prior valid raw cohort");
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);

        let windows_dir = temporary.path().join("slot-only-windows-v2");
        let exported = export_prospective_exact_state_outcome_blind_windows_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
            &windows_dir,
        )
        .expect("the outcome-blind exporter must retain a truncated diagnostic window");
        assert_eq!(exported.complete_window_count, 0);
        let rows = fs::read_to_string(windows_dir.join("outcome_blind_windows_v2.jsonl"))
            .expect("read Slot-only diagnostic window");
        let row: serde_json::Value = serde_json::from_str(
            rows.lines()
                .next()
                .expect("one Slot-only diagnostic window"),
        )
        .expect("parse Slot-only diagnostic window");
        assert_eq!(row["status"], "truncated_at_run_end");
        assert_eq!(
            row["source_reconciled_full_block_frontier_slot"],
            serde_json::json!(104u64),
            "the naked Slot 105 must not advance source availability"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_wall_clock_step_cannot_create_outcome_window_coverage() {
        let temporary = tempdir().expect("temporary V2 wall-step fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for wall-step fixture");
        let raw_dir = temporary.path().join("wall-step-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "wall-clock-step-cannot-create-window",
            &semantics,
            QualifiedExportFixtureVariantV2::WallClockStepCannotCreateWindow,
        );
        let exact_dir = temporary.path().join("wall-step-exact-v2");
        let summary =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect("wall-label movement must not invalidate otherwise complete raw evidence");
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);
        let windows_dir = temporary.path().join("wall-step-windows-v2");
        let exported = export_prospective_exact_state_outcome_blind_windows_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
            &windows_dir,
        )
        .expect("wall-step fixture must emit a diagnostic outcome-blind window");
        assert_eq!(exported.complete_window_count, 0);
        let row: serde_json::Value =
            fs::read_to_string(windows_dir.join("outcome_blind_windows_v2.jsonl"))
                .expect("read wall-step diagnostic window")
                .lines()
                .next()
                .map(|line| serde_json::from_str(line).expect("parse wall-step diagnostic window"))
                .expect("one wall-step diagnostic window");
        assert_eq!(row["status"], "truncated_at_run_end");
        assert_eq!(
            row["source_reconciled_full_block_frontier_ingress_wall_ms"],
            serde_json::json!(241_000u64),
            "wall time remains an audit label"
        );
        assert_eq!(
            row["source_reconciled_full_block_frontier_ingress_monotonic_ms"],
            serde_json::json!(1_002u64),
            "the monotonic domain, not the 241000ms wall label, is cutoff authority"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_block_meta_without_full_block_fails_raw_qualification() {
        let temporary = tempdir().expect("temporary V2 unmatched-BlockMeta fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for unmatched-BlockMeta fixture");
        let raw_dir = temporary
            .path()
            .join("block-meta-without-full-block-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "block-meta-without-full-block",
            &semantics,
            QualifiedExportFixtureVariantV2::BlockMetaWithoutForwardFullBlock,
        );
        let exact_dir = temporary
            .path()
            .join("block-meta-without-full-block-exact-v2");
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                "accepted-cohort BlockMeta without a full-block payload must fail raw authority",
            );
        assert!(
            error
                .to_string()
                .contains("BlockMeta slot 105 lacks matching full-block payload"),
            "unexpected raw-authority error: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "raw authority failure must not publish exact output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_reconciled_block_pair_without_finalized_slot_fails_raw_qualification() {
        let temporary = tempdir().expect("temporary V2 pair-without-Slot fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for pair-without-Slot fixture");
        let raw_dir = temporary.path().join("pair-without-finalized-slot-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "reconciled-block-pair-without-finalized-slot",
            &semantics,
            QualifiedExportFixtureVariantV2::ReconciledBlockPairWithoutFinalizedSlot,
        );
        let exact_dir = temporary
            .path()
            .join("pair-without-finalized-slot-exact-v2");
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                    "a known executed block without any Slot evidence must not be relabeled as a provisional tail",
                );
        assert!(
            error
                .to_string()
                .contains("BlockMeta/full-block slot 105 lacks retained Slot evidence"),
            "unexpected raw-authority error: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "raw authority failure must not publish exact output"
        );
    }

    #[cfg(unix)]
    fn assert_v2_finalized_slot_parent_authority_fails_raw_qualification(
        label: &str,
        variant: QualifiedExportFixtureVariantV2,
        expected_error: &str,
    ) {
        let temporary = tempdir().expect("temporary V2 finalized-Slot-parent fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for finalized-Slot-parent fixture");
        let raw_dir = temporary.path().join(format!("{label}-raw-v2"));
        let exact_dir = temporary.path().join(format!("{label}-exact-v2"));

        // This uses the public source writer without projection corruption:
        // every retained SubscribeUpdate and its raw projection agree.  The
        // only inconsistency is that finalized Slot parent authority is None
        // while the independently retained BlockMeta/full-block pair names a
        // concrete parent.
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            &format!("finalized-slot-parent-authority-{label}"),
            &semantics,
            variant,
        );
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                "a finalized Slot parent must not be supplied by another Slot status or BlockMeta",
            );
        assert!(
            error.to_string().contains(expected_error),
            "finalized Slot parent authority must be the failing contract: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "finalized Slot parent authority failure must not publish exact output"
        );
        assert!(
            !temporary
                .path()
                .join(format!(".{label}-exact-v2.partial"))
                .exists(),
            "finalized Slot parent authority failure must happen before an exact partial directory exists"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_finalized_slot_parent_none_cannot_be_repaired_by_block_meta() {
        assert_v2_finalized_slot_parent_authority_fails_raw_qualification(
            "finalized-slot-parent-none",
            QualifiedExportFixtureVariantV2::FinalizedSlotParentNone,
            "finalized Slot parent differs from BlockMeta/full-block identity for accepted cohort slot 104",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_processed_slot_parent_cannot_be_stitched_with_parentless_finalized_slot() {
        assert_v2_finalized_slot_parent_authority_fails_raw_qualification(
            "processed-parent-then-finalized-none",
            QualifiedExportFixtureVariantV2::ProcessedParentThenFinalizedParentNone,
            "finalized Slot parent differs from BlockMeta/full-block identity for accepted cohort slot 104",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_conflicting_finalized_slot_parents_fail_raw_qualification() {
        assert_v2_finalized_slot_parent_authority_fails_raw_qualification(
            "conflicting-finalized-slot-parents",
            QualifiedExportFixtureVariantV2::ConflictingFinalizedSlotParents,
            "finalized Slot evidence assigns more than one parent to a slot",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_jointly_omitted_whole_parent_block_fails_raw_qualification() {
        let temporary = tempdir().expect("temporary V2 jointly-omitted fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for jointly-omitted fixture");
        let raw_dir = temporary.path().join("jointly-omitted-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "jointly-omitted-whole-parent-block",
            &semantics,
            QualifiedExportFixtureVariantV2::OmitWholeBuyBlock,
        );
        let exact_dir = temporary.path().join("jointly-omitted-exact-v2");
        let error = qualify_prospective_exact_state_raw_run_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
        )
        .expect_err(
            "equal filtered/full Pump maps with a jointly omitted produced parent block must fail",
        );
        assert!(
            error
                .to_string()
                .contains("references missing or unreconciled parent slot 104"),
            "the parent-linked ledger, rather than equality of two incomplete Pump maps, must explain failure: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "raw authority failure must not publish exact output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_parent_blockhash_mismatch_fails_raw_qualification() {
        let temporary = tempdir().expect("temporary V2 parent-blockhash mismatch fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for parent-blockhash mismatch fixture");
        let raw_dir = temporary.path().join("parent-blockhash-mismatch-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "parent-blockhash-mismatch",
            &semantics,
            QualifiedExportFixtureVariantV2::ParentBlockhashMismatch,
        );
        let exact_dir = temporary.path().join("parent-blockhash-mismatch-exact-v2");
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                    "a child whose retained parent_blockhash differs from the retained parent block must fail raw authority",
                );
        assert!(
            error
                .to_string()
                .contains("parent blockhash differs from retained parent slot 104"),
            "cross-slot parent blockhash mismatch must be the failure: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "parent-chain raw authority failure must not publish exact output"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_skipped_numeric_slot_with_retained_parent_chain_qualifies() {
        let temporary = tempdir().expect("temporary V2 skipped-slot fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for skipped-slot fixture");
        let raw_dir = temporary.path().join("skipped-numeric-slot-raw-v2");
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            "skipped-numeric-slot",
            &semantics,
            QualifiedExportFixtureVariantV2::SkippedNumericSlot,
        );
        let exact_dir = temporary.path().join("skipped-numeric-slot-exact-v2");
        let summary =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect(
                    "a valid 103 -> 105 parent edge must not be confused with a provider omission",
                );
        assert_eq!(summary.status, PumpExactStateCapabilityStatusV2::Qualified);

        let windows_dir = temporary.path().join("skipped-numeric-slot-windows-v2");
        let exported = export_prospective_exact_state_outcome_blind_windows_v2(
            &raw_dir,
            &semantics_path,
            &exact_dir,
            &windows_dir,
        )
        .expect("a parent-linked chain across a skipped numeric slot must remain exportable");
        assert_eq!(exported.complete_window_count, 1);
        let rows = fs::read_to_string(windows_dir.join("outcome_blind_windows_v2.jsonl"))
            .expect("read skipped-slot outcome window");
        let row: serde_json::Value = serde_json::from_str(
            rows.lines()
                .next()
                .expect("one skipped-slot outcome window"),
        )
        .expect("parse skipped-slot outcome window");
        assert_eq!(
            row["source_reconciled_full_block_frontier_slot"],
            serde_json::json!(106u64),
            "frontier must be the linked chain tip, not a numerically contiguous slot"
        );
    }

    #[cfg(unix)]
    fn assert_v2_projection_corruption_fails_raw_qualification(
        label: &str,
        variant: QualifiedExportFixtureVariantV2,
        expected_error: &str,
    ) {
        let temporary = tempdir().expect("temporary V2 projection-corruption fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for projection-corruption fixture");
        let raw_dir = temporary.path().join(format!("{label}-raw-v2"));
        let exact_dir = temporary.path().join(format!("{label}-exact-v2"));
        write_complete_raw_fixture_for_qualified_export_v2(
            &raw_dir,
            &format!("projection-corruption-{label}"),
            &semantics,
            variant,
        );
        let error =
            qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                .expect_err(
                    "a rewrapped raw record whose projection drifts from its retained protobuf must fail raw authority",
                );
        assert!(
            error.to_string().contains(expected_error),
            "projection corruption {label} must fail at retained-payload binding: {error:#}"
        );
        assert!(
            !exact_dir.exists(),
            "projection binding failure must not publish exact output"
        );
        assert!(
            !temporary
                .path()
                .join(format!(".{label}-exact-v2.partial"))
                .exists(),
            "projection binding failure must happen before an exact partial directory exists"
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_account_projection_mismatch_fails_raw_qualification() {
        assert_v2_projection_corruption_fails_raw_qualification(
            "account-projection-mismatch",
            QualifiedExportFixtureVariantV2::AccountProjectionMismatch,
            "Pump-owned account update projection differs from retained protobuf",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_slot_projection_mismatch_fails_raw_qualification() {
        assert_v2_projection_corruption_fails_raw_qualification(
            "slot-projection-mismatch",
            QualifiedExportFixtureVariantV2::SlotProjectionMismatch,
            "Slot update projection differs from retained protobuf",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_block_meta_projection_mismatch_fails_raw_qualification() {
        assert_v2_projection_corruption_fails_raw_qualification(
            "block-meta-projection-mismatch",
            QualifiedExportFixtureVariantV2::BlockMetaProjectionMismatch,
            "BlockMeta projection differs from retained protobuf",
        );
    }

    #[cfg(unix)]
    #[test]
    fn public_v2_event_cpi_identity_and_canonical_state_mismatches_block_qualification() {
        let temporary = tempdir().expect("temporary V2 Event-CPI negative fixture root");
        let semantics_path = prospective_v2_semantics_manifest_path_for_test();
        let semantics = load_pump_exact_state_semantics_authority_v2(&semantics_path)
            .expect("real vendored V2 semantics must load for Event-CPI negative fixtures");
        for (label, variant) in [
            (
                "wrong-mint",
                QualifiedExportFixtureVariantV2::WrongEventMint,
            ),
            (
                "wrong-user",
                QualifiedExportFixtureVariantV2::WrongEventUser,
            ),
            (
                "wrong-ix-name",
                QualifiedExportFixtureVariantV2::WrongEventIxName,
            ),
            (
                "wrong-quote-mint",
                QualifiedExportFixtureVariantV2::WrongEventQuoteMint,
            ),
            (
                "wrong-canonical-reserve",
                QualifiedExportFixtureVariantV2::WrongEventCanonicalReserve,
            ),
        ] {
            let raw_dir = temporary.path().join(format!("{label}-raw-v2"));
            let exact_dir = temporary.path().join(format!("{label}-exact-v2"));
            write_complete_raw_fixture_for_qualified_export_v2(
                &raw_dir,
                &format!("event-cpi-{label}"),
                &semantics,
                variant,
            );
            let summary =
                qualify_prospective_exact_state_raw_run_v2(&raw_dir, &semantics_path, &exact_dir)
                    .expect(
                    "malformed Event-CPI remains a durable blocked diagnostic, not a raw I/O error",
                );
            assert_eq!(
                summary.status,
                PumpExactStateCapabilityStatusV2::Blocked,
                "Event-CPI {label} must not qualify"
            );
            let receipt: serde_json::Value = serde_json::from_slice(
                &fs::read(exact_dir.join("exact_state_capability_v2.json"))
                    .expect("read blocked Event-CPI receipt"),
            )
            .expect("parse blocked Event-CPI receipt");
            assert!(
                receipt["successful_rooted_unknown_occurrence_count"]
                    .as_u64()
                    .is_some_and(|count| count >= 1),
                "Event-CPI {label} must become an explicit Unknown occurrence: {receipt}"
            );
            assert!(
                receipt["blockers"]
                    .as_array()
                    .is_some_and(|blockers| blockers.iter().any(|value| {
                        value == "mutation_inventory_incomplete"
                            || value == "exact_coverage_below_threshold"
                    })),
                "Event-CPI {label} must remain fail-closed: {receipt}"
            );
        }
    }

    #[test]
    fn v2_rejects_pump_owned_account_outside_bonding_curve_or_global_contract() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let account = SubscribeUpdateAccount {
            account: Some(SubscribeUpdateAccountInfo {
                pubkey: Pubkey::new_unique().to_bytes().to_vec(),
                lamports: 123,
                owner: pump.to_bytes().to_vec(),
                executable: false,
                rent_epoch: 7,
                data: vec![0xde, 0xad],
                write_version: 9,
                txn_signature: None,
            }),
            slot: 88,
            is_startup: false,
        };
        let error = one_source_record(11, source_update(account))
            .expect_err("out-of-scope Pump account must fail the source contract");
        assert!(format!("{error:#}").contains("outside canonical Global/BondingCurve scope"));
    }

    #[test]
    fn v2_source_payload_retains_the_whole_subscribe_update_envelope() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let source = source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![0xca, 0xfe],
        ));
        let expected_payload = source.update.encode_to_vec();
        let record = one_source_record(12, source).expect("convert complete source envelope");
        let PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) = record else {
            panic!("expected Pump-owned account update");
        };
        assert_eq!(
            update.source_payload.as_slice(),
            expected_payload.as_slice()
        );
        assert_eq!(
            update.source.payload_hash_blake3,
            hash_bytes_v2(&expected_payload)
        );
        let decoded = SubscribeUpdate::decode(update.source_payload.as_slice())
            .expect("stored V2 source payload is a complete SubscribeUpdate");
        assert_eq!(
            decoded.filters,
            vec!["pump_research_exact_state_v2_bonding_curves".to_owned()]
        );
        assert!(matches!(
            decoded.update_oneof,
            Some(UpdateOneof::Account(_))
        ));
    }

    #[test]
    fn v2_rejects_account_not_owned_by_pump_even_if_source_filter_claims_otherwise() {
        let account = pump_owned_account(Pubkey::new_unique(), Pubkey::new_unique(), vec![1]);
        let error = one_source_record(0, source_update(account))
            .expect_err("non-Pump owner must fail the source contract");
        assert!(error.to_string().contains("non-Pump owner"));
    }

    #[test]
    fn v2_preserves_an_unfiltered_full_block_in_bounded_source_chunks() {
        let block = SubscribeUpdateBlock {
            slot: 91,
            parent_slot: 90,
            blockhash: "blockhash-test".to_owned(),
            parent_blockhash: "parent-blockhash-test".to_owned(),
            executed_transaction_count: 17,
            ..SubscribeUpdateBlock::default()
        };
        let source = source_block_update(block);
        let expected_source_payload = source.update.encode_to_vec();
        let records = raw_records_from_source_v2(27, source)
            .expect("convert unfiltered full V2 block evidence");
        assert_eq!(records.len(), 3, "small block uses one bounded chunk");
        let PumpExactStateRawRecordV2::FullBlockPayloadStarted(started) = &records[0] else {
            panic!("first record must bind full block identity");
        };
        assert_eq!(started.source.capture_sequence, 27);
        assert_eq!(started.slot, 91);
        assert_eq!(started.parent_slot, 90);
        assert_eq!(started.executed_transaction_count, 17);
        assert_eq!(
            started.event_time.ingress_wall_ts_ms,
            Some(1_700_000_000_000)
        );
        assert_eq!(started.event_time.ingress_monotonic_ts_ms, Some(42));
        assert!(
            started.source_payload_bytes
                <= ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_FULL_BLOCK_CHUNK_BYTES_V2
                    as u64
        );
        let PumpExactStateRawRecordV2::FullBlockPayloadChunk(chunk) = &records[1] else {
            panic!("second record must retain full block source bytes");
        };
        assert_eq!(chunk.source_capture_sequence, 27);
        assert_eq!(chunk.chunk_index, 0);
        assert_eq!(chunk.bytes, expected_source_payload);
        let PumpExactStateRawRecordV2::FullBlockPayloadCompleted(completed) = &records[2] else {
            panic!("last record must bind complete full-block payload");
        };
        assert_eq!(completed.source_capture_sequence, 27);
        assert_eq!(
            completed.source_payload_blake3,
            started.source.payload_hash_blake3
        );
        assert_eq!(
            completed.source_payload_sha256,
            started.source_payload_sha256
        );
    }

    #[test]
    fn v2_classifies_canonical_global_before_any_data_layout_assumption() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let global =
            Pubkey::from_str(PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1).expect("canonical Pump Global");
        let record = one_source_record(7, source_update(pump_owned_account(global, pump, vec![])))
            .expect("convert canonical global");
        let PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) = record else {
            panic!("expected Pump-owned account update");
        };
        assert_eq!(
            update.evidence_class,
            PumpExactStateAccountEvidenceClassV2::CanonicalGlobal
        );
    }

    #[test]
    fn v2_bounded_ingress_owns_a_protobuf_byte_vector_not_a_decoded_object_graph() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let source = source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![0xaa, 0xbb, 0xcc],
        ));
        let expected_payload = source.update.encode_to_vec();
        let required_lane = required_source_lane_observation_v2(&source)
            .expect("source fixture belongs to required account lane");
        let queued =
            queued_source_update_v2(17, source, required_lane).expect("queue source update");
        assert_eq!(queued.encoded_update, expected_payload);
        assert!(
            queued.byte_cost
                >= u64::try_from(queued.encoded_update.len()).expect("payload length fits u64")
        );
        let decoded = queued
            .decode_update()
            .expect("decode retained V2 protobuf bytes");
        assert_eq!(queued.capture_sequence, 17);
        assert!(queued.byte_cost >= u64::try_from(expected_payload.len()).expect("payload length"));
        assert_eq!(decoded.encode_to_vec(), expected_payload);
        assert_eq!(queued.stream_epoch, 4);
    }

    fn decode_v2_segment(
        path: &Path,
    ) -> (
        ghost_core::pump_research_exact_tape_v2::PumpExactStateSegmentHeaderV2,
        Vec<PumpExactStateRawRecordV2>,
    ) {
        let bytes = fs::read(path).expect("read V2 raw segment");
        const MAGIC_BYTES: usize = 8;
        let header_payload_len = u32::from_le_bytes(
            bytes[MAGIC_BYTES..MAGIC_BYTES + 4]
                .try_into()
                .expect("header payload length"),
        ) as usize;
        let header_end = MAGIC_BYTES + 4 + header_payload_len + 32;
        let header = ghost_core::pump_research_exact_tape_v2::PumpExactStateRawCodecV2::decode_segment_header(
            &bytes[..header_end],
        )
        .expect("decode V2 header");
        let mut cursor = header_end;
        let mut records = Vec::new();
        while cursor < bytes.len() {
            let payload_len = u32::from_le_bytes(
                bytes[cursor..cursor + 4]
                    .try_into()
                    .expect("record payload length"),
            ) as usize;
            let end = cursor + 4 + payload_len + 32;
            records.push(
                ghost_core::pump_research_exact_tape_v2::PumpExactStateRawCodecV2::decode_record(
                    &bytes[cursor..end],
                )
                .expect("decode V2 record"),
            );
            cursor = end;
        }
        (header, records)
    }

    #[test]
    fn v2_writer_publishes_an_independent_segment_with_canonical_curve_evidence() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([3; 32]),
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        writer
            .write_source(
                0,
                source_update(pump_owned_account(Pubkey::new_unique(), pump, vec![0x42])),
            )
            .expect("write V2 source record");
        let receipt = writer
            .close_current(true)
            .expect("close V2 segment")
            .expect("published V2 segment receipt");

        assert_eq!(writer.receipts(), std::slice::from_ref(&receipt));
        assert!(raw_dir.join(&receipt.filename).is_file());
        assert!(
            !raw_dir
                .join(format!("{}.partial", receipt.filename))
                .exists(),
            "only an atomically published V2 segment may remain"
        );
        let (header, records) = decode_v2_segment(&raw_dir.join(&receipt.filename));
        assert_eq!(header.run_id, "prospective-exact-test");
        assert_eq!(header.stream_epoch, 4);
        assert_eq!(records.len(), 2);
        assert!(matches!(
            &records[0],
            PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update)
                if update.evidence_class == PumpExactStateAccountEvidenceClassV2::CanonicalBondingCurve
        ));
        assert!(matches!(
            &records[1],
            PumpExactStateRawRecordV2::SegmentClosed(footer)
                if footer.clean_shutdown && footer.accepted_record_count == 1
        ));
        assert_eq!(
            receipt.file_bytes,
            fs::metadata(raw_dir.join(&receipt.filename))
                .expect("segment metadata")
                .len()
        );
        assert_eq!(writer.raw_bytes_written(), receipt.file_bytes);
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;

            assert_eq!(
                fs::metadata(raw_dir.join(&receipt.filename))
                    .expect("segment metadata")
                    .permissions()
                    .mode()
                    & 0o777,
                0o600,
                "published V2 raw segments must not depend on process umask for authority privacy"
            );
        }
    }

    #[test]
    fn v2_writer_uses_the_exact_protobuf_bytes_owned_by_ingress() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([35; 32]),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let source = source_update(pump_owned_account(Pubkey::new_unique(), pump, vec![0x44]));
        let mut ingress_envelope = source.update.clone();
        ingress_envelope.filters = vec!["queued-source-provenance".to_owned()];
        let ingress_payload = ingress_envelope.encode_to_vec();

        writer
            .write_source_with_payload(0, source, ingress_payload.clone())
            .expect("persist exact ingress payload");
        writer.close_current(true).expect("close V2 segment");
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        let PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) = &records[0] else {
            panic!("expected Pump-owned account update");
        };
        assert_eq!(update.source_payload.as_slice(), ingress_payload.as_slice());
        assert_eq!(
            SubscribeUpdate::decode(update.source_payload.as_slice())
                .expect("stored ingress payload decodes")
                .filters,
            vec!["queued-source-provenance".to_owned()]
        );
    }

    #[test]
    fn v2_writer_keeps_the_terminal_footer_inside_the_segment_byte_cap() {
        const SEGMENT_CAP: u64 = 8 * 1024;
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([33; 32]),
            Duration::from_secs(60),
            SEGMENT_CAP,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create footer-bounded V2 writer");
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        for sequence in 0..2 {
            writer
                .write_source(
                    sequence,
                    source_update(pump_owned_account(
                        Pubkey::new_unique(),
                        pump,
                        vec![sequence as u8],
                    )),
                )
                .expect("write V2 source record under a physical segment cap");
        }
        writer
            .close_current(true)
            .expect("close footer-bounded segment");
        assert!(!writer.receipts().is_empty());
        for receipt in writer.receipts() {
            assert!(
                receipt.file_bytes <= SEGMENT_CAP,
                "segment {} is {} bytes, above physical cap {}",
                receipt.segment_index,
                receipt.file_bytes,
                SEGMENT_CAP
            );
        }
    }

    #[test]
    fn v2_writer_preserves_full_block_evidence_in_the_same_capture_sequence() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([31; 32]),
            Duration::from_secs(60),
            16 * 1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        writer
            .write_source(
                41,
                source_block_update(SubscribeUpdateBlock {
                    slot: 101,
                    parent_slot: 100,
                    blockhash: "full-block-test".to_owned(),
                    parent_blockhash: "parent-full-block-test".to_owned(),
                    executed_transaction_count: 3,
                    ..SubscribeUpdateBlock::default()
                }),
            )
            .expect("persist bounded full block evidence");
        writer.close_current(true).expect("close V2 segment");
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        assert!(matches!(
            records.as_slice(),
            [
                PumpExactStateRawRecordV2::FullBlockPayloadStarted(started),
                PumpExactStateRawRecordV2::FullBlockPayloadChunk(chunk),
                PumpExactStateRawRecordV2::FullBlockPayloadCompleted(completed),
                PumpExactStateRawRecordV2::SegmentClosed(_),
            ] if started.source.capture_sequence == 41
                && chunk.source_capture_sequence == 41
                && completed.source_capture_sequence == 41
        ));
    }

    #[test]
    fn v2_writer_rejects_an_unfinished_full_block_payload_at_clean_close() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([31; 32]),
            Duration::from_secs(60),
            16 * 1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        let records = raw_records_from_source_v2(42, source_full_block_update(106))
            .expect("convert bounded full-block evidence");
        let started = records
            .first()
            .expect("full block begins with a started record");
        writer
            .write_record(4, Some(42), started)
            .expect("persist only the started record");
        writer
            .full_block_reconciliation
            .observe_written_record(started)
            .expect("track the durable started record");

        let error = writer
            .close_current(true)
            .expect_err("an uncompleted full-block payload must prevent a clean close");
        assert!(error
            .to_string()
            .contains("full block payload reconciliation is incomplete"));
        assert!(!raw_dir.join("segment_00000.bin").exists());
        assert!(raw_dir.join("segment_00000.bin.partial").exists());
    }

    #[test]
    fn v2_writer_fails_before_creating_a_segment_when_raw_byte_budget_cannot_hold_its_header() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([32; 32]),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
            1,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create bounded V2 writer");
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let error = writer
            .write_source(
                0,
                source_update(pump_owned_account(Pubkey::new_unique(), pump, vec![1])),
            )
            .expect_err("insufficient V2 raw byte budget must fail closed");
        assert!(format!("{error:#}").contains("raw byte budget exceeded"));
        assert!(!raw_dir.join("segment_00000.bin").exists());
        assert!(!raw_dir.join("segment_00000.bin.partial").exists());
        assert_eq!(writer.raw_bytes_written(), 0);
    }

    #[test]
    fn v2_writer_rotates_on_source_epoch_and_chains_the_prefix_digest() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([4; 32]),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        writer
            .write_source(
                0,
                source_update(pump_owned_account(Pubkey::new_unique(), pump, vec![1])),
            )
            .expect("write epoch one");
        let mut second = source_update(pump_owned_account(Pubkey::new_unique(), pump, vec![2]));
        second.stream_epoch = 5;
        writer.write_source(1, second).expect("write epoch two");
        writer.close_current(true).expect("close final segment");

        assert_eq!(writer.receipts().len(), 2);
        let (first_header, first_records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        let (second_header, second_records) = decode_v2_segment(&raw_dir.join("segment_00001.bin"));
        assert_eq!(first_header.stream_epoch, 4);
        assert_eq!(second_header.stream_epoch, 5);
        let PumpExactStateRawRecordV2::SegmentClosed(first_footer) =
            first_records.last().expect("first footer")
        else {
            panic!("first segment must end with a footer");
        };
        assert!(!first_footer.clean_shutdown);
        assert_eq!(
            second_header.previous_segment_blake3,
            Some(first_footer.segment_blake3)
        );
        assert!(matches!(
            second_records.last(),
            Some(PumpExactStateRawRecordV2::SegmentClosed(footer)) if footer.clean_shutdown
        ));
    }

    fn start_test_capture_coordinator_v3(raw_dir: &Path) -> PumpExactStateCaptureCoordinatorV2 {
        PumpExactStateCaptureCoordinatorV2::start(
            raw_dir,
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start bounded PRXTAPE3 coordinator")
    }

    #[test]
    fn v3_readiness_requires_all_lanes_and_one_durable_boundary_before_clean_close() {
        let temporary = tempdir().expect("temporary PRXTAPE3 root");
        let missing = start_test_capture_coordinator_v3(&temporary.path().join("missing-lane"));
        let missing_sink = missing.source_sink();
        missing_sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        missing_sink.try_capture(source_transaction_update(90, 1));
        missing_sink.try_capture(source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![1],
        )));
        missing_sink.try_capture(source_slot_update(90));
        missing_sink.try_capture(source_block_meta_update(90));
        missing.finish_source();
        let missing_summary = missing.finish_and_join();
        assert!(
            !missing_summary.clean_shutdown,
            "a source lacking any required lane must be incomplete"
        );
        assert_eq!(
            missing.source_lifecycle().source_readiness_status,
            "failed",
            "the missing lane must prevent a clean cohort from starting"
        );

        let raw_dir = temporary.path().join("complete-five-lane");
        let coordinator = start_test_capture_coordinator_v3(&raw_dir);
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        sink.try_capture(source_transaction_update(91, 2));
        sink.try_capture(source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![2],
        )));
        sink.try_capture(source_slot_update(91));
        sink.try_capture(source_block_meta_update(91));
        sink.try_capture(source_full_block_update(91));

        let runtime = tokio::runtime::Runtime::new().expect("test Tokio runtime");
        let seal = runtime
            .block_on(persist_stream_readiness_boundary_v2(
                &coordinator,
                Duration::from_secs(1),
            ))
            .expect("five accepted lanes must seal one durable readiness boundary");
        assert_eq!(seal.cohort_slots_strictly_after, 91);
        assert_eq!(
            seal.source_readiness,
            source_readiness_for_test(91, 88, 91, 91, 91)
        );

        let duplicate = coordinator.arm_stream_boundary();
        assert!(duplicate
            .expect_err("a second stream boundary must be rejected")
            .to_string()
            .contains("exactly once"));

        // A source update admitted after the durable acknowledgement must be
        // ordered after the reserved boundary marker, never physically ahead
        // of it on the data lane.
        sink.try_capture(source_slot_update(92));
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(
            summary.clean_shutdown,
            "five-lane stream must close cleanly: {summary:?}"
        );
        assert_eq!(summary.accepted_readiness_boundary_records, 1);
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        let boundary_index = records
            .iter()
            .position(|record| {
                matches!(
                    record,
                    PumpExactStateRawRecordV2::ProspectiveStreamBoundary(_)
                )
            })
            .expect("the writer must flush one persisted readiness boundary");
        assert_eq!(
            records
                .iter()
                .filter(|record| matches!(
                    record,
                    PumpExactStateRawRecordV2::ProspectiveStreamBoundary(_)
                ))
                .count(),
            1,
            "the writer must flush exactly one persisted readiness boundary"
        );
        let post_boundary_slot_sequence = records
            .iter()
            .skip(boundary_index + 1)
            .find_map(|record| match record {
                PumpExactStateRawRecordV2::PrimarySlotUpdate(slot) if slot.slot == 92 => {
                    Some(slot.source.capture_sequence)
                }
                _ => None,
            })
            .expect("post-boundary Slot source must follow the raw boundary frame");
        assert_eq!(post_boundary_slot_sequence, 6);
    }

    #[test]
    fn v3_source_epoch_change_fails_closed_before_a_boundary_can_be_sealed() {
        let temporary = tempdir().expect("temporary PRXTAPE3 root");
        let coordinator = start_test_capture_coordinator_v3(&temporary.path().join("epoch-change"));
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        sink.source_stream_established(5);
        assert!(
            coordinator.capture_abort().is_cancelled(),
            "a reconnect/epoch change must stop the source rather than combine epochs"
        );
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown);
        assert!(coordinator
            .source_lifecycle()
            .fatal_capture_error
            .as_deref()
            .is_some_and(|reason| reason.contains("reconnected")));
    }

    #[test]
    fn v3_config_rejects_retired_bootstrap_fields() {
        let source = r#"
primary_provider_id = "primary"
grpc_endpoint = "https://grpc.example.invalid"
program_data_rpc_endpoint = "https://rpc.example.invalid"
semantics_manifest_path = "/protected/operator/semantics.json"
output_dir = "/protected/research/prxtape3"
bootstrap_queue_capacity = 1
"#;
        let error = toml::from_str::<PumpExactStateCaptureConfigV2>(source)
            .expect_err("retired bootstrap fields must not silently load");
        assert!(error
            .to_string()
            .contains("unknown field `bootstrap_queue_capacity`"));
    }

    #[tokio::test]
    async fn v3_program_data_receipt_rpc_uses_base64_and_reads_only_program_and_programdata_accounts(
    ) {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind local ProgramData RPC mock");
        let endpoint = format!(
            "http://{}",
            listener
                .local_addr()
                .expect("local ProgramData RPC address")
        );
        let pump_program = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let program_data = Pubkey::new_unique();
        let program_account_data = bincode::serialize(&UpgradeableLoaderState::Program {
            programdata_address: program_data,
        })
        .expect("encode mock Program account");
        let program_data_account_data = bincode::serialize(&UpgradeableLoaderState::ProgramData {
            slot: 77,
            upgrade_authority_address: None,
        })
        .expect("encode mock ProgramData account");
        let server = tokio::spawn(async move {
            let mut observed_methods = Vec::new();
            let mut observed_accounts = Vec::new();
            let mut observed_encodings = Vec::new();
            for (expected_account, account_data) in [
                (pump_program, program_account_data),
                (program_data, program_data_account_data),
            ] {
                let (mut socket, _) = listener
                    .accept()
                    .await
                    .expect("accept ProgramData RPC read");
                let request = read_v2_mock_rpc_request(&mut socket).await;
                observed_methods.push(request["method"].as_str().expect("RPC method").to_owned());
                observed_accounts.push(
                    request["params"]
                        .as_array()
                        .and_then(|params| params.first())
                        .and_then(serde_json::Value::as_str)
                        .expect("getAccountInfo account parameter")
                        .to_owned(),
                );
                observed_encodings.push(
                    request["params"]
                        .as_array()
                        .and_then(|params| params.get(1))
                        .and_then(|config| config.get("encoding"))
                        .and_then(serde_json::Value::as_str)
                        .expect("ProgramData RPC request must use explicit Base64 encoding")
                        .to_owned(),
                );
                assert_eq!(
                    observed_accounts.last(),
                    Some(&expected_account.to_string()),
                    "ProgramData receipt may read only its expected account"
                );
                write_v2_mock_rpc_response(&mut socket, request["id"].clone(), 900, &account_data)
                    .await;
            }
            (observed_methods, observed_accounts, observed_encodings)
        });

        let receipt = observe_program_data_receipt_v2(&endpoint, None, "x-test", pump_program)
            .await
            .expect("ProgramData receipt from local mock");
        let (methods, accounts, encodings) = server.await.expect("ProgramData RPC mock task");
        assert_eq!(methods, vec!["getAccountInfo", "getAccountInfo"]);
        assert_eq!(
            accounts,
            vec![pump_program.to_string(), program_data.to_string()]
        );
        assert_eq!(encodings, vec!["base64", "base64"]);
        assert_eq!(
            receipt.pump_programdata_pubkey.into_inner(),
            program_data.to_bytes()
        );
        assert!(
            !methods.iter().any(|method| method == "getProgramAccounts"),
            "stream-only V3 receipt RPC must never scan the Pump account universe"
        );
    }

    #[test]
    fn v2_config_rejects_optional_capture_and_nonisolated_output_roots() {
        let temporary = tempdir().expect("temporary V2 config root");
        let dedicated_root = temporary.path().join("prospective-v2");
        create_private_directory_v2(&dedicated_root).expect("create private V2 output root");
        let config = PumpExactStateCaptureConfigV2 {
            primary_provider_id: "primary".to_owned(),
            grpc_endpoint: "https://grpc.example.invalid".to_owned(),
            grpc_auth_token_env: None,
            grpc_auth_header: "x-token".to_owned(),
            program_data_rpc_endpoint: "https://rpc.example.invalid".to_owned(),
            program_data_rpc_auth_token_env: None,
            program_data_rpc_auth_header: "x-api-key".to_owned(),
            pump_program_id: PUMP_FUN_PROGRAM_ID.to_owned(),
            semantics_manifest_path: temporary.path().join("semantics.json"),
            output_dir: PathBuf::from("datasets/pump-research/raw"),
            required_for_run: true,
            source_queue_capacity: 1,
            source_queue_max_bytes: MIN_V2_SOURCE_QUEUE_MAX_BYTES,
            cohort_capture_wall_ms: MIN_V2_COHORT_CAPTURE_WALL_MS,
            min_free_bytes: MIN_V2_MIN_FREE_BYTES,
            max_raw_bytes: MIN_V2_MAX_RAW_BYTES,
            source_readiness_timeout_ms: 1,
            flush_interval_ms: 1,
            segment_max_bytes: MIN_V2_SEGMENT_MAX_BYTES,
            segment_max_duration_ms: 1,
        };
        assert!(config.validate().is_err());

        let mut optional = config;
        optional.output_dir = dedicated_root.clone();
        optional.required_for_run = false;
        assert!(optional
            .validate()
            .expect_err("optional V2 capture must fail closed")
            .to_string()
            .contains("required_for_run"));

        let frozen_run = temporary.path().join("historical-run");
        create_private_directory_v2(&frozen_run).expect("create historical run root");
        let frozen_raw = frozen_run.join("raw");
        create_private_directory_v2(&frozen_raw).expect("create frozen raw root");
        fs::write(frozen_raw.join("run_completion_receipt.json"), b"{}\n")
            .expect("write frozen completion marker");
        let inside_frozen_run = frozen_run.join("prospective-output");
        create_private_directory_v2(&inside_frozen_run)
            .expect("create accidental output directory inside frozen run");
        optional.required_for_run = true;
        optional.output_dir = inside_frozen_run;
        assert!(optional
            .validate()
            .expect_err("V2 output inside frozen run must fail before I/O")
            .to_string()
            .contains("inside existing raw/run authority"));

        let mut too_short = optional;
        too_short.output_dir = dedicated_root;
        too_short.cohort_capture_wall_ms = MIN_V2_COHORT_CAPTURE_WALL_MS - 1;
        assert!(too_short
            .validate()
            .expect_err("unbounded/too-short V2 cohort wall must fail closed")
            .to_string()
            .contains("cohort_capture_wall_ms"));
    }

    #[test]
    fn v2_storage_contract_requires_reserve_plus_raw_budget_without_overflow() {
        assert_eq!(
            required_v2_storage_bytes(14_000_000_000, 16_000_000_000)
                .expect("valid configured V2 storage contract"),
            30_000_000_000 + V2_CAPTURE_METADATA_ALLOWANCE_BYTES
        );
        assert!(required_v2_storage_bytes(u64::MAX, 1).is_err());
    }

    fn sealed_preflight_bundle_fixture_v2() -> (
        tempfile::TempDir,
        PathBuf,
        PumpExactStateOperatorPreflightReceiptV2,
    ) {
        let temporary = tempdir().expect("temporary V2 preflight bundle root");
        let bundle_dir = temporary.path().join("bundle");
        create_private_directory_v2(&bundle_dir).expect("create private V2 bundle");
        let release_dir = bundle_dir.join(V2_OPERATOR_PREFLIGHT_RELEASE_DIR);
        create_private_directory_v2(&release_dir).expect("create private V2 release directory");

        let binary_bytes = b"fixture sealed V2 release binary\n";
        let release_binary_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE);
        write_bytes_create_new_v2(
            &release_binary_path,
            binary_bytes,
            0o700,
            "test sealed V2 release binary",
        )
        .expect("write sealed V2 release binary");
        let release_binary_digest = digest_bytes_v2(binary_bytes);

        let build_log = b"[stdout]\nfixture build\n[stderr]\n";
        let build_log_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE);
        write_bytes_create_new_v2(&build_log_path, build_log, 0o600, "test V2 build log")
            .expect("write V2 build log");
        let build_log_digest = digest_bytes_v2(build_log);

        let build_receipt = PumpExactStateFreshBuildReceiptV2 {
            schema_version: V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION,
            build_semantics: V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS.to_owned(),
            repository_commit: "e5122fa1fa0321f249905b1e3aada936d36ba5a3".to_owned(),
            cargo_command: fresh_build_cargo_command_v2(),
            build_log_digest: build_log_digest.clone(),
            release_binary_digest: release_binary_digest.clone(),
            build_started_wall_ms: 1,
            build_completed_wall_ms: 2,
        };
        let build_receipt_path = bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE);
        write_json_create_new_v2(&build_receipt_path, &build_receipt)
            .expect("write V2 build receipt");
        let build_receipt_bytes = read_bounded_regular_file_v2(
            &build_receipt_path,
            "test V2 build receipt",
            V2_OPERATOR_PREFLIGHT_RECEIPT_MAX_BYTES,
        )
        .expect("read V2 build receipt");

        let receipt = PumpExactStateOperatorPreflightReceiptV2 {
            schema_version: V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION,
            receipt_kind: "pump_exact_state_operator_preflight_v2".to_owned(),
            created_wall_ms: 3,
            repository_commit: build_receipt.repository_commit.clone(),
            git_status_digest: digest_bytes_v2(b""),
            git_status_entry_count: 0,
            capture_config_digest: digest_bytes_v2(b"fixture-config"),
            preflight_executable_digest: release_binary_digest.clone(),
            release_binary_file: V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE.to_owned(),
            release_binary_digest,
            build_log_file: V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE.to_owned(),
            build_log_digest,
            build_receipt_file: V2_OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE.to_owned(),
            build_receipt_digest: digest_bytes_v2(&build_receipt_bytes),
            build_semantics: V2_OPERATOR_PREFLIGHT_BUILD_SEMANTICS.to_owned(),
            source_request_fingerprint_blake3: "fixture-request-fingerprint".to_owned(),
            source_capture_semantics: "fixture-source-semantics".to_owned(),
            source_max_decoded_message_bytes: 1,
            semantics_id: "fixture-semantics".to_owned(),
            semantics_manifest_digest: digest_bytes_v2(b"fixture-semantics-manifest"),
            vendored_idl_digest: digest_bytes_v2(b"fixture-vendored-idl"),
            expected_program_data_hash_blake3: "11".repeat(32),
            cohort_capture_wall_ms: MIN_V2_COHORT_CAPTURE_WALL_MS,
            min_free_bytes: MIN_V2_MIN_FREE_BYTES,
            max_raw_bytes: MIN_V2_MAX_RAW_BYTES,
            required_storage_bytes: required_v2_storage_bytes(
                MIN_V2_MIN_FREE_BYTES,
                MIN_V2_MAX_RAW_BYTES,
            )
            .expect("fixture storage contract"),
            output_filesystem_available_bytes_at_preflight: u64::MAX,
        };
        (temporary, bundle_dir, receipt)
    }

    #[test]
    fn v2_sealed_preflight_bundle_rejects_release_or_build_log_drift() {
        let (_temporary, bundle_dir, receipt) = sealed_preflight_bundle_fixture_v2();
        validate_operator_preflight_bundle_contents_v2(&bundle_dir, &receipt)
            .expect("complete sealed bundle must validate before capture");

        fs::write(
            bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE),
            b"mutated build log\n",
        )
        .expect("mutate only temporary fixture build log");
        let error = validate_operator_preflight_bundle_contents_v2(&bundle_dir, &receipt)
            .expect_err("build-log drift must invalidate capture authority");
        assert!(format!("{error:#}").contains("fresh-build log digest"));

        fs::write(
            bundle_dir.join(V2_OPERATOR_PREFLIGHT_BUILD_LOG_FILE),
            b"[stdout]\nfixture build\n[stderr]\n",
        )
        .expect("restore temporary fixture build log");
        fs::write(
            bundle_dir.join(V2_OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE),
            b"replacement",
        )
        .expect("mutate only temporary fixture release binary");
        let error = validate_operator_preflight_bundle_contents_v2(&bundle_dir, &receipt)
            .expect_err("release-binary drift must invalidate capture authority");
        assert!(format!("{error:#}").contains("fresh release binary digest"));
    }

    #[cfg(unix)]
    #[test]
    fn v2_operator_config_reader_requires_a_private_regular_file() {
        use std::os::unix::fs::PermissionsExt;

        let temporary = tempdir().expect("temporary V2 config authority root");
        let config_path = temporary.path().join("capture.toml");
        fs::write(&config_path, b"primary_provider_id = 'fixture'\n")
            .expect("write temporary V2 config");
        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o644))
            .expect("make temporary V2 config non-private");
        let error = read_private_regular_file_v2(&config_path, "test V2 config", 1024)
            .expect_err("world-readable V2 operator config must fail closed");
        assert!(format!("{error:#}").contains("must not grant group/other access"));

        fs::set_permissions(&config_path, fs::Permissions::from_mode(0o600))
            .expect("make temporary V2 config private");
        assert_eq!(
            read_private_regular_file_v2(&config_path, "test V2 config", 1024)
                .expect("private V2 config must be readable"),
            b"primary_provider_id = 'fixture'\n"
        );
    }

    #[cfg(unix)]
    #[test]
    fn v2_bounded_authority_reader_fails_closed_if_a_regular_file_becomes_fifo() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let temporary = tempdir().expect("temporary authority root");
        let authority_path = temporary.path().join("capture.toml");
        let moved_regular_path = temporary.path().join("capture.toml.before-fifo");
        fs::write(&authority_path, b"key = 'value'\n").expect("write regular authority file");
        let fifo_path = CString::new(authority_path.as_os_str().as_bytes())
            .expect("temporary path contains no NUL");
        let started = Instant::now();
        let error = read_bounded_regular_file_v2_after_precheck(
            &authority_path,
            "test authority",
            1024,
            || {
                fs::rename(&authority_path, &moved_regular_path)
                    .expect("move regular file after precheck");
                // SAFETY: `fifo_path` is a NUL-terminated path within a
                // private temporary directory, and no other test owns it.
                assert_eq!(unsafe { libc::mkfifo(fifo_path.as_ptr(), 0o600) }, 0);
            },
        )
        .expect_err("a FIFO substituted after precheck must not block or pass");
        assert!(
            started.elapsed() < Duration::from_secs(1),
            "O_NONBLOCK must turn the substitution into a bounded local failure"
        );
        assert!(
            format!("{error:#}").contains("not a bounded regular file"),
            "unexpected error: {error:#}"
        );
    }
}
