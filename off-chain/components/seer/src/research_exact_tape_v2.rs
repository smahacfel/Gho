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
    research_tape::observe_program_data_receipt,
};
use anyhow::{bail, Context, Result};
use base64::{engine::general_purpose::STANDARD as BASE64_STANDARD, Engine as _};
use ghost_core::{
    pump_research_exact_tape_v2::{
        PumpExactStateAccountEvidenceClassV2, PumpExactStateBlockMetaEvidenceV2,
        PumpExactStateBootstrapCommitmentV2, PumpExactStateBootstrapProgramOwnedAccountV2,
        PumpExactStateBootstrapResponseChunkV2, PumpExactStateBootstrapSnapshotCompletedV2,
        PumpExactStateBootstrapSnapshotStartedV2, PumpExactStateCoverageBoundaryV2,
        PumpExactStateCoverageGapReasonV2, PumpExactStateCoverageGapV2,
        PumpExactStateFullBlockPayloadChunkV2, PumpExactStateFullBlockPayloadCompletedV2,
        PumpExactStateFullBlockPayloadStartedV2, PumpExactStateProspectiveReadinessBoundaryV2,
        PumpExactStateProviderRoleV2, PumpExactStatePumpOwnedAccountUpdateV2,
        PumpExactStateRawRecordV2, PumpExactStateSlotEvidenceV2, PumpExactStateSourceEnvelopeV2,
        PumpExactStateSourceReadinessV2, PumpExactStateTransactionEvidenceV2,
    },
    pump_research_tape::{
        PumpProgramDataReceiptV1, PumpResearchEventTimeV1, PumpResearchStoragePubkeyV1,
        PumpResearchStorageSignatureV1, PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1,
    },
    LocalCoverageBoundaryV1, LocalCoverageGapReasonV1, LocalCoverageGapV1,
};
use parking_lot::Mutex;
use prost::Message;
use reqwest::{
    header::{HeaderMap, HeaderName, HeaderValue, CONTENT_TYPE, USER_AGENT},
    Url,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sha2::{Digest, Sha256};
use solana_sdk::{pubkey::Pubkey, signature::Signature};
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

const EXACT_STATE_TAPE_V2_CONFIG_SCHEMA_VERSION: u16 = 1;
const EXACT_STATE_TAPE_V2_RUN_SCHEMA_VERSION: u16 = 1;
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
/// gives the finalized-bootstrap control plane a chance to persist one record.
/// Without this fairness bound a permanently busy source channel could starve
/// the bootstrap boundary indefinitely.
const V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE: usize = 256;
const DEFAULT_V2_BOOTSTRAP_QUEUE_CAPACITY: usize = 4_096;
const DEFAULT_V2_BOOTSTRAP_ENQUEUE_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_V2_STREAM_ESTABLISH_TIMEOUT_MS: u64 = 30_000;
const DEFAULT_V2_BOOTSTRAP_RPC_TIMEOUT_MS: u64 = 300_000;
const DEFAULT_V2_BOOTSTRAP_RESPONSE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_V2_BOOTSTRAP_RPC_TIMEOUT_MS: u64 = 600_000;
const MAX_V2_BOOTSTRAP_RESPONSE_MAX_BYTES: u64 = 512 * 1024 * 1024;
const MAX_V2_SOURCE_QUEUE_MAX_BYTES: u64 = 2 * 1024 * 1024 * 1024;
const DEFAULT_V2_FLUSH_INTERVAL_MS: u64 = 1_000;
const DEFAULT_V2_SEGMENT_MAX_BYTES: u64 = 256 * 1024 * 1024;
const DEFAULT_V2_SEGMENT_MAX_DURATION_MS: u64 = 300_000;
const V2_CAPTURE_CONFIG_MAX_BYTES: u64 = 128 * 1024;
/// Atomic sentinel for a required source lane that has not yet admitted an
/// update.  Slot `u64::MAX` is rejected as malformed source evidence, so the
/// sentinel cannot be confused with an observed lane slot.
const V2_REQUIRED_LANE_SLOT_UNSET: u64 = u64::MAX;

/// Dedicated configuration for a prospective Exact-State Tape V2 run.
///
/// It is deliberately not embedded in `SeerConfig`: no active Ghost runtime
/// source, candidate, or execution behavior can select it.  The optional
/// bootstrap RPC is a *source-provider initial-state snapshot*, not GO-E,
/// audit, historic backfill, or a source of repairs for GO-D.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PumpExactStateCaptureConfigV2 {
    pub primary_provider_id: String,
    pub grpc_endpoint: String,
    #[serde(default)]
    pub grpc_auth_token_env: Option<String>,
    #[serde(default = "default_v2_grpc_auth_header")]
    pub grpc_auth_header: String,
    pub bootstrap_rpc_endpoint: String,
    #[serde(default)]
    pub bootstrap_rpc_auth_token_env: Option<String>,
    #[serde(default = "default_v2_rpc_auth_header")]
    pub bootstrap_rpc_auth_header: String,
    #[serde(default = "default_v2_pump_program_id")]
    pub pump_program_id: String,
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
    /// Maximum duration of the prospective cohort after the finalized
    /// bootstrap/readiness boundary has been sealed.  A wall-deadline is a
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
    #[serde(default = "default_v2_bootstrap_queue_capacity")]
    pub bootstrap_queue_capacity: usize,
    #[serde(default = "default_v2_bootstrap_enqueue_timeout_ms")]
    pub bootstrap_enqueue_timeout_ms: u64,
    #[serde(default = "default_v2_stream_establish_timeout_ms")]
    pub stream_establish_timeout_ms: u64,
    #[serde(default = "default_v2_bootstrap_rpc_timeout_ms")]
    pub bootstrap_rpc_timeout_ms: u64,
    #[serde(default = "default_v2_bootstrap_response_max_bytes")]
    pub bootstrap_response_max_bytes: u64,
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
        validate_v2_endpoint("bootstrap_rpc_endpoint", &self.bootstrap_rpc_endpoint)?;
        validate_v2_trimmed("bootstrap_rpc_auth_header", &self.bootstrap_rpc_auth_header)?;
        if let Some(name) = &self.grpc_auth_token_env {
            validate_v2_trimmed("grpc_auth_token_env", name)?;
        }
        if let Some(name) = &self.bootstrap_rpc_auth_token_env {
            validate_v2_trimmed("bootstrap_rpc_auth_token_env", name)?;
        }
        if self.output_dir.as_os_str().is_empty() {
            bail!("V2 output_dir must not be empty");
        }
        if self.output_dir.components().any(|component| {
            matches!(component, std::path::Component::Normal(name) if name == "raw" || name == "raw-v2")
        }) {
            bail!(
                "V2 output_dir must be a dedicated parent, never an existing raw/raw-v2 tape directory"
            );
        }
        validate_v2_output_root_isolated(&self.output_dir)?;
        if self.source_queue_capacity == 0 || self.bootstrap_queue_capacity == 0 {
            bail!("V2 queue capacities must be greater than zero");
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
                "bootstrap_enqueue_timeout_ms",
                self.bootstrap_enqueue_timeout_ms,
            ),
            (
                "stream_establish_timeout_ms",
                self.stream_establish_timeout_ms,
            ),
            ("bootstrap_rpc_timeout_ms", self.bootstrap_rpc_timeout_ms),
            ("flush_interval_ms", self.flush_interval_ms),
            ("segment_max_duration_ms", self.segment_max_duration_ms),
        ] {
            if value == 0 {
                bail!("V2 {name} must be greater than zero");
            }
        }
        if self.bootstrap_rpc_timeout_ms > MAX_V2_BOOTSTRAP_RPC_TIMEOUT_MS {
            bail!(
                "V2 bootstrap_rpc_timeout_ms {} exceeds hard maximum {}",
                self.bootstrap_rpc_timeout_ms,
                MAX_V2_BOOTSTRAP_RPC_TIMEOUT_MS
            );
        }
        if self.bootstrap_response_max_bytes == 0
            || self.bootstrap_response_max_bytes > MAX_V2_BOOTSTRAP_RESPONSE_MAX_BYTES
        {
            bail!(
                "V2 bootstrap_response_max_bytes must be in 1..={}, got {}",
                MAX_V2_BOOTSTRAP_RESPONSE_MAX_BYTES,
                self.bootstrap_response_max_bytes
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

    fn resolve_bootstrap_rpc_auth_token(&self) -> Result<Option<String>> {
        resolve_v2_optional_env(
            "bootstrap RPC",
            self.bootstrap_rpc_auth_token_env.as_deref(),
        )
    }
}

const V2_OPERATOR_PREFLIGHT_SCHEMA_VERSION: u16 = 2;
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
}

pub fn preflight_prospective_exact_state_capture_v2_from_config_path(
    config_path: &Path,
) -> Result<PumpExactStateCapturePreflightSummaryV2> {
    let (config, config_bytes) = PumpExactStateCaptureConfigV2::load(config_path)?;
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
    })
}

/// Fresh offline build evidence retained inside one create-new preflight
/// bundle.  It intentionally claims a clean-commit locked offline build, not
/// a broader sealed-source-snapshot contract owned by the historical V1 flow.
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
/// Capture must execute that copied binary; the bootstrap executable that
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
    bootstrap_executable_digest: PumpExactStateDigestV2,
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
#[derive(Clone, Debug, PartialEq, Eq)]
struct PumpExactStateValidatedOperatorPreflightV2 {
    receipt: PumpExactStateOperatorPreflightReceiptV2,
    receipt_digest: PumpExactStateDigestV2,
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
    let repository_root = repository_root_v2()?;
    let git_status = require_clean_repository_at_v2(&repository_root)?;
    let output_filesystem_available_bytes_at_preflight = require_v2_capture_storage_budget(
        &config.output_dir,
        config.min_free_bytes,
        config.max_raw_bytes,
    )?;
    let repository_commit = repository_commit_at_v2(&repository_root)?;
    let bootstrap_executable_digest = digest_running_executable_v2()?;
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
        bootstrap_executable_digest,
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
        config.bootstrap_rpc_auth_token_env.as_deref(),
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
    })
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

const fn default_v2_bootstrap_queue_capacity() -> usize {
    DEFAULT_V2_BOOTSTRAP_QUEUE_CAPACITY
}

const fn default_v2_bootstrap_enqueue_timeout_ms() -> u64 {
    DEFAULT_V2_BOOTSTRAP_ENQUEUE_TIMEOUT_MS
}

const fn default_v2_stream_establish_timeout_ms() -> u64 {
    DEFAULT_V2_STREAM_ESTABLISH_TIMEOUT_MS
}

const fn default_v2_bootstrap_rpc_timeout_ms() -> u64 {
    DEFAULT_V2_BOOTSTRAP_RPC_TIMEOUT_MS
}

const fn default_v2_bootstrap_response_max_bytes() -> u64 {
    DEFAULT_V2_BOOTSTRAP_RESPONSE_MAX_BYTES
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

#[derive(Debug)]
struct PumpExactStateBootstrapSnapshotV2 {
    snapshot_id_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    started_wall_ts_ms: u64,
    started_monotonic_ts_ms: u64,
    finalized_context_slot: u64,
    response_bytes: Vec<u8>,
    response_sha256: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    response_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
    accounts: Vec<PumpExactStateBootstrapProgramOwnedAccountV2>,
    account_set_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1,
}

/// Bootstrap result accepted by the capture control plane.  The snapshot and
/// source readiness are paired before persistence so a later caller cannot
/// accidentally seal a finalized GPA response that predates a required
/// Yellowstone lane.
struct PumpExactStateBootstrapOverlapV2 {
    snapshot: PumpExactStateBootstrapSnapshotV2,
    source_readiness: PumpExactStateSourceReadinessV2,
    snapshot_attempt_count: u64,
}

#[derive(Clone, Debug)]
struct PumpExactStateBootstrapSealV2 {
    finalized_context_slot: u64,
    source_readiness: PumpExactStateSourceReadinessV2,
    snapshot_attempt_count: u64,
}

fn exact_state_bootstrap_http_client_v2(
    endpoint: &str,
    auth_token: Option<&str>,
    auth_header: &str,
    timeout: Duration,
) -> Result<reqwest::Client> {
    validate_v2_endpoint("bootstrap_rpc_endpoint", endpoint)?;
    let mut headers = HeaderMap::new();
    headers.insert(
        USER_AGENT,
        HeaderValue::from_static("ghost-pump-exact-state-tape-v2/1"),
    );
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    if let Some(token) = auth_token {
        let name = HeaderName::from_bytes(auth_header.as_bytes())
            .context("V2 bootstrap RPC auth header is invalid")?;
        let value = HeaderValue::from_str(token)
            .context("V2 bootstrap RPC auth token is not a valid HTTP header value")?;
        headers.insert(name, value);
    }
    reqwest::Client::builder()
        .default_headers(headers)
        .timeout(timeout)
        .pool_idle_timeout(timeout)
        .build()
        .context("build V2 standalone bootstrap RPC client")
}

/// Fetch one bounded, finalized, source-provider `getProgramAccounts` snapshot
/// while the V2 gRPC stream is already established.  The raw JSON response is
/// retained byte-for-byte in V2 bootstrap chunk records; its parsed account
/// projection is merely an indexed convenience view tied to the same digest.
async fn fetch_finalized_program_accounts_snapshot_v2(
    endpoint: &str,
    auth_token: Option<&str>,
    auth_header: &str,
    pump_program_id: Pubkey,
    timeout: Duration,
    response_max_bytes: u64,
) -> Result<PumpExactStateBootstrapSnapshotV2> {
    let started_wall_ts_ms = wall_clock_ms_v2();
    let started_monotonic_ts_ms = crate::types::arrival_time_ms();
    let request = json!({
        "jsonrpc": "2.0",
        "id": "pump_exact_state_v2_bootstrap",
        "method": "getProgramAccounts",
        "params": [
            pump_program_id.to_string(),
            {
                "commitment": "finalized",
                "encoding": "base64",
                "withContext": true
            }
        ]
    });
    let client = exact_state_bootstrap_http_client_v2(endpoint, auth_token, auth_header, timeout)?;
    let mut response = client
        .post(endpoint)
        .body(serde_json::to_vec(&request).context("encode V2 bootstrap JSON-RPC request")?)
        .send()
        .await
        .context("send V2 finalized getProgramAccounts bootstrap request")?;
    let status = response.status();
    if !status.is_success() {
        bail!("V2 finalized getProgramAccounts bootstrap returned HTTP status {status}");
    }
    if let Some(content_length) = response.content_length() {
        if content_length > response_max_bytes {
            bail!(
                "V2 bootstrap response Content-Length {} exceeds configured maximum {}",
                content_length,
                response_max_bytes
            );
        }
    }
    let mut response_bytes = Vec::new();
    while let Some(chunk) = response
        .chunk()
        .await
        .context("read V2 bootstrap response body")?
    {
        let new_len = response_bytes
            .len()
            .checked_add(chunk.len())
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap response length overflow"))?;
        if u64::try_from(new_len).unwrap_or(u64::MAX) > response_max_bytes {
            bail!(
                "V2 bootstrap response exceeded configured maximum {} bytes",
                response_max_bytes
            );
        }
        response_bytes.extend_from_slice(&chunk);
    }
    if response_bytes.is_empty() {
        bail!("V2 bootstrap response body is empty");
    }
    parse_finalized_program_accounts_snapshot_v2(
        response_bytes,
        pump_program_id,
        started_wall_ts_ms,
        started_monotonic_ts_ms,
    )
}

fn remaining_bootstrap_budget_v2(deadline: Instant) -> Result<Duration> {
    let remaining = deadline
        .checked_duration_since(Instant::now())
        .ok_or_else(|| {
            anyhow::anyhow!("V2 finalized bootstrap exhausted its configured timeout budget")
        })?;
    if remaining.is_zero() {
        bail!("V2 finalized bootstrap exhausted its configured timeout budget");
    }
    Ok(remaining)
}

/// Wait for each required Yellowstone lane, then obtain a finalized
/// getProgramAccounts snapshot whose context is no older than the latest
/// first-observed lane slot.  A stale finalized response is not a usable
/// baseline: retry only within the already hash-pinned bootstrap timeout.
async fn fetch_bootstrap_with_source_overlap_v2(
    coordinator: &PumpExactStateCaptureCoordinatorV2,
    endpoint: &str,
    auth_token: Option<&str>,
    auth_header: &str,
    pump_program_id: Pubkey,
    timeout: Duration,
    response_max_bytes: u64,
) -> Result<PumpExactStateBootstrapOverlapV2> {
    let deadline = Instant::now()
        .checked_add(timeout)
        .ok_or_else(|| anyhow::anyhow!("V2 bootstrap deadline overflow"))?;
    let source_readiness = coordinator
        .wait_for_required_source_lanes(remaining_bootstrap_budget_v2(deadline)?)
        .await?;
    let mut snapshot_attempt_count = 0u64;
    loop {
        let request_timeout = remaining_bootstrap_budget_v2(deadline)?;
        snapshot_attempt_count = snapshot_attempt_count
            .checked_add(1)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap snapshot attempt counter overflow"))?;
        let snapshot = fetch_finalized_program_accounts_snapshot_v2(
            endpoint,
            auth_token,
            auth_header,
            pump_program_id,
            request_timeout,
            response_max_bytes,
        )
        .await?;
        if snapshot.finalized_context_slot >= source_readiness.source_readiness_slot {
            return Ok(PumpExactStateBootstrapOverlapV2 {
                snapshot,
                source_readiness: source_readiness.clone(),
                snapshot_attempt_count,
            });
        }

        let remaining = remaining_bootstrap_budget_v2(deadline)?;
        // A finalized RPC can lag the established processed stream.  Do not
        // reinterpret its older snapshot as an overlap: wait briefly, bounded
        // by the original bootstrap deadline, then fetch a new finalized
        // response from the same source provider.
        tokio::time::sleep(remaining.min(Duration::from_millis(250))).await;
    }
}

fn parse_finalized_program_accounts_snapshot_v2(
    response_bytes: Vec<u8>,
    pump_program_id: Pubkey,
    started_wall_ts_ms: u64,
    started_monotonic_ts_ms: u64,
) -> Result<PumpExactStateBootstrapSnapshotV2> {
    let response: Value =
        serde_json::from_slice(&response_bytes).context("parse V2 bootstrap JSON-RPC response")?;
    if response.get("jsonrpc").and_then(Value::as_str) != Some("2.0") {
        bail!("V2 bootstrap response has no JSON-RPC 2.0 marker");
    }
    if response.get("id").and_then(Value::as_str) != Some("pump_exact_state_v2_bootstrap") {
        bail!("V2 bootstrap response JSON-RPC id is not the requested bootstrap id");
    }
    if let Some(error) = response.get("error") {
        bail!("V2 bootstrap JSON-RPC error: {error}");
    }
    let result = response
        .get("result")
        .ok_or_else(|| anyhow::anyhow!("V2 bootstrap response has no result"))?;
    let finalized_context_slot = result
        .get("context")
        .and_then(|context| context.get("slot"))
        .and_then(Value::as_u64)
        .ok_or_else(|| anyhow::anyhow!("V2 bootstrap response has no finalized context.slot"))?;
    let values = result
        .get("value")
        .and_then(Value::as_array)
        .ok_or_else(|| {
            anyhow::anyhow!("V2 bootstrap response result.value is not an account array")
        })?;
    if values.is_empty() {
        bail!("V2 bootstrap response contains no Pump-program-owned accounts");
    }
    let response_blake3 = hash_bytes_v2(&response_bytes);
    let response_sha256 = sha256_storage_hash_v2(&response_bytes);
    let mut accounts = Vec::with_capacity(values.len());
    let mut account_set_hasher = blake3::Hasher::new();
    for (index, value) in values.iter().enumerate() {
        let pubkey = value
            .get("pubkey")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks pubkey"))?
            .parse::<Pubkey>()
            .with_context(|| format!("V2 bootstrap account {index} has invalid pubkey"))?;
        let account = value
            .get("account")
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks account object"))?;
        let owner = account
            .get("owner")
            .and_then(Value::as_str)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks owner"))?
            .parse::<Pubkey>()
            .with_context(|| format!("V2 bootstrap account {index} has invalid owner"))?;
        if owner != pump_program_id {
            bail!(
                "V2 bootstrap account {} owner {} differs from Pump program {}",
                pubkey,
                owner,
                pump_program_id
            );
        }
        let lamports = account
            .get("lamports")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks lamports"))?;
        let executable = account
            .get("executable")
            .and_then(Value::as_bool)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks executable"))?;
        let rent_epoch = account
            .get("rentEpoch")
            .and_then(Value::as_u64)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} lacks rentEpoch"))?;
        let data = account
            .get("data")
            .and_then(Value::as_array)
            .ok_or_else(|| anyhow::anyhow!("V2 bootstrap account {index} data is not an array"))?;
        if data.len() != 2 || data[1].as_str() != Some("base64") {
            bail!(
                "V2 bootstrap account {index} data must be exactly [base64, base64] without compression"
            );
        }
        let encoded = data[0].as_str().ok_or_else(|| {
            anyhow::anyhow!("V2 bootstrap account {index} base64 data is not a string")
        })?;
        let raw_account_data = BASE64_STANDARD
            .decode(encoded)
            .with_context(|| format!("decode V2 bootstrap account {index} base64 data"))?;
        let response_account_index = u64::try_from(index)
            .map_err(|_| anyhow::anyhow!("V2 bootstrap account index overflow"))?;
        let raw_account_data_hash_blake3 = hash_bytes_v2(&raw_account_data);
        account_set_hasher.update(&response_account_index.to_le_bytes());
        account_set_hasher.update(&pubkey.to_bytes());
        account_set_hasher.update(&owner.to_bytes());
        account_set_hasher.update(&lamports.to_le_bytes());
        account_set_hasher.update(&[u8::from(executable)]);
        account_set_hasher.update(&rent_epoch.to_le_bytes());
        account_set_hasher.update(&raw_account_data_hash_blake3.into_inner());
        account_set_hasher.update(
            &u64::try_from(raw_account_data.len())
                .unwrap_or(u64::MAX)
                .to_le_bytes(),
        );
        accounts.push(PumpExactStateBootstrapProgramOwnedAccountV2 {
            snapshot_id_blake3: response_blake3,
            response_account_index,
            account_pubkey: PumpResearchStoragePubkeyV1::from(pubkey.to_bytes()),
            owner_program: PumpResearchStoragePubkeyV1::from(owner.to_bytes()),
            lamports,
            executable,
            rent_epoch,
            raw_account_data,
            raw_account_data_hash_blake3,
        });
    }
    Ok(PumpExactStateBootstrapSnapshotV2 {
        snapshot_id_blake3: response_blake3,
        started_wall_ts_ms,
        started_monotonic_ts_ms,
        finalized_context_slot,
        response_bytes,
        response_sha256,
        response_blake3,
        accounts,
        account_set_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
            *account_set_hasher.finalize().as_bytes(),
        ),
    })
}

fn persist_bootstrap_snapshot_v2(
    coordinator: &PumpExactStateCaptureCoordinatorV2,
    provider_id: &str,
    pump_program_id: Pubkey,
    source_stream_epoch: u64,
    bootstrap: PumpExactStateBootstrapOverlapV2,
    enqueue_timeout: Duration,
) -> Result<PumpExactStateBootstrapSealV2> {
    if source_stream_epoch == 0 {
        bail!("V2 bootstrap cannot be written without an established source stream epoch");
    }
    let PumpExactStateBootstrapOverlapV2 {
        snapshot,
        source_readiness,
        snapshot_attempt_count,
    } = bootstrap;
    validate_source_readiness_v2(&source_readiness)?;
    if snapshot.finalized_context_slot < source_readiness.source_readiness_slot {
        bail!(
            "V2 finalized bootstrap context slot {} predates required source readiness slot {}",
            snapshot.finalized_context_slot,
            source_readiness.source_readiness_slot,
        );
    }
    let response_chunk_count = u64::try_from(
        snapshot
            .response_bytes
            .chunks(
                ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_BOOTSTRAP_RESPONSE_CHUNK_BYTES_V2,
            )
            .len(),
    )
    .map_err(|_| anyhow::anyhow!("V2 bootstrap response chunk count overflow"))?;
    let source_capture_sequence_exclusive = coordinator.next_capture_sequence_exclusive();
    coordinator.enqueue_bootstrap_record(
        source_stream_epoch,
        PumpExactStateRawRecordV2::BootstrapSnapshotStarted(
            PumpExactStateBootstrapSnapshotStartedV2 {
                snapshot_id_blake3: snapshot.snapshot_id_blake3,
                provider_id: provider_id.to_owned(),
                pump_program_id: PumpResearchStoragePubkeyV1::from(pump_program_id.to_bytes()),
                commitment: PumpExactStateBootstrapCommitmentV2::Finalized,
                source_stream_epoch_at_start: source_stream_epoch,
                started_wall_ts_ms: snapshot.started_wall_ts_ms,
                started_monotonic_ts_ms: snapshot.started_monotonic_ts_ms,
            },
        ),
        enqueue_timeout,
    )?;
    for (index, bytes) in snapshot
        .response_bytes
        .chunks(
            ghost_core::pump_research_exact_tape_v2::PUMP_EXACT_STATE_TAPE_BOOTSTRAP_RESPONSE_CHUNK_BYTES_V2,
        )
        .enumerate()
    {
        coordinator.enqueue_bootstrap_record(
            source_stream_epoch,
            PumpExactStateRawRecordV2::BootstrapResponseChunk(
                PumpExactStateBootstrapResponseChunkV2 {
                    snapshot_id_blake3: snapshot.snapshot_id_blake3,
                    chunk_index: u64::try_from(index)
                        .map_err(|_| anyhow::anyhow!("V2 bootstrap chunk index overflow"))?,
                    bytes: bytes.to_vec(),
                },
            ),
            enqueue_timeout,
        )?;
    }
    let account_count = u64::try_from(snapshot.accounts.len())
        .map_err(|_| anyhow::anyhow!("V2 bootstrap account count overflow"))?;
    for account in snapshot.accounts {
        coordinator.enqueue_bootstrap_record(
            source_stream_epoch,
            PumpExactStateRawRecordV2::BootstrapProgramOwnedAccount(account),
            enqueue_timeout,
        )?;
    }
    coordinator.enqueue_bootstrap_record(
        source_stream_epoch,
        PumpExactStateRawRecordV2::BootstrapSnapshotCompleted(
            PumpExactStateBootstrapSnapshotCompletedV2 {
                snapshot_id_blake3: snapshot.snapshot_id_blake3,
                finalized_context_slot: snapshot.finalized_context_slot,
                response_sha256: snapshot.response_sha256,
                response_blake3: snapshot.response_blake3,
                response_bytes: u64::try_from(snapshot.response_bytes.len()).unwrap_or(u64::MAX),
                response_chunk_count,
                account_count,
                account_set_blake3: snapshot.account_set_blake3,
                source_stream_epoch_at_completion: source_stream_epoch,
                completed_wall_ts_ms: wall_clock_ms_v2(),
                completed_monotonic_ts_ms: crate::types::arrival_time_ms(),
            },
        ),
        enqueue_timeout,
    )?;
    coordinator.enqueue_bootstrap_record(
        source_stream_epoch,
        PumpExactStateRawRecordV2::ProspectiveReadinessBoundary(
            PumpExactStateProspectiveReadinessBoundaryV2 {
                snapshot_id_blake3: snapshot.snapshot_id_blake3,
                finalized_context_slot: snapshot.finalized_context_slot,
                source_readiness: source_readiness.clone(),
                finalized_context_slot_covers_source_readiness: true,
                source_stream_epoch,
                source_capture_sequence_exclusive,
                cohort_slots_strictly_after: snapshot.finalized_context_slot,
                sealed_wall_ts_ms: wall_clock_ms_v2(),
                sealed_monotonic_ts_ms: crate::types::arrival_time_ms(),
            },
        ),
        enqueue_timeout,
    )?;
    coordinator.seal_bootstrap_complete()?;
    Ok(PumpExactStateBootstrapSealV2 {
        finalized_context_slot: snapshot.finalized_context_slot,
        source_readiness,
        snapshot_attempt_count,
    })
}

/// Defensive control-plane validation for the source-readiness payload before
/// it becomes frozen raw evidence.  The normal ingress path constructs this
/// value from atomics, but treating the derived maximum as a checked invariant
/// here prevents any future caller from lowering the declared readiness slot
/// and accepting an older finalized bootstrap snapshot.
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
struct PumpExactStateRunStartManifestV2 {
    storage_format_version: u16,
    schema_version: u16,
    capture_config_schema_version: u16,
    run_id: String,
    repository_commit: String,
    running_executable_digest: PumpExactStateDigestV2,
    operator_preflight_receipt_digest: PumpExactStateDigestV2,
    sealed_release_binary_digest: PumpExactStateDigestV2,
    sealed_fresh_build_receipt_digest: PumpExactStateDigestV2,
    sealed_build_semantics: String,
    capture_config_digest: PumpExactStateDigestV2,
    capture_contract_sha256: String,
    source_request_fingerprint_blake3: String,
    source_capture_semantics: String,
    source_max_decoded_message_bytes: u64,
    primary_provider_id: String,
    grpc_endpoint_digest: PumpExactStateDigestV2,
    bootstrap_rpc_endpoint_digest: PumpExactStateDigestV2,
    bootstrap_rpc_auth_mode: String,
    pump_program_id: String,
    program_data_at_start: PumpProgramDataReceiptV1,
    cohort_capture_wall_ms: u64,
    min_free_bytes: u64,
    max_raw_bytes: u64,
    required_storage_bytes: u64,
    output_filesystem_available_bytes_at_start: u64,
    capture_started_wall_ms: u64,
    capture_started_monotonic_ms: u64,
    required_for_run: bool,
}

/// An explicit normal boundary for the raw prospective cohort.  A raw V2 run
/// may finish only after the operator requested shutdown or the hash-pinned
/// cohort wall deadline elapsed.  A source task that simply disappears never
/// acquires one of these normal termination values.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PumpExactStateCaptureTerminationV2 {
    OperatorSignal,
    CohortWallDeadline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpExactStateRunCompletionReceiptV2 {
    storage_format_version: u16,
    schema_version: u16,
    run_id: String,
    status: PumpExactStateCaptureRunStatusV2,
    clean_shutdown: bool,
    bootstrap_finalized_context_slot: Option<u64>,
    bootstrap_source_readiness: Option<PumpExactStateSourceReadinessV2>,
    bootstrap_source_overlap_proven: bool,
    bootstrap_snapshot_attempt_count: u64,
    bootstrap_completed: bool,
    running_executable_at_completion: Option<PumpExactStateDigestV2>,
    running_executable_unchanged: bool,
    program_data_at_start: PumpProgramDataReceiptV1,
    program_data_at_completion: Option<PumpProgramDataReceiptV1>,
    program_data_unchanged: bool,
    cohort_capture_termination: Option<PumpExactStateCaptureTerminationV2>,
    cohort_capture_elapsed_ms: Option<u64>,
    min_free_bytes: u64,
    max_raw_bytes: u64,
    output_filesystem_available_bytes_at_completion: Option<u64>,
    storage_reserve_maintained: bool,
    raw_byte_budget_respected: bool,
    required_source_lanes_observed: bool,
    source_lifecycle: PumpExactStateCaptureSourceLifecycleV2,
    writer: PumpExactStateWriterSummaryV2,
    segment_list: Vec<PumpExactStateSegmentReceiptV2>,
    completion_wall_ms: u64,
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

fn program_data_receipts_match_v2(
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
/// -> established all-Pump-owned gRPC stream
/// -> bounded finalized getProgramAccounts bootstrap
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
    let bootstrap_rpc_auth_token = config.resolve_bootstrap_rpc_auth_token()?;
    let program_data_at_start = observe_program_data_receipt(
        &config.bootstrap_rpc_endpoint,
        bootstrap_rpc_auth_token.as_deref(),
        &config.bootstrap_rpc_auth_header,
        pump_program_id,
    )
    .await
    .context("V2 requires finalized Pump ProgramData before opening its source stream")?;
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
        primary_provider_id: config.primary_provider_id.clone(),
        grpc_endpoint_digest: digest_bytes_v2(config.grpc_endpoint.as_bytes()),
        bootstrap_rpc_endpoint_digest: digest_bytes_v2(config.bootstrap_rpc_endpoint.as_bytes()),
        bootstrap_rpc_auth_mode: if config.bootstrap_rpc_auth_token_env.is_some() {
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
        config.bootstrap_queue_capacity,
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
            coordinator.fail_bootstrap();
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
    let bootstrap_result = async {
        let start_epoch = coordinator
            .wait_for_stream_established(Duration::from_millis(
                config.stream_establish_timeout_ms,
            ))
            .await?;
        let bootstrap = fetch_bootstrap_with_source_overlap_v2(
            &coordinator,
            &config.bootstrap_rpc_endpoint,
            bootstrap_rpc_auth_token.as_deref(),
            &config.bootstrap_rpc_auth_header,
            pump_program_id,
            Duration::from_millis(config.bootstrap_rpc_timeout_ms),
            config.bootstrap_response_max_bytes,
        )
        .await?;
        let completion_epoch = coordinator.established_stream_epoch().ok_or_else(|| {
            anyhow::anyhow!("V2 source stream disappeared while finalized bootstrap was in flight")
        })?;
        if completion_epoch != start_epoch {
            bail!(
                "V2 source stream reconnected from epoch {} to {} while finalized bootstrap was in flight",
                start_epoch,
                completion_epoch
            );
        }
        persist_bootstrap_snapshot_v2(
            &coordinator,
            &config.primary_provider_id,
            pump_program_id,
            completion_epoch,
            bootstrap,
            Duration::from_millis(config.bootstrap_enqueue_timeout_ms),
        )
    }
    .await;

    let mut cohort_capture_termination = None;
    let mut cohort_capture_elapsed_ms = None;
    let (source_result, bootstrap_seal) = match bootstrap_result {
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
            coordinator.fail_bootstrap();
            source.request_shutdown();
            let task_result = source_task
                .await
                .map_err(|join| anyhow::anyhow!("V2 source task join failure: {join}"))
                .and_then(|result| result);
            let message = match task_result {
                Ok(()) => format!("V2 bootstrap failed: {error:#}"),
                Err(source_error) => {
                    format!("V2 bootstrap failed: {error:#}; source also ended: {source_error:#}")
                }
            };
            (Err(anyhow::anyhow!(message)), None)
        }
    };
    coordinator.finish_source();
    let writer = coordinator.finish_and_join();
    let source_lifecycle = coordinator.source_lifecycle();
    let program_data_completion_result = observe_program_data_receipt(
        &config.bootstrap_rpc_endpoint,
        bootstrap_rpc_auth_token.as_deref(),
        &config.bootstrap_rpc_auth_header,
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
    let bootstrap_context_slot = bootstrap_seal
        .as_ref()
        .map(|seal| seal.finalized_context_slot);
    let bootstrap_source_readiness = bootstrap_seal
        .as_ref()
        .map(|seal| seal.source_readiness.clone());
    let bootstrap_source_overlap_proven = bootstrap_seal.as_ref().is_some_and(|seal| {
        seal.finalized_context_slot >= seal.source_readiness.source_readiness_slot
    });
    let bootstrap_snapshot_attempt_count = bootstrap_seal
        .as_ref()
        .map_or(0, |seal| seal.snapshot_attempt_count);
    let bootstrap_completed = source_lifecycle.bootstrap_status == "complete";
    let clean_shutdown = source_result.is_ok()
        && writer.clean_shutdown
        && source_lifecycle.stream_established
        && source_lifecycle.source_workers_cleanly_stopped
        && source_lifecycle.dropped_source_updates == 0
        && source_lifecycle.source_queue_bytes_at_close == 0
        && source_lifecycle.fatal_capture_error.is_none()
        && source_lifecycle.source_worker_error.is_none()
        && bootstrap_completed
        && bootstrap_context_slot.is_some()
        && bootstrap_source_overlap_proven
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
        bootstrap_finalized_context_slot: bootstrap_context_slot,
        bootstrap_source_readiness,
        bootstrap_source_overlap_proven,
        bootstrap_snapshot_attempt_count,
        bootstrap_completed,
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
        bootstrap_status: "failed".to_owned(),
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
        bootstrap_finalized_context_slot: None,
        bootstrap_source_readiness: None,
        bootstrap_source_overlap_proven: false,
        bootstrap_snapshot_attempt_count: 0,
        bootstrap_completed: false,
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
                    "V2 all-Pump-owned account source emitted account {} with non-Pump owner {}",
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
            let canonical_global = Pubkey::from_str(PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1)
                .context("canonical Pump Global pubkey is invalid")?;
            let evidence_class = if account_pubkey.into_inner() == canonical_global.to_bytes() {
                PumpExactStateAccountEvidenceClassV2::CanonicalGlobal
            } else if account.data.starts_with(&BONDING_CURVE_DISC) {
                PumpExactStateAccountEvidenceClassV2::CanonicalBondingCurve
            } else {
                PumpExactStateAccountEvidenceClassV2::OtherPumpOwned
            };
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
/// and all data must first satisfy `raw_records_from_source_v2` or a dedicated
/// bootstrap record contract.
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

    /// Bootstrap records are written through the same segment chain but have
    /// no gRPC capture sequence.  The caller must supply the established
    /// source epoch so a bootstrap cannot be silently blended across a
    /// reconnect boundary.
    fn write_bootstrap_record(
        &mut self,
        stream_epoch: u64,
        record: PumpExactStateRawRecordV2,
    ) -> Result<()> {
        if source_stream_epoch_v2(&record).is_some() {
            bail!("V2 bootstrap writer received a source record");
        }
        self.write_record(stream_epoch, None, &record)
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
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
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

fn source_stream_epoch_v2(record: &PumpExactStateRawRecordV2) -> Option<u64> {
    match record {
        PumpExactStateRawRecordV2::PrimaryTransaction(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PrimarySlotUpdate(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::PrimaryBlockMeta(value) => Some(value.source.stream_epoch),
        PumpExactStateRawRecordV2::FullBlockPayloadStarted(value) => {
            Some(value.source.stream_epoch)
        }
        PumpExactStateRawRecordV2::CoverageGap(value) => Some(value.stream_epoch),
        PumpExactStateRawRecordV2::FullBlockPayloadChunk(_)
        | PumpExactStateRawRecordV2::FullBlockPayloadCompleted(_)
        | PumpExactStateRawRecordV2::BootstrapSnapshotStarted(_)
        | PumpExactStateRawRecordV2::BootstrapResponseChunk(_)
        | PumpExactStateRawRecordV2::BootstrapProgramOwnedAccount(_)
        | PumpExactStateRawRecordV2::BootstrapSnapshotCompleted(_)
        | PumpExactStateRawRecordV2::ProspectiveReadinessBoundary(_)
        | PumpExactStateRawRecordV2::SegmentClosed(_) => None,
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
    BootstrapQueueDisconnected = 4,
    BootstrapQueueTimeout = 5,
    WriterFailure = 6,
    WriterPanic = 7,
    WriterJoinPanic = 8,
    BootstrapNotSealed = 9,
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
            4 => Self::BootstrapQueueDisconnected,
            5 => Self::BootstrapQueueTimeout,
            6 => Self::WriterFailure,
            7 => Self::WriterPanic,
            8 => Self::WriterJoinPanic,
            9 => Self::BootstrapNotSealed,
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
            Self::BootstrapQueueDisconnected => {
                Some("V2 bootstrap queue disconnected before snapshot evidence was persisted")
            }
            Self::BootstrapQueueTimeout => {
                Some("V2 bootstrap queue did not drain within its bounded enqueue deadline")
            }
            Self::WriterFailure => Some("V2 raw writer failed"),
            Self::WriterPanic => Some("V2 raw writer thread panicked"),
            Self::WriterJoinPanic => {
                Some("V2 raw writer thread panicked before reporting a failure")
            }
            Self::BootstrapNotSealed => {
                Some("V2 capture ended without one complete finalized bootstrap snapshot")
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
enum PumpExactStateBootstrapStatusV2 {
    Pending = 0,
    Complete = 1,
    Failed = 2,
}

impl PumpExactStateBootstrapStatusV2 {
    fn from_raw(raw: u8) -> Self {
        match raw {
            0 => Self::Pending,
            1 => Self::Complete,
            2 => Self::Failed,
            _ => Self::Failed,
        }
    }
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateCaptureSourceLifecycleV2 {
    pub stream_established: bool,
    pub established_stream_epoch: Option<u64>,
    pub source_updates_received: u64,
    pub admitted_source_updates: u64,
    pub dropped_source_updates: u64,
    pub source_queue_peak_bytes: u64,
    pub source_queue_bytes_at_close: u64,
    pub source_workers_cleanly_stopped: bool,
    pub required_lane_first_slots: Option<PumpExactStateSourceReadinessV2>,
    pub bootstrap_status: String,
    pub fatal_capture_error: Option<String>,
    pub source_worker_error: Option<String>,
}

/// Persisted source-lane census.  This is writer-owned: a lane is counted
/// only after its decoded `SubscribeUpdate` has been converted and durably
/// written into the V2 segment chain.  It is therefore independent from the
/// ingress admission counters used to establish the bootstrap overlap.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpExactStateRequiredLaneCensusV2 {
    pub transaction_messages: u64,
    pub account_updates: u64,
    pub slot_updates: u64,
    pub block_meta_updates: u64,
    pub full_blocks_started: u64,
    pub full_block_chunks: u64,
    pub full_blocks_completed: u64,
    pub incomplete_full_block_payloads: u64,
    pub unbound_full_block_chunks: u64,
    pub full_block_payloads_reconciled: bool,
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
    pub segments: Vec<PumpExactStateSegmentReceiptV2>,
    pub raw_bytes_written: u64,
    pub accepted_source_records: u64,
    pub accepted_bootstrap_records: u64,
    pub required_lane_census: PumpExactStateRequiredLaneCensusV2,
    pub persisted_ingress_gap_missing_events: u64,
    pub persisted_ingress_gap_episodes: u64,
    pub gap_count: u64,
    pub clean_shutdown: bool,
    pub error: Option<String>,
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

struct QueuedBootstrapRecordV2 {
    stream_epoch: u64,
    record: PumpExactStateRawRecordV2,
}

enum OrderedIngressEventV2 {
    Source(QueuedSourceUpdateV2),
    Dropped(DroppedSourceUpdateV2),
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
        bootstrap_status: PumpExactStateBootstrapStatusV2,
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
            bootstrap_status: match bootstrap_status {
                PumpExactStateBootstrapStatusV2::Pending => "pending",
                PumpExactStateBootstrapStatusV2::Complete => "complete",
                PumpExactStateBootstrapStatusV2::Failed => "failed",
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
        self.final_capture_sequence.store(
            self.next_capture_sequence.load(Ordering::Acquire),
            Ordering::Release,
        );
        self.source_finished.store(true, Ordering::Release);
    }

    fn next_capture_sequence_exclusive(&self) -> u64 {
        self.next_capture_sequence.load(Ordering::Acquire)
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

    async fn wait_for_stream_established(&self, timeout: Duration) -> Result<u64> {
        let deadline = tokio::time::Instant::now() + timeout;
        loop {
            let epoch = self.established_stream_epoch.load(Ordering::Acquire);
            if self.stream_established.load(Ordering::Acquire) && epoch != 0 {
                return Ok(epoch);
            }
            tokio::time::timeout_at(deadline, self.stream_ready.notified())
                .await
                .map_err(|_| {
                    anyhow::anyhow!("V2 source stream did not establish before bootstrap deadline")
                })?;
        }
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
                        "V2 source did not admit transaction/account/slot/block-meta/full-block evidence before bootstrap deadline"
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
        let capture_sequence = self.next_capture_sequence.fetch_add(1, Ordering::AcqRel);
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

/// V2 writer/coordinator.  The bootstrap has a separate bounded queue because
/// a large initial snapshot must never block the Yellowstone receive task.
/// A bootstrap producer may wait on its own queue outside the hot path; if the
/// source queue overflows while that happens, the run records a gap and fails
/// closed rather than silently dropping source data.
pub(crate) struct PumpExactStateCaptureCoordinatorV2 {
    ingress: Arc<PumpExactStateCaptureIngressV2>,
    bootstrap_tx: crossbeam_channel::Sender<QueuedBootstrapRecordV2>,
    bootstrap_status: Arc<AtomicU8>,
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
        bootstrap_queue_capacity: usize,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
        max_raw_bytes: u64,
        min_free_bytes: u64,
    ) -> Result<Self> {
        if queue_capacity == 0 || bootstrap_queue_capacity == 0 || source_queue_max_bytes == 0 {
            bail!("V2 capture queue capacities and source byte capacity must be greater than zero");
        }
        if max_raw_bytes == 0 || min_free_bytes == 0 {
            bail!("V2 capture raw byte budget and storage reserve must be greater than zero");
        }
        let (data_tx, data_rx) = crossbeam_channel::bounded(queue_capacity);
        let (control_tx, control_rx) = crossbeam_channel::bounded(queue_capacity);
        let (bootstrap_tx, bootstrap_rx) = crossbeam_channel::bounded(bootstrap_queue_capacity);
        let capture_abort = CancellationToken::new();
        let ingress = Arc::new(PumpExactStateCaptureIngressV2::new(
            data_tx,
            control_tx,
            queue_capacity,
            source_queue_max_bytes,
            capture_abort,
        ));
        let progress = Arc::new(Mutex::new(PumpExactStateWriterSummaryV2::default()));
        let bootstrap_status = Arc::new(AtomicU8::new(
            PumpExactStateBootstrapStatusV2::Pending as u8,
        ));
        let writer_ingress = Arc::clone(&ingress);
        let writer_progress = Arc::clone(&progress);
        let writer_bootstrap_status = Arc::clone(&bootstrap_status);
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
                        bootstrap_rx,
                        Arc::clone(&writer_ingress),
                        Arc::clone(&writer_progress),
                        Arc::clone(&writer_bootstrap_status),
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
            bootstrap_tx,
            bootstrap_status,
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

    pub(crate) fn next_capture_sequence_exclusive(&self) -> u64 {
        self.ingress.next_capture_sequence_exclusive()
    }

    pub(crate) async fn wait_for_stream_established(&self, timeout: Duration) -> Result<u64> {
        self.ingress.wait_for_stream_established(timeout).await
    }

    pub(crate) async fn wait_for_required_source_lanes(
        &self,
        timeout: Duration,
    ) -> Result<PumpExactStateSourceReadinessV2> {
        self.ingress.wait_for_required_source_lanes(timeout).await
    }

    /// Queue one source-bootstrap record from the capture control plane.  This
    /// method is forbidden after the snapshot has been sealed and uses a
    /// bounded wait; it is never invoked by a Yellowstone receive task.
    pub(crate) fn enqueue_bootstrap_record(
        &self,
        stream_epoch: u64,
        record: PumpExactStateRawRecordV2,
        timeout: Duration,
    ) -> Result<()> {
        if !matches!(
            PumpExactStateBootstrapStatusV2::from_raw(
                self.bootstrap_status.load(Ordering::Acquire)
            ),
            PumpExactStateBootstrapStatusV2::Pending
        ) {
            bail!("V2 bootstrap records cannot be queued after the snapshot is sealed");
        }
        self.bootstrap_tx
            .send_timeout(
                QueuedBootstrapRecordV2 {
                    stream_epoch,
                    record,
                },
                timeout,
            )
            .map_err(|error| {
                let reason = match error {
                    crossbeam_channel::SendTimeoutError::Timeout(_) => {
                        PumpExactStateCaptureFatalReasonV2::BootstrapQueueTimeout
                    }
                    crossbeam_channel::SendTimeoutError::Disconnected(_) => {
                        PumpExactStateCaptureFatalReasonV2::BootstrapQueueDisconnected
                    }
                };
                self.ingress.record_fatal_capture_error(reason);
                self.ingress.cancel_source_from_writer_if_fatal();
                anyhow::anyhow!("V2 bootstrap record enqueue failed: {reason:?}")
            })
    }

    /// Seal a complete bootstrap only after its terminal receipt record was
    /// enqueued.  The writer drains the already queued records before it can
    /// declare a clean run.
    pub(crate) fn seal_bootstrap_complete(&self) -> Result<()> {
        self.bootstrap_status
            .compare_exchange(
                PumpExactStateBootstrapStatusV2::Pending as u8,
                PumpExactStateBootstrapStatusV2::Complete as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .map_err(|_| anyhow::anyhow!("V2 bootstrap cannot transition to complete"))?;
        Ok(())
    }

    pub(crate) fn fail_bootstrap(&self) {
        let _ = self.bootstrap_status.compare_exchange(
            PumpExactStateBootstrapStatusV2::Pending as u8,
            PumpExactStateBootstrapStatusV2::Failed as u8,
            Ordering::AcqRel,
            Ordering::Acquire,
        );
        self.ingress
            .record_fatal_capture_error(PumpExactStateCaptureFatalReasonV2::BootstrapNotSealed);
        self.ingress.cancel_source_from_writer_if_fatal();
    }

    pub(crate) fn finish_source(&self) {
        self.ingress.finish();
    }

    pub(crate) fn source_lifecycle(&self) -> PumpExactStateCaptureSourceLifecycleV2 {
        self.ingress
            .lifecycle(PumpExactStateBootstrapStatusV2::from_raw(
                self.bootstrap_status.load(Ordering::Acquire),
            ))
    }

    #[must_use]
    pub(crate) fn finish_and_join(&self) -> PumpExactStateWriterSummaryV2 {
        if matches!(
            PumpExactStateBootstrapStatusV2::from_raw(
                self.bootstrap_status.load(Ordering::Acquire)
            ),
            PumpExactStateBootstrapStatusV2::Pending
        ) {
            self.fail_bootstrap();
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
    bootstrap_rx: crossbeam_channel::Receiver<QueuedBootstrapRecordV2>,
    ingress: Arc<PumpExactStateCaptureIngressV2>,
    progress: Arc<Mutex<PumpExactStateWriterSummaryV2>>,
    bootstrap_status: Arc<AtomicU8>,
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
            process_ordered_ingress_event_v2(
                event,
                &mut writer,
                &mut local_gap_tracker,
                &progress,
                &ingress,
            )?;
            next_capture_sequence = next_capture_sequence
                .checked_add(1)
                .ok_or_else(|| anyhow::anyhow!("V2 capture sequence overflow"))?;
            made_progress = true;
        }
        // Admit at most one bootstrap item per loop after source sequencing,
        // so a large snapshot cannot starve the primary source evidence path.
        if let Ok(bootstrap) = bootstrap_rx.try_recv() {
            writer.write_bootstrap_record(bootstrap.stream_epoch, bootstrap.record)?;
            let mut summary = progress.lock();
            summary.raw_bytes_written = writer.raw_bytes_written();
            summary.accepted_bootstrap_records =
                summary.accepted_bootstrap_records.saturating_add(1);
            made_progress = true;
        }

        if ingress.source_finished.load(Ordering::Acquire) {
            let status =
                PumpExactStateBootstrapStatusV2::from_raw(bootstrap_status.load(Ordering::Acquire));
            if matches!(status, PumpExactStateBootstrapStatusV2::Failed) {
                bail!("V2 bootstrap failed before source closure");
            }
            if matches!(status, PumpExactStateBootstrapStatusV2::Complete)
                && next_capture_sequence == ingress.final_capture_sequence.load(Ordering::Acquire)
                && pending.is_empty()
                && data_rx.is_empty()
                && control_rx.is_empty()
                && bootstrap_rx.is_empty()
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
                    PumpExactStateBootstrapStatusV2::Pending => {
                        // The coordinator will mark the bootstrap failed or
                        // complete before joining.  Do not spin: wait below.
                    }
                    PumpExactStateBootstrapStatusV2::Complete if bootstrap_rx.is_empty() => {
                        bail!(
                            "V2 source finished at capture sequence {} but writer did not reach sequence {}",
                            ingress.final_capture_sequence.load(Ordering::Acquire),
                            next_capture_sequence
                        );
                    }
                    PumpExactStateBootstrapStatusV2::Complete
                    | PumpExactStateBootstrapStatusV2::Failed => {}
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
                recv(bootstrap_rx) -> bootstrap => match bootstrap {
                    Ok(bootstrap) => {
                        writer.write_bootstrap_record(bootstrap.stream_epoch, bootstrap.record)?;
                        let mut summary = progress.lock();
                        summary.raw_bytes_written = writer.raw_bytes_written();
                        summary.accepted_bootstrap_records = summary.accepted_bootstrap_records.saturating_add(1);
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
/// bootstrap readiness/census gates; an unexpected message shape is rejected
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
    use std::fs;
    use tempfile::tempdir;
    use yellowstone_grpc_proto::prelude::{
        SubscribeUpdate, SubscribeUpdateAccount, SubscribeUpdateAccountInfo, SubscribeUpdateBlock,
        SubscribeUpdateBlockMeta, SubscribeUpdateSlot, SubscribeUpdateTransaction,
        SubscribeUpdateTransactionInfo,
    };

    const TEST_V2_MAX_RAW_BYTES: u64 = 64 * 1024 * 1024;
    const TEST_V2_MIN_FREE_BYTES: u64 = 1;

    fn source_update(account: SubscribeUpdateAccount) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch: 4,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_exact_state_v2_all_pump_owned".to_owned()],
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

    fn bootstrap_snapshot_for_test(
        finalized_context_slot: u64,
    ) -> PumpExactStateBootstrapSnapshotV2 {
        let response_bytes = b"{\"jsonrpc\":\"2.0\"}".to_vec();
        let response_blake3 = hash_bytes_v2(&response_bytes);
        PumpExactStateBootstrapSnapshotV2 {
            snapshot_id_blake3: response_blake3,
            started_wall_ts_ms: 10,
            started_monotonic_ts_ms: 11,
            finalized_context_slot,
            response_sha256: sha256_storage_hash_v2(&response_bytes),
            response_blake3,
            response_bytes,
            accounts: Vec::new(),
            account_set_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from(
                [7; 32],
            ),
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

    #[test]
    fn v2_retains_unknown_pump_owned_account_as_raw_other_evidence() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let account = pump_owned_account(Pubkey::new_unique(), pump, vec![0xde, 0xad]);
        let record = one_source_record(11, source_update(account))
            .expect("convert all-Pump-owned source record");
        let PumpExactStateRawRecordV2::PumpOwnedAccountUpdate(update) = record else {
            panic!("expected Pump-owned account update");
        };
        assert_eq!(
            update.evidence_class,
            PumpExactStateAccountEvidenceClassV2::OtherPumpOwned
        );
        assert_eq!(update.source.capture_sequence, 11);
        assert_eq!(update.source.stream_epoch, 4);
        assert_eq!(update.raw_account_data, vec![0xde, 0xad]);
        assert_eq!(
            update.raw_account_data_hash_blake3,
            hash_bytes_v2(&[0xde, 0xad])
        );
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
            vec!["pump_research_exact_state_v2_all_pump_owned".to_owned()]
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
    fn v2_writer_publishes_an_independent_segment_with_other_pump_owned_evidence() {
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
                if update.evidence_class == PumpExactStateAccountEvidenceClassV2::OtherPumpOwned
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

    #[test]
    fn v2_writer_keeps_bootstrap_records_in_the_same_epoch_chain_without_source_sequence() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let mut writer = PumpExactStateRawSegmentWriterV2::new(
            raw_dir.clone(),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([5; 32]),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("create V2 writer");
        writer
            .write_bootstrap_record(
                9,
                PumpExactStateRawRecordV2::BootstrapSnapshotStarted(
                    ghost_core::pump_research_exact_tape_v2::PumpExactStateBootstrapSnapshotStartedV2 {
                        snapshot_id_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([8; 32]),
                        provider_id: "primary-test".to_owned(),
                        pump_program_id: ghost_core::pump_research_tape::PumpResearchStoragePubkeyV1::from([7; 32]),
                        commitment: ghost_core::pump_research_exact_tape_v2::PumpExactStateBootstrapCommitmentV2::Finalized,
                        source_stream_epoch_at_start: 9,
                        started_wall_ts_ms: 1,
                        started_monotonic_ts_ms: 2,
                    },
                ),
            )
            .expect("write bootstrap boundary");
        writer.close_current(true).expect("close V2 segment");
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        assert!(matches!(
            records.first(),
            Some(PumpExactStateRawRecordV2::BootstrapSnapshotStarted(_))
        ));
        assert_eq!(writer.receipts()[0].first_capture_sequence, None);
        assert_eq!(writer.receipts()[0].last_capture_sequence, None);
    }

    fn bootstrap_started_record(epoch: u64) -> PumpExactStateRawRecordV2 {
        PumpExactStateRawRecordV2::BootstrapSnapshotStarted(
            ghost_core::pump_research_exact_tape_v2::PumpExactStateBootstrapSnapshotStartedV2 {
                snapshot_id_blake3: ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([9; 32]),
                provider_id: "primary-test".to_owned(),
                pump_program_id: ghost_core::pump_research_tape::PumpResearchStoragePubkeyV1::from([7; 32]),
                commitment: ghost_core::pump_research_exact_tape_v2::PumpExactStateBootstrapCommitmentV2::Finalized,
                source_stream_epoch_at_start: epoch,
                started_wall_ts_ms: 1,
                started_monotonic_ts_ms: 2,
            },
        )
    }

    #[test]
    fn v2_coordinator_requires_a_sealed_bootstrap_before_clean_completion() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![1],
        )));
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown);
        assert!(summary
            .error
            .as_deref()
            .is_some_and(|error| error.contains("bootstrap")));
        assert_eq!(coordinator.source_lifecycle().bootstrap_status, "failed");
    }

    #[test]
    fn v2_coordinator_drains_source_and_bootstrap_records_before_clean_close() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &raw_dir,
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_transaction_update(101, 0x11));
        let mut account = pump_owned_account(Pubkey::new_unique(), pump, vec![2]);
        account.slot = 102;
        sink.try_capture(source_update(account));
        sink.try_capture(source_slot_update(103));
        sink.try_capture(source_block_meta_update(104));
        sink.try_capture(source_full_block_update(105));
        coordinator
            .enqueue_bootstrap_record(4, bootstrap_started_record(4), Duration::from_secs(1))
            .expect("queue bootstrap record");
        coordinator
            .seal_bootstrap_complete()
            .expect("seal bootstrap");
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(summary.clean_shutdown, "{summary:?}");
        assert_eq!(summary.accepted_source_records, 5);
        assert_eq!(summary.accepted_bootstrap_records, 1);
        assert!(summary.required_lane_census.all_required_lanes_observed());
        assert_eq!(summary.required_lane_census.transaction_messages, 1);
        assert_eq!(summary.required_lane_census.account_updates, 1);
        assert_eq!(summary.required_lane_census.slot_updates, 1);
        assert_eq!(summary.required_lane_census.block_meta_updates, 1);
        assert_eq!(summary.required_lane_census.full_blocks_started, 1);
        assert_eq!(summary.required_lane_census.full_blocks_completed, 1);
        assert_eq!(summary.segments.len(), 1);
        assert!(raw_dir.join("segment_00000.bin").exists());
        assert_eq!(coordinator.source_lifecycle().bootstrap_status, "complete");
    }

    #[test]
    fn v2_coordinator_marks_a_run_unclean_when_a_required_lane_is_missing() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &raw_dir,
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_transaction_update(101, 0x12));
        let mut account = pump_owned_account(Pubkey::new_unique(), pump, vec![3]);
        account.slot = 102;
        sink.try_capture(source_update(account));
        sink.try_capture(source_slot_update(103));
        sink.try_capture(source_block_meta_update(104));
        coordinator
            .enqueue_bootstrap_record(4, bootstrap_started_record(4), Duration::from_secs(1))
            .expect("queue bootstrap record");
        coordinator
            .seal_bootstrap_complete()
            .expect("seal bootstrap");
        coordinator.finish_source();

        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown, "{summary:?}");
        assert!(!summary.required_lane_census.all_required_lanes_observed());
        assert_eq!(summary.required_lane_census.full_blocks_started, 0);
        assert_eq!(summary.required_lane_census.full_blocks_completed, 0);
        assert!(summary
            .error
            .as_deref()
            .is_some_and(|error| { error.contains("required source lane census is incomplete") }));
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        assert!(matches!(
            records.last(),
            Some(PumpExactStateRawRecordV2::SegmentClosed(footer)) if !footer.clean_shutdown
        ));
    }

    #[test]
    fn v2_source_readiness_requires_every_lane_and_uses_the_latest_first_slot() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_transaction_update(101, 0x13));
        let mut account = pump_owned_account(Pubkey::new_unique(), pump, vec![4]);
        account.slot = 102;
        sink.try_capture(source_update(account));
        sink.try_capture(source_slot_update(103));
        sink.try_capture(source_block_meta_update(104));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_time()
            .build()
            .expect("build current-thread Tokio runtime");
        let error = runtime
            .block_on(coordinator.wait_for_required_source_lanes(Duration::from_millis(10)))
            .expect_err("missing full-block lane must not establish source readiness");
        assert!(error
            .to_string()
            .contains("transaction/account/slot/block-meta/full-block"));

        sink.try_capture(source_full_block_update(105));
        let readiness = runtime
            .block_on(coordinator.wait_for_required_source_lanes(Duration::from_secs(1)))
            .expect("every required lane establishes readiness");
        assert_eq!(readiness.first_transaction_slot, 101);
        assert_eq!(readiness.first_account_update_slot, 102);
        assert_eq!(readiness.first_slot_update_slot, 103);
        assert_eq!(readiness.first_block_meta_slot, 104);
        assert_eq!(readiness.first_full_block_slot, 105);
        assert_eq!(readiness.source_readiness_slot, 105);

        coordinator.fail_bootstrap();
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown);
    }

    #[test]
    fn v2_bootstrap_retries_a_stale_finalized_snapshot_within_the_original_budget() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_transaction_update(101, 0x14));
        let mut account = pump_owned_account(Pubkey::new_unique(), pump, vec![5]);
        account.slot = 102;
        sink.try_capture(source_update(account));
        sink.try_capture(source_slot_update(103));
        sink.try_capture(source_block_meta_update(104));
        sink.try_capture(source_full_block_update(105));

        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build Tokio runtime with local TCP support");
        let bootstrap = runtime.block_on(async {
            use tokio::io::AsyncWriteExt as _;

            let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
                .await
                .expect("bind local bootstrap fixture listener");
            let endpoint = format!(
                "http://{}",
                listener
                    .local_addr()
                    .expect("read local bootstrap fixture address")
            );
            let stale = bootstrap_response_bytes_at_slot(PUMP_FUN_PROGRAM_ID, "base64", 104);
            let overlapping =
                bootstrap_response_bytes_at_slot(PUMP_FUN_PROGRAM_ID, "base64", 105);
            let server = tokio::spawn(async move {
                for body in [stale, overlapping] {
                    let (mut stream, _) = listener
                        .accept()
                        .await
                        .expect("accept local bootstrap request");
                    let response_head = format!(
                        "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                        body.len(),
                    );
                    stream
                        .write_all(response_head.as_bytes())
                        .await
                        .expect("write local bootstrap response head");
                    stream
                        .write_all(&body)
                        .await
                        .expect("write local bootstrap response body");
                    stream
                        .shutdown()
                        .await
                        .expect("close local bootstrap response");
                }
            });

            let bootstrap = fetch_bootstrap_with_source_overlap_v2(
                &coordinator,
                &endpoint,
                None,
                "x-test-token",
                Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID"),
                Duration::from_secs(2),
                1024 * 1024,
            )
            .await
            .expect("retry stale finalized response until source overlap exists");
            server.await.expect("join local bootstrap fixture server");
            bootstrap
        });
        assert_eq!(bootstrap.snapshot_attempt_count, 2);
        assert_eq!(bootstrap.snapshot.finalized_context_slot, 105);
        assert_eq!(bootstrap.source_readiness.source_readiness_slot, 105);

        coordinator.fail_bootstrap();
        coordinator.finish_source();
        assert!(!coordinator.finish_and_join().clean_shutdown);
    }

    #[test]
    fn v2_writer_admits_bootstrap_during_a_busy_source_backlog() {
        const SOURCE_BACKLOG: usize = V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE * 2;
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let (data_tx, data_rx) = crossbeam_channel::bounded(SOURCE_BACKLOG + 1);
        let (control_tx, control_rx) = crossbeam_channel::bounded(SOURCE_BACKLOG + 1);
        let (bootstrap_tx, bootstrap_rx) = crossbeam_channel::bounded(1);
        let ingress = Arc::new(PumpExactStateCaptureIngressV2::new(
            data_tx,
            control_tx,
            SOURCE_BACKLOG + 1,
            64 * 1024 * 1024,
            CancellationToken::new(),
        ));
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        for sequence in 0..SOURCE_BACKLOG {
            let source = source_update(pump_owned_account(
                Pubkey::new_unique(),
                pump,
                vec![u8::try_from(sequence % 256).expect("test byte")],
            ));
            let required_lane = required_source_lane_observation_v2(&source)
                .expect("source fixture belongs to required account lane");
            let queued = queued_source_update_v2(
                u64::try_from(sequence).expect("sequence fits u64"),
                source,
                required_lane,
            )
            .expect("serialize bounded V2 source update");
            assert!(
                ingress.try_reserve_source_bytes(queued.byte_cost),
                "test source update must fit the explicit byte budget"
            );
            ingress
                .data_tx
                .send(queued)
                .expect("preload bounded source backlog");
        }
        bootstrap_tx
            .send(QueuedBootstrapRecordV2 {
                stream_epoch: 4,
                record: bootstrap_started_record(4),
            })
            .expect("preload bootstrap record");
        let bootstrap_status = Arc::new(AtomicU8::new(
            PumpExactStateBootstrapStatusV2::Complete as u8,
        ));
        ingress
            .final_capture_sequence
            .store(SOURCE_BACKLOG as u64, Ordering::Release);
        ingress.source_finished.store(true, Ordering::Release);
        let progress = Arc::new(Mutex::new(PumpExactStateWriterSummaryV2::default()));
        raw_writer_main_v2(
            &raw_dir,
            "prospective-exact-test",
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([34; 32]),
            data_rx,
            control_rx,
            bootstrap_rx,
            Arc::clone(&ingress),
            Arc::clone(&progress),
            bootstrap_status,
            Duration::from_millis(1),
            2 * 1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("writer processes bounded source backlog and bootstrap fairly");

        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        let bootstrap_position = records
            .iter()
            .position(|record| {
                matches!(
                    record,
                    PumpExactStateRawRecordV2::BootstrapSnapshotStarted(_)
                )
            })
            .expect("bootstrap record persisted in the source segment chain");
        assert_eq!(
            bootstrap_position,
            V2_WRITER_INGRESS_DRAIN_BUDGET_PER_LANE,
            "the writer must service bootstrap after one finite source batch, not after draining the whole busy backlog"
        );
        assert_eq!(
            progress.lock().accepted_bootstrap_records,
            1,
            "bootstrap record was accepted before source closure"
        );
    }

    #[test]
    fn v2_coordinator_fails_closed_on_a_source_epoch_change() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        sink.source_stream_established(5);

        assert!(
            coordinator.capture_abort().is_cancelled(),
            "an epoch change must cancel the source before it can blend two source lifecycles"
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
    fn v2_coordinator_fails_closed_when_an_established_stream_interrupts_before_reconnect() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        sink.source_stream_interrupted(4, "transport closed before reconnect".to_owned());

        assert!(
            coordinator.capture_abort().is_cancelled(),
            "an established-stream interruption must stop V2 before retry can hide the gap"
        );
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown);
        let lifecycle = coordinator.source_lifecycle();
        assert!(lifecycle
            .fatal_capture_error
            .as_deref()
            .is_some_and(|reason| reason.contains("stream was interrupted")));
        assert!(lifecycle
            .source_worker_error
            .as_deref()
            .is_some_and(|reason| reason.contains("transport closed before reconnect")));
    }

    #[test]
    fn v2_coordinator_fails_closed_when_one_source_update_exceeds_byte_budget() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        let sink = coordinator.source_sink();
        sink.source_stream_established(4);
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        sink.try_capture(source_update(pump_owned_account(
            Pubkey::new_unique(),
            pump,
            vec![1],
        )));

        assert!(
            coordinator.capture_abort().is_cancelled(),
            "an oversized decoded source update must stop capture before unbounded queue growth"
        );
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(!summary.clean_shutdown);
        let lifecycle = coordinator.source_lifecycle();
        assert_eq!(lifecycle.source_queue_peak_bytes, 0);
        assert_eq!(lifecycle.source_queue_bytes_at_close, 0);
        assert_eq!(lifecycle.dropped_source_updates, 1);
        assert!(lifecycle
            .fatal_capture_error
            .as_deref()
            .is_some_and(|reason| reason.contains("byte budget")));
    }

    #[test]
    fn v2_bootstrap_persistence_seals_an_explicit_prospective_readiness_boundary() {
        let temporary = tempdir().expect("temporary raw root");
        let raw_dir = temporary.path().join("raw-v2");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &raw_dir,
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        coordinator.source_sink().source_stream_established(4);
        let source_readiness = source_readiness_for_test(119, 120, 121, 122, 123);
        let seal = persist_bootstrap_snapshot_v2(
            &coordinator,
            "primary-test",
            Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID"),
            4,
            PumpExactStateBootstrapOverlapV2 {
                snapshot: bootstrap_snapshot_for_test(123),
                source_readiness: source_readiness.clone(),
                snapshot_attempt_count: 2,
            },
            Duration::from_secs(1),
        )
        .expect("persist bounded V2 bootstrap");
        assert_eq!(seal.finalized_context_slot, 123);
        assert_eq!(seal.source_readiness, source_readiness);
        assert_eq!(seal.snapshot_attempt_count, 2);
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(
            !summary.clean_shutdown,
            "no required source lanes were durably written"
        );
        let (_, records) = decode_v2_segment(&raw_dir.join("segment_00000.bin"));
        assert!(matches!(
            records.as_slice(),
            [
                PumpExactStateRawRecordV2::BootstrapSnapshotStarted(_),
                PumpExactStateRawRecordV2::BootstrapResponseChunk(_),
                PumpExactStateRawRecordV2::BootstrapSnapshotCompleted(_),
                PumpExactStateRawRecordV2::ProspectiveReadinessBoundary(boundary),
                PumpExactStateRawRecordV2::SegmentClosed(_),
            ] if boundary.finalized_context_slot == 123
                && boundary.cohort_slots_strictly_after == 123
                && boundary.source_stream_epoch == 4
                && boundary.source_capture_sequence_exclusive == 0
                && boundary.source_readiness == source_readiness
                && boundary.finalized_context_slot_covers_source_readiness
        ));
    }

    #[test]
    fn v2_bootstrap_persistence_rejects_a_snapshot_before_source_readiness() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        coordinator.source_sink().source_stream_established(4);
        let source_readiness = source_readiness_for_test(119, 120, 121, 122, 123);
        let error = persist_bootstrap_snapshot_v2(
            &coordinator,
            "primary-test",
            Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID"),
            4,
            PumpExactStateBootstrapOverlapV2 {
                snapshot: bootstrap_snapshot_for_test(122),
                source_readiness,
                snapshot_attempt_count: 1,
            },
            Duration::from_secs(1),
        )
        .expect_err("a finalized snapshot before source readiness must fail closed");
        assert!(error
            .to_string()
            .contains("predates required source readiness slot"));
        coordinator.fail_bootstrap();
        coordinator.finish_source();
        assert!(!coordinator.finish_and_join().clean_shutdown);
    }

    #[test]
    fn v2_bootstrap_persistence_rejects_a_forged_source_readiness_maximum() {
        let temporary = tempdir().expect("temporary raw root");
        let coordinator = PumpExactStateCaptureCoordinatorV2::start(
            &temporary.path().join("raw-v2"),
            "prospective-exact-test".to_owned(),
            ghost_core::pump_research_tape::PumpResearchStorageHashV1::from([6; 32]),
            16,
            1024 * 1024,
            16,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
            TEST_V2_MAX_RAW_BYTES,
            TEST_V2_MIN_FREE_BYTES,
        )
        .expect("start V2 coordinator");
        coordinator.source_sink().source_stream_established(4);
        let mut source_readiness = source_readiness_for_test(119, 120, 121, 122, 123);
        source_readiness.source_readiness_slot = 122;
        let error = persist_bootstrap_snapshot_v2(
            &coordinator,
            "primary-test",
            Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID"),
            4,
            PumpExactStateBootstrapOverlapV2 {
                snapshot: bootstrap_snapshot_for_test(123),
                source_readiness,
                snapshot_attempt_count: 1,
            },
            Duration::from_secs(1),
        )
        .expect_err("a lowered source readiness maximum must fail closed");
        assert!(error
            .to_string()
            .contains("differs from required-lane maximum"));
        coordinator.fail_bootstrap();
        coordinator.finish_source();
        assert!(!coordinator.finish_and_join().clean_shutdown);
    }

    fn bootstrap_response_bytes_at_slot(owner: &str, encoding: &str, slot: u64) -> Vec<u8> {
        let account = Pubkey::new_unique();
        serde_json::to_vec(&json!({
            "jsonrpc": "2.0",
            "id": "pump_exact_state_v2_bootstrap",
            "result": {
                "context": { "slot": slot },
                "value": [{
                    "pubkey": account.to_string(),
                    "account": {
                        "lamports": 42,
                        "owner": owner,
                        "executable": false,
                        "rentEpoch": 9,
                        "data": ["AQID", encoding]
                    }
                }]
            }
        }))
        .expect("serialize bootstrap response fixture")
    }

    fn bootstrap_response_bytes(owner: &str, encoding: &str) -> Vec<u8> {
        bootstrap_response_bytes_at_slot(owner, encoding, 1234)
    }

    #[test]
    fn v2_bootstrap_parser_preserves_raw_response_and_requires_pump_ownership() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let response = bootstrap_response_bytes(PUMP_FUN_PROGRAM_ID, "base64");
        let snapshot = parse_finalized_program_accounts_snapshot_v2(response.clone(), pump, 1, 2)
            .expect("parse valid finalized V2 bootstrap response");
        assert_eq!(snapshot.finalized_context_slot, 1234);
        assert_eq!(snapshot.response_bytes, response);
        assert_eq!(snapshot.accounts.len(), 1);
        assert_eq!(snapshot.accounts[0].raw_account_data, vec![1, 2, 3]);
        assert_eq!(
            snapshot.accounts[0].raw_account_data_hash_blake3,
            hash_bytes_v2(&[1, 2, 3])
        );

        let non_pump = bootstrap_response_bytes(&Pubkey::new_unique().to_string(), "base64");
        assert!(
            parse_finalized_program_accounts_snapshot_v2(non_pump, pump, 1, 2)
                .expect_err("non-Pump snapshot account must fail closed")
                .to_string()
                .contains("differs from Pump program")
        );
    }

    #[test]
    fn v2_bootstrap_parser_rejects_nonliteral_base64_account_encoding() {
        let pump = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("Pump program ID");
        let response = bootstrap_response_bytes(PUMP_FUN_PROGRAM_ID, "base64+zstd");
        assert!(
            parse_finalized_program_accounts_snapshot_v2(response, pump, 1, 2)
                .expect_err("compressed/bootstrap-normalized account data is not source-lossless")
                .to_string()
                .contains("exactly [base64, base64]")
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
            bootstrap_rpc_endpoint: "https://rpc.example.invalid".to_owned(),
            bootstrap_rpc_auth_token_env: None,
            bootstrap_rpc_auth_header: "x-api-key".to_owned(),
            pump_program_id: PUMP_FUN_PROGRAM_ID.to_owned(),
            output_dir: PathBuf::from("datasets/pump-research/raw"),
            required_for_run: true,
            source_queue_capacity: 1,
            source_queue_max_bytes: MIN_V2_SOURCE_QUEUE_MAX_BYTES,
            cohort_capture_wall_ms: MIN_V2_COHORT_CAPTURE_WALL_MS,
            min_free_bytes: MIN_V2_MIN_FREE_BYTES,
            max_raw_bytes: MIN_V2_MAX_RAW_BYTES,
            bootstrap_queue_capacity: 1,
            bootstrap_enqueue_timeout_ms: 1,
            stream_establish_timeout_ms: 1,
            bootstrap_rpc_timeout_ms: 1,
            bootstrap_response_max_bytes: 1,
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
            bootstrap_executable_digest: release_binary_digest.clone(),
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
