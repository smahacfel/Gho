//! Standalone, research-only Pump Research Evidence Tape V1 capture.
//!
//! This module owns PR-A's durable boundary:
//!
//! ```text
//! decoded Yellowstone SubscribeUpdate
//!     -> bounded nonblocking ingress
//!     -> dedicated blocking writer thread
//!     -> immutable V1 raw segments and receipts
//! ```
//!
//! It deliberately does **not** participate in the active Seer runtime,
//! candidate filtering, parser, Event Bus, account-state authority, or
//! execution path.  The frozen V1 storage types live in `ghost-core`; this
//! module only converts decoded source messages at the capture boundary.

use crate::{
    grpc_connection::{
        pump_research_subscription_request_fingerprint_blake3_v1, PumpResearchSourceConnectionV1,
        PumpResearchSourceSinkV1, PumpResearchSourceUpdateV1,
    },
    local_gap::LocalGapTracker,
};
use anyhow::{bail, Context, Result};
use ghost_core::{
    pump_research_tape::{
        self as tape, PumpPrimaryAccountUpdateEvidenceV1, PumpPrimaryBlockMetaEvidenceV1,
        PumpPrimarySlotEvidenceV1, PumpPrimaryTransactionEvidenceV1, PumpRawCoverageBoundaryV1,
        PumpRawCoverageGapReasonV1, PumpRawCoverageGapV1, PumpRawSegmentClosedV1,
        PumpRawSegmentHeaderV1, PumpRawSourceEnvelopeV1, PumpResearchAccountRoleV1,
        PumpResearchEventTimeV1, PumpResearchProviderRoleV1, PumpResearchRawCodecErrorV1,
        PumpResearchRawCodecV1, PumpResearchRawRecordV1, PumpResearchRunCompletionReceiptV1,
        PumpResearchRunCompletionStatusV1, PumpResearchRunStartManifestV1,
        PumpResearchSegmentReceiptV1, PumpResearchStorageHashV1, PumpResearchStoragePubkeyV1,
        PumpResearchStorageSignatureV1, PUMP_RESEARCH_BINCODE_VERSION_V1,
        PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1, PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1,
        PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1, PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
        PUMP_RESEARCH_SOURCE_CAPTURE_SEMANTICS_V1, PUMP_RESEARCH_SOURCE_CLIENT_CRATE_V1,
        PUMP_RESEARCH_SOURCE_CLIENT_VERSION_V1, PUMP_RESEARCH_SOURCE_PROTO_CRATE_V1,
        PUMP_RESEARCH_SOURCE_PROTO_CRATE_VERSION_V1, PUMP_RESEARCH_SOURCE_PROTO_DESCRIPTOR_HASH_V1,
        PUMP_RESEARCH_SOURCE_PROTO_SCHEMA_VERSION_V1, PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
    },
    LocalCoverageBoundaryV1, LocalCoverageGapReasonV1, LocalCoverageGapV1,
};
use parking_lot::Mutex;
use prost::Message;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_sdk::{
    bpf_loader_upgradeable::{self, UpgradeableLoaderState},
    commitment_config::CommitmentConfig,
    pubkey::Pubkey,
    signature::Signature,
};
use std::{
    collections::{BTreeMap, BTreeSet},
    env,
    ffi::OsString,
    fs::{self, File, OpenOptions},
    io::{BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    process::Command,
    sync::{
        atomic::{AtomicBool, AtomicU64, AtomicU8, AtomicUsize, Ordering},
        Arc,
    },
    thread::{self, JoinHandle},
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use yellowstone_grpc_proto::prelude::subscribe_update::UpdateOneof;

const RAW_SCHEMA_VERSION_V1: u16 = 1;
const TIME_CONTRACT_VERSION_V1: u16 = 1;
const DEFAULT_QUEUE_CAPACITY_V1: usize = 2_048;
const DEFAULT_FLUSH_INTERVAL_MS_V1: u64 = 1_000;
const DEFAULT_SEGMENT_MAX_BYTES_V1: u64 = 256 * 1024 * 1024;
const DEFAULT_SEGMENT_MAX_DURATION_MS_V1: u64 = 300_000;
const WRITER_IDLE_POLL_V1: Duration = Duration::from_millis(5);

/// Standalone configuration consumed only by `pump-research-tape capture`.
///
/// It intentionally is not embedded in `SeerConfig`: therefore adding this
/// capture path cannot switch, reconfigure, or weaken the active runtime's
/// Yellowstone path.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PumpResearchCaptureConfigV1 {
    /// Stable identity of the one primary Yellowstone provider.
    pub primary_provider_id: String,
    /// Yellowstone endpoint for the standalone source stream.
    pub grpc_endpoint: String,
    /// Environment variable containing the provider credential.  The secret
    /// value is resolved at launch and is never persisted in artifacts.
    #[serde(default)]
    pub grpc_auth_token_env: Option<String>,
    #[serde(default = "default_grpc_auth_header")]
    pub grpc_auth_header: String,
    /// Read-only RPC endpoint used solely for finalized Program/ProgramData
    /// receipts at run start and completion.
    pub rpc_endpoint: String,
    /// Optional environment variable containing a read-only RPC credential.
    /// The value is resolved only immediately before ProgramData RPC and is
    /// never persisted in the operator bundle or raw artifacts.
    #[serde(default)]
    pub rpc_auth_token_env: Option<String>,
    #[serde(default = "default_rpc_auth_header")]
    pub rpc_auth_header: String,
    #[serde(default = "default_pump_program_id")]
    pub pump_program_id: String,
    /// Parent directory.  Every capture creates `<output_dir>/<run_id>/raw`.
    pub output_dir: PathBuf,
    #[serde(default = "default_required_for_run")]
    pub required_for_run: bool,
    #[serde(default = "default_queue_capacity")]
    pub queue_capacity: usize,
    #[serde(default = "default_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_segment_max_bytes")]
    pub segment_max_bytes: u64,
    #[serde(default = "default_segment_max_duration_ms")]
    pub segment_max_duration_ms: u64,
    /// The frozen V1 record limit is a contract, not a tuning knob.  It is
    /// accepted in TOML only to make an attempted incompatible override fail
    /// explicitly rather than silently being ignored.
    #[serde(default = "default_record_max_bytes")]
    pub record_max_bytes: usize,
}

impl PumpResearchCaptureConfigV1 {
    /// Load the standalone TOML configuration and retain its exact bytes for
    /// the immutable run-manifest hash.
    pub fn load(path: &Path) -> Result<(Self, Vec<u8>)> {
        let bytes = fs::read(path).with_context(|| {
            format!("read Pump Research Tape capture config {}", path.display())
        })?;
        let text = std::str::from_utf8(&bytes)
            .with_context(|| format!("capture config {} is not UTF-8 TOML", path.display()))?;
        let config: Self = toml::from_str(text).with_context(|| {
            format!("parse Pump Research Tape capture config {}", path.display())
        })?;
        config.validate()?;
        Ok((config, bytes))
    }

    pub fn validate(&self) -> Result<()> {
        validate_trimmed("primary_provider_id", &self.primary_provider_id)?;
        validate_trimmed("grpc_endpoint", &self.grpc_endpoint)?;
        validate_trimmed("grpc_auth_header", &self.grpc_auth_header)?;
        validate_trimmed("rpc_endpoint", &self.rpc_endpoint)?;
        validate_trimmed("rpc_auth_header", &self.rpc_auth_header)?;
        if let Some(name) = &self.grpc_auth_token_env {
            validate_trimmed("grpc_auth_token_env", name)?;
        }
        if let Some(name) = &self.rpc_auth_token_env {
            validate_trimmed("rpc_auth_token_env", name)?;
        }
        if self.output_dir.as_os_str().is_empty() {
            bail!("output_dir must not be empty");
        }
        if self.queue_capacity == 0 {
            bail!("queue_capacity must be greater than zero");
        }
        if self.flush_interval_ms == 0 {
            bail!("flush_interval_ms must be greater than zero");
        }
        if self.segment_max_bytes == 0 {
            bail!("segment_max_bytes must be greater than zero");
        }
        if self.segment_max_duration_ms == 0 {
            bail!("segment_max_duration_ms must be greater than zero");
        }
        if self.record_max_bytes != PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 {
            bail!(
                "record_max_bytes must equal frozen V1 limit {} bytes, got {}",
                PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
                self.record_max_bytes
            );
        }
        let configured_program = self
            .pump_program_id
            .parse::<Pubkey>()
            .context("pump_program_id is not a valid Solana pubkey")?;
        let frozen_program = PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1
            .parse::<Pubkey>()
            .context("frozen V1 Pump program ID is not a valid pubkey")?;
        if configured_program != frozen_program {
            bail!(
                "pump_program_id {} differs from frozen V1 Pump program {}",
                configured_program,
                frozen_program
            );
        }
        Ok(())
    }

    fn resolve_grpc_auth_token(&self) -> Result<Option<String>> {
        match &self.grpc_auth_token_env {
            None => Ok(None),
            Some(variable) => env::var(variable)
                .with_context(|| {
                    format!("read gRPC credential from environment variable {variable}")
                })
                .map(Some),
        }
    }

    fn resolve_rpc_auth_token(&self) -> Result<Option<String>> {
        match &self.rpc_auth_token_env {
            None => Ok(None),
            Some(variable) => env::var(variable)
                .with_context(|| {
                    format!("read RPC credential from environment variable {variable}")
                })
                .map(Some),
        }
    }
}

fn default_grpc_auth_header() -> String {
    "x-token".to_owned()
}

fn default_rpc_auth_header() -> String {
    "x-api-key".to_owned()
}

fn default_pump_program_id() -> String {
    PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1.to_owned()
}

const fn default_required_for_run() -> bool {
    true
}

const fn default_queue_capacity() -> usize {
    DEFAULT_QUEUE_CAPACITY_V1
}

const fn default_flush_interval_ms() -> u64 {
    DEFAULT_FLUSH_INTERVAL_MS_V1
}

const fn default_segment_max_bytes() -> u64 {
    DEFAULT_SEGMENT_MAX_BYTES_V1
}

const fn default_segment_max_duration_ms() -> u64 {
    DEFAULT_SEGMENT_MAX_DURATION_MS_V1
}

const fn default_record_max_bytes() -> usize {
    PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1
}

fn validate_trimmed(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim() != value {
        bail!("{name} must be non-empty and have no surrounding whitespace");
    }
    Ok(())
}

// ---------------------------------------------------------------------------
// Operator preflight provenance (outside the frozen raw V1 storage contract).
// ---------------------------------------------------------------------------
//
// `PumpResearchRawRecordV1` and the JSON run-start manifest are frozen CS0
// artifacts.  A dirty worktree therefore cannot be represented by adding
// fields to them.  The operator preflight below deliberately creates a
// separate immutable bundle and a run-local binding sidecar.  Together they
// prove the exact executable, Cargo lock, tracked patch and untracked source
// inventory that were validated immediately before a capture starts.

const OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1: u16 = 1;
const OPERATOR_PREFLIGHT_RECEIPT_FILE_V1: &str = "operator_preflight_receipt_v1.json";
const OPERATOR_PREFLIGHT_STATUS_FILE_V1: &str = "git_status_porcelain_v1.bin";
const OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1: &str = "tracked_worktree.patch";
const OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1: &str = "untracked_inventory_v1.json";
const OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1: &str = "capture_config_redacted_v1.json";
const OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1: &str = "source_snapshot";
const OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1: &str =
    "source_snapshot_manifest_v1.json";
const OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE_V1: &str = "release/pump-research-tape";
const OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1: &str = "release/build_receipt_v1.json";
const OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1: &str = "release/build.log";
const OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1: &str = "release/build_environment_v1.json";
pub(crate) const OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1: &str =
    "operator_preflight_binding_v1.json";
const OPERATOR_PREFLIGHT_RECEIPT_KIND_V1: &str = "pump_research_operator_preflight_v1";
pub(crate) const OPERATOR_PREFLIGHT_BINDING_KIND_V1: &str =
    "pump_research_capture_provenance_binding_v1";
const OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1: &str =
    "full_current_worktree_snapshot_plus_required_ignored_fixtures_v1";
const OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1: &str =
    "external_operator_config_digest_and_redacted_projection_only_v1";
pub(crate) const OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1: &str =
    "fresh_cargo_target_locked_offline_release_from_isolated_snapshot_staging_clean_toolchain_binary_child_env_and_cargo_config_strict_allowlist_v5";
pub(crate) const OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1: &str =
    "configured_operator_credential_bytes_absent_from_sealed_bundle_v1";
const OPERATOR_PREFLIGHT_SANITIZED_BUILD_ENVIRONMENT_V1: &str =
    "env_clear_controlled_path_home_cargo_home_target_dir_direct_toolchain_isolated_cargo_config_v3";
const OPERATOR_PREFLIGHT_SANITIZED_CARGO_HOME_V1: &str =
    "fresh_cargo_home_with_offline_registry_cache_and_git_db_only_v1";
const OPERATOR_PREFLIGHT_ISOLATED_CARGO_CONFIG_SCOPE_V1: &str =
    "sealed_snapshot_cargo_config_strict_allowlist_only_no_cargo_config_in_staging_or_ancestor_hierarchy_v2";
const OPERATOR_PREFLIGHT_ISOLATED_BUILD_STAGING_SEMANTICS_V1: &str =
    "create_new_temp_staging_root_materialized_from_verified_sealed_snapshot_v1";
const OPERATOR_PREFLIGHT_FRESH_BUILD_DENIED_ENV_V1: &[&str] = &[
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
const OPERATOR_PREFLIGHT_REQUIRED_IGNORED_ARTIFACTS_V1: &[&str] =
    &["ghost-core/tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json"];

/// Pair of content digests used only by the operator provenance sidecar.
/// Raw V1 storage continues to use its frozen fixed-width BLAKE3 wrapper.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchOperatorDigestV1 {
    pub sha256: String,
    pub blake3: String,
    pub bytes: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorPreflightUntrackedEntryV1 {
    repository_relative_path: String,
    source_kind: PumpResearchOperatorSourceEntryKindV1,
    digest: PumpResearchOperatorDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorPreflightUntrackedInventoryV1 {
    schema_version: u16,
    entries: Vec<PumpResearchOperatorPreflightUntrackedEntryV1>,
}

/// Classification is explicit because the source snapshot includes ordinary
/// untracked files as well as the narrowly allowlisted ignored fixture(s)
/// required by frozen CS0 tests.  `target/`, datasets and arbitrary ignored
/// files are intentionally not admitted by this contract.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PumpResearchOperatorSourceEntryKindV1 {
    Tracked,
    Untracked,
    RequiredIgnoredFixture,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorSourceSnapshotEntryV1 {
    repository_relative_path: String,
    source_kind: PumpResearchOperatorSourceEntryKindV1,
    digest: PumpResearchOperatorDigestV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorSourceSnapshotManifestV1 {
    schema_version: u16,
    source_tree_semantics: String,
    entries: Vec<PumpResearchOperatorSourceSnapshotEntryV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorBuildEnvironmentV1 {
    child_environment_semantics: String,
    build_staging_semantics: String,
    cargo_target_dir_semantics: String,
    cargo_home_semantics: String,
    cargo_config_scope_semantics: String,
    cargo_executable_digest: PumpResearchOperatorDigestV1,
    rustc_executable_digest: PumpResearchOperatorDigestV1,
    rustflags_digest: Option<PumpResearchOperatorDigestV1>,
    cargo_encoded_rustflags_digest: Option<PumpResearchOperatorDigestV1>,
    cargo_build_environment_digests: BTreeMap<String, PumpResearchOperatorDigestV1>,
    cargo_profile_release_environment_digests: BTreeMap<String, PumpResearchOperatorDigestV1>,
    cargo_home_digest: Option<PumpResearchOperatorDigestV1>,
    /// The build reads Cargo configuration only from the sealed snapshot. The
    /// fresh CARGO_HOME and every staging ancestor are checked to contain no
    /// `.cargo/config{,.toml}` file, so no digest-only external config can
    /// influence the sealed binary.
    cargo_config_file_digests: BTreeMap<String, Option<PumpResearchOperatorDigestV1>>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchOperatorBuildReceiptV1 {
    schema_version: u16,
    build_semantics: String,
    cargo_command: Vec<String>,
    cargo_profile: String,
    source_snapshot_manifest_digest_before_build: PumpResearchOperatorDigestV1,
    source_snapshot_manifest_digest_after_build: PumpResearchOperatorDigestV1,
    cargo_lock_digest: PumpResearchOperatorDigestV1,
    build_environment_digest: PumpResearchOperatorDigestV1,
    build_log_digest: PumpResearchOperatorDigestV1,
    cargo_executable_digest: PumpResearchOperatorDigestV1,
    rustc_executable_digest: PumpResearchOperatorDigestV1,
    rustc_version: String,
    cargo_version: String,
    release_binary_digest: PumpResearchOperatorDigestV1,
    build_started_wall_ms: u64,
    build_completed_wall_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchCaptureConfigRedactedV1 {
    primary_provider_id: String,
    grpc_endpoint_digest: PumpResearchOperatorDigestV1,
    grpc_auth_token_env: Option<String>,
    grpc_auth_token_present: bool,
    grpc_auth_header: String,
    rpc_endpoint_digest: PumpResearchOperatorDigestV1,
    rpc_auth_token_env: Option<String>,
    rpc_auth_token_present: bool,
    rpc_auth_header: String,
    pump_program_id: String,
    output_dir: String,
    required_for_run: bool,
    queue_capacity: usize,
    flush_interval_ms: u64,
    segment_max_bytes: u64,
    segment_max_duration_ms: u64,
    record_max_bytes: usize,
}

/// Immutable receipt created by `pump-research-tape preflight`.  It is a
/// separate operator artifact, not a new raw-record variant and not an
/// extension of the frozen V1 run manifest.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchOperatorPreflightReceiptV1 {
    pub schema_version: u16,
    pub receipt_kind: String,
    pub created_wall_ms: u64,
    pub source_tree_semantics: String,
    /// The Git parent commit, explicitly qualified by the patch and inventory
    /// fields below when `repository_worktree_state` is `dirty`.
    pub repository_commit: String,
    pub repository_branch: Option<String>,
    pub repository_worktree_state: String,
    pub git_status_porcelain_file: String,
    pub git_status_porcelain_digest: PumpResearchOperatorDigestV1,
    pub tracked_worktree_patch_file: String,
    pub tracked_worktree_patch_digest: PumpResearchOperatorDigestV1,
    pub untracked_inventory_file: String,
    pub untracked_inventory_digest: PumpResearchOperatorDigestV1,
    pub untracked_entry_count: u64,
    pub source_snapshot_manifest_file: String,
    pub source_snapshot_manifest_digest: PumpResearchOperatorDigestV1,
    pub source_snapshot_entry_count: u64,
    pub cargo_lock_digest: PumpResearchOperatorDigestV1,
    pub release_binary_file: String,
    pub release_binary_digest: PumpResearchOperatorDigestV1,
    pub build_receipt_file: String,
    pub build_receipt_digest: PumpResearchOperatorDigestV1,
    pub build_log_file: String,
    pub build_log_digest: PumpResearchOperatorDigestV1,
    pub build_environment_file: String,
    pub build_environment_digest: PumpResearchOperatorDigestV1,
    pub build_semantics: String,
    pub credential_scan_semantics: String,
    pub cargo_executable_digest: PumpResearchOperatorDigestV1,
    pub rustc_executable_digest: PumpResearchOperatorDigestV1,
    pub rustc_version: String,
    pub cargo_version: String,
    pub config_semantics: String,
    /// The external operator TOML never enters the bundle.  This digest binds
    /// it while the redacted projection records safe non-secret semantics.
    pub config_bytes_digest: PumpResearchOperatorDigestV1,
    pub redacted_config_file: String,
    pub redacted_config_digest: PumpResearchOperatorDigestV1,
    pub artifact_provenance_fingerprint: PumpResearchOperatorDigestV1,
}

/// Small result printed by the standalone CLI after the immutable preflight
/// bundle has been durably published.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpResearchOperatorPreflightSummaryV1 {
    pub bundle_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub release_binary_digest: PumpResearchOperatorDigestV1,
    pub artifact_provenance_fingerprint: PumpResearchOperatorDigestV1,
}

/// Run-local sidecar linking one admitted raw run to the exact preflight
/// receipt.  The sidecar is written only after the required start ProgramData
/// receipt succeeds; its two timestamps deliberately distinguish validation
/// (before provider I/O) from the later sidecar write.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct PumpResearchCaptureProvenanceBindingV1 {
    pub(crate) schema_version: u16,
    pub(crate) binding_kind: String,
    pub(crate) run_id: String,
    pub(crate) receipt_validated_wall_ms: u64,
    pub(crate) binding_written_wall_ms: u64,
    pub(crate) preflight_receipt_digest: PumpResearchOperatorDigestV1,
    pub(crate) artifact_provenance_fingerprint: PumpResearchOperatorDigestV1,
    pub(crate) repository_commit: String,
    pub(crate) repository_worktree_state: String,
    pub(crate) release_binary_digest: PumpResearchOperatorDigestV1,
    pub(crate) config_bytes_digest: PumpResearchOperatorDigestV1,
    /// Added after the historical vulnerable PR-A runs. Missing values decode
    /// to the conservative legacy/ineligible state in the offline certifier.
    #[serde(default)]
    pub(crate) build_semantics: String,
    #[serde(default)]
    pub(crate) credential_scan_semantics: String,
    #[serde(default)]
    pub(crate) qualification_provenance_eligible: bool,
    #[serde(default)]
    pub(crate) sealed_release_binary_digest: Option<PumpResearchOperatorDigestV1>,
}

struct PumpResearchOperatorPreflightMaterialV1 {
    repository_root: PathBuf,
    repository_commit: String,
    repository_branch: Option<String>,
    git_status_porcelain: Vec<u8>,
    git_status_porcelain_digest: PumpResearchOperatorDigestV1,
    tracked_worktree_patch: Vec<u8>,
    tracked_worktree_patch_digest: PumpResearchOperatorDigestV1,
    untracked_inventory: PumpResearchOperatorPreflightUntrackedInventoryV1,
    untracked_inventory_bytes: Vec<u8>,
    untracked_inventory_digest: PumpResearchOperatorDigestV1,
    source_snapshot_manifest: PumpResearchOperatorSourceSnapshotManifestV1,
    source_snapshot_manifest_bytes: Vec<u8>,
    source_snapshot_manifest_digest: PumpResearchOperatorDigestV1,
    cargo_lock_digest: PumpResearchOperatorDigestV1,
    cargo_executable_path: PathBuf,
    cargo_executable_digest: PumpResearchOperatorDigestV1,
    rustc_executable_path: PathBuf,
    rustc_executable_digest: PumpResearchOperatorDigestV1,
    rustc_version: String,
    cargo_version: String,
    config_bytes_digest: PumpResearchOperatorDigestV1,
    redacted_config_bytes: Vec<u8>,
    redacted_config_digest: PumpResearchOperatorDigestV1,
    artifact_provenance_fingerprint: PumpResearchOperatorDigestV1,
}

#[derive(Serialize)]
struct PumpResearchOperatorPreflightFingerprintInputV1<'a> {
    source_tree_semantics: &'a str,
    repository_commit: &'a str,
    repository_branch: &'a Option<String>,
    git_status_porcelain_digest: &'a PumpResearchOperatorDigestV1,
    tracked_worktree_patch_digest: &'a PumpResearchOperatorDigestV1,
    untracked_inventory_digest: &'a PumpResearchOperatorDigestV1,
    source_snapshot_manifest_digest: &'a PumpResearchOperatorDigestV1,
    cargo_lock_digest: &'a PumpResearchOperatorDigestV1,
    release_binary_digest: &'a PumpResearchOperatorDigestV1,
    build_receipt_digest: &'a PumpResearchOperatorDigestV1,
    build_log_digest: &'a PumpResearchOperatorDigestV1,
    build_environment_digest: &'a PumpResearchOperatorDigestV1,
    build_semantics: &'a str,
    credential_scan_semantics: &'a str,
    cargo_executable_digest: &'a PumpResearchOperatorDigestV1,
    rustc_executable_digest: &'a PumpResearchOperatorDigestV1,
    rustc_version: &'a str,
    cargo_version: &'a str,
    config_semantics: &'a str,
    config_bytes_digest: &'a PumpResearchOperatorDigestV1,
    redacted_config_digest: &'a PumpResearchOperatorDigestV1,
}

/// The bootstrap command must not be a standard Cargo debug artifact.  The
/// binary that may actually capture is not this bootstrap executable: it is
/// freshly built from the sealed source snapshot and copied into the bundle.
fn require_non_debug_operator_bootstrap_binary() -> Result<()> {
    if cfg!(debug_assertions) {
        bail!(
            "Pump Research operator preflight and capture require a non-debug bootstrap binary; run `cargo build --release -p seer --bin pump-research-tape` and invoke `target/release/pump-research-tape` directly"
        );
    }
    Ok(())
}

fn validate_operator_endpoint_reference(name: &str, endpoint: &str) -> Result<()> {
    // Endpoint credentials must remain in named environment variables, never
    // in a URL.  A root-only HTTPS origin is deliberately required: provider
    // tokens cannot be hidden in userinfo, query, fragment or a provider path.
    // The known template values are also rejected so a no-op preflight cannot
    // be mistaken for provider authorization.
    if endpoint == "https://your-yellowstone-provider.example"
        || endpoint == "https://your-finalized-rpc.example"
    {
        bail!("{name} is a placeholder; provide an approved public HTTPS origin");
    }
    let parsed =
        Url::parse(endpoint).with_context(|| format!("{name} is not a structurally valid URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.query().is_some()
        || parsed.fragment().is_some()
        || !(parsed.path().is_empty() || parsed.path() == "/")
    {
        bail!(
            "{name} must be a public root-only HTTPS origin without userinfo, path, query or fragment; use named auth environment variables instead of URL credentials"
        );
    }
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpResearchOperatorAuthPresenceV1 {
    grpc_token_present: bool,
    rpc_token_present: bool,
}

fn validate_operator_preflight_config(
    config: &PumpResearchCaptureConfigV1,
) -> Result<PumpResearchOperatorAuthPresenceV1> {
    config.validate()?;
    validate_operator_endpoint_reference("grpc_endpoint", &config.grpc_endpoint)?;
    validate_operator_endpoint_reference("rpc_endpoint", &config.rpc_endpoint)?;
    let grpc_token_present = match config.resolve_grpc_auth_token()? {
        Some(token) if token.trim().is_empty() => {
            bail!("the configured gRPC credential environment variable resolves to an empty value")
        }
        Some(_) => true,
        None => false,
    };
    let rpc_token_present = match config.resolve_rpc_auth_token()? {
        Some(token) if token.trim().is_empty() => {
            bail!("the configured RPC credential environment variable resolves to an empty value")
        }
        Some(_) => true,
        None => false,
    };
    Ok(PumpResearchOperatorAuthPresenceV1 {
        grpc_token_present,
        rpc_token_present,
    })
}

pub(crate) fn operator_digest_bytes(bytes: &[u8]) -> PumpResearchOperatorDigestV1 {
    PumpResearchOperatorDigestV1 {
        sha256: format!("{:x}", Sha256::digest(bytes)),
        blake3: blake3::hash(bytes).to_hex().to_string(),
        bytes: bytes.len() as u64,
    }
}

pub(crate) fn operator_digest_file(path: &Path) -> Result<PumpResearchOperatorDigestV1> {
    let file =
        File::open(path).with_context(|| format!("open provenance artifact {}", path.display()))?;
    let mut reader = BufReader::new(file);
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("read provenance artifact {}", path.display()))?;
        if read == 0 {
            break;
        }
        sha256.update(&buffer[..read]);
        blake3.update(&buffer[..read]);
        bytes = bytes
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("provenance artifact exceeds u64 byte counter"))?;
    }
    Ok(PumpResearchOperatorDigestV1 {
        sha256: format!("{:x}", sha256.finalize()),
        blake3: blake3.finalize().to_hex().to_string(),
        bytes,
    })
}

fn immutable_json_bytes<T: Serialize>(value: &T) -> Result<Vec<u8>> {
    let mut bytes =
        serde_json::to_vec_pretty(value).context("serialize operator provenance JSON")?;
    bytes.push(b'\n');
    Ok(bytes)
}

fn write_bytes_create_new(path: &Path, bytes: &[u8]) -> Result<PumpResearchOperatorDigestV1> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create immutable provenance artifact {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write immutable provenance artifact {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync immutable provenance artifact {}", path.display()))?;
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "immutable provenance artifact {} has no parent",
            path.display()
        )
    })?;
    sync_directory(parent)?;
    Ok(operator_digest_bytes(bytes))
}

fn write_json_create_new_with_digest<T: Serialize>(
    path: &Path,
    value: &T,
) -> Result<PumpResearchOperatorDigestV1> {
    let bytes = immutable_json_bytes(value)?;
    write_bytes_create_new(path, &bytes)
}

fn copy_file_create_new_with_digest(
    source: &Path,
    destination: &Path,
) -> Result<PumpResearchOperatorDigestV1> {
    let source_metadata = fs::symlink_metadata(source)
        .with_context(|| format!("stat provenance source {}", source.display()))?;
    if !source_metadata.file_type().is_file() || source_metadata.file_type().is_symlink() {
        bail!(
            "provenance source {} must be a regular non-symlink file",
            source.display()
        );
    }
    let input = File::open(source)
        .with_context(|| format!("open provenance source {}", source.display()))?;
    let mut reader = BufReader::new(input);
    let mut output = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(destination)
        .with_context(|| format!("create provenance copy {}", destination.display()))?;
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut bytes = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader
            .read(&mut buffer)
            .with_context(|| format!("read provenance source {}", source.display()))?;
        if read == 0 {
            break;
        }
        output
            .write_all(&buffer[..read])
            .with_context(|| format!("write provenance copy {}", destination.display()))?;
        sha256.update(&buffer[..read]);
        blake3.update(&buffer[..read]);
        bytes = bytes
            .checked_add(read as u64)
            .ok_or_else(|| anyhow::anyhow!("provenance copy exceeds u64 byte counter"))?;
    }
    output
        .sync_all()
        .with_context(|| format!("sync provenance copy {}", destination.display()))?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(
            destination,
            fs::Permissions::from_mode(source_metadata.permissions().mode()),
        )
        .with_context(|| {
            format!(
                "preserve provenance source mode at {}",
                destination.display()
            )
        })?;
    }
    let parent = destination.parent().ok_or_else(|| {
        anyhow::anyhow!("provenance copy {} has no parent", destination.display())
    })?;
    sync_directory(parent)?;
    Ok(PumpResearchOperatorDigestV1 {
        sha256: format!("{:x}", sha256.finalize()),
        blake3: blake3.finalize().to_hex().to_string(),
        bytes,
    })
}

fn configured_operator_credential_scan_values(
    config: &PumpResearchCaptureConfigV1,
) -> Result<Vec<(String, Vec<u8>)>> {
    let mut values = Vec::<(String, Vec<u8>)>::new();
    for (kind, value) in [
        ("gRPC", config.resolve_grpc_auth_token()?),
        ("RPC", config.resolve_rpc_auth_token()?),
    ] {
        let Some(value) = value else {
            continue;
        };
        if value.is_empty() {
            bail!("configured {kind} credential unexpectedly resolved to an empty value");
        }
        if values
            .iter()
            .any(|(_, known_value)| known_value.as_slice() == value.as_bytes())
        {
            continue;
        }
        values.push((kind.to_owned(), value.into_bytes()));
    }
    Ok(values)
}

fn regular_file_contains_bytes(path: &Path, needle: &[u8]) -> Result<bool> {
    if needle.is_empty() {
        bail!("credential scan needle must not be empty");
    }
    ensure_regular_provenance_file(path)?;
    let file = File::open(path).with_context(|| {
        format!(
            "open sealed bundle artifact {} for credential scan",
            path.display()
        )
    })?;
    let mut reader = BufReader::new(file);
    let mut tail = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let read = reader.read(&mut buffer).with_context(|| {
            format!(
                "read sealed bundle artifact {} for credential scan",
                path.display()
            )
        })?;
        if read == 0 {
            return Ok(false);
        }
        let mut haystack = Vec::with_capacity(tail.len() + read);
        haystack.extend_from_slice(&tail);
        haystack.extend_from_slice(&buffer[..read]);
        if haystack
            .windows(needle.len())
            .any(|window| window == needle)
        {
            return Ok(true);
        }
        let keep = needle.len().saturating_sub(1).min(haystack.len());
        tail.clear();
        tail.extend_from_slice(&haystack[haystack.len() - keep..]);
    }
}

fn sealed_bundle_regular_files(bundle_dir: &Path) -> Result<Vec<PathBuf>> {
    let mut directories = vec![bundle_dir.to_path_buf()];
    let mut files = Vec::new();
    while let Some(directory) = directories.pop() {
        let metadata = fs::symlink_metadata(&directory)
            .with_context(|| format!("stat sealed bundle directory {}", directory.display()))?;
        if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
            bail!(
                "sealed bundle path {} must be a non-symlink directory",
                directory.display()
            );
        }
        let mut entries = fs::read_dir(&directory)
            .with_context(|| format!("read sealed bundle directory {}", directory.display()))?
            .collect::<std::result::Result<Vec<_>, _>>()
            .with_context(|| format!("iterate sealed bundle directory {}", directory.display()))?;
        entries.sort_by_key(|entry| entry.file_name());
        for entry in entries {
            let path = entry.path();
            let metadata = fs::symlink_metadata(&path)
                .with_context(|| format!("stat sealed bundle path {}", path.display()))?;
            if metadata.file_type().is_symlink() {
                bail!(
                    "sealed bundle path {} must not be a symlink",
                    path.display()
                );
            }
            if metadata.file_type().is_dir() {
                directories.push(path);
            } else if metadata.file_type().is_file() {
                files.push(path);
            } else {
                bail!(
                    "sealed bundle path {} must be a regular file or directory",
                    path.display()
                );
            }
        }
    }
    files.sort();
    Ok(files)
}

/// Defense in depth for the sanitized child environment: do not publish a
/// final preflight receipt if either configured provider credential occurs in
/// any materialized source/build/provenance artifact.  The diagnostic names
/// the artifact and credential class, never the secret bytes themselves.
fn ensure_credential_values_absent_from_sealed_bundle(
    bundle_dir: &Path,
    credentials: &[(String, Vec<u8>)],
) -> Result<()> {
    if credentials.is_empty() {
        return Ok(());
    }
    let files = sealed_bundle_regular_files(bundle_dir)?;
    for (credential_kind, credential) in credentials {
        for file in &files {
            if regular_file_contains_bytes(file, &credential)? {
                bail!(
                    "sealed operator preflight bundle contains configured {credential_kind} credential bytes in {}; final receipt is not published",
                    file.display()
                );
            }
        }
    }
    Ok(())
}

fn ensure_configured_credentials_absent_from_sealed_bundle(
    bundle_dir: &Path,
    config: &PumpResearchCaptureConfigV1,
) -> Result<()> {
    let credentials = configured_operator_credential_scan_values(config)?;
    ensure_credential_values_absent_from_sealed_bundle(bundle_dir, &credentials)
}

fn git_output_at(repository_root: &Path, arguments: &[&str]) -> Result<Vec<u8>> {
    let output = Command::new("git")
        .args(arguments)
        .current_dir(repository_root)
        .output()
        .with_context(|| format!("invoke git {} for operator provenance", arguments.join(" ")))?;
    if !output.status.success() {
        bail!(
            "git {} failed while collecting operator provenance",
            arguments.join(" ")
        );
    }
    Ok(output.stdout)
}

fn current_repository_root() -> Result<PathBuf> {
    let output = Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("invoke git rev-parse --show-toplevel for operator provenance")?;
    if !output.status.success() {
        bail!("operator preflight must run from inside the Git worktree")
    }
    let root = String::from_utf8(output.stdout)
        .context("git repository root is not valid UTF-8")?
        .trim()
        .to_owned();
    validate_trimmed("git_repository_root", &root)?;
    fs::canonicalize(&root).with_context(|| format!("canonicalize Git root {root}"))
}

/// The operator configuration is intentionally outside the Git worktree.
/// That makes the bundle's config contract unambiguous: only its digest and
/// redacted projection are persisted, never a raw TOML that may contain a
/// provider endpoint or unrelated local operator material.
fn validate_external_operator_config_path(
    repository_root: &Path,
    config_path: &Path,
) -> Result<PathBuf> {
    let metadata = fs::symlink_metadata(config_path).with_context(|| {
        format!(
            "stat external Pump Research operator config {}",
            config_path.display()
        )
    })?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!(
            "Pump Research operator config {} must be a regular non-symlink file outside the Git worktree",
            config_path.display()
        );
    }
    let canonical_config = fs::canonicalize(config_path).with_context(|| {
        format!(
            "canonicalize external Pump Research operator config {}",
            config_path.display()
        )
    })?;
    if canonical_config.starts_with(repository_root) {
        bail!(
            "Pump Research operator config must live outside the Git worktree; copy the tracked template to an operator-local protected path and pass that path to preflight/capture"
        );
    }
    Ok(canonical_config)
}

fn repository_commit_at(repository_root: &Path) -> Result<String> {
    let output = git_output_at(repository_root, &["rev-parse", "HEAD"])?;
    let commit = String::from_utf8(output)
        .context("git rev-parse HEAD returned invalid UTF-8")?
        .trim()
        .to_owned();
    validate_trimmed("repository_commit", &commit)?;
    Ok(commit)
}

fn repository_branch_at(repository_root: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args(["symbolic-ref", "--quiet", "--short", "HEAD"])
        .current_dir(repository_root)
        .output()
        .context("invoke git symbolic-ref for operator provenance")?;
    if output.status.code() == Some(1) {
        return Ok(None);
    }
    if !output.status.success() {
        bail!("git symbolic-ref failed while collecting operator provenance")
    }
    let branch = String::from_utf8(output.stdout)
        .context("git symbolic-ref returned invalid UTF-8")?
        .trim()
        .to_owned();
    validate_trimmed("repository_branch", &branch)?;
    Ok(Some(branch))
}

/// Resolve the direct Cargo/rustc binary selected by the repository's current
/// rustup toolchain.  Sealing only the rustup proxy would not bind the actual
/// executable bytes which compile the research binary.
fn resolve_executable_from_parent_path(program: &str, working_directory: &Path) -> Result<PathBuf> {
    let path = env::var_os("PATH")
        .ok_or_else(|| anyhow::anyhow!("PATH is unavailable while resolving {program}"))?;
    for directory in env::split_paths(&path) {
        if directory.as_os_str().is_empty() {
            continue;
        }
        let directory = if directory.is_absolute() {
            directory
        } else {
            env::current_dir()
                .context("resolve current directory while resolving operator toolchain")?
                .join(directory)
        };
        let candidate = directory.join(program);
        let metadata = match fs::metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("stat {program} candidate {}", candidate.display()))
            }
        };
        if !metadata.file_type().is_file() {
            continue;
        }
        let canonical_candidate = fs::canonicalize(&candidate)
            .with_context(|| format!("canonicalize {program} candidate {}", candidate.display()))?;
        let resolved = if canonical_candidate
            .file_name()
            .and_then(|name| name.to_str())
            == Some("rustup")
        {
            let output = Command::new(&canonical_candidate)
                .args(["which", program])
                .current_dir(working_directory)
                .output()
                .with_context(|| {
                    format!(
                        "resolve direct {program} through rustup {}",
                        canonical_candidate.display()
                    )
                })?;
            if !output.status.success() {
                bail!(
                    "rustup could not resolve direct {program} while collecting operator provenance"
                )
            }
            let value = String::from_utf8(output.stdout)
                .context("rustup which returned non-UTF-8 toolchain path")?
                .trim()
                .to_owned();
            if value.is_empty() {
                bail!("rustup resolved an empty direct {program} path")
            }
            fs::canonicalize(&value)
                .with_context(|| format!("canonicalize direct {program} path {value}"))?
        } else {
            canonical_candidate
        };
        let resolved_metadata = fs::symlink_metadata(&resolved)
            .with_context(|| format!("stat direct {program} executable {}", resolved.display()))?;
        if !resolved_metadata.file_type().is_file() || resolved_metadata.file_type().is_symlink() {
            bail!(
                "resolved direct {program} executable {} must be a regular non-symlink file",
                resolved.display()
            );
        }
        return Ok(resolved);
    }
    bail!("could not resolve regular executable {program} from PATH")
}

fn command_version_at(program: &Path, arguments: &[&str]) -> Result<String> {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .with_context(|| format!("invoke {} for operator provenance", program.display()))?;
    if !output.status.success() {
        bail!(
            "{} version command failed while collecting operator provenance",
            program.display()
        )
    }
    let version = String::from_utf8(output.stdout)
        .with_context(|| {
            format!(
                "{} version command returned invalid UTF-8",
                program.display()
            )
        })?
        .trim()
        .to_owned();
    validate_trimmed("operator_toolchain_version", &version)?;
    Ok(version)
}

fn checked_repository_relative_path(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path.components().any(|component| {
            matches!(
                component,
                Component::ParentDir | Component::RootDir | Component::Prefix(_)
            )
        })
    {
        bail!("Git provenance path is not a safe repository-relative path")
    }
    Ok(path.to_path_buf())
}

fn collect_untracked_inventory(
    repository_root: &Path,
) -> Result<(
    PumpResearchOperatorPreflightUntrackedInventoryV1,
    Vec<u8>,
    PumpResearchOperatorDigestV1,
)> {
    let output = git_output_at(
        repository_root,
        &["ls-files", "--others", "--exclude-standard", "-z"],
    )?;
    let mut entries = Vec::new();
    for raw_path in output
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
    {
        let repository_relative_path = std::str::from_utf8(raw_path)
            .context("operator provenance does not support non-UTF-8 untracked paths")?
            .to_owned();
        let relative_path = checked_repository_relative_path(&repository_relative_path)?;
        let absolute_path = repository_root.join(&relative_path);
        let metadata = fs::symlink_metadata(&absolute_path).with_context(|| {
            format!("stat untracked provenance file {}", absolute_path.display())
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            bail!(
                "untracked provenance path {} must be a regular non-symlink file",
                repository_relative_path
            );
        }
        entries.push(PumpResearchOperatorPreflightUntrackedEntryV1 {
            repository_relative_path,
            source_kind: PumpResearchOperatorSourceEntryKindV1::Untracked,
            digest: operator_digest_file(&absolute_path)?,
        });
    }
    let mut seen: BTreeSet<String> = entries
        .iter()
        .map(|entry| entry.repository_relative_path.clone())
        .collect();
    for required_path in OPERATOR_PREFLIGHT_REQUIRED_IGNORED_ARTIFACTS_V1 {
        let relative_path = checked_repository_relative_path(required_path)?;
        let absolute_path = repository_root.join(&relative_path);
        let metadata = fs::symlink_metadata(&absolute_path).with_context(|| {
            format!(
                "stat required ignored provenance fixture {}",
                absolute_path.display()
            )
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            bail!(
                "required ignored provenance fixture {required_path} must be a regular non-symlink file"
            );
        }
        let ignored = Command::new("git")
            .args(["check-ignore", "--quiet", "--no-index", "--", required_path])
            .current_dir(repository_root)
            .status()
            .with_context(|| format!("verify required ignored fixture {required_path}"))?;
        if !ignored.success() {
            bail!(
                "required provenance fixture {required_path} must be Git-ignored and explicitly listed by the operator contract"
            );
        }
        if !seen.insert((*required_path).to_owned()) {
            bail!(
                "required ignored provenance fixture {required_path} unexpectedly appeared in ordinary untracked inventory"
            );
        }
        entries.push(PumpResearchOperatorPreflightUntrackedEntryV1 {
            repository_relative_path: (*required_path).to_owned(),
            source_kind: PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture,
            digest: operator_digest_file(&absolute_path)?,
        });
    }
    entries.sort_by(|left, right| {
        left.repository_relative_path
            .cmp(&right.repository_relative_path)
    });
    let inventory = PumpResearchOperatorPreflightUntrackedInventoryV1 {
        schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
        entries,
    };
    let bytes = immutable_json_bytes(&inventory)?;
    let digest = operator_digest_bytes(&bytes);
    Ok((inventory, bytes, digest))
}

fn collect_source_snapshot_manifest(
    repository_root: &Path,
    untracked_inventory: &PumpResearchOperatorPreflightUntrackedInventoryV1,
) -> Result<PumpResearchOperatorSourceSnapshotManifestV1> {
    let tracked_output = git_output_at(repository_root, &["ls-files", "-z"])?;
    let mut entries = Vec::new();
    let mut paths = BTreeSet::new();
    for raw_path in tracked_output
        .split(|byte| *byte == b'\0')
        .filter(|path| !path.is_empty())
    {
        let repository_relative_path = std::str::from_utf8(raw_path)
            .context("operator provenance does not support non-UTF-8 tracked paths")?
            .to_owned();
        let relative_path = checked_repository_relative_path(&repository_relative_path)?;
        let absolute_path = repository_root.join(relative_path);
        let metadata = fs::symlink_metadata(&absolute_path).with_context(|| {
            format!(
                "stat tracked source-snapshot file {}",
                absolute_path.display()
            )
        })?;
        if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
            bail!(
                "tracked source-snapshot path {} must be an existing regular non-symlink file",
                repository_relative_path
            );
        }
        if !paths.insert(repository_relative_path.clone()) {
            bail!("duplicate tracked source-snapshot path {repository_relative_path}");
        }
        entries.push(PumpResearchOperatorSourceSnapshotEntryV1 {
            repository_relative_path,
            source_kind: PumpResearchOperatorSourceEntryKindV1::Tracked,
            digest: operator_digest_file(&absolute_path)?,
        });
    }
    for entry in &untracked_inventory.entries {
        if !paths.insert(entry.repository_relative_path.clone()) {
            bail!(
                "source-snapshot path {} appears in both tracked and untracked inventory",
                entry.repository_relative_path
            );
        }
        entries.push(PumpResearchOperatorSourceSnapshotEntryV1 {
            repository_relative_path: entry.repository_relative_path.clone(),
            source_kind: entry.source_kind.clone(),
            digest: entry.digest.clone(),
        });
    }
    entries.sort_by(|left, right| {
        left.repository_relative_path
            .cmp(&right.repository_relative_path)
    });
    Ok(PumpResearchOperatorSourceSnapshotManifestV1 {
        schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
        source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1.to_owned(),
        entries,
    })
}

fn build_redacted_config(
    config: &PumpResearchCaptureConfigV1,
    auth_presence: PumpResearchOperatorAuthPresenceV1,
) -> PumpResearchCaptureConfigRedactedV1 {
    PumpResearchCaptureConfigRedactedV1 {
        primary_provider_id: config.primary_provider_id.clone(),
        grpc_endpoint_digest: operator_digest_bytes(config.grpc_endpoint.as_bytes()),
        grpc_auth_token_env: config.grpc_auth_token_env.clone(),
        grpc_auth_token_present: auth_presence.grpc_token_present,
        grpc_auth_header: config.grpc_auth_header.clone(),
        rpc_endpoint_digest: operator_digest_bytes(config.rpc_endpoint.as_bytes()),
        rpc_auth_token_env: config.rpc_auth_token_env.clone(),
        rpc_auth_token_present: auth_presence.rpc_token_present,
        rpc_auth_header: config.rpc_auth_header.clone(),
        pump_program_id: config.pump_program_id.clone(),
        output_dir: config.output_dir.display().to_string(),
        required_for_run: config.required_for_run,
        queue_capacity: config.queue_capacity,
        flush_interval_ms: config.flush_interval_ms,
        segment_max_bytes: config.segment_max_bytes,
        segment_max_duration_ms: config.segment_max_duration_ms,
        record_max_bytes: config.record_max_bytes,
    }
}

fn collect_operator_preflight_material(
    config: &PumpResearchCaptureConfigV1,
    config_bytes: &[u8],
    auth_presence: PumpResearchOperatorAuthPresenceV1,
) -> Result<PumpResearchOperatorPreflightMaterialV1> {
    let repository_root = current_repository_root()?;
    let repository_commit = repository_commit_at(&repository_root)?;
    let repository_branch = repository_branch_at(&repository_root)?;
    let git_status_porcelain = git_output_at(
        &repository_root,
        &["status", "--porcelain=v1", "--untracked-files=all", "-z"],
    )?;
    let git_status_porcelain_digest = operator_digest_bytes(&git_status_porcelain);
    let tracked_worktree_patch = git_output_at(
        &repository_root,
        &["diff", "--binary", "--no-ext-diff", "HEAD", "--"],
    )?;
    let tracked_worktree_patch_digest = operator_digest_bytes(&tracked_worktree_patch);
    let (untracked_inventory, untracked_inventory_bytes, untracked_inventory_digest) =
        collect_untracked_inventory(&repository_root)?;
    let cargo_lock = repository_root.join("Cargo.lock");
    let cargo_lock_digest = operator_digest_file(&cargo_lock)
        .with_context(|| format!("hash Cargo.lock at {}", cargo_lock.display()))?;
    let cargo_executable_path = resolve_executable_from_parent_path("cargo", &repository_root)?;
    let cargo_executable_digest =
        operator_digest_file(&cargo_executable_path).with_context(|| {
            format!(
                "hash resolved Cargo executable {}",
                cargo_executable_path.display()
            )
        })?;
    let rustc_executable_path = resolve_executable_from_parent_path("rustc", &repository_root)?;
    let rustc_executable_digest =
        operator_digest_file(&rustc_executable_path).with_context(|| {
            format!(
                "hash resolved rustc executable {}",
                rustc_executable_path.display()
            )
        })?;
    let rustc_version = command_version_at(&rustc_executable_path, &["-Vv"])?;
    let cargo_version = command_version_at(&cargo_executable_path, &["-V"])?;
    let config_bytes_digest = operator_digest_bytes(config_bytes);
    let source_snapshot_manifest =
        collect_source_snapshot_manifest(&repository_root, &untracked_inventory)?;
    let source_snapshot_manifest_bytes = immutable_json_bytes(&source_snapshot_manifest)?;
    let source_snapshot_manifest_digest = operator_digest_bytes(&source_snapshot_manifest_bytes);
    let redacted_config = build_redacted_config(config, auth_presence);
    let redacted_config_bytes = immutable_json_bytes(&redacted_config)?;
    let redacted_config_digest = operator_digest_bytes(&redacted_config_bytes);
    let pending_build_digest = operator_digest_bytes(&[]);
    let fingerprint_input = PumpResearchOperatorPreflightFingerprintInputV1 {
        source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1,
        repository_commit: &repository_commit,
        repository_branch: &repository_branch,
        git_status_porcelain_digest: &git_status_porcelain_digest,
        tracked_worktree_patch_digest: &tracked_worktree_patch_digest,
        untracked_inventory_digest: &untracked_inventory_digest,
        source_snapshot_manifest_digest: &source_snapshot_manifest_digest,
        cargo_lock_digest: &cargo_lock_digest,
        // The final fingerprint is completed only after the fresh build.  A
        // non-persisted empty digest lets this collector serve the pre/post
        // build race check without claiming a source→binary relation.
        release_binary_digest: &pending_build_digest,
        build_receipt_digest: &pending_build_digest,
        build_log_digest: &pending_build_digest,
        build_environment_digest: &pending_build_digest,
        build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1,
        credential_scan_semantics: OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1,
        cargo_executable_digest: &cargo_executable_digest,
        rustc_executable_digest: &rustc_executable_digest,
        rustc_version: &rustc_version,
        cargo_version: &cargo_version,
        config_semantics: OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1,
        config_bytes_digest: &config_bytes_digest,
        redacted_config_digest: &redacted_config_digest,
    };
    let artifact_provenance_fingerprint = operator_digest_bytes(
        &serde_json::to_vec(&fingerprint_input)
            .context("serialize canonical operator provenance fingerprint")?,
    );
    Ok(PumpResearchOperatorPreflightMaterialV1 {
        repository_root,
        repository_commit,
        repository_branch,
        git_status_porcelain,
        git_status_porcelain_digest,
        tracked_worktree_patch,
        tracked_worktree_patch_digest,
        untracked_inventory,
        untracked_inventory_bytes,
        untracked_inventory_digest,
        source_snapshot_manifest,
        source_snapshot_manifest_bytes,
        source_snapshot_manifest_digest,
        cargo_lock_digest,
        cargo_executable_path,
        cargo_executable_digest,
        rustc_executable_path,
        rustc_executable_digest,
        rustc_version,
        cargo_version,
        config_bytes_digest,
        redacted_config_bytes,
        redacted_config_digest,
        artifact_provenance_fingerprint,
    })
}

fn ensure_operator_digest(
    label: &str,
    actual: &PumpResearchOperatorDigestV1,
    expected: &PumpResearchOperatorDigestV1,
) -> Result<()> {
    if actual != expected {
        bail!("operator provenance mismatch for {label}")
    }
    Ok(())
}

fn create_operator_preflight_bundle_dir(
    repository_root: &Path,
    requested_output_dir: &Path,
) -> Result<PathBuf> {
    if requested_output_dir.as_os_str().is_empty() {
        bail!("operator preflight output directory must not be empty")
    }
    let working_directory =
        env::current_dir().context("resolve operator preflight working directory")?;
    let absolute_request = if requested_output_dir.is_absolute() {
        requested_output_dir.to_path_buf()
    } else {
        working_directory.join(requested_output_dir)
    };
    if absolute_request
        .components()
        .any(|component| matches!(component, Component::ParentDir))
    {
        bail!(
            "operator preflight output must not contain `..`; choose an explicit ignored worktree path or a path outside the worktree"
        )
    }
    // Check the lexical requested path *before* creating a parent directory.
    // Otherwise a failed request such as `untracked/output` would itself dirty
    // the worktree before the preflight can fail closed.
    if absolute_request.starts_with(repository_root) {
        let relative = absolute_request
            .strip_prefix(repository_root)
            .context("derive Git-relative requested preflight output path")?;
        let relative = relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("operator preflight output is not UTF-8"))?;
        let status = Command::new("git")
            .args(["check-ignore", "--quiet", "--no-index", "--", relative])
            .current_dir(repository_root)
            .status()
            .context("check whether requested operator preflight output is Git-ignored")?;
        if !status.success() {
            bail!(
                "operator preflight output inside the worktree must be Git-ignored so creating the bundle cannot alter the captured source tree"
            );
        }
    }
    let parent = absolute_request.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "operator preflight output {} has no parent directory",
            requested_output_dir.display()
        )
    })?;
    let name = absolute_request.file_name().ok_or_else(|| {
        anyhow::anyhow!(
            "operator preflight output {} must name a new directory",
            requested_output_dir.display()
        )
    })?;
    if name.is_empty() || name == "." || name == ".." {
        bail!("operator preflight output must name a concrete new directory")
    }
    fs::create_dir_all(parent).with_context(|| {
        format!(
            "create operator preflight output parent {}",
            parent.display()
        )
    })?;
    let canonical_parent = fs::canonicalize(parent).with_context(|| {
        format!(
            "canonicalize operator preflight output parent {}",
            parent.display()
        )
    })?;
    let bundle_dir = canonical_parent.join(name);
    if bundle_dir.exists() {
        bail!(
            "operator preflight output {} already exists; immutable bundles are create-new only",
            bundle_dir.display()
        );
    }
    if bundle_dir.starts_with(repository_root) {
        let relative = bundle_dir
            .strip_prefix(repository_root)
            .context("derive Git-relative operator preflight output path")?;
        let relative = relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("operator preflight output is not UTF-8"))?;
        let status = Command::new("git")
            .args(["check-ignore", "--quiet", "--no-index", "--", relative])
            .current_dir(repository_root)
            .status()
            .context("re-check operator preflight output ignore status")?;
        if !status.success() {
            bail!("canonical operator preflight output inside the worktree is not Git-ignored");
        }
    }
    fs::create_dir(&bundle_dir).with_context(|| {
        format!(
            "create immutable operator preflight bundle {}",
            bundle_dir.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&bundle_dir, fs::Permissions::from_mode(0o700)).with_context(|| {
            format!(
                "restrict immutable operator preflight bundle permissions {}",
                bundle_dir.display()
            )
        })?;
    }
    sync_directory(&canonical_parent)?;
    Ok(bundle_dir)
}

fn create_preflight_directory(parent: &Path, relative: &Path) -> Result<PathBuf> {
    let relative = checked_repository_relative_path(
        relative
            .to_str()
            .ok_or_else(|| anyhow::anyhow!("provenance directory path is not UTF-8"))?,
    )?;
    let directory = parent.join(relative);
    fs::create_dir_all(&directory)
        .with_context(|| format!("create provenance directory {}", directory.display()))?;
    sync_directory(&directory)?;
    Ok(directory)
}

fn snapshot_full_source_tree(
    material: &PumpResearchOperatorPreflightMaterialV1,
    bundle_dir: &Path,
) -> Result<PathBuf> {
    snapshot_source_tree_from_manifest(
        &material.repository_root,
        &material.source_snapshot_manifest,
        bundle_dir,
    )
}

fn snapshot_source_tree_from_manifest(
    repository_root: &Path,
    manifest: &PumpResearchOperatorSourceSnapshotManifestV1,
    bundle_dir: &Path,
) -> Result<PathBuf> {
    let snapshot_root = create_preflight_directory(
        bundle_dir,
        Path::new(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1),
    )?;
    for entry in &manifest.entries {
        let relative_path = checked_repository_relative_path(&entry.repository_relative_path)?;
        let source = repository_root.join(&relative_path);
        let destination = snapshot_root.join(&relative_path);
        let destination_parent = destination.parent().ok_or_else(|| {
            anyhow::anyhow!("source snapshot destination has no parent directory")
        })?;
        fs::create_dir_all(destination_parent).with_context(|| {
            format!(
                "create source snapshot parent {}",
                destination_parent.display()
            )
        })?;
        let copied_digest = copy_file_create_new_with_digest(&source, &destination)?;
        ensure_operator_digest(
            &format!("source snapshot file {}", entry.repository_relative_path),
            &copied_digest,
            &entry.digest,
        )?;
    }
    sync_directory(&snapshot_root)?;
    Ok(snapshot_root)
}

fn verify_source_snapshot_contents(
    bundle_dir: &Path,
    manifest: &PumpResearchOperatorSourceSnapshotManifestV1,
) -> Result<PumpResearchOperatorDigestV1> {
    verify_source_snapshot_root_contents(
        &bundle_dir.join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1),
        manifest,
    )
}

/// Verify a materialised copy of the sealed source snapshot. The canonical
/// bundle snapshot and the short-lived isolated build staging tree both use
/// this exact verifier; a Cargo build is never allowed to read directly from
/// the bundle hierarchy.
fn verify_source_snapshot_root_contents(
    snapshot_root: &Path,
    manifest: &PumpResearchOperatorSourceSnapshotManifestV1,
) -> Result<PumpResearchOperatorDigestV1> {
    if manifest.schema_version != OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1
        || manifest.source_tree_semantics != OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1
    {
        bail!("operator preflight source snapshot manifest has an unsupported contract");
    }
    let metadata = fs::symlink_metadata(snapshot_root)
        .with_context(|| format!("stat source snapshot root {}", snapshot_root.display()))?;
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        bail!("operator preflight source snapshot root must be a directory, never a symlink");
    }
    let mut paths = BTreeSet::new();
    let mut required_ignored = BTreeSet::new();
    for entry in &manifest.entries {
        let relative_path = checked_repository_relative_path(&entry.repository_relative_path)?;
        if !paths.insert(entry.repository_relative_path.clone()) {
            bail!(
                "operator preflight source snapshot manifest has duplicate path {}",
                entry.repository_relative_path
            );
        }
        let file = snapshot_root.join(relative_path);
        ensure_regular_provenance_file(&file)?;
        ensure_operator_digest(
            "source snapshot file",
            &operator_digest_file(&file)?,
            &entry.digest,
        )?;
        if entry.source_kind == PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture {
            required_ignored.insert(entry.repository_relative_path.clone());
        }
    }
    let expected_required_ignored: BTreeSet<String> =
        OPERATOR_PREFLIGHT_REQUIRED_IGNORED_ARTIFACTS_V1
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
    if required_ignored != expected_required_ignored {
        bail!("operator preflight source snapshot lacks a required ignored fixture");
    }
    Ok(operator_digest_bytes(&immutable_json_bytes(manifest)?))
}

fn optional_regular_file_digest(path: &Path) -> Result<Option<PumpResearchOperatorDigestV1>> {
    if !path.exists() {
        return Ok(None);
    }
    ensure_regular_provenance_file(path)?;
    Ok(Some(operator_digest_file(path)?))
}

fn rejected_fresh_build_environment_overrides<I>(environment: I) -> Vec<String>
where
    I: IntoIterator<Item = (OsString, OsString)>,
{
    let environment: BTreeMap<OsString, OsString> = environment.into_iter().collect();
    let mut rejected = BTreeSet::new();
    for name in OPERATOR_PREFLIGHT_FRESH_BUILD_DENIED_ENV_V1 {
        if environment
            .get(&OsString::from(name))
            .is_some_and(|value| !value.is_empty())
        {
            rejected.insert((*name).to_owned());
        }
    }
    for (name, value) in &environment {
        let name = name.to_string_lossy();
        if name.starts_with("CARGO_PROFILE_RELEASE_") && !value.is_empty() {
            rejected.insert(name.into_owned());
        }
    }
    rejected.into_iter().collect()
}

/// A sealed preflight must not invoke Cargo with a caller-selected compiler,
/// wrapper, flags or release-profile override.  These settings are rejected
/// rather than merely hashed because their executable and semantic closure is
/// not part of the V1 operator bundle.
fn validate_fresh_build_parent_environment() -> Result<()> {
    let rejected = rejected_fresh_build_environment_overrides(env::vars_os());
    if rejected.is_empty() {
        return Ok(());
    }
    bail!(
        "operator preflight rejects unsealed compiler/build environment overrides: {}",
        rejected.join(", ")
    )
}

fn collect_build_environment(
    source_snapshot_root: &Path,
    cargo_executable_digest: PumpResearchOperatorDigestV1,
    rustc_executable_digest: PumpResearchOperatorDigestV1,
) -> Result<PumpResearchOperatorBuildEnvironmentV1> {
    // `validate_fresh_build_parent_environment` ran before this collector.
    // The recorded environment therefore describes the child we create below,
    // not a set of inherited settings that Cargo could silently consume.
    let cargo_build_environment_digests = BTreeMap::new();
    let cargo_profile_release_environment_digests = BTreeMap::new();
    let cargo_config_file_digests =
        validate_sealed_snapshot_cargo_configuration(source_snapshot_root)?;
    Ok(PumpResearchOperatorBuildEnvironmentV1 {
        child_environment_semantics: OPERATOR_PREFLIGHT_SANITIZED_BUILD_ENVIRONMENT_V1.to_owned(),
        build_staging_semantics: OPERATOR_PREFLIGHT_ISOLATED_BUILD_STAGING_SEMANTICS_V1.to_owned(),
        cargo_target_dir_semantics:
            "fresh_create_new_isolated_staging_target_directory_not_reused_or_persisted_v2"
                .to_owned(),
        cargo_home_semantics: OPERATOR_PREFLIGHT_SANITIZED_CARGO_HOME_V1.to_owned(),
        cargo_config_scope_semantics: OPERATOR_PREFLIGHT_ISOLATED_CARGO_CONFIG_SCOPE_V1.to_owned(),
        cargo_executable_digest,
        rustc_executable_digest,
        rustflags_digest: None,
        cargo_encoded_rustflags_digest: None,
        cargo_build_environment_digests,
        cargo_profile_release_environment_digests,
        cargo_home_digest: None,
        cargo_config_file_digests,
    })
}

/// Cargo's normal config lookup walks every ancestor of its current working
/// directory. The preflight therefore builds only from a create-new staging
/// root and rejects any config outside the materialised source snapshot. A
/// digest is not a sufficient closure for an external config: it could point
/// at an executable whose bytes are not part of the sealed source/toolchain.
fn ensure_no_cargo_config_outside_snapshot(source_snapshot_root: &Path) -> Result<()> {
    let mut ancestor = source_snapshot_root.parent();
    while let Some(directory) = ancestor {
        for name in ["config.toml", "config"] {
            let candidate = directory.join(".cargo").join(name);
            if candidate.exists() {
                ensure_regular_provenance_file(&candidate)?;
                bail!(
                    "isolated fresh build staging ancestor contains forbidden Cargo config {}",
                    candidate.display()
                );
            }
        }
        ancestor = directory.parent();
    }
    Ok(())
}

fn ensure_only_cargo_config_keys(
    path: &Path,
    scope: &str,
    table: &toml::map::Map<String, toml::Value>,
    allowed: &[&str],
) -> Result<()> {
    let unsupported: Vec<&str> = table
        .keys()
        .map(String::as_str)
        .filter(|key| !allowed.contains(key))
        .collect();
    if unsupported.is_empty() {
        return Ok(());
    }
    bail!(
        "sealed Cargo config {} contains unsupported {scope} key(s): {}",
        path.display(),
        unsupported.join(", ")
    )
}

fn validate_snapshot_cargo_config_strict_allowlist(
    path: &Path,
    config: &toml::Value,
) -> Result<()> {
    let Some(root) = config.as_table() else {
        bail!(
            "sealed Cargo config {} must be a TOML table",
            path.display()
        );
    };

    // This is an allowlist, not a denylist. Cargo adds configuration surfaces
    // over time, and any newly understood table or key must fail closed until a
    // later provenance version explicitly admits it.
    ensure_only_cargo_config_keys(path, "top-level", root, &["build", "profile"])?;

    if let Some(build_value) = root.get("build") {
        let Some(build) = build_value.as_table() else {
            bail!(
                "sealed Cargo config {} requires [build] to be a table",
                path.display()
            );
        };
        ensure_only_cargo_config_keys(path, "build", build, &["jobs", "rustflags"])?;

        if build
            .get("jobs")
            .is_some_and(|jobs| jobs.as_integer() != Some(4))
        {
            bail!(
                "sealed Cargo config {} only admits build.jobs = 4",
                path.display()
            );
        }

        if let Some(rustflags) = build.get("rustflags") {
            let approved = rustflags.as_array().is_some_and(|flags| {
                flags.len() == 2
                    && flags[0].as_str() == Some("-C")
                    && flags[1].as_str() == Some("target-cpu=native")
            });
            if !approved {
                bail!(
                    "sealed Cargo config {} only admits build.rustflags = [\"-C\", \"target-cpu=native\"]",
                    path.display()
                );
            }
        }
    }

    if let Some(profile_value) = root.get("profile") {
        let Some(profile) = profile_value.as_table() else {
            bail!(
                "sealed Cargo config {} requires [profile] to be a table",
                path.display()
            );
        };
        ensure_only_cargo_config_keys(path, "profile", profile, &["release"])?;

        if let Some(release_value) = profile.get("release") {
            let Some(release) = release_value.as_table() else {
                bail!(
                    "sealed Cargo config {} requires [profile.release] to be a table",
                    path.display()
                );
            };
            ensure_only_cargo_config_keys(
                path,
                "profile.release",
                release,
                &["opt-level", "lto", "codegen-units"],
            )?;

            for (key, expected) in [("opt-level", 3_i64), ("codegen-units", 4_i64)] {
                if release
                    .get(key)
                    .is_some_and(|value| value.as_integer() != Some(expected))
                {
                    bail!(
                        "sealed Cargo config {} only admits profile.release.{key} = {expected}",
                        path.display()
                    );
                }
            }
            if release
                .get("lto")
                .is_some_and(|value| value.as_bool() != Some(true))
            {
                bail!(
                    "sealed Cargo config {} only admits profile.release.lto = true",
                    path.display()
                );
            }
        }
    }
    Ok(())
}

/// Validate the only Cargo configuration Cargo is allowed to observe: config
/// files inside the verified snapshot itself. The accepted schema is closed:
/// only the current repository's exact rustflags/jobs/release-profile values
/// are admitted. Every unknown table, key or value fails before Cargo so a
/// sealed flag string cannot select an unsealed linker, object, sysroot,
/// target specification or other external build input.
fn validate_sealed_snapshot_cargo_configuration(
    source_snapshot_root: &Path,
) -> Result<BTreeMap<String, Option<PumpResearchOperatorDigestV1>>> {
    ensure_no_cargo_config_outside_snapshot(source_snapshot_root)?;
    let config_toml = source_snapshot_root.join(".cargo/config.toml");
    let config_legacy = source_snapshot_root.join(".cargo/config");
    if config_toml.exists() && config_legacy.exists() {
        bail!(
            "sealed source snapshot contains both .cargo/config.toml and .cargo/config; Cargo config precedence is intentionally not admitted"
        );
    }
    let mut digests = BTreeMap::new();
    for (label, path) in [
        ("sealed_snapshot/.cargo/config.toml", config_toml),
        ("sealed_snapshot/.cargo/config", config_legacy),
    ] {
        let digest = optional_regular_file_digest(&path)?;
        if digest.is_some() {
            let bytes = fs::read(&path)
                .with_context(|| format!("read sealed Cargo config {}", path.display()))?;
            let text = std::str::from_utf8(&bytes)
                .with_context(|| format!("sealed Cargo config {} is not UTF-8", path.display()))?;
            let parsed: toml::Value = toml::from_str(text)
                .with_context(|| format!("parse sealed Cargo config {}", path.display()))?;
            validate_snapshot_cargo_config_strict_allowlist(&path, &parsed)?;
        }
        digests.insert(label.to_owned(), digest);
    }
    Ok(digests)
}

fn operator_parent_cargo_home() -> Result<PathBuf> {
    let configured = env::var_os("CARGO_HOME")
        .map(PathBuf::from)
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".cargo")))
        .ok_or_else(|| anyhow::anyhow!("neither CARGO_HOME nor HOME is available"))?;
    let canonical = fs::canonicalize(&configured)
        .with_context(|| format!("canonicalize parent CARGO_HOME {}", configured.display()))?;
    let metadata = fs::metadata(&canonical)
        .with_context(|| format!("stat parent CARGO_HOME {}", canonical.display()))?;
    if !metadata.is_dir() {
        bail!(
            "parent CARGO_HOME {} is not a directory",
            canonical.display()
        );
    }
    Ok(canonical)
}

#[cfg(unix)]
fn symlink_offline_cargo_cache(source: &Path, destination: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(source) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => {
            return Err(error)
                .with_context(|| format!("stat offline Cargo cache {}", source.display()))
        }
    };
    if !metadata.file_type().is_dir() || metadata.file_type().is_symlink() {
        bail!(
            "offline Cargo cache {} must be a regular directory",
            source.display()
        );
    }
    std::os::unix::fs::symlink(source, destination).with_context(|| {
        format!(
            "link offline Cargo cache {} into sanitized CARGO_HOME {}",
            source.display(),
            destination.display()
        )
    })
}

#[cfg(not(unix))]
fn symlink_offline_cargo_cache(_source: &Path, _destination: &Path) -> Result<()> {
    bail!("sealed offline Pump Research preflight currently requires Unix cache isolation")
}

/// Construct a fresh CARGO_HOME without credentials or user configuration.
/// Only immutable-ish offline dependency inputs are made available: registry
/// cache/index and Git object databases.  Registry source trees and Git
/// checkouts are deliberately *not* reused, so Cargo reconstructs them inside
/// this new home from its cached source artifacts.
fn create_sanitized_cargo_home(staging_root: &Path) -> Result<PathBuf> {
    let cargo_home = staging_root.join("cargo-home");
    fs::create_dir(&cargo_home)
        .with_context(|| format!("create sanitized CARGO_HOME {}", cargo_home.display()))?;
    let parent_cargo_home = operator_parent_cargo_home()?;

    let registry = cargo_home.join("registry");
    fs::create_dir(&registry)
        .with_context(|| format!("create sanitized Cargo registry {}", registry.display()))?;
    symlink_offline_cargo_cache(
        &parent_cargo_home.join("registry/index"),
        &registry.join("index"),
    )?;
    symlink_offline_cargo_cache(
        &parent_cargo_home.join("registry/cache"),
        &registry.join("cache"),
    )?;

    let git = cargo_home.join("git");
    fs::create_dir(&git)
        .with_context(|| format!("create sanitized Cargo git cache {}", git.display()))?;
    symlink_offline_cargo_cache(&parent_cargo_home.join("git/db"), &git.join("db"))?;
    Ok(cargo_home)
}

fn controlled_fresh_build_path(cargo: &Path, rustc: &Path) -> Result<OsString> {
    let mut directories = Vec::<PathBuf>::new();
    for executable in [cargo, rustc] {
        let parent = executable.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "resolved toolchain executable {} has no parent directory",
                executable.display()
            )
        })?;
        if !directories.iter().any(|directory| directory == parent) {
            directories.push(parent.to_path_buf());
        }
    }
    for system_directory in [Path::new("/usr/bin"), Path::new("/bin")] {
        if system_directory.is_dir()
            && !directories
                .iter()
                .any(|directory| directory == system_directory)
        {
            directories.push(system_directory.to_path_buf());
        }
    }
    env::join_paths(directories).context("construct controlled PATH for sealed Cargo build")
}

struct PumpResearchSanitizedFreshBuildEnvironmentV1 {
    variables: BTreeMap<OsString, OsString>,
}

fn sanitized_fresh_build_environment(
    staging_root: &Path,
    cargo_target_dir: &Path,
    cargo_executable: &Path,
    rustc_executable: &Path,
) -> Result<PumpResearchSanitizedFreshBuildEnvironmentV1> {
    let cargo_home = create_sanitized_cargo_home(staging_root)?;
    let home = staging_root.join("home");
    fs::create_dir(&home).with_context(|| format!("create sanitized HOME {}", home.display()))?;
    let mut variables = BTreeMap::new();
    variables.insert(
        OsString::from("PATH"),
        controlled_fresh_build_path(cargo_executable, rustc_executable)?,
    );
    variables.insert(OsString::from("HOME"), home.into_os_string());
    variables.insert(OsString::from("CARGO_HOME"), cargo_home.into_os_string());
    variables.insert(
        OsString::from("CARGO_TARGET_DIR"),
        cargo_target_dir.as_os_str().to_owned(),
    );
    variables.insert(OsString::from("CARGO_NET_OFFLINE"), OsString::from("true"));
    variables.insert(OsString::from("CARGO_TERM_COLOR"), OsString::from("never"));
    Ok(PumpResearchSanitizedFreshBuildEnvironmentV1 { variables })
}

fn create_fresh_build_staging_root() -> Result<PathBuf> {
    let parent = env::temp_dir();
    for attempt in 0_u32..10_000 {
        let path = parent.join(format!(
            "pump-research-preflight-staging-{}-{}-{attempt}",
            wall_clock_ms(),
            std::process::id()
        ));
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(error) => {
                return Err(error).with_context(|| {
                    format!(
                        "create fresh isolated operator build staging root {}",
                        path.display()
                    )
                })
            }
        }
    }
    bail!("could not allocate a fresh isolated operator build staging root")
}

fn remove_fresh_build_staging_root(path: &Path) {
    // This directory was created by `create_fresh_build_staging_root` in the
    // system temporary directory during this exact preflight invocation.  It
    // is never a user-specified or repository path.
    if path.parent() == Some(env::temp_dir().as_path())
        && path
            .file_name()
            .and_then(|name| name.to_str())
            .is_some_and(|name| name.starts_with("pump-research-preflight-staging-"))
    {
        if let Err(error) = fs::remove_dir_all(path) {
            warn!(path = %path.display(), error = %error, "could not remove self-created fresh operator build target directory");
        }
    }
}

struct PumpResearchFreshBuildStagingRootV1 {
    path: PathBuf,
    source_root: PathBuf,
    cargo_target_dir: PathBuf,
}

impl PumpResearchFreshBuildStagingRootV1 {
    fn create_from_sealed_snapshot(
        bundle_dir: &Path,
        manifest: &PumpResearchOperatorSourceSnapshotManifestV1,
    ) -> Result<Self> {
        let path = create_fresh_build_staging_root()?;
        let source_root = path.join("source");
        let cargo_target_dir = path.join("target");
        let creation = (|| -> Result<()> {
            fs::create_dir(&source_root).with_context(|| {
                format!(
                    "create isolated build source root {}",
                    source_root.display()
                )
            })?;
            fs::create_dir(&cargo_target_dir).with_context(|| {
                format!(
                    "create isolated build target root {}",
                    cargo_target_dir.display()
                )
            })?;
            let sealed_snapshot_root = bundle_dir.join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1);
            for entry in &manifest.entries {
                let relative_path =
                    checked_repository_relative_path(&entry.repository_relative_path)?;
                let source = sealed_snapshot_root.join(&relative_path);
                let destination = source_root.join(&relative_path);
                let parent = destination.parent().ok_or_else(|| {
                    anyhow::anyhow!("isolated build source destination has no parent")
                })?;
                fs::create_dir_all(parent).with_context(|| {
                    format!("create isolated build source parent {}", parent.display())
                })?;
                let copied_digest = copy_file_create_new_with_digest(&source, &destination)?;
                ensure_operator_digest(
                    &format!(
                        "isolated build source file {}",
                        entry.repository_relative_path
                    ),
                    &copied_digest,
                    &entry.digest,
                )?;
            }
            ensure_operator_digest(
                "isolated build source snapshot contents",
                &verify_source_snapshot_root_contents(&source_root, manifest)?,
                &operator_digest_bytes(&immutable_json_bytes(manifest)?),
            )?;
            ensure_no_cargo_config_outside_snapshot(&source_root)?;
            Ok(())
        })();
        if let Err(error) = creation {
            remove_fresh_build_staging_root(&path);
            return Err(error);
        }
        Ok(Self {
            path,
            source_root,
            cargo_target_dir,
        })
    }
}

impl Drop for PumpResearchFreshBuildStagingRootV1 {
    fn drop(&mut self) {
        remove_fresh_build_staging_root(&self.path);
    }
}

struct PumpResearchFreshBuildOutputV1 {
    fresh_staging_root: PumpResearchFreshBuildStagingRootV1,
    release_binary_path: PathBuf,
    release_binary_digest: PumpResearchOperatorDigestV1,
    build_log_bytes: Vec<u8>,
    build_environment_bytes: Vec<u8>,
    build_receipt: PumpResearchOperatorBuildReceiptV1,
}

fn build_fresh_release_from_source_snapshot(
    bundle_dir: &Path,
    material: &PumpResearchOperatorPreflightMaterialV1,
    config: &PumpResearchCaptureConfigV1,
) -> Result<PumpResearchFreshBuildOutputV1> {
    let source_snapshot_before =
        verify_source_snapshot_contents(bundle_dir, &material.source_snapshot_manifest)?;
    ensure_operator_digest(
        "source snapshot manifest before build",
        &source_snapshot_before,
        &material.source_snapshot_manifest_digest,
    )?;
    let staging_root = PumpResearchFreshBuildStagingRootV1::create_from_sealed_snapshot(
        bundle_dir,
        &material.source_snapshot_manifest,
    )?;
    ensure_operator_digest(
        "isolated build source snapshot before build",
        &verify_source_snapshot_root_contents(
            &staging_root.source_root,
            &material.source_snapshot_manifest,
        )?,
        &material.source_snapshot_manifest_digest,
    )?;
    let build_environment = collect_build_environment(
        &staging_root.source_root,
        material.cargo_executable_digest.clone(),
        material.rustc_executable_digest.clone(),
    )?;
    let build_environment_bytes = immutable_json_bytes(&build_environment)?;
    let build_environment_digest = operator_digest_bytes(&build_environment_bytes);
    let command = vec![
        "cargo".to_owned(),
        "build".to_owned(),
        "--locked".to_owned(),
        "--offline".to_owned(),
        "--release".to_owned(),
        "-p".to_owned(),
        "seer".to_owned(),
        "--bin".to_owned(),
        "pump-research-tape".to_owned(),
    ];
    let started_wall_ms = wall_clock_ms();
    let sanitized_environment = sanitized_fresh_build_environment(
        &staging_root.path,
        &staging_root.cargo_target_dir,
        &material.cargo_executable_path,
        &material.rustc_executable_path,
    )?;
    let mut cargo = Command::new(&material.cargo_executable_path);
    cargo
        .args(&command[1..])
        .current_dir(&staging_root.source_root)
        // The preflight process can legitimately hold the source and RPC
        // credentials for later provider I/O.  Cargo, rustc and build scripts
        // must never inherit them.  Start from zero and supply only the
        // controlled build inputs above.
        .env_clear()
        .envs(&sanitized_environment.variables);
    // This is redundant after `env_clear`, intentionally explicit, and keeps
    // the named operator-secret policy visible at the call site even if the
    // child-environment construction changes later.
    for name in [
        config.grpc_auth_token_env.as_deref(),
        config.rpc_auth_token_env.as_deref(),
    ]
    .into_iter()
    .flatten()
    {
        cargo.env_remove(name);
    }
    let output = cargo
        .output()
        .context("run fresh locked offline release build from sealed source snapshot");
    let completed_wall_ms = wall_clock_ms();
    let output = match output {
        Ok(output) => output,
        Err(error) => return Err(error),
    };
    let mut build_log_bytes = Vec::new();
    build_log_bytes.extend_from_slice(b"[stdout]\n");
    build_log_bytes.extend_from_slice(&output.stdout);
    build_log_bytes.extend_from_slice(b"\n[stderr]\n");
    build_log_bytes.extend_from_slice(&output.stderr);
    if !output.status.success() {
        bail!(
            "fresh locked offline release build from sealed source snapshot failed with status {}",
            output.status
        );
    }
    let release_binary_path = staging_root
        .cargo_target_dir
        .join("release")
        .join("pump-research-tape");
    let release_binary_digest = operator_digest_file(&release_binary_path).with_context(|| {
        format!(
            "hash fresh sealed release binary {}",
            release_binary_path.display()
        )
    })?;
    let source_snapshot_after =
        verify_source_snapshot_contents(bundle_dir, &material.source_snapshot_manifest)?;
    ensure_operator_digest(
        "source snapshot manifest after build",
        &source_snapshot_after,
        &material.source_snapshot_manifest_digest,
    )?;
    ensure_operator_digest(
        "isolated build source snapshot after build",
        &verify_source_snapshot_root_contents(
            &staging_root.source_root,
            &material.source_snapshot_manifest,
        )?,
        &material.source_snapshot_manifest_digest,
    )?;
    let build_environment_after = collect_build_environment(
        &staging_root.source_root,
        material.cargo_executable_digest.clone(),
        material.rustc_executable_digest.clone(),
    )?;
    ensure_operator_digest(
        "fresh build environment after build",
        &operator_digest_bytes(&immutable_json_bytes(&build_environment_after)?),
        &build_environment_digest,
    )?;
    let build_log_digest = operator_digest_bytes(&build_log_bytes);
    let build_receipt = PumpResearchOperatorBuildReceiptV1 {
        schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
        build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
        cargo_command: command,
        cargo_profile: "release".to_owned(),
        source_snapshot_manifest_digest_before_build: source_snapshot_before,
        source_snapshot_manifest_digest_after_build: source_snapshot_after,
        cargo_lock_digest: material.cargo_lock_digest.clone(),
        build_environment_digest,
        build_log_digest,
        cargo_executable_digest: material.cargo_executable_digest.clone(),
        rustc_executable_digest: material.rustc_executable_digest.clone(),
        rustc_version: material.rustc_version.clone(),
        cargo_version: material.cargo_version.clone(),
        release_binary_digest: release_binary_digest.clone(),
        build_started_wall_ms: started_wall_ms,
        build_completed_wall_ms: completed_wall_ms,
    };
    // The binary must be copied before the fresh target is removed.  The
    // caller owns this path until it has made that immutable copy.
    Ok(PumpResearchFreshBuildOutputV1 {
        fresh_staging_root: staging_root,
        release_binary_path,
        release_binary_digest,
        build_log_bytes,
        build_environment_bytes,
        build_receipt,
    })
}

fn receipt_artifact_fingerprint(
    receipt: &PumpResearchOperatorPreflightReceiptV1,
) -> Result<PumpResearchOperatorDigestV1> {
    let input = PumpResearchOperatorPreflightFingerprintInputV1 {
        source_tree_semantics: &receipt.source_tree_semantics,
        repository_commit: &receipt.repository_commit,
        repository_branch: &receipt.repository_branch,
        git_status_porcelain_digest: &receipt.git_status_porcelain_digest,
        tracked_worktree_patch_digest: &receipt.tracked_worktree_patch_digest,
        untracked_inventory_digest: &receipt.untracked_inventory_digest,
        source_snapshot_manifest_digest: &receipt.source_snapshot_manifest_digest,
        cargo_lock_digest: &receipt.cargo_lock_digest,
        release_binary_digest: &receipt.release_binary_digest,
        build_receipt_digest: &receipt.build_receipt_digest,
        build_log_digest: &receipt.build_log_digest,
        build_environment_digest: &receipt.build_environment_digest,
        build_semantics: &receipt.build_semantics,
        credential_scan_semantics: &receipt.credential_scan_semantics,
        cargo_executable_digest: &receipt.cargo_executable_digest,
        rustc_executable_digest: &receipt.rustc_executable_digest,
        rustc_version: &receipt.rustc_version,
        cargo_version: &receipt.cargo_version,
        config_semantics: &receipt.config_semantics,
        config_bytes_digest: &receipt.config_bytes_digest,
        redacted_config_digest: &receipt.redacted_config_digest,
    };
    Ok(operator_digest_bytes(&serde_json::to_vec(&input).context(
        "serialize operator preflight receipt fingerprint",
    )?))
}

/// Create the immutable release/source provenance bundle that must be
/// validated by a later `capture` invocation.  This command is local-only: it
/// performs no Yellowstone or RPC request and never writes credential values.
pub fn create_operator_preflight_from_config_path(
    config_path: &Path,
    output_dir: &Path,
) -> Result<PumpResearchOperatorPreflightSummaryV1> {
    require_non_debug_operator_bootstrap_binary()?;
    let repository_root = current_repository_root()?;
    validate_external_operator_config_path(&repository_root, config_path)?;
    let (config, config_bytes) = PumpResearchCaptureConfigV1::load(config_path)?;
    let auth_presence = validate_operator_preflight_config(&config)?;
    // Reject parent-process build controls before creating a bundle.  The
    // sealed build may use only the explicit child environment constructed
    // below; an override cannot be retroactively made safe by merely hashing
    // its string value.
    validate_fresh_build_parent_environment()?;
    let material = collect_operator_preflight_material(&config, &config_bytes, auth_presence)?;
    let bundle_dir = create_operator_preflight_bundle_dir(&material.repository_root, output_dir)?;

    let status_digest = write_bytes_create_new(
        &bundle_dir.join(OPERATOR_PREFLIGHT_STATUS_FILE_V1),
        &material.git_status_porcelain,
    )?;
    ensure_operator_digest(
        "persisted git status porcelain",
        &status_digest,
        &material.git_status_porcelain_digest,
    )?;
    let patch_digest = write_bytes_create_new(
        &bundle_dir.join(OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1),
        &material.tracked_worktree_patch,
    )?;
    ensure_operator_digest(
        "persisted tracked worktree patch",
        &patch_digest,
        &material.tracked_worktree_patch_digest,
    )?;
    let inventory_digest = write_bytes_create_new(
        &bundle_dir.join(OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1),
        &material.untracked_inventory_bytes,
    )?;
    ensure_operator_digest(
        "persisted untracked inventory",
        &inventory_digest,
        &material.untracked_inventory_digest,
    )?;
    let source_snapshot_manifest_digest = write_bytes_create_new(
        &bundle_dir.join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1),
        &material.source_snapshot_manifest_bytes,
    )?;
    ensure_operator_digest(
        "persisted source snapshot manifest",
        &source_snapshot_manifest_digest,
        &material.source_snapshot_manifest_digest,
    )?;
    let redacted_config_digest = write_bytes_create_new(
        &bundle_dir.join(OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1),
        &material.redacted_config_bytes,
    )?;
    ensure_operator_digest(
        "persisted redacted config",
        &redacted_config_digest,
        &material.redacted_config_digest,
    )?;
    snapshot_full_source_tree(&material, &bundle_dir)?;
    ensure_operator_digest(
        "persisted source snapshot contents",
        &verify_source_snapshot_contents(&bundle_dir, &material.source_snapshot_manifest)?,
        &material.source_snapshot_manifest_digest,
    )?;
    let release_dir = create_preflight_directory(&bundle_dir, Path::new("release"))?;
    let fresh_build = build_fresh_release_from_source_snapshot(&bundle_dir, &material, &config)?;
    let build_result = (|| -> Result<(
        PumpResearchOperatorDigestV1,
        PumpResearchOperatorDigestV1,
        PumpResearchOperatorDigestV1,
        PumpResearchOperatorDigestV1,
    )> {
        let build_environment_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1),
            &fresh_build.build_environment_bytes,
        )?;
        ensure_operator_digest(
            "persisted fresh-build environment",
            &build_environment_digest,
            &fresh_build.build_receipt.build_environment_digest,
        )?;
        let build_log_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1),
            &fresh_build.build_log_bytes,
        )?;
        ensure_operator_digest(
            "persisted fresh-build log",
            &build_log_digest,
            &fresh_build.build_receipt.build_log_digest,
        )?;
        let build_receipt_digest = write_json_create_new_with_digest(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1),
            &fresh_build.build_receipt,
        )?;
        let copied_binary_digest = copy_file_create_new_with_digest(
            &fresh_build.release_binary_path,
            &release_dir.join("pump-research-tape"),
        )?;
        ensure_operator_digest(
            "copied fresh sealed release binary",
            &copied_binary_digest,
            &fresh_build.release_binary_digest,
        )?;
        Ok((
            copied_binary_digest,
            build_receipt_digest,
            build_log_digest,
            build_environment_digest,
        ))
    })();
    let fresh_staging_root = fresh_build.fresh_staging_root;
    drop(fresh_staging_root);
    let (copied_binary_digest, build_receipt_digest, build_log_digest, build_environment_digest) =
        build_result?;

    // Re-collect after every bundle write.  The output itself is required to
    // be ignored/outside the worktree, so any difference here is a genuine
    // source, binary, Cargo, toolchain or config race and must fail closed.
    let final_material =
        collect_operator_preflight_material(&config, &config_bytes, auth_presence)?;
    ensure_operator_digest(
        "operator preflight source fingerprint after bundle creation",
        &final_material.artifact_provenance_fingerprint,
        &material.artifact_provenance_fingerprint,
    )?;
    // The source snapshot, Cargo output and copied executable now all exist,
    // while the final receipt does not.  A scan failure deliberately leaves a
    // diagnosable incomplete directory but never publishes a reusable sealed
    // preflight receipt.
    ensure_configured_credentials_absent_from_sealed_bundle(&bundle_dir, &config)?;

    let mut receipt = PumpResearchOperatorPreflightReceiptV1 {
        schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
        receipt_kind: OPERATOR_PREFLIGHT_RECEIPT_KIND_V1.to_owned(),
        created_wall_ms: wall_clock_ms(),
        source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1.to_owned(),
        repository_commit: material.repository_commit.clone(),
        repository_branch: material.repository_branch.clone(),
        repository_worktree_state: if material.git_status_porcelain.is_empty() {
            "clean".to_owned()
        } else {
            "dirty".to_owned()
        },
        git_status_porcelain_file: OPERATOR_PREFLIGHT_STATUS_FILE_V1.to_owned(),
        git_status_porcelain_digest: material.git_status_porcelain_digest.clone(),
        tracked_worktree_patch_file: OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1.to_owned(),
        tracked_worktree_patch_digest: material.tracked_worktree_patch_digest.clone(),
        untracked_inventory_file: OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1.to_owned(),
        untracked_inventory_digest: material.untracked_inventory_digest.clone(),
        untracked_entry_count: material.untracked_inventory.entries.len() as u64,
        source_snapshot_manifest_file: OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1
            .to_owned(),
        source_snapshot_manifest_digest: material.source_snapshot_manifest_digest.clone(),
        source_snapshot_entry_count: material.source_snapshot_manifest.entries.len() as u64,
        cargo_lock_digest: material.cargo_lock_digest.clone(),
        release_binary_file: OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE_V1.to_owned(),
        release_binary_digest: copied_binary_digest,
        build_receipt_file: OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1.to_owned(),
        build_receipt_digest,
        build_log_file: OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1.to_owned(),
        build_log_digest,
        build_environment_file: OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1.to_owned(),
        build_environment_digest,
        build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
        credential_scan_semantics: OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1.to_owned(),
        cargo_executable_digest: material.cargo_executable_digest.clone(),
        rustc_executable_digest: material.rustc_executable_digest.clone(),
        rustc_version: material.rustc_version.clone(),
        cargo_version: material.cargo_version.clone(),
        config_semantics: OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1.to_owned(),
        config_bytes_digest: material.config_bytes_digest.clone(),
        redacted_config_file: OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1.to_owned(),
        redacted_config_digest: material.redacted_config_digest.clone(),
        artifact_provenance_fingerprint: operator_digest_bytes(
            b"pending-operator-preflight-fingerprint",
        ),
    };
    receipt.artifact_provenance_fingerprint = receipt_artifact_fingerprint(&receipt)?;
    ensure_operator_digest(
        "operator preflight receipt fingerprint",
        &receipt_artifact_fingerprint(&receipt)?,
        &receipt.artifact_provenance_fingerprint,
    )?;
    let receipt_path = bundle_dir.join(OPERATOR_PREFLIGHT_RECEIPT_FILE_V1);
    write_json_create_new_with_digest(&receipt_path, &receipt)?;
    sync_directory(&bundle_dir)?;
    Ok(PumpResearchOperatorPreflightSummaryV1 {
        bundle_dir,
        receipt_path,
        release_binary_digest: receipt.release_binary_digest,
        artifact_provenance_fingerprint: receipt.artifact_provenance_fingerprint,
    })
}

struct ValidatedOperatorPreflightV1 {
    receipt: PumpResearchOperatorPreflightReceiptV1,
    receipt_digest: PumpResearchOperatorDigestV1,
    receipt_validated_wall_ms: u64,
}

impl ValidatedOperatorPreflightV1 {
    fn binding_for_run(&self, run_id: &str) -> Result<PumpResearchCaptureProvenanceBindingV1> {
        // `validate_operator_preflight_for_capture` has already checked this
        // receipt before any provider I/O. Keep the check at the binding
        // boundary as well: a run may be qualification-eligible only when its
        // sidecar explicitly carries the current sealed build/auth contract.
        if self.receipt.build_semantics != OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1
            || self.receipt.credential_scan_semantics
                != OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1
        {
            bail!(
                "cannot bind capture run to a preflight receipt without the current sealed build/auth semantics"
            );
        }
        Ok(PumpResearchCaptureProvenanceBindingV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            binding_kind: OPERATOR_PREFLIGHT_BINDING_KIND_V1.to_owned(),
            run_id: run_id.to_owned(),
            receipt_validated_wall_ms: self.receipt_validated_wall_ms,
            binding_written_wall_ms: wall_clock_ms(),
            preflight_receipt_digest: self.receipt_digest.clone(),
            artifact_provenance_fingerprint: self.receipt.artifact_provenance_fingerprint.clone(),
            repository_commit: self.receipt.repository_commit.clone(),
            repository_worktree_state: self.receipt.repository_worktree_state.clone(),
            release_binary_digest: self.receipt.release_binary_digest.clone(),
            config_bytes_digest: self.receipt.config_bytes_digest.clone(),
            build_semantics: self.receipt.build_semantics.clone(),
            credential_scan_semantics: self.receipt.credential_scan_semantics.clone(),
            qualification_provenance_eligible: true,
            sealed_release_binary_digest: Some(self.receipt.release_binary_digest.clone()),
        })
    }
}

fn ensure_regular_provenance_file(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)
        .with_context(|| format!("stat immutable provenance artifact {}", path.display()))?;
    if !metadata.file_type().is_file() || metadata.file_type().is_symlink() {
        bail!(
            "immutable provenance artifact {} must be a regular non-symlink file",
            path.display()
        );
    }
    Ok(())
}

fn bundle_artifact_path(bundle_dir: &Path, relative_path: &str) -> Result<PathBuf> {
    let relative_path = checked_repository_relative_path(relative_path)?;
    let path = bundle_dir.join(relative_path);
    if !path.starts_with(bundle_dir) {
        bail!("provenance bundle artifact escaped its immutable bundle")
    }
    ensure_regular_provenance_file(&path)?;
    Ok(path)
}

fn verify_bundle_artifact_digest(
    bundle_dir: &Path,
    relative_path: &str,
    expected: &PumpResearchOperatorDigestV1,
    label: &str,
) -> Result<PathBuf> {
    let path = bundle_artifact_path(bundle_dir, relative_path)?;
    ensure_operator_digest(label, &operator_digest_file(&path)?, expected)?;
    Ok(path)
}

fn load_operator_preflight_receipt(
    receipt_path: &Path,
) -> Result<(
    PumpResearchOperatorPreflightReceiptV1,
    PumpResearchOperatorDigestV1,
)> {
    ensure_regular_provenance_file(receipt_path)?;
    let bytes = fs::read(receipt_path).with_context(|| {
        format!(
            "read immutable operator preflight receipt {}",
            receipt_path.display()
        )
    })?;
    let receipt: PumpResearchOperatorPreflightReceiptV1 = serde_json::from_slice(&bytes)
        .with_context(|| {
            format!(
                "parse immutable operator preflight receipt {}",
                receipt_path.display()
            )
        })?;
    if receipt.schema_version != OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1
        || receipt.receipt_kind != OPERATOR_PREFLIGHT_RECEIPT_KIND_V1
        || receipt.source_tree_semantics != OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1
        || receipt.build_semantics != OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1
        || receipt.credential_scan_semantics != OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1
        || receipt.config_semantics != OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1
    {
        bail!("operator preflight receipt has an unsupported provenance contract")
    }
    ensure_operator_digest(
        "operator preflight receipt self fingerprint",
        &receipt_artifact_fingerprint(&receipt)?,
        &receipt.artifact_provenance_fingerprint,
    )?;
    Ok((receipt, operator_digest_bytes(&bytes)))
}

fn verify_operator_preflight_bundle(
    receipt_path: &Path,
    receipt: &PumpResearchOperatorPreflightReceiptV1,
) -> Result<()> {
    let bundle_dir = receipt_path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "operator preflight receipt {} has no bundle directory",
            receipt_path.display()
        )
    })?;
    if receipt.git_status_porcelain_file != OPERATOR_PREFLIGHT_STATUS_FILE_V1
        || receipt.tracked_worktree_patch_file != OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1
        || receipt.untracked_inventory_file != OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1
        || receipt.source_snapshot_manifest_file
            != OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1
        || receipt.redacted_config_file != OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1
        || receipt.release_binary_file != OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE_V1
        || receipt.build_receipt_file != OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1
        || receipt.build_log_file != OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1
        || receipt.build_environment_file != OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1
    {
        bail!("operator preflight receipt references an unexpected artifact layout")
    }
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.git_status_porcelain_file,
        &receipt.git_status_porcelain_digest,
        "persisted Git status porcelain",
    )?;
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.tracked_worktree_patch_file,
        &receipt.tracked_worktree_patch_digest,
        "persisted tracked worktree patch",
    )?;
    let inventory_path = verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.untracked_inventory_file,
        &receipt.untracked_inventory_digest,
        "persisted untracked inventory",
    )?;
    let source_snapshot_manifest_path = verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.source_snapshot_manifest_file,
        &receipt.source_snapshot_manifest_digest,
        "persisted source snapshot manifest",
    )?;
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.redacted_config_file,
        &receipt.redacted_config_digest,
        "persisted redacted config",
    )?;
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.release_binary_file,
        &receipt.release_binary_digest,
        "copied release binary",
    )?;
    let build_receipt_path = verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.build_receipt_file,
        &receipt.build_receipt_digest,
        "persisted fresh-build receipt",
    )?;
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.build_log_file,
        &receipt.build_log_digest,
        "persisted fresh-build log",
    )?;
    verify_bundle_artifact_digest(
        bundle_dir,
        &receipt.build_environment_file,
        &receipt.build_environment_digest,
        "persisted fresh-build environment",
    )?;

    let inventory_bytes = fs::read(&inventory_path).with_context(|| {
        format!(
            "read immutable untracked inventory {}",
            inventory_path.display()
        )
    })?;
    let inventory: PumpResearchOperatorPreflightUntrackedInventoryV1 =
        serde_json::from_slice(&inventory_bytes).with_context(|| {
            format!(
                "parse immutable untracked inventory {}",
                inventory_path.display()
            )
        })?;
    if inventory.schema_version != OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1
        || inventory.entries.len() as u64 != receipt.untracked_entry_count
    {
        bail!("operator preflight untracked inventory count/schema mismatch")
    }
    let mut required_ignored = BTreeSet::new();
    for entry in &inventory.entries {
        checked_repository_relative_path(&entry.repository_relative_path)?;
        match &entry.source_kind {
            PumpResearchOperatorSourceEntryKindV1::Untracked => {}
            PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture => {
                required_ignored.insert(entry.repository_relative_path.clone());
            }
            PumpResearchOperatorSourceEntryKindV1::Tracked => {
                bail!("untracked inventory must not contain a tracked source entry")
            }
        }
    }
    let expected_required_ignored: BTreeSet<String> =
        OPERATOR_PREFLIGHT_REQUIRED_IGNORED_ARTIFACTS_V1
            .iter()
            .map(|path| (*path).to_owned())
            .collect();
    if required_ignored != expected_required_ignored {
        bail!("operator preflight required ignored-fixture inventory mismatch")
    }
    let source_snapshot_manifest_bytes =
        fs::read(&source_snapshot_manifest_path).with_context(|| {
            format!(
                "read immutable source snapshot manifest {}",
                source_snapshot_manifest_path.display()
            )
        })?;
    let source_snapshot_manifest: PumpResearchOperatorSourceSnapshotManifestV1 =
        serde_json::from_slice(&source_snapshot_manifest_bytes).with_context(|| {
            format!(
                "parse immutable source snapshot manifest {}",
                source_snapshot_manifest_path.display()
            )
        })?;
    if source_snapshot_manifest.entries.len() as u64 != receipt.source_snapshot_entry_count {
        bail!("operator preflight source snapshot entry count mismatch")
    }
    ensure_operator_digest(
        "persisted source snapshot contents",
        &verify_source_snapshot_contents(bundle_dir, &source_snapshot_manifest)?,
        &receipt.source_snapshot_manifest_digest,
    )?;
    let build_receipt_bytes = fs::read(&build_receipt_path).with_context(|| {
        format!(
            "read immutable fresh-build receipt {}",
            build_receipt_path.display()
        )
    })?;
    let build_receipt: PumpResearchOperatorBuildReceiptV1 =
        serde_json::from_slice(&build_receipt_bytes).with_context(|| {
            format!(
                "parse immutable fresh-build receipt {}",
                build_receipt_path.display()
            )
        })?;
    if build_receipt.schema_version != OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1
        || build_receipt.build_semantics != OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1
        || build_receipt.cargo_profile != "release"
        || build_receipt.cargo_command
            != [
                "cargo",
                "build",
                "--locked",
                "--offline",
                "--release",
                "-p",
                "seer",
                "--bin",
                "pump-research-tape",
            ]
            .iter()
            .map(|value| (*value).to_owned())
            .collect::<Vec<_>>()
        || build_receipt.rustc_version != receipt.rustc_version
        || build_receipt.cargo_version != receipt.cargo_version
        || build_receipt.cargo_executable_digest != receipt.cargo_executable_digest
        || build_receipt.rustc_executable_digest != receipt.rustc_executable_digest
    {
        bail!("operator preflight fresh-build receipt has an unsupported contract")
    }
    ensure_operator_digest(
        "fresh-build source snapshot before build",
        &build_receipt.source_snapshot_manifest_digest_before_build,
        &receipt.source_snapshot_manifest_digest,
    )?;
    ensure_operator_digest(
        "fresh-build source snapshot after build",
        &build_receipt.source_snapshot_manifest_digest_after_build,
        &receipt.source_snapshot_manifest_digest,
    )?;
    ensure_operator_digest(
        "fresh-build Cargo.lock",
        &build_receipt.cargo_lock_digest,
        &receipt.cargo_lock_digest,
    )?;
    ensure_operator_digest(
        "fresh-build environment",
        &build_receipt.build_environment_digest,
        &receipt.build_environment_digest,
    )?;
    ensure_operator_digest(
        "fresh-build log",
        &build_receipt.build_log_digest,
        &receipt.build_log_digest,
    )?;
    ensure_operator_digest(
        "fresh-build release binary",
        &build_receipt.release_binary_digest,
        &receipt.release_binary_digest,
    )?;
    let build_environment_path = bundle_artifact_path(bundle_dir, &receipt.build_environment_file)?;
    let build_environment_bytes = fs::read(&build_environment_path).with_context(|| {
        format!(
            "read immutable fresh-build environment {}",
            build_environment_path.display()
        )
    })?;
    let build_environment: PumpResearchOperatorBuildEnvironmentV1 =
        serde_json::from_slice(&build_environment_bytes).with_context(|| {
            format!(
                "parse immutable fresh-build environment {}",
                build_environment_path.display()
            )
        })?;
    if build_environment.child_environment_semantics
        != OPERATOR_PREFLIGHT_SANITIZED_BUILD_ENVIRONMENT_V1
        || build_environment.build_staging_semantics
            != OPERATOR_PREFLIGHT_ISOLATED_BUILD_STAGING_SEMANTICS_V1
        || build_environment.cargo_home_semantics != OPERATOR_PREFLIGHT_SANITIZED_CARGO_HOME_V1
        || build_environment.cargo_config_scope_semantics
            != OPERATOR_PREFLIGHT_ISOLATED_CARGO_CONFIG_SCOPE_V1
        || build_environment.cargo_executable_digest != receipt.cargo_executable_digest
        || build_environment.rustc_executable_digest != receipt.rustc_executable_digest
        || build_environment.rustflags_digest.is_some()
        || build_environment.cargo_encoded_rustflags_digest.is_some()
        || !build_environment.cargo_build_environment_digests.is_empty()
        || !build_environment
            .cargo_profile_release_environment_digests
            .is_empty()
        || build_environment.cargo_home_digest.is_some()
    {
        bail!("operator preflight fresh-build environment has an unsupported isolation contract");
    }
    for label in build_environment.cargo_config_file_digests.keys() {
        if !matches!(
            label.as_str(),
            "sealed_snapshot/.cargo/config.toml" | "sealed_snapshot/.cargo/config"
        ) {
            bail!(
                "operator preflight fresh-build environment admits non-snapshot Cargo config input {label}"
            );
        }
    }
    Ok(())
}

fn validate_receipt_matches_current_material(
    receipt: &PumpResearchOperatorPreflightReceiptV1,
    material: &PumpResearchOperatorPreflightMaterialV1,
) -> Result<()> {
    if receipt.repository_commit != material.repository_commit
        || receipt.repository_branch != material.repository_branch
        || receipt.repository_worktree_state
            != if material.git_status_porcelain.is_empty() {
                "clean"
            } else {
                "dirty"
            }
        || receipt.rustc_version != material.rustc_version
        || receipt.cargo_version != material.cargo_version
        || receipt.cargo_executable_digest != material.cargo_executable_digest
        || receipt.rustc_executable_digest != material.rustc_executable_digest
    {
        bail!("operator preflight repository/toolchain identity no longer matches the current capture process")
    }
    ensure_operator_digest(
        "current Git status porcelain",
        &material.git_status_porcelain_digest,
        &receipt.git_status_porcelain_digest,
    )?;
    ensure_operator_digest(
        "current tracked worktree patch",
        &material.tracked_worktree_patch_digest,
        &receipt.tracked_worktree_patch_digest,
    )?;
    ensure_operator_digest(
        "current untracked inventory",
        &material.untracked_inventory_digest,
        &receipt.untracked_inventory_digest,
    )?;
    ensure_operator_digest(
        "current source snapshot manifest",
        &material.source_snapshot_manifest_digest,
        &receipt.source_snapshot_manifest_digest,
    )?;
    ensure_operator_digest(
        "current Cargo.lock",
        &material.cargo_lock_digest,
        &receipt.cargo_lock_digest,
    )?;
    ensure_operator_digest(
        "current capture config",
        &material.config_bytes_digest,
        &receipt.config_bytes_digest,
    )?;
    ensure_operator_digest(
        "current redacted capture config",
        &material.redacted_config_digest,
        &receipt.redacted_config_digest,
    )?;
    Ok(())
}

fn validate_operator_preflight_for_capture(
    config_path: &Path,
    config: &PumpResearchCaptureConfigV1,
    config_bytes: &[u8],
    receipt_path: &Path,
) -> Result<ValidatedOperatorPreflightV1> {
    require_non_debug_operator_bootstrap_binary()?;
    let repository_root = current_repository_root()?;
    validate_external_operator_config_path(&repository_root, config_path)?;
    let auth_presence = validate_operator_preflight_config(config)?;
    let (receipt, receipt_digest) = load_operator_preflight_receipt(receipt_path)?;
    verify_operator_preflight_bundle(receipt_path, &receipt)?;
    let material = collect_operator_preflight_material(config, config_bytes, auth_presence)?;
    validate_receipt_matches_current_material(&receipt, &material)?;
    let current_executable = fs::canonicalize(
        env::current_exe().context("resolve capture executable for sealed preflight validation")?,
    )
    .context("canonicalize capture executable for sealed preflight validation")?;
    ensure_operator_digest(
        "current capture executable",
        &operator_digest_file(&current_executable)?,
        &receipt.release_binary_digest,
    )?;
    Ok(ValidatedOperatorPreflightV1 {
        receipt,
        receipt_digest,
        receipt_validated_wall_ms: wall_clock_ms(),
    })
}

/// Outcome returned to the standalone binary after immutable receipts have
/// been written.  A non-`Complete` status retains raw evidence but makes the
/// run non-qualifiable; callers should return a non-zero process status.
#[derive(Clone, Debug)]
pub struct PumpResearchCaptureRunSummaryV1 {
    pub run_id: String,
    pub raw_dir: PathBuf,
    pub status: PumpResearchRunCompletionStatusV1,
    pub gap_count: u64,
    pub clean_shutdown: bool,
    pub source_error: Option<String>,
    pub writer_error: Option<String>,
    pub completion_receipt_error: Option<String>,
}

/// Test-only probe at the exact transition from the pure receipt-validation
/// phase to code that can initiate provider I/O.  It is deliberately absent
/// from release builds and lets the regression assert the public capture API,
/// not merely a helper, stops before RPC/source construction on invalid input.
#[cfg(test)]
static TEST_PROVIDER_IO_PHASE_ENTRIES_V1: AtomicU64 = AtomicU64::new(0);

impl PumpResearchCaptureRunSummaryV1 {
    #[must_use]
    pub const fn is_complete(&self) -> bool {
        matches!(self.status, PumpResearchRunCompletionStatusV1::Complete)
    }
}

#[derive(Debug)]
struct CapturePathsV1 {
    run_id: String,
    raw_dir: PathBuf,
    start_manifest_path: PathBuf,
    completion_receipt_path: PathBuf,
}

impl CapturePathsV1 {
    fn create(output_dir: &Path) -> Result<Self> {
        fs::create_dir_all(output_dir).with_context(|| {
            format!("create Pump Research output root {}", output_dir.display())
        })?;

        let base = format!("pump-research-{}-{}", wall_clock_ms(), std::process::id());
        for suffix in 0u32..10_000 {
            let run_id = if suffix == 0 {
                base.clone()
            } else {
                format!("{base}-{suffix}")
            };
            let run_dir = output_dir.join(&run_id);
            match fs::create_dir(&run_dir) {
                Ok(()) => {
                    let raw_dir = run_dir.join("raw");
                    fs::create_dir(&raw_dir).with_context(|| {
                        format!("create immutable raw directory {}", raw_dir.display())
                    })?;
                    return Ok(Self {
                        run_id,
                        start_manifest_path: raw_dir.join("run_start_manifest.json"),
                        completion_receipt_path: raw_dir.join("run_completion_receipt.json"),
                        raw_dir,
                    });
                }
                Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(error).with_context(|| {
                        format!(
                            "create immutable Pump Research run directory {}",
                            run_dir.display()
                        )
                    });
                }
            }
        }
        bail!("could not allocate a unique immutable Pump Research run ID")
    }
}

/// Run `pump-research-tape capture` from a TOML configuration path and a
/// previously sealed operator-provenance receipt.  Receipt validation happens
/// before either ProgramData RPC or Yellowstone source I/O.
pub async fn run_capture_from_config_path(
    config_path: &Path,
    provenance_receipt_path: &Path,
) -> Result<PumpResearchCaptureRunSummaryV1> {
    let (config, config_bytes, validated_preflight) =
        prepare_capture_before_provider_io(config_path, provenance_receipt_path)?;
    run_capture(config, config_bytes, validated_preflight).await
}

/// Pure local phase deliberately kept separate from `run_capture`: a failed
/// receipt, source/config drift or non-sealed executable returns here, before
/// any RPC client or Yellowstone connection can be constructed.
fn prepare_capture_before_provider_io(
    config_path: &Path,
    provenance_receipt_path: &Path,
) -> Result<(
    PumpResearchCaptureConfigV1,
    Vec<u8>,
    ValidatedOperatorPreflightV1,
)> {
    let (config, config_bytes) = PumpResearchCaptureConfigV1::load(config_path)?;
    let validated_preflight = validate_operator_preflight_for_capture(
        config_path,
        &config,
        &config_bytes,
        provenance_receipt_path,
    )?;
    Ok((config, config_bytes, validated_preflight))
}

/// Run standalone raw capture.  The finalized start ProgramData receipt is
/// deliberately obtained before any output run directory or source stream is
/// opened.  If it cannot be proven, no source record is admitted.
async fn run_capture(
    config: PumpResearchCaptureConfigV1,
    config_bytes: Vec<u8>,
    validated_preflight: ValidatedOperatorPreflightV1,
) -> Result<PumpResearchCaptureRunSummaryV1> {
    config.validate()?;
    // Resolve the secret before any provider I/O.  The preflight validated its
    // presence without persisting it; this second resolution is the only
    // value forwarded to the source connector.
    let auth_token = config.resolve_grpc_auth_token()?;
    let rpc_auth_token = config.resolve_rpc_auth_token()?;
    let pump_program_id = config
        .pump_program_id
        .parse::<Pubkey>()
        .context("validated pump_program_id unexpectedly failed to parse")?;

    #[cfg(test)]
    TEST_PROVIDER_IO_PHASE_ENTRIES_V1.fetch_add(1, Ordering::AcqRel);

    let start_receipt = observe_program_data_receipt(
        &config.rpc_endpoint,
        rpc_auth_token.as_deref(),
        &config.rpc_auth_header,
        pump_program_id,
    )
    .await
    .context("Pump ProgramData start receipt is required before opening the source stream")?;
    let paths = CapturePathsV1::create(&config.output_dir)?;
    let provenance_binding = validated_preflight.binding_for_run(&paths.run_id)?;
    write_json_create_new(
        &paths
            .raw_dir
            .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
        &provenance_binding,
    )?;
    let started_wall_ms = wall_clock_ms();
    let started_monotonic_ms = crate::types::arrival_time_ms();
    let manifest = build_start_manifest(
        &paths.run_id,
        &config,
        &config_bytes,
        started_wall_ms,
        started_monotonic_ms,
        &start_receipt,
    )?;
    write_json_create_new(&paths.start_manifest_path, &manifest)?;

    let coordinator = match PumpResearchCaptureCoordinatorV1::start(
        &paths.raw_dir,
        paths.run_id.clone(),
        config.queue_capacity,
        Duration::from_millis(config.flush_interval_ms),
        config.segment_max_bytes,
        Duration::from_millis(config.segment_max_duration_ms),
    ) {
        Ok(coordinator) => coordinator,
        Err(error) => {
            write_prestream_incomplete_receipt(
                &paths,
                PumpResearchWriterSummaryV1::default(),
                observe_program_data_receipt(
                    &config.rpc_endpoint,
                    rpc_auth_token.as_deref(),
                    &config.rpc_auth_header,
                    pump_program_id,
                )
                .await
                .ok(),
            )?;
            return Err(error.context(format!(
                "Pump Research writer could not start; incomplete receipt retained at {}",
                paths.completion_receipt_path.display()
            )));
        }
    };
    let sink = coordinator.source_sink();
    let source = match PumpResearchSourceConnectionV1::new(
        config.grpc_endpoint.clone(),
        auth_token,
        config.grpc_auth_header.clone(),
        config.primary_provider_id.clone(),
        config.queue_capacity,
        sink,
        coordinator.capture_abort(),
    ) {
        Ok(source) => Arc::new(source),
        Err(error) => {
            coordinator.finish_source();
            let writer = coordinator.finish_and_join();
            write_prestream_incomplete_receipt(
                &paths,
                writer,
                observe_program_data_receipt(
                    &config.rpc_endpoint,
                    rpc_auth_token.as_deref(),
                    &config.rpc_auth_header,
                    pump_program_id,
                )
                .await
                .ok(),
            )?;
            return Err(error.context(format!(
                "Pump Research source could not start; incomplete receipt retained at {}",
                paths.completion_receipt_path.display()
            )));
        }
    };

    info!(
        run_id = %paths.run_id,
        raw_dir = %paths.raw_dir.display(),
        provider_id = %config.primary_provider_id,
        "Pump Research Tape V1 raw capture started"
    );

    let source_result = run_source_until_signal(Arc::clone(&source)).await;
    // `PumpResearchSourceConnectionV1::run` normally delivers this itself.
    // This idempotent second call covers a task-level error before its terminal
    // notification without making source finishing a second lifecycle path.
    coordinator.finish_source();
    let writer_summary = coordinator.finish_and_join();
    let source_lifecycle = coordinator.source_lifecycle();

    let completion_result = observe_program_data_receipt(
        &config.rpc_endpoint,
        rpc_auth_token.as_deref(),
        &config.rpc_auth_header,
        pump_program_id,
    )
    .await;
    let completion_error = completion_result.as_ref().err().map(ToString::to_string);
    let completion_receipt = completion_result.ok();
    let status = completion_status(
        config.required_for_run,
        &source_result,
        &writer_summary,
        &source_lifecycle,
        completion_receipt.as_ref(),
        &start_receipt,
    );
    let clean_shutdown = source_result.is_ok()
        && writer_summary.clean_shutdown
        && completion_receipt.is_some()
        && source_lifecycle.source_workers_cleanly_stopped
        && source_lifecycle.fatal_capture_error.is_none()
        && source_lifecycle.source_worker_error.is_none();
    let receipt = build_completion_receipt(
        &paths.run_id,
        &writer_summary,
        &source_lifecycle,
        completion_receipt.as_ref(),
        clean_shutdown,
        status,
    );
    write_json_create_new(&paths.completion_receipt_path, &receipt)?;

    let summary = PumpResearchCaptureRunSummaryV1 {
        run_id: paths.run_id,
        raw_dir: paths.raw_dir,
        status,
        gap_count: writer_summary.gap_count,
        clean_shutdown,
        source_error: source_result.err().map(|error| format!("{error:#}")),
        writer_error: writer_summary.error.clone(),
        completion_receipt_error: completion_error,
    };
    if summary.is_complete() {
        info!(
            run_id = %summary.run_id,
            raw_dir = %summary.raw_dir.display(),
            gap_count = summary.gap_count,
            "Pump Research Tape V1 raw capture completed cleanly"
        );
    } else {
        warn!(
            run_id = %summary.run_id,
            raw_dir = %summary.raw_dir.display(),
            status = ?summary.status,
            gap_count = summary.gap_count,
            "Pump Research Tape V1 retained raw evidence but run is not complete"
        );
    }
    Ok(summary)
}

fn write_prestream_incomplete_receipt(
    paths: &CapturePathsV1,
    writer: PumpResearchWriterSummaryV1,
    completion: Option<tape::PumpProgramDataReceiptV1>,
) -> Result<()> {
    let receipt = build_completion_receipt(
        &paths.run_id,
        &writer,
        &PumpResearchSourceLifecycleV1::default(),
        completion.as_ref(),
        false,
        PumpResearchRunCompletionStatusV1::Incomplete,
    );
    write_json_create_new(&paths.completion_receipt_path, &receipt)
}

async fn run_source_until_signal(source: Arc<PumpResearchSourceConnectionV1>) -> Result<()> {
    let source_for_run = Arc::clone(&source);
    let source_future = source_for_run.run();
    tokio::pin!(source_future);
    tokio::select! {
        result = &mut source_future => result,
        signal = tokio::signal::ctrl_c() => {
            signal.context("wait for capture shutdown signal")?;
            info!("Pump Research Tape capture received shutdown signal; draining accepted records");
            source.request_shutdown();
            source_future.await
        }
    }
}

fn build_start_manifest(
    run_id: &str,
    config: &PumpResearchCaptureConfigV1,
    config_bytes: &[u8],
    started_wall_ms: u64,
    started_monotonic_ms: u64,
    receipt: &tape::PumpProgramDataReceiptV1,
) -> Result<PumpResearchRunStartManifestV1> {
    Ok(PumpResearchRunStartManifestV1 {
        storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
        schema_version: RAW_SCHEMA_VERSION_V1,
        run_id: run_id.to_owned(),
        repository_commit: repository_commit()?,
        binary_hash_blake3: hash_current_executable()?,
        config_hash_blake3: hash_bytes(config_bytes),
        raw_event_schema_version: RAW_SCHEMA_VERSION_V1,
        decoder_version: format!(
            "pump_research_capture_v1;prost={};bincode={}",
            tape::PUMP_RESEARCH_PROST_VERSION_V1,
            PUMP_RESEARCH_BINCODE_VERSION_V1
        ),
        primary_provider_id: config.primary_provider_id.clone(),
        primary_provider_role: PumpResearchProviderRoleV1::PrimaryAuthority,
        commitment: "processed".to_owned(),
        subscription_request_fingerprint_blake3: PumpResearchStorageHashV1::from(
            pump_research_subscription_request_fingerprint_blake3_v1(),
        ),
        stream_epoch: 0,
        capture_started_wall_ms: started_wall_ms,
        capture_started_monotonic_ms: started_monotonic_ms,
        time_contract_version: TIME_CONTRACT_VERSION_V1,
        required_for_run: config.required_for_run,
        source_proto_schema_version: PUMP_RESEARCH_SOURCE_PROTO_SCHEMA_VERSION_V1.to_owned(),
        source_proto_descriptor_hash: PUMP_RESEARCH_SOURCE_PROTO_DESCRIPTOR_HASH_V1.to_owned(),
        source_proto_crate: PUMP_RESEARCH_SOURCE_PROTO_CRATE_V1.to_owned(),
        source_proto_crate_version: PUMP_RESEARCH_SOURCE_PROTO_CRATE_VERSION_V1.to_owned(),
        source_client_crate: PUMP_RESEARCH_SOURCE_CLIENT_CRATE_V1.to_owned(),
        source_client_version: PUMP_RESEARCH_SOURCE_CLIENT_VERSION_V1.to_owned(),
        source_capture_semantics: PUMP_RESEARCH_SOURCE_CAPTURE_SEMANTICS_V1.to_owned(),
        pump_program_id: receipt.pump_program_id,
        pump_program_account_owner: receipt.pump_program_account_owner,
        pump_programdata_pubkey: receipt.pump_programdata_pubkey,
        program_data_owner: receipt.program_data_owner,
        program_data_hash_algorithm: PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1.to_owned(),
        program_data_hash_at_start: receipt.program_data_hash_blake3,
        program_deployment_slot_at_start: receipt.program_deployment_slot,
        program_observed_context_slot_at_start: receipt.observed_context_slot,
        program_receipt_commitment: receipt.commitment.clone(),
    })
}

fn build_completion_receipt(
    run_id: &str,
    writer: &PumpResearchWriterSummaryV1,
    source_lifecycle: &PumpResearchSourceLifecycleV1,
    completion: Option<&tape::PumpProgramDataReceiptV1>,
    clean_shutdown: bool,
    status: PumpResearchRunCompletionStatusV1,
) -> PumpResearchRunCompletionReceiptV1 {
    PumpResearchRunCompletionReceiptV1 {
        storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
        run_id: run_id.to_owned(),
        capture_ended_wall_ms: wall_clock_ms(),
        pump_program_id_at_completion: completion.map(|receipt| receipt.pump_program_id),
        pump_program_account_owner_at_completion: completion
            .map(|receipt| receipt.pump_program_account_owner),
        pump_programdata_pubkey_at_completion: completion
            .map(|receipt| receipt.pump_programdata_pubkey),
        program_data_owner_at_completion: completion.map(|receipt| receipt.program_data_owner),
        program_data_hash_at_completion: completion.map(|receipt| receipt.program_data_hash_blake3),
        program_deployment_slot_at_completion: completion
            .and_then(|receipt| receipt.program_deployment_slot),
        program_observed_context_slot_at_completion: completion
            .map(|receipt| receipt.observed_context_slot),
        program_receipt_commitment_at_completion: completion
            .map(|receipt| receipt.commitment.clone()),
        segment_list: writer.segments.clone(),
        gap_count: writer.gap_count,
        source_stream_established: source_lifecycle.stream_established,
        first_source_update_received: source_lifecycle.source_updates_received > 0,
        source_workers_cleanly_stopped: source_lifecycle.source_workers_cleanly_stopped,
        received_source_update_count: source_lifecycle.source_updates_received,
        admitted_source_update_count: source_lifecycle.admitted_source_updates,
        persisted_source_record_count: writer.accepted_source_records,
        dropped_source_update_count: source_lifecycle.dropped_source_updates,
        persisted_ingress_gap_episode_count: writer.persisted_ingress_gap_episodes,
        persisted_ingress_gap_missing_event_count: writer.persisted_ingress_gap_missing_events,
        source_lifecycle_error: source_lifecycle.source_worker_error.clone(),
        capture_failure: source_lifecycle.fatal_capture_error.clone(),
        clean_shutdown,
        status,
    }
}

fn completion_status(
    required_for_run: bool,
    source_result: &Result<()>,
    writer: &PumpResearchWriterSummaryV1,
    source_lifecycle: &PumpResearchSourceLifecycleV1,
    completion: Option<&tape::PumpProgramDataReceiptV1>,
    start: &tape::PumpProgramDataReceiptV1,
) -> PumpResearchRunCompletionStatusV1 {
    if !source_result.is_ok()
        || !writer.clean_shutdown
        || writer.error.is_some()
        || !source_lifecycle.stream_established
        || source_lifecycle.source_updates_received == 0
        || source_lifecycle.admitted_source_updates == 0
        || !source_lifecycle.source_workers_cleanly_stopped
        || source_lifecycle.fatal_capture_error.is_some()
        || source_lifecycle.source_worker_error.is_some()
        || writer.accepted_source_records == 0
        || writer.segments.is_empty()
        || source_lifecycle.dropped_source_updates != writer.persisted_ingress_gap_missing_events
        || completion.is_none()
    {
        return PumpResearchRunCompletionStatusV1::Incomplete;
    }
    let Some(completion) = completion else {
        // Kept as an explicit branch rather than an `expect`: a missing
        // completion receipt is an ordinary incomplete-capture outcome.
        return PumpResearchRunCompletionStatusV1::Incomplete;
    };
    if !program_receipts_match(start, completion) {
        return PumpResearchRunCompletionStatusV1::ProgramVersionBoundary;
    }
    if required_for_run && writer.gap_count > 0 {
        return PumpResearchRunCompletionStatusV1::Incomplete;
    }
    PumpResearchRunCompletionStatusV1::Complete
}

fn program_receipts_match(
    start: &tape::PumpProgramDataReceiptV1,
    completion: &tape::PumpProgramDataReceiptV1,
) -> bool {
    start.pump_program_id == completion.pump_program_id
        && start.pump_program_account_owner == completion.pump_program_account_owner
        && start.pump_programdata_pubkey == completion.pump_programdata_pubkey
        && start.program_data_owner == completion.program_data_owner
        && start.program_data_hash_blake3 == completion.program_data_hash_blake3
        && match (
            start.program_deployment_slot,
            completion.program_deployment_slot,
        ) {
            (Some(left), Some(right)) => left == right,
            _ => true,
        }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpResearchProgramDataRpcAuthModeV1 {
    ExplicitStandaloneAuth,
    StandaloneNoAuth,
}

/// Select the ProgramData receipt client without consulting the legacy
/// process-global RPC auth surface.  This is intentionally shared by start,
/// completion and pre-stream failure receipts so an omitted PR-A RPC token
/// can never fall back to a Yellowstone credential merely because the RPC
/// endpoint happens to be an NLN host.
fn program_data_receipt_rpc_client(
    rpc_endpoint: &str,
    rpc_auth_token: Option<&str>,
    rpc_auth_header: &str,
) -> Result<(
    solana_client::nonblocking::rpc_client::RpcClient,
    PumpResearchProgramDataRpcAuthModeV1,
)> {
    match rpc_auth_token {
        Some(token) => Ok((
            crate::rpc_http_client::new_async_rpc_client_with_explicit_auth(
                rpc_endpoint.to_owned(),
                rpc_auth_header,
                token,
            )
            .map_err(anyhow::Error::msg)?,
            PumpResearchProgramDataRpcAuthModeV1::ExplicitStandaloneAuth,
        )),
        None => Ok((
            crate::rpc_http_client::new_async_rpc_client_without_legacy_auth(
                rpc_endpoint.to_owned(),
            )
            .map_err(anyhow::Error::msg)?,
            PumpResearchProgramDataRpcAuthModeV1::StandaloneNoAuth,
        )),
    }
}

async fn observe_program_data_receipt(
    rpc_endpoint: &str,
    rpc_auth_token: Option<&str>,
    rpc_auth_header: &str,
    pump_program_id: Pubkey,
) -> Result<tape::PumpProgramDataReceiptV1> {
    let (client, _) =
        program_data_receipt_rpc_client(rpc_endpoint, rpc_auth_token, rpc_auth_header)?;
    let program_response = client
        .get_account_with_commitment(&pump_program_id, CommitmentConfig::finalized())
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
        .get_account_with_commitment(&programdata_pubkey, CommitmentConfig::finalized())
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

    Ok(tape::PumpProgramDataReceiptV1 {
        pump_program_id: tape::pump_research_storage_pubkey_v1(pump_program_id),
        pump_program_account_owner: tape::pump_research_storage_pubkey_v1(program_account.owner),
        pump_programdata_pubkey: tape::pump_research_storage_pubkey_v1(programdata_pubkey),
        program_data_owner: tape::pump_research_storage_pubkey_v1(programdata_account.owner),
        program_data_hash_algorithm: PUMP_RESEARCH_PROGRAM_DATA_HASH_ALGORITHM_V1.to_owned(),
        program_data_hash_blake3: hash_bytes(&programdata_account.data),
        program_deployment_slot: deployment_slot,
        // This context belongs to the exact raw ProgramData bytes that were
        // hashed above.  Do not synthesize a later context by combining two
        // independent RPC responses.
        observed_context_slot: programdata_response.context.slot,
        commitment: "finalized".to_owned(),
    })
}

fn repository_commit() -> Result<String> {
    let output = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .output()
        .context("invoke git rev-parse HEAD for immutable run manifest")?;
    if !output.status.success() {
        bail!("git rev-parse HEAD failed while creating immutable run manifest");
    }
    let commit = String::from_utf8(output.stdout)
        .context("git rev-parse HEAD returned invalid UTF-8")?
        .trim()
        .to_owned();
    validate_trimmed("repository_commit", &commit)?;
    Ok(commit)
}

fn hash_current_executable() -> Result<PumpResearchStorageHashV1> {
    let executable = env::current_exe().context("resolve pump-research-tape executable path")?;
    let bytes = fs::read(&executable).with_context(|| {
        format!(
            "read capture executable {} for manifest hash",
            executable.display()
        )
    })?;
    Ok(hash_bytes(&bytes))
}

fn hash_bytes(bytes: &[u8]) -> PumpResearchStorageHashV1 {
    PumpResearchStorageHashV1::from(*blake3::hash(bytes).as_bytes())
}

fn write_json_create_new<T: serde::Serialize>(path: &Path, value: &T) -> Result<()> {
    let bytes = serde_json::to_vec_pretty(value).context("serialize immutable research receipt")?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create immutable artifact {}", path.display()))?;
    file.write_all(&bytes)
        .with_context(|| format!("write immutable artifact {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("terminate immutable artifact {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync immutable artifact {}", path.display()))?;
    let parent = path.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "immutable artifact {} has no parent directory",
            path.display()
        )
    })?;
    sync_directory(parent)?;
    Ok(())
}

fn wall_clock_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

/// A source update after allocation of the monotonic run-local capture
/// sequence.  Sequence advances at every capture admission *attempt*, not
/// only for successful queue sends.  The writer merges this lane with dropped
/// source markers by sequence before constructing a typed coverage episode.
struct QueuedSourceUpdateV1 {
    capture_sequence: u64,
    update: PumpResearchSourceUpdateV1,
}

/// A deliberately small, fixed-width marker for a source update rejected by
/// the bounded data lane.  It is the only extra work performed by the receive
/// path on saturation: no mutex, BLAKE3, protobuf encoding, bincode, or disk
/// I/O is allowed there.
struct DroppedSourceUpdateV1 {
    capture_sequence: u64,
    provider_id: String,
    stream_epoch: u64,
    boundary: PumpRawCoverageBoundaryV1,
    queue_high_water: usize,
}

enum CaptureControlV1 {
    DroppedSource(DroppedSourceUpdateV1),
}

enum OrderedIngressEventV1 {
    Source(QueuedSourceUpdateV1),
    Dropped(DroppedSourceUpdateV1),
}

/// Fail-closed conditions which can be raised by the receive path without a
/// mutex, allocation, protobuf serialization, or hashing.  The detailed
/// writer error is retained independently in `PumpResearchWriterSummaryV1`;
/// this compact code is the source-side cancellation/lifecycle proof.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u8)]
enum PumpResearchCaptureFatalReasonV1 {
    None = 0,
    DropControlLaneSaturated = 1,
    DropControlLaneDisconnected = 2,
    DataLaneDisconnected = 3,
    WriterFailure = 4,
    WriterPanic = 5,
    WriterJoinPanic = 6,
}

impl PumpResearchCaptureFatalReasonV1 {
    const fn from_raw(value: u8) -> Self {
        match value {
            1 => Self::DropControlLaneSaturated,
            2 => Self::DropControlLaneDisconnected,
            3 => Self::DataLaneDisconnected,
            4 => Self::WriterFailure,
            5 => Self::WriterPanic,
            6 => Self::WriterJoinPanic,
            _ => Self::None,
        }
    }

    const fn message(self) -> Option<&'static str> {
        match self {
            Self::None => None,
            Self::DropControlLaneSaturated => Some(
                "Pump Research reserved drop-control lane saturated before a typed coverage gap could be persisted",
            ),
            Self::DropControlLaneDisconnected => Some(
                "Pump Research drop-control lane disconnected before a typed coverage gap could be persisted",
            ),
            Self::DataLaneDisconnected => Some(
                "Pump Research data lane disconnected while source updates were still arriving",
            ),
            Self::WriterFailure => Some("Pump Research raw writer failed"),
            Self::WriterPanic => Some("Pump Research raw writer thread panicked"),
            Self::WriterJoinPanic => Some(
                "Pump Research raw writer thread panicked before reporting a failure",
            ),
        }
    }
}

#[derive(Clone, Debug, Default)]
struct PumpResearchSourceLifecycleV1 {
    stream_established: bool,
    source_updates_received: u64,
    admitted_source_updates: u64,
    source_workers_cleanly_stopped: bool,
    dropped_source_updates: u64,
    fatal_capture_error: Option<String>,
    source_worker_error: Option<String>,
}

/// Shared bounded ingress, used directly by the Yellowstone receive task.
///
/// The normal receive path performs only atomic bookkeeping and bounded
/// ownership transfer.  Stateful local-gap construction is writer-owned so a
/// receive task cannot take a blocking mutex or compute a gap hash.
struct PumpResearchCaptureIngressV1 {
    data_tx: crossbeam_channel::Sender<QueuedSourceUpdateV1>,
    control_tx: crossbeam_channel::Sender<CaptureControlV1>,
    data_capacity: usize,
    next_capture_sequence: AtomicU64,
    final_capture_sequence: AtomicU64,
    accepting: AtomicBool,
    /// Calls that passed the admission gate but have not yet emitted either
    /// a data record or its ordered drop marker.  Only shutdown waits for this
    /// counter; the receive path never waits on it.
    active_capture_calls: AtomicUsize,
    finish_started: AtomicBool,
    source_finished: AtomicBool,
    stream_established: AtomicBool,
    source_updates_received: AtomicU64,
    admitted_source_updates: AtomicU64,
    source_workers_cleanly_stopped: AtomicBool,
    dropped_data_records: AtomicU64,
    fatal_capture_reason: AtomicU8,
    /// Guards the one cancellation dispatch owned by the writer/coordinator.
    /// It is deliberately distinct from `fatal_capture_reason`: the receive
    /// task may record a fatal reason but must never synchronously traverse a
    /// `CancellationToken` tree.
    fatal_source_cancel_dispatched: AtomicBool,
    source_worker_error: Mutex<Option<String>>,
    capture_abort: CancellationToken,
}

/// A receive-side admission reservation.  Its drop is the release point that
/// lets shutdown snapshot a truthful final capture sequence without racing an
/// in-flight `try_send`.
struct PumpResearchCaptureAdmissionGuardV1<'a> {
    active_capture_calls: &'a AtomicUsize,
}

impl Drop for PumpResearchCaptureAdmissionGuardV1<'_> {
    fn drop(&mut self) {
        self.active_capture_calls.fetch_sub(1, Ordering::Release);
    }
}

impl PumpResearchCaptureIngressV1 {
    fn new(
        data_tx: crossbeam_channel::Sender<QueuedSourceUpdateV1>,
        control_tx: crossbeam_channel::Sender<CaptureControlV1>,
        data_capacity: usize,
        capture_abort: CancellationToken,
    ) -> Self {
        Self {
            data_tx,
            control_tx,
            data_capacity,
            next_capture_sequence: AtomicU64::new(0),
            final_capture_sequence: AtomicU64::new(0),
            accepting: AtomicBool::new(true),
            active_capture_calls: AtomicUsize::new(0),
            finish_started: AtomicBool::new(false),
            source_finished: AtomicBool::new(false),
            stream_established: AtomicBool::new(false),
            source_updates_received: AtomicU64::new(0),
            admitted_source_updates: AtomicU64::new(0),
            source_workers_cleanly_stopped: AtomicBool::new(false),
            dropped_data_records: AtomicU64::new(0),
            fatal_capture_reason: AtomicU8::new(PumpResearchCaptureFatalReasonV1::None as u8),
            fatal_source_cancel_dispatched: AtomicBool::new(false),
            source_worker_error: Mutex::new(None),
            capture_abort,
        }
    }

    fn capture_abort(&self) -> CancellationToken {
        self.capture_abort.clone()
    }

    /// Record a fail-closed condition from the gRPC receive task.
    ///
    /// This method is intentionally limited to an atomic transition.  In
    /// particular, it must not call `CancellationToken::cancel()`: cancelling
    /// can synchronously walk and lock the token tree.  The writer/coordinator
    /// observes this reason and performs that control-plane work outside the
    /// Yellowstone receive task.
    fn record_fatal_capture_error(&self, reason: PumpResearchCaptureFatalReasonV1) -> bool {
        self.fatal_capture_reason
            .compare_exchange(
                PumpResearchCaptureFatalReasonV1::None as u8,
                reason as u8,
                Ordering::AcqRel,
                Ordering::Acquire,
            )
            .is_ok()
    }

    fn fatal_capture_reason(&self) -> PumpResearchCaptureFatalReasonV1 {
        PumpResearchCaptureFatalReasonV1::from_raw(
            self.fatal_capture_reason.load(Ordering::Acquire),
        )
    }

    /// Dispatch source cancellation from the writer/coordinator control
    /// plane, never from `try_capture`.  `CancellationToken::cancel()` may
    /// take synchronous locks internally, so this is intentionally not a
    /// receive-path operation.
    fn cancel_source_from_writer_if_fatal(&self) -> bool {
        if matches!(
            self.fatal_capture_reason(),
            PumpResearchCaptureFatalReasonV1::None
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

    fn lifecycle(&self) -> PumpResearchSourceLifecycleV1 {
        PumpResearchSourceLifecycleV1 {
            stream_established: self.stream_established.load(Ordering::Acquire),
            source_updates_received: self.source_updates_received.load(Ordering::Acquire),
            admitted_source_updates: self.admitted_source_updates.load(Ordering::Acquire),
            source_workers_cleanly_stopped: self
                .source_workers_cleanly_stopped
                .load(Ordering::Acquire),
            dropped_source_updates: self.dropped_data_records.load(Ordering::Acquire),
            fatal_capture_error: self.fatal_capture_reason().message().map(str::to_owned),
            source_worker_error: self.source_worker_error.lock().clone(),
        }
    }

    fn finish(&self) {
        if self.finish_started.swap(true, Ordering::AcqRel) {
            return;
        }
        self.accepting.store(false, Ordering::Release);
        // `try_capture` itself never waits.  The terminal source boundary is
        // outside the receive loop and must wait only long enough for a call
        // that already owns an admission reservation to emit its ordered
        // outcome before final_capture_sequence is frozen.
        while self.active_capture_calls.load(Ordering::Acquire) != 0 {
            std::thread::yield_now();
        }
        self.final_capture_sequence.store(
            self.next_capture_sequence.load(Ordering::Acquire),
            Ordering::Release,
        );
        // Source terminality is intentionally separate from control-lane
        // traffic.  The writer verifies that it has received every sequence
        // through `final_capture_sequence` before closing a segment.
        self.source_finished.store(true, Ordering::Release);
    }
}

impl PumpResearchSourceSinkV1 for PumpResearchCaptureIngressV1 {
    fn source_stream_established(&self, _stream_epoch: u64) {
        self.stream_established.store(true, Ordering::Release);
    }

    fn try_capture(&self, update: PumpResearchSourceUpdateV1) {
        let Some(_admission) = self.try_begin_capture() else {
            return;
        };
        let capture_sequence = self.next_capture_sequence.fetch_add(1, Ordering::AcqRel);
        self.source_updates_received.fetch_add(1, Ordering::Relaxed);
        match self.data_tx.try_send(QueuedSourceUpdateV1 {
            capture_sequence,
            update,
        }) {
            Ok(()) => {
                self.admitted_source_updates.fetch_add(1, Ordering::Relaxed);
            }
            Err(crossbeam_channel::TrySendError::Full(queued)) => {
                self.dropped_data_records.fetch_add(1, Ordering::Relaxed);
                let boundary = source_raw_boundary(&queued.update);
                let dropped = DroppedSourceUpdateV1 {
                    capture_sequence: queued.capture_sequence,
                    provider_id: queued.update.provider_id,
                    stream_epoch: queued.update.stream_epoch,
                    boundary,
                    queue_high_water: self.data_capacity,
                };
                match self
                    .control_tx
                    .try_send(CaptureControlV1::DroppedSource(dropped))
                {
                    Ok(()) => {}
                    Err(crossbeam_channel::TrySendError::Full(_)) => {
                        self.record_fatal_capture_error(
                            PumpResearchCaptureFatalReasonV1::DropControlLaneSaturated,
                        );
                    }
                    Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                        self.record_fatal_capture_error(
                            PumpResearchCaptureFatalReasonV1::DropControlLaneDisconnected,
                        );
                    }
                }
            }
            Err(crossbeam_channel::TrySendError::Disconnected(_)) => {
                self.dropped_data_records.fetch_add(1, Ordering::Relaxed);
                self.record_fatal_capture_error(
                    PumpResearchCaptureFatalReasonV1::DataLaneDisconnected,
                );
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

impl PumpResearchCaptureIngressV1 {
    /// Enter the nonblocking receive-side admission region.  A shutdown racing
    /// immediately after the first load is caught by the second load; in that
    /// case no capture sequence or source count is allocated after terminal
    /// state has begun.
    fn try_begin_capture(&self) -> Option<PumpResearchCaptureAdmissionGuardV1<'_>> {
        if !self.accepting.load(Ordering::Acquire) {
            return None;
        }
        self.active_capture_calls.fetch_add(1, Ordering::AcqRel);
        if !self.accepting.load(Ordering::Acquire) {
            self.active_capture_calls.fetch_sub(1, Ordering::Release);
            return None;
        }
        Some(PumpResearchCaptureAdmissionGuardV1 {
            active_capture_calls: &self.active_capture_calls,
        })
    }
}

fn source_boundary(update: &PumpResearchSourceUpdateV1) -> LocalCoverageBoundaryV1 {
    let mut boundary = LocalCoverageBoundaryV1::default();
    match update.update.update_oneof.as_ref() {
        Some(UpdateOneof::Transaction(transaction)) => {
            boundary.slot = Some(transaction.slot);
            boundary.signature = transaction
                .transaction
                .as_ref()
                .and_then(|info| signature_from_bytes(&info.signature).ok());
        }
        Some(UpdateOneof::Account(account)) => boundary.slot = Some(account.slot),
        Some(UpdateOneof::Slot(slot)) => boundary.slot = Some(slot.slot),
        Some(UpdateOneof::BlockMeta(meta)) => boundary.slot = Some(meta.slot),
        Some(UpdateOneof::TransactionStatus(status)) => {
            boundary.slot = Some(status.slot);
            boundary.signature = signature_from_bytes(&status.signature).ok();
        }
        Some(UpdateOneof::Block(block)) => boundary.slot = Some(block.slot),
        Some(UpdateOneof::Entry(entry)) => boundary.slot = Some(entry.slot),
        Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_)) | None => {}
    }
    boundary
}

fn source_raw_boundary(update: &PumpResearchSourceUpdateV1) -> PumpRawCoverageBoundaryV1 {
    let mut boundary = PumpRawCoverageBoundaryV1::default();
    match update.update.update_oneof.as_ref() {
        Some(UpdateOneof::Transaction(transaction)) => {
            boundary.slot = Some(transaction.slot);
            boundary.signature = transaction.transaction.as_ref().and_then(|info| {
                <[u8; 64]>::try_from(info.signature.as_slice())
                    .ok()
                    .map(PumpResearchStorageSignatureV1::from)
            });
        }
        Some(UpdateOneof::Account(account)) => boundary.slot = Some(account.slot),
        Some(UpdateOneof::Slot(slot)) => boundary.slot = Some(slot.slot),
        Some(UpdateOneof::BlockMeta(meta)) => boundary.slot = Some(meta.slot),
        Some(UpdateOneof::TransactionStatus(status)) => {
            boundary.slot = Some(status.slot);
            boundary.signature = <[u8; 64]>::try_from(status.signature.as_slice())
                .ok()
                .map(PumpResearchStorageSignatureV1::from);
        }
        Some(UpdateOneof::Block(block)) => boundary.slot = Some(block.slot),
        Some(UpdateOneof::Entry(entry)) => boundary.slot = Some(entry.slot),
        Some(UpdateOneof::Ping(_)) | Some(UpdateOneof::Pong(_)) | None => {}
    }
    boundary
}

fn local_boundary_from_raw(boundary: &PumpRawCoverageBoundaryV1) -> LocalCoverageBoundaryV1 {
    LocalCoverageBoundaryV1 {
        slot: boundary.slot,
        signature: boundary
            .signature
            .map(|signature| Signature::from(signature.into_inner())),
    }
}

fn raw_gap_from_local(gap: LocalCoverageGapV1) -> PumpRawCoverageGapV1 {
    PumpRawCoverageGapV1 {
        gap_id_blake3: PumpResearchStorageHashV1::from(gap.gap_id_blake3),
        provider_id: gap.provider_id,
        stream_epoch: gap.stream_epoch,
        episode_sequence: gap.episode_sequence,
        reason: match gap.reason {
            LocalCoverageGapReasonV1::IngressQueueSaturated => {
                PumpRawCoverageGapReasonV1::IngressQueueSaturated
            }
            LocalCoverageGapReasonV1::WalQueueSaturated => {
                PumpRawCoverageGapReasonV1::WalQueueSaturated
            }
            LocalCoverageGapReasonV1::EvidenceQueueSaturated => {
                PumpRawCoverageGapReasonV1::EvidenceQueueSaturated
            }
            LocalCoverageGapReasonV1::IpcEgressQueueSaturated => {
                PumpRawCoverageGapReasonV1::IpcEgressQueueSaturated
            }
        },
        before: raw_boundary_from_local(&gap.before),
        after: raw_boundary_from_local(&gap.after),
        missing_event_count: gap.missing_event_count,
        first_dropped: raw_boundary_from_local(&gap.first_dropped),
        last_dropped: raw_boundary_from_local(&gap.last_dropped),
        queue_high_water: gap.queue_high_water as u64,
        started_at_wall_ms: gap.started_at_ms,
        ended_at_wall_ms: gap.ended_at_ms,
        recovered: gap.recovered,
    }
}

fn raw_boundary_from_local(boundary: &LocalCoverageBoundaryV1) -> PumpRawCoverageBoundaryV1 {
    PumpRawCoverageBoundaryV1 {
        slot: boundary.slot,
        signature: boundary
            .signature
            .as_ref()
            .and_then(|signature| fixed_signature(signature.as_ref()).ok()),
    }
}

/// Dedicated capture-writer lifecycle.  It owns the only blocking filesystem
/// work on this path; `PumpResearchCaptureIngressV1` is intentionally limited
/// to bounded `try_send` calls and local-gap bookkeeping.
struct PumpResearchCaptureCoordinatorV1 {
    ingress: Arc<PumpResearchCaptureIngressV1>,
    join: Mutex<Option<JoinHandle<()>>>,
    progress: Arc<Mutex<PumpResearchWriterSummaryV1>>,
}

impl PumpResearchCaptureCoordinatorV1 {
    fn start(
        raw_dir: &Path,
        run_id: String,
        queue_capacity: usize,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
    ) -> Result<Self> {
        if queue_capacity == 0 {
            bail!("Pump Research capture queue_capacity must be greater than zero");
        }
        let (data_tx, data_rx) = crossbeam_channel::bounded(queue_capacity);
        // A rejected data update needs a reserved, bounded outcome marker so
        // the writer can persist an exact coverage episode.  Matching the data
        // capacity prevents a short full-queue burst from immediately losing
        // the marker; exhaustion still fails closed and asks the writer/control
        // plane to cancel the source outside the receive task.
        let (control_tx, control_rx) = crossbeam_channel::bounded(queue_capacity);
        let capture_abort = CancellationToken::new();
        let ingress = Arc::new(PumpResearchCaptureIngressV1::new(
            data_tx,
            control_tx,
            queue_capacity,
            capture_abort.clone(),
        ));
        let progress = Arc::new(Mutex::new(PumpResearchWriterSummaryV1::default()));
        let writer_ingress = Arc::clone(&ingress);
        let writer_progress = Arc::clone(&progress);
        let raw_dir = raw_dir.to_path_buf();
        let join = thread::Builder::new()
            .name("pump-research-tape-writer-v1".to_owned())
            .spawn(move || {
                let writer_ingress_for_main = Arc::clone(&writer_ingress);
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    raw_writer_main(
                        &raw_dir,
                        &run_id,
                        data_rx,
                        control_rx,
                        writer_ingress_for_main,
                        Arc::clone(&writer_progress),
                        flush_interval,
                        segment_max_bytes,
                        segment_max_duration,
                    )
                }));
                match result {
                    Ok(Ok(())) => {}
                    Ok(Err(error)) => {
                        error!(error = %error, "Pump Research Tape raw writer stopped with an error");
                        let message = format!("{error:#}");
                        {
                            let mut progress = writer_progress.lock();
                            progress.error = Some(message.clone());
                            progress.clean_shutdown = false;
                        }
                        writer_ingress.record_fatal_capture_error(
                            PumpResearchCaptureFatalReasonV1::WriterFailure,
                        );
                        writer_ingress.cancel_source_from_writer_if_fatal();
                    }
                    Err(_) => {
                        let message = "Pump Research raw writer thread panicked".to_owned();
                        error!("{message}");
                        {
                            let mut progress = writer_progress.lock();
                            progress.error = Some(message.clone());
                            progress.clean_shutdown = false;
                        }
                        writer_ingress.record_fatal_capture_error(
                            PumpResearchCaptureFatalReasonV1::WriterPanic,
                        );
                        writer_ingress.cancel_source_from_writer_if_fatal();
                    }
                }
            })
            .context("spawn bounded Pump Research raw writer thread")?;

        Ok(Self {
            ingress,
            join: Mutex::new(Some(join)),
            progress,
        })
    }

    fn source_sink(&self) -> Arc<dyn PumpResearchSourceSinkV1> {
        Arc::clone(&self.ingress) as Arc<dyn PumpResearchSourceSinkV1>
    }

    fn capture_abort(&self) -> CancellationToken {
        self.ingress.capture_abort()
    }

    fn source_lifecycle(&self) -> PumpResearchSourceLifecycleV1 {
        self.ingress.lifecycle()
    }

    fn finish_source(&self) {
        self.ingress.finish();
    }

    /// Finish accepting source events, then drain both the data and reserved
    /// control lanes before joining the one writer thread.  This method is not
    /// callable from the receive hot path.
    #[must_use]
    fn finish_and_join(&self) -> PumpResearchWriterSummaryV1 {
        self.ingress.finish();
        if let Some(handle) = self.join.lock().take() {
            if handle.join().is_err() {
                let mut progress = self.progress.lock();
                progress.error = Some("Pump Research raw writer thread panicked".to_owned());
                progress.clean_shutdown = false;
                self.ingress
                    .record_fatal_capture_error(PumpResearchCaptureFatalReasonV1::WriterJoinPanic);
                self.ingress.cancel_source_from_writer_if_fatal();
            }
        }
        let mut summary = self.progress.lock().clone();
        let lifecycle = self.ingress.lifecycle();
        if let Some(error) = lifecycle.fatal_capture_error {
            summary
                .error
                .get_or_insert_with(|| format!("Pump Research capture failed closed: {error}"));
            summary.clean_shutdown = false;
        }
        if lifecycle.dropped_source_updates != summary.persisted_ingress_gap_missing_events {
            summary.error.get_or_insert_with(|| {
                format!(
                    "Pump Research data queue dropped {} source updates but persisted typed ingress gaps account for {}",
                    lifecycle.dropped_source_updates, summary.persisted_ingress_gap_missing_events
                )
            });
            summary.clean_shutdown = false;
        }
        summary
    }
}

#[derive(Clone, Debug, Default)]
struct PumpResearchWriterSummaryV1 {
    segments: Vec<PumpResearchSegmentReceiptV1>,
    /// Source updates successfully materialized as a V1 primary source record.
    accepted_source_records: u64,
    /// Sum of `missing_event_count` for persisted ingress saturation episodes.
    persisted_ingress_gap_missing_events: u64,
    /// Number of persisted ingress saturation episodes (not record-limit gaps).
    persisted_ingress_gap_episodes: u64,
    gap_count: u64,
    clean_shutdown: bool,
    error: Option<String>,
}

struct OpenSegmentV1 {
    index: u64,
    stream_epoch: u64,
    partial_path: PathBuf,
    final_path: PathBuf,
    writer: BufWriter<File>,
    /// Hash of header and all non-footer records; this is the frozen value
    /// embedded in `PumpRawSegmentClosedV1` and used for segment chaining.
    prefix_hasher: blake3::Hasher,
    /// Hashes of every byte physically written to the published file,
    /// including the footer.  They are updated incrementally in the writer
    /// and never require loading an entire segment back into memory.
    file_blake3_hasher: blake3::Hasher,
    file_sha256_hasher: Sha256,
    bytes_before_footer: u64,
    records_before_footer: u64,
    first_capture_sequence: Option<u64>,
    last_capture_sequence: Option<u64>,
    opened_at: Instant,
}

struct RawSegmentWriterV1 {
    raw_dir: PathBuf,
    run_id: String,
    flush_interval: Duration,
    segment_max_bytes: u64,
    segment_max_duration: Duration,
    next_index: u64,
    previous_segment_blake3: Option<PumpResearchStorageHashV1>,
    current: Option<OpenSegmentV1>,
    last_flush: Instant,
    last_source_boundary: PumpRawCoverageBoundaryV1,
    receipt_sink: Option<Arc<Mutex<PumpResearchWriterSummaryV1>>>,
    #[cfg(test)]
    fail_next_open_for_test: bool,
}

/// Test-only probe used by the explicit capture-enabled A/B gate. It never
/// exists in production builds and therefore cannot add a branch, lock, or
/// delay to the receive or writer paths outside the harness.
#[cfg(test)]
#[derive(Clone)]
struct RawWriterSlowIoProbeV1 {
    delay: Duration,
    entered_tx: std::sync::mpsc::Sender<Instant>,
}

#[cfg(test)]
struct RawWriterSlowIoProbeGuardV1;

#[cfg(test)]
impl Drop for RawWriterSlowIoProbeGuardV1 {
    fn drop(&mut self) {
        let mut slot = raw_writer_slow_io_probe_slot()
            .lock()
            .expect("test slow-I/O probe mutex must not be poisoned");
        *slot = None;
    }
}

#[cfg(test)]
fn raw_writer_slow_io_probe_slot() -> &'static std::sync::Mutex<Option<RawWriterSlowIoProbeV1>> {
    static SLOT: std::sync::OnceLock<std::sync::Mutex<Option<RawWriterSlowIoProbeV1>>> =
        std::sync::OnceLock::new();
    SLOT.get_or_init(|| std::sync::Mutex::new(None))
}

#[cfg(test)]
fn install_raw_writer_slow_io_probe(
    delay: Duration,
    entered_tx: std::sync::mpsc::Sender<Instant>,
) -> RawWriterSlowIoProbeGuardV1 {
    let mut slot = raw_writer_slow_io_probe_slot()
        .lock()
        .expect("test slow-I/O probe mutex must not be poisoned");
    assert!(
        slot.is_none(),
        "the isolated capture-enabled A/B harness owns the test slow-I/O probe"
    );
    *slot = Some(RawWriterSlowIoProbeV1 { delay, entered_tx });
    RawWriterSlowIoProbeGuardV1
}

#[cfg(test)]
fn maybe_delay_raw_writer_before_sync_for_test() {
    let probe = raw_writer_slow_io_probe_slot()
        .lock()
        .expect("test slow-I/O probe mutex must not be poisoned")
        .clone();
    if let Some(probe) = probe {
        // This notification and delay are test-only. They let the harness
        // record a receive-side fatal reason while the writer is deliberately
        // inside a slow rotation/sync window.
        let _ = probe.entered_tx.send(Instant::now());
        thread::sleep(probe.delay);
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SourceWriteOutcomeV1 {
    SourceRecord,
    RecordLimitGap,
}

impl RawSegmentWriterV1 {
    #[cfg(test)]
    fn new(
        raw_dir: PathBuf,
        run_id: String,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
    ) -> Self {
        Self::new_with_receipt_sink(
            raw_dir,
            run_id,
            flush_interval,
            segment_max_bytes,
            segment_max_duration,
            None,
        )
    }

    fn new_with_receipt_sink(
        raw_dir: PathBuf,
        run_id: String,
        flush_interval: Duration,
        segment_max_bytes: u64,
        segment_max_duration: Duration,
        receipt_sink: Option<Arc<Mutex<PumpResearchWriterSummaryV1>>>,
    ) -> Self {
        Self {
            raw_dir,
            run_id,
            flush_interval,
            segment_max_bytes,
            segment_max_duration,
            next_index: 0,
            previous_segment_blake3: None,
            current: None,
            last_flush: Instant::now(),
            last_source_boundary: PumpRawCoverageBoundaryV1::default(),
            receipt_sink,
            #[cfg(test)]
            fail_next_open_for_test: false,
        }
    }

    fn ensure_open(&mut self, stream_epoch: u64) -> Result<()> {
        if self.current.is_some() {
            return Ok(());
        }
        #[cfg(test)]
        if std::mem::replace(&mut self.fail_next_open_for_test, false) {
            bail!("injected raw segment open failure");
        }
        let index = self.next_index;
        let filename = format!("segment_{index:05}.bin");
        let final_path = self.raw_dir.join(&filename);
        let partial_path = self.raw_dir.join(format!("{filename}.partial"));
        let opened_wall_ts_ms = wall_clock_ms();
        let header = PumpRawSegmentHeaderV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            run_id: self.run_id.clone(),
            segment_index: index,
            stream_epoch,
            opened_wall_ts_ms,
            opened_monotonic_ts_ms: crate::types::arrival_time_ms(),
            previous_segment_blake3: self.previous_segment_blake3,
        };
        let header_bytes = PumpResearchRawCodecV1::encode_segment_header(&header)
            .context("encode frozen V1 raw segment header")?;
        let file = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&partial_path)
            .with_context(|| format!("create raw partial segment {}", partial_path.display()))?;
        let mut writer = BufWriter::new(file);
        writer
            .write_all(&header_bytes)
            .with_context(|| format!("write raw segment header {}", partial_path.display()))?;
        let mut prefix_hasher = blake3::Hasher::new();
        prefix_hasher.update(&header_bytes);
        let mut file_blake3_hasher = blake3::Hasher::new();
        file_blake3_hasher.update(&header_bytes);
        let mut file_sha256_hasher = Sha256::new();
        file_sha256_hasher.update(&header_bytes);
        self.current = Some(OpenSegmentV1 {
            index,
            stream_epoch,
            partial_path,
            final_path,
            writer,
            prefix_hasher,
            file_blake3_hasher,
            file_sha256_hasher,
            bytes_before_footer: header_bytes.len() as u64,
            records_before_footer: 0,
            first_capture_sequence: None,
            last_capture_sequence: None,
            opened_at: Instant::now(),
        });
        self.next_index = self.next_index.saturating_add(1);
        self.last_flush = Instant::now();
        Ok(())
    }

    fn rotate_if_needed(&mut self, stream_epoch: u64, upcoming_frame_len: usize) -> Result<()> {
        let rotate = self.current.as_ref().is_some_and(|current| {
            current.stream_epoch != stream_epoch
                || (current.records_before_footer > 0
                    && (current.opened_at.elapsed() >= self.segment_max_duration
                        || current
                            .bytes_before_footer
                            .saturating_add(upcoming_frame_len as u64)
                            > self.segment_max_bytes))
        });
        if rotate {
            self.close_current(false)?;
        }
        self.ensure_open(stream_epoch)
    }

    fn write_record(
        &mut self,
        record: PumpResearchRawRecordV1,
        stream_epoch: u64,
        capture_sequence: Option<u64>,
    ) -> Result<()> {
        let frame = PumpResearchRawCodecV1::encode_record(&record)
            .context("encode frozen V1 raw record")?;
        self.rotate_if_needed(stream_epoch, frame.len())?;
        let current = self
            .current
            .as_mut()
            .ok_or_else(|| anyhow::anyhow!("raw segment was not opened"))?;
        current
            .writer
            .write_all(&frame)
            .with_context(|| format!("append raw record to {}", current.partial_path.display()))?;
        current.prefix_hasher.update(&frame);
        current.file_blake3_hasher.update(&frame);
        current.file_sha256_hasher.update(&frame);
        current.bytes_before_footer = current
            .bytes_before_footer
            .saturating_add(frame.len() as u64);
        current.records_before_footer = current.records_before_footer.saturating_add(1);
        if let Some(sequence) = capture_sequence {
            current.first_capture_sequence.get_or_insert(sequence);
            current.last_capture_sequence = Some(sequence);
        }
        if self.last_flush.elapsed() >= self.flush_interval {
            current
                .writer
                .flush()
                .with_context(|| format!("flush raw segment {}", current.partial_path.display()))?;
            self.last_flush = Instant::now();
        }
        Ok(())
    }

    fn write_source(&mut self, queued: QueuedSourceUpdateV1) -> Result<SourceWriteOutcomeV1> {
        let stream_epoch = queued.update.stream_epoch;
        let boundary = source_boundary(&queued.update);
        let provider_id = queued.update.provider_id.clone();
        let ingress_wall_ts_ms = queued.update.ingress_wall_ts_ms;
        let record = raw_record_from_source(queued.capture_sequence, queued.update)?;
        match PumpResearchRawCodecV1::encode_record(&record) {
            Ok(_) => {
                self.write_record(record, stream_epoch, Some(queued.capture_sequence))?;
                self.last_source_boundary = raw_boundary_from_local(&boundary);
                Ok(SourceWriteOutcomeV1::SourceRecord)
            }
            Err(PumpResearchRawCodecErrorV1::RecordTooLarge { actual_bytes, .. }) => {
                self.write_record_limit_gap(
                    stream_epoch,
                    queued.capture_sequence,
                    provider_id,
                    raw_boundary_from_local(&boundary),
                    ingress_wall_ts_ms,
                    actual_bytes,
                )?;
                Ok(SourceWriteOutcomeV1::RecordLimitGap)
            }
            Err(error) => Err(anyhow::anyhow!(error)),
        }
    }

    fn write_record_limit_gap(
        &mut self,
        stream_epoch: u64,
        capture_sequence: u64,
        provider_id: String,
        dropped: PumpRawCoverageBoundaryV1,
        timestamp_ms: u64,
        actual_bytes: usize,
    ) -> Result<()> {
        let mut id_hasher = blake3::Hasher::new();
        id_hasher.update(b"pump_research_record_exceeds_frozen_limit_v1");
        id_hasher.update(&(provider_id.len() as u64).to_le_bytes());
        id_hasher.update(provider_id.as_bytes());
        id_hasher.update(&stream_epoch.to_le_bytes());
        id_hasher.update(&capture_sequence.to_le_bytes());
        id_hasher.update(&(actual_bytes as u64).to_le_bytes());
        let gap = PumpRawCoverageGapV1 {
            gap_id_blake3: PumpResearchStorageHashV1::from(*id_hasher.finalize().as_bytes()),
            provider_id,
            stream_epoch,
            episode_sequence: capture_sequence,
            reason: PumpRawCoverageGapReasonV1::RecordExceedsFrozenLimit,
            before: self.last_source_boundary,
            // The record is omitted permanently; no later observed event can
            // prove an `after` boundary for this particular writer rejection.
            after: PumpRawCoverageBoundaryV1::default(),
            missing_event_count: 1,
            first_dropped: dropped,
            last_dropped: dropped,
            queue_high_water: actual_bytes as u64,
            started_at_wall_ms: timestamp_ms,
            ended_at_wall_ms: timestamp_ms,
            recovered: false,
        };
        self.write_record(
            PumpResearchRawRecordV1::CoverageGap(gap),
            stream_epoch,
            None,
        )
    }

    fn write_gap(&mut self, gap: PumpRawCoverageGapV1) -> Result<()> {
        let stream_epoch = gap.stream_epoch;
        self.write_record(
            PumpResearchRawRecordV1::CoverageGap(gap),
            stream_epoch,
            None,
        )
    }

    /// Publish evidence for a segment as soon as its final path exists.  The
    /// following directory sync can still fail, in which case the run becomes
    /// incomplete, but a visible `.bin` is never omitted from the completion
    /// receipt merely because a later durability step failed.
    fn publish_closed_receipt(&self, receipt: PumpResearchSegmentReceiptV1) {
        if let Some(sink) = self.receipt_sink.as_ref() {
            sink.lock().segments.push(receipt);
        }
    }

    #[cfg(test)]
    fn inject_next_open_failure(&mut self) {
        self.fail_next_open_for_test = true;
    }

    fn close_current(
        &mut self,
        clean_shutdown: bool,
    ) -> Result<Option<PumpResearchSegmentReceiptV1>> {
        let Some(mut current) = self.current.take() else {
            return Ok(None);
        };
        let prefix_hash =
            PumpResearchStorageHashV1::from(*current.prefix_hasher.finalize().as_bytes());
        let footer = PumpRawSegmentClosedV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            segment_index: current.index,
            accepted_record_count: current.records_before_footer,
            data_bytes: current.bytes_before_footer,
            segment_blake3: prefix_hash,
            closed_wall_ts_ms: wall_clock_ms(),
            clean_shutdown,
        };
        let footer_frame =
            PumpResearchRawCodecV1::encode_record(&PumpResearchRawRecordV1::SegmentClosed(footer))
                .context("encode frozen V1 raw segment footer")?;
        current.writer.write_all(&footer_frame).with_context(|| {
            format!(
                "append raw segment footer {}",
                current.partial_path.display()
            )
        })?;
        current.file_blake3_hasher.update(&footer_frame);
        current.file_sha256_hasher.update(&footer_frame);
        #[cfg(test)]
        maybe_delay_raw_writer_before_sync_for_test();
        current
            .writer
            .flush()
            .with_context(|| format!("flush raw segment {}", current.partial_path.display()))?;
        current
            .writer
            .get_ref()
            .sync_all()
            .with_context(|| format!("sync raw segment {}", current.partial_path.display()))?;
        drop(current.writer);
        fs::rename(&current.partial_path, &current.final_path).with_context(|| {
            format!(
                "atomically publish raw segment {} -> {}",
                current.partial_path.display(),
                current.final_path.display()
            )
        })?;
        let sha256: [u8; 32] = current.file_sha256_hasher.finalize().into();
        let file_blake3 =
            PumpResearchStorageHashV1::from(*current.file_blake3_hasher.finalize().as_bytes());
        let filename = current
            .final_path
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| anyhow::anyhow!("published segment filename is not UTF-8"))?
            .to_owned();
        let receipt = PumpResearchSegmentReceiptV1 {
            segment_index: current.index,
            filename,
            file_sha256: PumpResearchStorageHashV1::from(sha256),
            file_blake3,
            first_capture_sequence: current.first_capture_sequence,
            last_capture_sequence: current.last_capture_sequence,
            accepted_record_count: current.records_before_footer,
        };
        self.publish_closed_receipt(receipt.clone());
        sync_directory(&self.raw_dir)?;
        self.previous_segment_blake3 = Some(prefix_hash);
        Ok(Some(receipt))
    }
}

fn sync_directory(path: &Path) -> Result<()> {
    let directory = File::open(path)
        .with_context(|| format!("open raw segment directory {} for sync", path.display()))?;
    directory
        .sync_all()
        .with_context(|| format!("sync raw segment directory {}", path.display()))
}

fn raw_writer_main(
    raw_dir: &Path,
    run_id: &str,
    data_rx: crossbeam_channel::Receiver<QueuedSourceUpdateV1>,
    control_rx: crossbeam_channel::Receiver<CaptureControlV1>,
    ingress: Arc<PumpResearchCaptureIngressV1>,
    progress: Arc<Mutex<PumpResearchWriterSummaryV1>>,
    flush_interval: Duration,
    segment_max_bytes: u64,
    segment_max_duration: Duration,
) -> Result<()> {
    let mut writer = RawSegmentWriterV1::new_with_receipt_sink(
        raw_dir.to_path_buf(),
        run_id.to_owned(),
        flush_interval,
        segment_max_bytes,
        segment_max_duration,
        Some(Arc::clone(&progress)),
    );
    // This is the bounded hand-off immediately after the Yellowstone source
    // tap, so the persisted reason is ingress saturation rather than a
    // downstream evidence-sink failure.
    let mut local_gap_tracker =
        LocalGapTracker::new(LocalCoverageGapReasonV1::IngressQueueSaturated);
    let mut pending = BTreeMap::<u64, OrderedIngressEventV1>::new();
    let mut next_capture_sequence = 0u64;

    loop {
        // A fatal reason may have been atomically recorded by the receive
        // task while this writer was idle.  Token cancellation is deliberately
        // dispatched here, rather than in `try_capture`, because Tokio's
        // CancellationToken may synchronously lock and walk its waiter tree.
        // Keep draining afterwards: accepted evidence remains immutable and a
        // missing ordered outcome must still make the run fail closed.
        let _ = ingress.cancel_source_from_writer_if_fatal();
        let mut made_progress = false;
        while let Ok(CaptureControlV1::DroppedSource(dropped)) = control_rx.try_recv() {
            insert_ordered_ingress_event(
                &mut pending,
                dropped.capture_sequence,
                OrderedIngressEventV1::Dropped(dropped),
            )?;
            made_progress = true;
        }
        while let Ok(queued) = data_rx.try_recv() {
            let capture_sequence = queued.capture_sequence;
            insert_ordered_ingress_event(
                &mut pending,
                capture_sequence,
                OrderedIngressEventV1::Source(queued),
            )?;
            made_progress = true;
        }

        while let Some(event) = pending.remove(&next_capture_sequence) {
            process_ordered_ingress_event(event, &mut writer, &mut local_gap_tracker, &progress)?;
            next_capture_sequence = next_capture_sequence.saturating_add(1);
            made_progress = true;
        }

        if ingress.source_finished.load(Ordering::Acquire) {
            let final_capture_sequence = ingress.final_capture_sequence.load(Ordering::Acquire);
            if next_capture_sequence == final_capture_sequence
                && pending.is_empty()
                && data_rx.is_empty()
                && control_rx.is_empty()
            {
                // A terminal saturation episode has no truthful subsequent
                // admitted source boundary.  Preserve it as an explicit,
                // unrecovered process-boundary gap in the writer, never from
                // the receive task.
                local_gap_tracker.close_open_without_after();
                persist_completed_local_gaps(&mut local_gap_tracker, &mut writer, &progress)?;

                let lifecycle = ingress.lifecycle();
                let summary = progress.lock().clone();
                if lifecycle.dropped_source_updates != summary.persisted_ingress_gap_missing_events
                {
                    bail!(
                        "Pump Research dropped {} source updates but persisted ingress gaps account for {}",
                        lifecycle.dropped_source_updates,
                        summary.persisted_ingress_gap_missing_events
                    );
                }
                writer.close_current(true)?;
                let mut state = progress.lock();
                state.clean_shutdown = true;
                return Ok(());
            }
            if data_rx.is_empty() && control_rx.is_empty() {
                bail!(
                    "Pump Research source finished at capture sequence {} but writer is missing sequence {}",
                    final_capture_sequence,
                    next_capture_sequence
                );
            }
        }

        if !made_progress {
            crossbeam_channel::select! {
                recv(control_rx) -> control => match control {
                    Ok(CaptureControlV1::DroppedSource(dropped)) => {
                        insert_ordered_ingress_event(
                            &mut pending,
                            dropped.capture_sequence,
                            OrderedIngressEventV1::Dropped(dropped),
                        )?;
                    }
                    Err(_) if !ingress.source_finished.load(Ordering::Acquire) => {
                        bail!("Pump Research drop-control lane disconnected before source lifecycle finished");
                    }
                    Err(_) => {}
                },
                recv(data_rx) -> source => match source {
                    Ok(queued) => {
                        let capture_sequence = queued.capture_sequence;
                        insert_ordered_ingress_event(
                            &mut pending,
                            capture_sequence,
                            OrderedIngressEventV1::Source(queued),
                        )?;
                    }
                    Err(_) if !ingress.source_finished.load(Ordering::Acquire) => {
                        bail!("Pump Research data queue disconnected before source lifecycle finished");
                    }
                    Err(_) => {}
                },
                default(WRITER_IDLE_POLL_V1) => {}
            }
        }
    }
}

fn insert_ordered_ingress_event(
    pending: &mut BTreeMap<u64, OrderedIngressEventV1>,
    capture_sequence: u64,
    event: OrderedIngressEventV1,
) -> Result<()> {
    if pending.insert(capture_sequence, event).is_some() {
        bail!(
            "Pump Research received duplicate ingress outcome for capture sequence {capture_sequence}"
        );
    }
    Ok(())
}

fn process_ordered_ingress_event(
    event: OrderedIngressEventV1,
    writer: &mut RawSegmentWriterV1,
    local_gap_tracker: &mut LocalGapTracker,
    progress: &Arc<Mutex<PumpResearchWriterSummaryV1>>,
) -> Result<()> {
    match event {
        OrderedIngressEventV1::Source(queued) => {
            let boundary = source_boundary(&queued.update);
            local_gap_tracker.observe_admitted(boundary);
            persist_completed_local_gaps(local_gap_tracker, writer, progress)?;
            let outcome = writer.write_source(queued)?;
            let mut state = progress.lock();
            match outcome {
                SourceWriteOutcomeV1::SourceRecord => {
                    state.accepted_source_records = state.accepted_source_records.saturating_add(1);
                }
                SourceWriteOutcomeV1::RecordLimitGap => {
                    state.gap_count = state.gap_count.saturating_add(1);
                }
            }
        }
        OrderedIngressEventV1::Dropped(dropped) => {
            local_gap_tracker.observe_saturation(
                dropped.provider_id,
                dropped.stream_epoch,
                local_boundary_from_raw(&dropped.boundary),
                dropped.queue_high_water,
            );
        }
    }
    Ok(())
}

fn persist_completed_local_gaps(
    local_gap_tracker: &mut LocalGapTracker,
    writer: &mut RawSegmentWriterV1,
    progress: &Arc<Mutex<PumpResearchWriterSummaryV1>>,
) -> Result<()> {
    while let Some(gap) = local_gap_tracker.take_completed() {
        let missing_event_count = gap.missing_event_count;
        writer.write_gap(raw_gap_from_local(gap))?;
        let mut state = progress.lock();
        state.gap_count = state.gap_count.saturating_add(1);
        state.persisted_ingress_gap_episodes =
            state.persisted_ingress_gap_episodes.saturating_add(1);
        state.persisted_ingress_gap_missing_events = state
            .persisted_ingress_gap_missing_events
            .saturating_add(missing_event_count);
    }
    // The writer drains completed episodes immediately after every ordered
    // source outcome, so normal capture cannot accumulate the generic
    // tracker's 1,024-entry completed buffer.  Keep this explicit fail-closed
    // assertion anyway: a future refactor must never convert an overflow into
    // an unreported loss merely because some earlier gaps were persisted.
    if local_gap_tracker.completed_overflowed() {
        bail!(
            "Pump Research local coverage-gap tracker overflowed after dropping completed episodes"
        );
    }
    Ok(())
}

/// Convert exactly one admitted decoded source message into a frozen V1 raw
/// record.  This runs only in the writer thread.  The source payload is the
/// deterministic `prost` encoding of the selected `update_oneof` payload, not
/// an alleged original gRPC wire frame.
fn raw_record_from_source(
    capture_sequence: u64,
    update: PumpResearchSourceUpdateV1,
) -> Result<PumpResearchRawRecordV1> {
    let event_time = PumpResearchEventTimeV1 {
        chain_event_ts_ms: None,
        ingress_wall_ts_ms: Some(update.ingress_wall_ts_ms),
        ingress_monotonic_ts_ms: Some(update.ingress_monotonic_ts_ms),
    };
    let source = |source_payload: &[u8]| PumpRawSourceEnvelopeV1 {
        provider_id: update.provider_id.clone(),
        provider_role: PumpResearchProviderRoleV1::PrimaryAuthority,
        stream_epoch: update.stream_epoch,
        capture_sequence,
        payload_hash_blake3: hash_bytes(source_payload),
    };

    match update.update.update_oneof {
        Some(UpdateOneof::Transaction(transaction)) => {
            let source_payload = transaction.encode_to_vec();
            let transaction_info = transaction.transaction.ok_or_else(|| {
                anyhow::anyhow!("source SubscribeUpdateTransaction lacks transaction info")
            })?;
            let signature = fixed_signature(&transaction_info.signature)
                .context("source SubscribeUpdateTransaction has non-64-byte signature")?;
            let tx_index = u32::try_from(transaction_info.index).ok();
            Ok(PumpResearchRawRecordV1::PrimaryTransaction(
                PumpPrimaryTransactionEvidenceV1 {
                    source: source(&source_payload),
                    slot: transaction.slot,
                    tx_index,
                    signature,
                    event_time,
                    // BlockMeta is independently preserved.  A later
                    // BlockMeta must not retroactively mutate this immutable
                    // source record merely to fill this convenience field.
                    block_time: None,
                    source_payload,
                },
            ))
        }
        Some(UpdateOneof::Account(account_update)) => {
            let source_payload = account_update.encode_to_vec();
            let account = account_update.account.ok_or_else(|| {
                anyhow::anyhow!("source SubscribeUpdateAccount lacks account payload")
            })?;
            let account_pubkey = fixed_pubkey(&account.pubkey)
                .context("source SubscribeUpdateAccount has non-32-byte pubkey")?;
            let owner_program = fixed_pubkey(&account.owner)
                .context("source SubscribeUpdateAccount has non-32-byte owner")?;
            let txn_signature = match account.txn_signature {
                Some(signature) => Some(
                    fixed_signature(&signature)
                        .context("source AccountUpdate has non-64-byte txn_signature")?,
                ),
                None => None,
            };
            let global = PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1
                .parse::<Pubkey>()
                .context("frozen canonical Pump Global pubkey is invalid")?;
            let account_role = if account_pubkey == tape::pump_research_storage_pubkey_v1(global) {
                PumpResearchAccountRoleV1::TransitionDependencyGlobal
            } else {
                PumpResearchAccountRoleV1::BondingCurve
            };
            let raw_account_data_hash_blake3 = hash_bytes(&account.data);
            Ok(PumpResearchRawRecordV1::PrimaryAccountUpdate(
                PumpPrimaryAccountUpdateEvidenceV1 {
                    source: source(&source_payload),
                    account_role,
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
            ))
        }
        Some(UpdateOneof::Slot(slot_update)) => {
            let source_payload = slot_update.encode_to_vec();
            Ok(PumpResearchRawRecordV1::PrimarySlotUpdate(
                PumpPrimarySlotEvidenceV1 {
                    source: source(&source_payload),
                    slot: slot_update.slot,
                    parent: slot_update.parent,
                    source_status: slot_update.status,
                    event_time,
                    source_payload,
                },
            ))
        }
        Some(UpdateOneof::BlockMeta(block_meta)) => {
            let source_payload = block_meta.encode_to_vec();
            Ok(PumpResearchRawRecordV1::PrimaryBlockMeta(
                PumpPrimaryBlockMetaEvidenceV1 {
                    source: source(&source_payload),
                    slot: block_meta.slot,
                    parent_slot: block_meta.parent_slot,
                    block_time: block_meta.block_time.map(|time| time.timestamp),
                    event_time,
                    source_payload,
                },
            ))
        }
        Some(UpdateOneof::TransactionStatus(_)) => {
            bail!("Pump Research V1 source profile received unsupported TransactionStatus update")
        }
        Some(UpdateOneof::Block(_)) => {
            bail!("Pump Research V1 source profile received unsupported Block update")
        }
        Some(UpdateOneof::Entry(_)) => {
            bail!("Pump Research V1 source profile received Entry although Entry is disabled")
        }
        Some(UpdateOneof::Ping(_)) => {
            bail!("Pump Research V1 source profile received unsupported Ping update")
        }
        Some(UpdateOneof::Pong(_)) => {
            bail!("Pong must be handled by the gRPC receive loop before raw capture")
        }
        None => bail!("Pump Research V1 source message has no update_oneof payload"),
    }
}

fn fixed_pubkey(bytes: &[u8]) -> Result<PumpResearchStoragePubkeyV1> {
    let fixed: [u8; 32] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 32 bytes, got {}", bytes.len()))?;
    Ok(PumpResearchStoragePubkeyV1::from(fixed))
}

fn fixed_signature(bytes: &[u8]) -> Result<PumpResearchStorageSignatureV1> {
    let fixed: [u8; 64] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 64 bytes, got {}", bytes.len()))?;
    Ok(PumpResearchStorageSignatureV1::from(fixed))
}

fn signature_from_bytes(bytes: &[u8]) -> Result<Signature> {
    let fixed: [u8; 64] = bytes
        .try_into()
        .map_err(|_| anyhow::anyhow!("expected 64-byte signature, got {}", bytes.len()))?;
    Ok(Signature::from(fixed))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{binary_parser::DISC_BUY, grpc_connection::PUMP_FUN_PROGRAM_ID};
    use std::str::FromStr;
    use tempfile::tempdir;
    use yellowstone_grpc_proto::prelude::{
        CompiledInstruction, Message as GrpcMessage, SubscribeUpdate, SubscribeUpdateAccount,
        SubscribeUpdateAccountInfo, SubscribeUpdateBlockMeta, SubscribeUpdateSlot,
        SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo, Transaction as GrpcTransaction,
        TransactionStatusMeta,
    };

    fn source_update(oneof: UpdateOneof, stream_epoch: u64) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "primary-test".to_owned(),
            stream_epoch,
            ingress_wall_ts_ms: 1_700_000_000_000,
            ingress_monotonic_ts_ms: 42,
            update: SubscribeUpdate {
                filters: vec!["pump_research_transactions".to_owned()],
                update_oneof: Some(oneof),
            },
        }
    }

    fn transaction_update(
        slot: u64,
        signature_byte: u8,
        stream_epoch: u64,
    ) -> PumpResearchSourceUpdateV1 {
        source_update(
            UpdateOneof::Transaction(SubscribeUpdateTransaction {
                transaction: Some(SubscribeUpdateTransactionInfo {
                    signature: vec![signature_byte; 64],
                    is_vote: false,
                    transaction: None,
                    meta: None,
                    index: 0,
                }),
                slot,
            }),
            stream_epoch,
        )
    }

    fn account_update(
        slot: u64,
        account_pubkey: Pubkey,
        data: Vec<u8>,
        stream_epoch: u64,
    ) -> PumpResearchSourceUpdateV1 {
        source_update(
            UpdateOneof::Account(SubscribeUpdateAccount {
                account: Some(SubscribeUpdateAccountInfo {
                    pubkey: account_pubkey.to_bytes().to_vec(),
                    lamports: 1,
                    owner: PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1
                        .parse::<Pubkey>()
                        .expect("frozen Pump program id")
                        .to_bytes()
                        .to_vec(),
                    executable: false,
                    rent_epoch: 0,
                    data,
                    write_version: 7,
                    txn_signature: Some(vec![99; 64]),
                }),
                slot,
                is_startup: true,
            }),
            stream_epoch,
        )
    }

    fn decode_segment(path: &Path) -> (PumpRawSegmentHeaderV1, Vec<PumpResearchRawRecordV1>) {
        let bytes = fs::read(path).expect("read published segment");
        assert!(bytes.starts_with(&tape::PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1));
        let header_payload_len = u32::from_le_bytes(
            bytes[8..12]
                .try_into()
                .expect("segment header length bytes"),
        ) as usize;
        let header_end = 8 + 4 + header_payload_len + 32;
        let header = PumpResearchRawCodecV1::decode_segment_header(&bytes[..header_end])
            .expect("decode frozen header");
        let mut cursor = header_end;
        let mut records = Vec::new();
        while cursor < bytes.len() {
            let payload_len = u32::from_le_bytes(
                bytes[cursor..cursor + 4]
                    .try_into()
                    .expect("record length bytes"),
            ) as usize;
            let end = cursor + 4 + payload_len + 32;
            records.push(
                PumpResearchRawCodecV1::decode_record(&bytes[cursor..end])
                    .expect("decode frozen record"),
            );
            cursor = end;
        }
        (header, records)
    }

    #[test]
    fn standalone_config_rejects_any_record_limit_other_than_frozen_v1_value() {
        let config: PumpResearchCaptureConfigV1 = toml::from_str(
            r#"
primary_provider_id = "primary"
grpc_endpoint = "https://grpc.example"
rpc_endpoint = "https://rpc.example"
output_dir = "datasets/pump-research"
record_max_bytes = 16777215
"#,
        )
        .expect("parse config");
        assert!(config.validate().is_err());
    }

    #[test]
    fn source_record_uses_deterministic_inner_protobuf_payload_not_wire_claim() {
        let update = transaction_update(123, 7, 4);
        let record = raw_record_from_source(9, update).expect("convert transaction source record");
        let PumpResearchRawRecordV1::PrimaryTransaction(transaction) = record else {
            panic!("expected transaction raw record");
        };
        let decoded = SubscribeUpdateTransaction::decode(transaction.source_payload.as_slice())
            .expect("decode deterministic prost payload");
        assert_eq!(decoded.slot, 123);
        assert_eq!(
            transaction.source.payload_hash_blake3,
            hash_bytes(&transaction.source_payload)
        );
        assert_eq!(transaction.source.capture_sequence, 9);
        assert_eq!(transaction.event_time.ingress_monotonic_ts_ms, Some(42));
    }

    #[test]
    fn every_supported_raw_update_preserves_its_decoded_inner_payload() {
        let global = PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1
            .parse::<Pubkey>()
            .expect("frozen canonical Global");
        let account_record =
            raw_record_from_source(1, account_update(10, global, vec![1, 2, 3, 4], 2))
                .expect("convert account");
        let PumpResearchRawRecordV1::PrimaryAccountUpdate(account) = account_record else {
            panic!("expected account record");
        };
        assert_eq!(
            account.account_role,
            PumpResearchAccountRoleV1::TransitionDependencyGlobal
        );
        assert!(account.is_startup);
        assert_eq!(
            SubscribeUpdateAccount::decode(account.source_payload.as_slice())
                .expect("decode account payload")
                .slot,
            10
        );

        let slot_record = raw_record_from_source(
            2,
            source_update(
                UpdateOneof::Slot(SubscribeUpdateSlot {
                    slot: 11,
                    parent: Some(10),
                    status: 2,
                }),
                2,
            ),
        )
        .expect("convert slot");
        let PumpResearchRawRecordV1::PrimarySlotUpdate(slot) = slot_record else {
            panic!("expected slot record");
        };
        assert_eq!(
            SubscribeUpdateSlot::decode(slot.source_payload.as_slice())
                .expect("decode slot payload")
                .parent,
            Some(10)
        );

        let block_record = raw_record_from_source(
            3,
            source_update(
                UpdateOneof::BlockMeta(SubscribeUpdateBlockMeta {
                    slot: 12,
                    blockhash: "blockhash".to_owned(),
                    rewards: None,
                    block_time: None,
                    block_height: None,
                    parent_slot: 11,
                    parent_blockhash: "parent".to_owned(),
                    executed_transaction_count: 1,
                    entries_count: 0,
                }),
                2,
            ),
        )
        .expect("convert block meta");
        let PumpResearchRawRecordV1::PrimaryBlockMeta(block) = block_record else {
            panic!("expected block-meta record");
        };
        assert_eq!(
            SubscribeUpdateBlockMeta::decode(block.source_payload.as_slice())
                .expect("decode block-meta payload")
                .parent_slot,
            11
        );
    }

    #[test]
    fn bounded_capture_queue_emits_ordered_drop_markers_without_receive_path_gap_work() {
        let (data_tx, data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, control_rx) = crossbeam_channel::bounded(8);
        let ingress =
            PumpResearchCaptureIngressV1::new(data_tx, control_tx, 1, CancellationToken::new());

        ingress.try_capture(transaction_update(10, 1, 1));
        ingress.try_capture(transaction_update(11, 2, 1));
        ingress.try_capture(transaction_update(12, 3, 1));
        let _first = data_rx.recv().expect("first source update admitted");
        ingress.try_capture(transaction_update(13, 4, 1));

        let CaptureControlV1::DroppedSource(first) = control_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("first bounded drop marker");
        let CaptureControlV1::DroppedSource(last) = control_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("second bounded drop marker");
        assert_eq!(first.capture_sequence, 1);
        assert_eq!(last.capture_sequence, 2);
        assert_eq!(first.boundary.slot, Some(11));
        assert_eq!(last.boundary.slot, Some(12));
        assert!(ingress.lifecycle().fatal_capture_error.is_none());
    }

    #[test]
    fn terminal_saturation_marker_is_handed_to_writer_before_source_finished() {
        let (data_tx, _data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, control_rx) = crossbeam_channel::bounded(8);
        let ingress =
            PumpResearchCaptureIngressV1::new(data_tx, control_tx, 1, CancellationToken::new());

        ingress.try_capture(transaction_update(20, 1, 1));
        ingress.try_capture(transaction_update(21, 2, 1));
        ingress.finish_source();

        assert!(ingress.source_finished.load(Ordering::Acquire));
        let CaptureControlV1::DroppedSource(dropped) = control_rx
            .recv_timeout(Duration::from_millis(100))
            .expect("terminal drop must reach writer before finished flag");
        assert_eq!(dropped.capture_sequence, 1);
        assert_eq!(dropped.boundary.slot, Some(21));
        assert_eq!(ingress.final_capture_sequence.load(Ordering::Acquire), 2);
    }

    #[test]
    fn source_finish_waits_for_an_inflight_receive_admission_before_freezing_sequence() {
        let (data_tx, _data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, _control_rx) = crossbeam_channel::bounded(1);
        let ingress = Arc::new(PumpResearchCaptureIngressV1::new(
            data_tx,
            control_tx,
            1,
            CancellationToken::new(),
        ));
        let admission = ingress
            .try_begin_capture()
            .expect("test reservation must enter before shutdown");
        let for_finish = Arc::clone(&ingress);
        let (started_tx, started_rx) = std::sync::mpsc::channel();
        let (finished_tx, finished_rx) = std::sync::mpsc::channel();
        let finisher = std::thread::spawn(move || {
            started_tx.send(()).expect("signal finish start");
            for_finish.finish();
            finished_tx.send(()).expect("signal finish completion");
        });

        started_rx.recv().expect("finish thread started");
        assert!(
            finished_rx.recv_timeout(Duration::from_millis(20)).is_err(),
            "shutdown must not freeze a final sequence while receive admission owns a reservation"
        );
        assert!(!ingress.source_finished.load(Ordering::Acquire));
        drop(admission);
        finished_rx
            .recv_timeout(Duration::from_millis(250))
            .expect("finish completes after receive admission releases");
        finisher.join().expect("finish thread joins");
        assert!(ingress.source_finished.load(Ordering::Acquire));
        assert_eq!(ingress.final_capture_sequence.load(Ordering::Acquire), 0);
    }

    #[test]
    fn direct_ingress_saturation_regression_stays_nonblocking() {
        // This is intentionally a direct ingress regression, not the PR-A
        // capture-enabled A/B hot-path qualification gate and not a real
        // Yellowstone provider run.  It exercises the standalone ingress with
        // its data lane deliberately full.  The receive side must retain
        // bounded progress by emitting tiny control markers; it may not wait
        // for a writer, lock a gap tracker, serialize protobuf, hash a gap
        // identifier, or synchronously cancel the source.
        const UPDATES: usize = 4_096;
        let (data_tx, _data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, _control_rx) = crossbeam_channel::bounded(UPDATES);
        let abort = CancellationToken::new();
        let ingress = PumpResearchCaptureIngressV1::new(data_tx, control_tx, 1, abort.clone());
        let updates: Vec<_> = (0..UPDATES)
            .map(|index| transaction_update(index as u64 + 1, index as u8, 1))
            .collect();
        let mut latencies = Vec::with_capacity(UPDATES);

        for update in updates {
            let started = Instant::now();
            ingress.try_capture(update);
            latencies.push(started.elapsed());
        }

        latencies.sort_unstable();
        let p99 = latencies[(UPDATES * 99) / 100];
        let lifecycle = ingress.lifecycle();
        assert!(
            !abort.is_cancelled() && lifecycle.fatal_capture_error.is_none(),
            "a control lane sized for the burst must preserve every saturation marker"
        );
        assert_eq!(lifecycle.source_updates_received, UPDATES as u64);
        assert_eq!(lifecycle.admitted_source_updates, 1);
        assert_eq!(lifecycle.dropped_source_updates, (UPDATES - 1) as u64);
        assert!(
            p99 < Duration::from_millis(100),
            "direct receive ingress p99 {:?} indicates a blocking wait under saturation",
            p99
        );
    }

    #[test]
    fn writer_drains_accepted_records_and_publishes_only_closed_segment() {
        let directory = tempdir().expect("temporary raw directory");
        let coordinator = PumpResearchCaptureCoordinatorV1::start(
            directory.path(),
            "run-clean".to_owned(),
            8,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
        )
        .expect("start bounded raw writer");
        let sink = coordinator.source_sink();
        sink.try_capture(source_update(
            UpdateOneof::Slot(SubscribeUpdateSlot {
                slot: 100,
                parent: Some(99),
                status: 0,
            }),
            3,
        ));
        sink.try_capture(transaction_update(100, 11, 3));
        sink.try_capture(account_update(100, Pubkey::new_unique(), vec![1, 2, 3], 3));
        sink.try_capture(source_update(
            UpdateOneof::BlockMeta(SubscribeUpdateBlockMeta {
                slot: 100,
                blockhash: "blockhash".to_owned(),
                rewards: None,
                block_time: None,
                block_height: None,
                parent_slot: 99,
                parent_blockhash: "parent".to_owned(),
                executed_transaction_count: 1,
                entries_count: 0,
            }),
            3,
        ));
        sink.finish_source();
        let summary = coordinator.finish_and_join();

        assert!(summary.clean_shutdown, "writer must join after drain");
        assert!(summary.error.is_none());
        assert_eq!(summary.gap_count, 0);
        assert_eq!(summary.segments.len(), 1);
        let segment_path = directory.path().join(&summary.segments[0].filename);
        assert!(
            segment_path.exists(),
            "footer must precede atomic publication"
        );
        assert!(
            !directory.path().join("segment_00000.bin.partial").exists(),
            "clean writer must not leave a partial segment"
        );
        let (header, records) = decode_segment(&segment_path);
        assert_eq!(header.stream_epoch, 3);
        assert!(matches!(
            records.last(),
            Some(PumpResearchRawRecordV1::SegmentClosed(
                PumpRawSegmentClosedV1 {
                    clean_shutdown: true,
                    ..
                }
            ))
        ));
        assert_eq!(records.len(), 5);
        assert!(matches!(
            &records[0],
            PumpResearchRawRecordV1::PrimarySlotUpdate(_)
        ));
        assert!(matches!(
            &records[1],
            PumpResearchRawRecordV1::PrimaryTransaction(_)
        ));
        assert!(matches!(
            &records[2],
            PumpResearchRawRecordV1::PrimaryAccountUpdate(_)
        ));
        assert!(matches!(
            &records[3],
            PumpResearchRawRecordV1::PrimaryBlockMeta(_)
        ));
    }

    #[test]
    fn writer_rotates_segment_when_stream_epoch_changes() {
        let directory = tempdir().expect("temporary epoch-rotation raw directory");
        let coordinator = PumpResearchCaptureCoordinatorV1::start(
            directory.path(),
            "run-epoch-rotation".to_owned(),
            8,
            Duration::from_millis(1),
            1024 * 1024,
            Duration::from_secs(60),
        )
        .expect("start bounded epoch-rotation writer");
        let sink = coordinator.source_sink();
        sink.try_capture(transaction_update(100, 1, 1));
        sink.try_capture(transaction_update(101, 2, 2));
        sink.finish_source();
        let summary = coordinator.finish_and_join();

        assert!(summary.clean_shutdown);
        assert!(summary.error.is_none());
        assert_eq!(summary.accepted_source_records, 2);
        assert_eq!(summary.segments.len(), 2);

        let first_path = directory.path().join(&summary.segments[0].filename);
        let second_path = directory.path().join(&summary.segments[1].filename);
        let (first_header, first_records) = decode_segment(&first_path);
        let (second_header, second_records) = decode_segment(&second_path);

        assert_eq!(first_header.segment_index, 0);
        assert_eq!(first_header.stream_epoch, 1);
        assert_eq!(first_header.previous_segment_blake3, None);
        let first_prefix_hash = match first_records.last() {
            Some(PumpResearchRawRecordV1::SegmentClosed(footer)) => footer.segment_blake3,
            other => panic!("first epoch segment lacks a terminal footer: {other:?}"),
        };
        assert!(matches!(
            first_records.last(),
            Some(PumpResearchRawRecordV1::SegmentClosed(
                PumpRawSegmentClosedV1 {
                    segment_index: 0,
                    accepted_record_count: 1,
                    clean_shutdown: false,
                    ..
                }
            ))
        ));

        assert_eq!(second_header.segment_index, 1);
        assert_eq!(second_header.stream_epoch, 2);
        assert_eq!(
            second_header.previous_segment_blake3,
            Some(first_prefix_hash),
            "epoch rotation must preserve the prefix chain"
        );
        assert!(matches!(
            second_records.last(),
            Some(PumpResearchRawRecordV1::SegmentClosed(
                PumpRawSegmentClosedV1 {
                    segment_index: 1,
                    accepted_record_count: 1,
                    clean_shutdown: true,
                    ..
                }
            ))
        ));

        let PumpResearchRawRecordV1::PrimaryTransaction(first_transaction) = &first_records[0]
        else {
            panic!("first epoch segment must contain the epoch-1 transaction");
        };
        let PumpResearchRawRecordV1::PrimaryTransaction(second_transaction) = &second_records[0]
        else {
            panic!("second epoch segment must contain the epoch-2 transaction");
        };
        assert_eq!(first_transaction.source.stream_epoch, 1);
        assert_eq!(second_transaction.source.stream_epoch, 2);
        assert!(!directory.path().join("segment_00000.bin.partial").exists());
        assert!(!directory.path().join("segment_00001.bin.partial").exists());
    }

    #[test]
    fn writer_leaves_partial_segment_if_process_dies_before_footer_publication() {
        let directory = tempdir().expect("temporary raw directory");
        let mut writer = RawSegmentWriterV1::new(
            directory.path().to_path_buf(),
            "run-partial".to_owned(),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
        );
        writer
            .write_source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: transaction_update(200, 33, 2),
            })
            .expect("write source record before crash simulation");
        assert!(directory.path().join("segment_00000.bin.partial").exists());
        assert!(!directory.path().join("segment_00000.bin").exists());
        drop(writer);
    }

    #[test]
    fn record_above_frozen_limit_becomes_typed_gap_not_a_larger_storage_format() {
        let directory = tempdir().expect("temporary raw directory");
        let mut writer = RawSegmentWriterV1::new(
            directory.path().to_path_buf(),
            "run-oversize".to_owned(),
            Duration::from_secs(60),
            32 * 1024 * 1024,
            Duration::from_secs(60),
        );
        let outcome = writer
            .write_source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: account_update(
                    300,
                    Pubkey::new_unique(),
                    vec![0; PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 + 1],
                    1,
                ),
            })
            .expect("oversized source record must convert to a typed gap");
        assert_eq!(outcome, SourceWriteOutcomeV1::RecordLimitGap);
        let receipt = writer
            .close_current(true)
            .expect("close oversized segment")
            .expect("segment receipt");
        let (_, records) = decode_segment(&directory.path().join(receipt.filename));
        assert!(matches!(
            records.first(),
            Some(PumpResearchRawRecordV1::CoverageGap(PumpRawCoverageGapV1 {
                reason: PumpRawCoverageGapReasonV1::RecordExceedsFrozenLimit,
                recovered: false,
                ..
            }))
        ));
    }

    fn complete_writer_summary_for_status() -> PumpResearchWriterSummaryV1 {
        PumpResearchWriterSummaryV1 {
            segments: vec![PumpResearchSegmentReceiptV1 {
                segment_index: 0,
                filename: "segment_00000.bin".to_owned(),
                file_sha256: PumpResearchStorageHashV1::from([1; 32]),
                file_blake3: PumpResearchStorageHashV1::from([2; 32]),
                first_capture_sequence: Some(0),
                last_capture_sequence: Some(0),
                accepted_record_count: 1,
            }],
            accepted_source_records: 1,
            persisted_ingress_gap_missing_events: 0,
            persisted_ingress_gap_episodes: 0,
            gap_count: 0,
            clean_shutdown: true,
            error: None,
        }
    }

    fn complete_source_lifecycle_for_status() -> PumpResearchSourceLifecycleV1 {
        PumpResearchSourceLifecycleV1 {
            stream_established: true,
            source_updates_received: 1,
            admitted_source_updates: 1,
            source_workers_cleanly_stopped: true,
            dropped_source_updates: 0,
            fatal_capture_error: None,
            source_worker_error: None,
        }
    }

    fn program_receipt_fixture() -> tape::PumpProgramDataReceiptV1 {
        tape::PumpProgramDataReceiptV1 {
            pump_program_id: PumpResearchStoragePubkeyV1::from([1; 32]),
            pump_program_account_owner: PumpResearchStoragePubkeyV1::from([2; 32]),
            pump_programdata_pubkey: PumpResearchStoragePubkeyV1::from([3; 32]),
            program_data_owner: PumpResearchStoragePubkeyV1::from([2; 32]),
            program_data_hash_algorithm: "blake3-256".to_owned(),
            program_data_hash_blake3: PumpResearchStorageHashV1::from([4; 32]),
            program_deployment_slot: Some(5),
            observed_context_slot: 6,
            commitment: "finalized".to_owned(),
        }
    }

    #[test]
    fn completion_never_marks_unestablished_or_zero_record_source_complete() {
        // Predicate-level coverage only.  This deliberately does not claim to
        // simulate OS Ctrl-C, connector shutdown, or a real Yellowstone
        // transport lifecycle; those remain operational/provider checks.
        let start = program_receipt_fixture();
        let completion = start.clone();
        let source_result: Result<()> = Ok(());
        let writer = complete_writer_summary_for_status();
        let lifecycle = complete_source_lifecycle_for_status();
        assert_eq!(
            completion_status(
                true,
                &source_result,
                &writer,
                &lifecycle,
                Some(&completion),
                &start,
            ),
            PumpResearchRunCompletionStatusV1::Complete
        );

        let mut no_stream = lifecycle.clone();
        no_stream.stream_established = false;
        assert_eq!(
            completion_status(
                true,
                &source_result,
                &writer,
                &no_stream,
                Some(&completion),
                &start,
            ),
            PumpResearchRunCompletionStatusV1::Incomplete
        );

        let mut zero_records = lifecycle;
        zero_records.source_updates_received = 0;
        zero_records.admitted_source_updates = 0;
        let mut zero_writer = writer;
        zero_writer.accepted_source_records = 0;
        zero_writer.segments.clear();
        assert_eq!(
            completion_status(
                true,
                &source_result,
                &zero_writer,
                &zero_records,
                Some(&completion),
                &start,
            ),
            PumpResearchRunCompletionStatusV1::Incomplete
        );
    }

    #[test]
    fn drop_control_overflow_defers_cancellation_to_writer_control_plane() {
        let (data_tx, _data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, _control_rx) = crossbeam_channel::bounded(1);
        let abort = CancellationToken::new();
        let ingress = PumpResearchCaptureIngressV1::new(data_tx, control_tx, 1, abort.clone());
        ingress.try_capture(transaction_update(10, 1, 1));
        ingress.try_capture(transaction_update(11, 2, 1));
        ingress.try_capture(transaction_update(12, 3, 1));

        assert!(
            !abort.is_cancelled(),
            "receive-side fatal recording must not synchronously cancel the source token"
        );
        let lifecycle = ingress.lifecycle();
        assert_eq!(lifecycle.dropped_source_updates, 2);
        assert!(lifecycle.fatal_capture_error.is_some());
        assert!(
            ingress.cancel_source_from_writer_if_fatal(),
            "writer/control plane must dispatch the pending fatal cancellation"
        );
        assert!(abort.is_cancelled());
        assert!(
            !ingress.cancel_source_from_writer_if_fatal(),
            "source cancellation is dispatched exactly once"
        );

        let start = program_receipt_fixture();
        let completion = start.clone();
        let source_result: Result<()> = Ok(());
        let mut writer = complete_writer_summary_for_status();
        writer.persisted_ingress_gap_missing_events = 1;
        let mut failed_lifecycle = complete_source_lifecycle_for_status();
        failed_lifecycle.dropped_source_updates = 2;
        failed_lifecycle.fatal_capture_error = lifecycle.fatal_capture_error;
        assert_eq!(
            completion_status(
                false,
                &source_result,
                &writer,
                &failed_lifecycle,
                Some(&completion),
                &start,
            ),
            PumpResearchRunCompletionStatusV1::Incomplete,
            "an unpersisted drop marker is fatal even for optional capture"
        );
    }

    #[test]
    fn writer_poll_dispatches_pending_receive_fatal_cancellation_off_hot_path() {
        let directory = tempdir().expect("temporary raw directory");
        let (data_tx, data_rx) = crossbeam_channel::bounded(1);
        let (control_tx, control_rx) = crossbeam_channel::bounded(1);
        let abort = CancellationToken::new();
        let ingress = Arc::new(PumpResearchCaptureIngressV1::new(
            data_tx,
            control_tx,
            1,
            abort.clone(),
        ));
        ingress
            .record_fatal_capture_error(PumpResearchCaptureFatalReasonV1::DropControlLaneSaturated);
        assert!(
            !abort.is_cancelled(),
            "recording the receive-side fatal reason must not cancel synchronously"
        );

        let writer_ingress = Arc::clone(&ingress);
        let progress = Arc::new(Mutex::new(PumpResearchWriterSummaryV1::default()));
        let writer_progress = Arc::clone(&progress);
        let writer_directory = directory.path().to_path_buf();
        let writer = std::thread::spawn(move || {
            raw_writer_main(
                &writer_directory,
                "run-fatal-poll",
                data_rx,
                control_rx,
                writer_ingress,
                writer_progress,
                Duration::from_millis(1),
                1024 * 1024,
                Duration::from_secs(60),
            )
        });

        let deadline = Instant::now() + Duration::from_secs(1);
        while !abort.is_cancelled() && Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(1));
        }
        assert!(
            abort.is_cancelled(),
            "writer bounded poll must dispatch pending fatal cancellation"
        );

        ingress.finish();
        writer
            .join()
            .expect("writer poll thread joins")
            .expect("writer exits cleanly after source finish");
    }

    #[test]
    fn writer_owned_gap_certifier_coalesces_continuous_drops_in_order() {
        let directory = tempdir().expect("temporary raw directory");
        let mut writer = RawSegmentWriterV1::new(
            directory.path().to_path_buf(),
            "run-gap-order".to_owned(),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
        );
        let mut tracker = LocalGapTracker::new(LocalCoverageGapReasonV1::EvidenceQueueSaturated);
        let progress = Arc::new(Mutex::new(PumpResearchWriterSummaryV1::default()));
        let first = transaction_update(10, 1, 1);
        let dropped_one = transaction_update(11, 2, 1);
        let dropped_two = transaction_update(12, 3, 1);
        let after = transaction_update(13, 4, 1);

        process_ordered_ingress_event(
            OrderedIngressEventV1::Source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: first,
            }),
            &mut writer,
            &mut tracker,
            &progress,
        )
        .expect("write first source event");
        for (capture_sequence, update) in [(1, dropped_one), (2, dropped_two)] {
            process_ordered_ingress_event(
                OrderedIngressEventV1::Dropped(DroppedSourceUpdateV1 {
                    capture_sequence,
                    provider_id: update.provider_id.clone(),
                    stream_epoch: update.stream_epoch,
                    boundary: source_raw_boundary(&update),
                    queue_high_water: 1,
                }),
                &mut writer,
                &mut tracker,
                &progress,
            )
            .expect("record ordered dropped marker");
        }
        process_ordered_ingress_event(
            OrderedIngressEventV1::Source(QueuedSourceUpdateV1 {
                capture_sequence: 3,
                update: after,
            }),
            &mut writer,
            &mut tracker,
            &progress,
        )
        .expect("write source after saturation");

        let receipt = writer
            .close_current(true)
            .expect("close segment")
            .expect("published segment");
        let (_, records) = decode_segment(&directory.path().join(receipt.filename));
        let gaps: Vec<_> = records
            .iter()
            .filter_map(|record| match record {
                PumpResearchRawRecordV1::CoverageGap(gap) => Some(gap),
                _ => None,
            })
            .collect();
        assert_eq!(gaps.len(), 1);
        assert_eq!(gaps[0].missing_event_count, 2);
        assert_eq!(gaps[0].first_dropped.slot, Some(11));
        assert_eq!(gaps[0].last_dropped.slot, Some(12));
        assert_eq!(gaps[0].after.slot, Some(13));
        assert_eq!(
            progress.lock().persisted_ingress_gap_missing_events,
            2,
            "writer-side reconciliation counts each dropped source update"
        );
    }

    #[test]
    fn writer_persists_more_than_the_generic_completed_gap_cap_without_loss() {
        // `LocalGapTracker` has a bounded completed queue for its active
        // runtime users.  Pump Research drains it after every ordered source
        // outcome, so an arbitrarily long sequence of separate saturation
        // episodes must not silently lose episode 1,025.
        const EPISODES: usize = 1_025;
        let directory = tempdir().expect("temporary raw directory");
        let mut writer = RawSegmentWriterV1::new(
            directory.path().to_path_buf(),
            "run-gap-cap".to_owned(),
            Duration::from_secs(60),
            16 * 1024 * 1024,
            Duration::from_secs(60),
        );
        let mut tracker = LocalGapTracker::new(LocalCoverageGapReasonV1::EvidenceQueueSaturated);
        let progress = Arc::new(Mutex::new(PumpResearchWriterSummaryV1::default()));

        process_ordered_ingress_event(
            OrderedIngressEventV1::Source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: transaction_update(1, 1, 1),
            }),
            &mut writer,
            &mut tracker,
            &progress,
        )
        .expect("write initial source boundary");

        for episode in 0..EPISODES {
            let dropped =
                transaction_update(10_000 + episode as u64, (episode as u8).wrapping_add(2), 1);
            let dropped_sequence = 1 + (episode as u64 * 2);
            process_ordered_ingress_event(
                OrderedIngressEventV1::Dropped(DroppedSourceUpdateV1 {
                    capture_sequence: dropped_sequence,
                    provider_id: dropped.provider_id.clone(),
                    stream_epoch: dropped.stream_epoch,
                    boundary: source_raw_boundary(&dropped),
                    queue_high_water: 1,
                }),
                &mut writer,
                &mut tracker,
                &progress,
            )
            .expect("record saturation marker");
            process_ordered_ingress_event(
                OrderedIngressEventV1::Source(QueuedSourceUpdateV1 {
                    capture_sequence: dropped_sequence + 1,
                    update: transaction_update(
                        20_000 + episode as u64,
                        (episode as u8).wrapping_add(3),
                        1,
                    ),
                }),
                &mut writer,
                &mut tracker,
                &progress,
            )
            .expect("write source boundary after saturation");
        }

        assert!(
            !tracker.completed_overflowed(),
            "writer must drain coverage gaps before the generic 1,024-entry completed queue can overflow"
        );
        let receipt = writer
            .close_current(true)
            .expect("close gap-cap segment")
            .expect("segment receipt");
        let (_, records) = decode_segment(&directory.path().join(receipt.filename));
        let persisted_gaps = records
            .iter()
            .filter(|record| matches!(record, PumpResearchRawRecordV1::CoverageGap(_)))
            .count();
        assert_eq!(persisted_gaps, EPISODES);
        assert_eq!(
            progress.lock().persisted_ingress_gap_episodes,
            EPISODES as u64
        );
        assert_eq!(
            progress.lock().persisted_ingress_gap_missing_events,
            EPISODES as u64
        );
    }

    #[test]
    fn published_segment_receipt_survives_next_open_failure() {
        let directory = tempdir().expect("temporary raw directory");
        let progress = Arc::new(Mutex::new(PumpResearchWriterSummaryV1::default()));
        let mut writer = RawSegmentWriterV1::new_with_receipt_sink(
            directory.path().to_path_buf(),
            "run-rotate-failure".to_owned(),
            Duration::from_secs(60),
            1,
            Duration::from_secs(60),
            Some(Arc::clone(&progress)),
        );
        writer
            .write_source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: transaction_update(20, 1, 1),
            })
            .expect("write first segment source");
        writer.inject_next_open_failure();
        assert!(writer
            .write_source(QueuedSourceUpdateV1 {
                capture_sequence: 1,
                update: transaction_update(21, 2, 1),
            })
            .is_err());
        let published = &progress.lock().segments;
        assert_eq!(published.len(), 1);
        assert!(directory.path().join(&published[0].filename).exists());
    }

    #[test]
    fn incremental_segment_hashes_equal_bytes_on_disk() {
        let directory = tempdir().expect("temporary raw directory");
        let mut writer = RawSegmentWriterV1::new(
            directory.path().to_path_buf(),
            "run-incremental-hash".to_owned(),
            Duration::from_secs(60),
            1024 * 1024,
            Duration::from_secs(60),
        );
        writer
            .write_source(QueuedSourceUpdateV1 {
                capture_sequence: 0,
                update: transaction_update(22, 3, 1),
            })
            .expect("write source");
        let receipt = writer
            .close_current(true)
            .expect("close source")
            .expect("receipt");
        let bytes = fs::read(directory.path().join(&receipt.filename)).expect("read fixture bytes");
        let mut sha256 = Sha256::new();
        sha256.update(&bytes);
        let sha256: [u8; 32] = sha256.finalize().into();
        assert_eq!(receipt.file_sha256, PumpResearchStorageHashV1::from(sha256));
        assert_eq!(receipt.file_blake3, hash_bytes(&bytes));
    }

    #[test]
    fn writer_open_failure_cancels_source_capture() {
        let file = tempfile::NamedTempFile::new().expect("temporary file as invalid raw directory");
        let coordinator = PumpResearchCaptureCoordinatorV1::start(
            file.path(),
            "run-writer-failure".to_owned(),
            1,
            Duration::from_millis(1),
            1024,
            Duration::from_secs(60),
        )
        .expect("writer thread starts before opening a segment");
        let abort = coordinator.capture_abort();
        coordinator
            .source_sink()
            .try_capture(transaction_update(30, 1, 1));
        coordinator.finish_source();
        let summary = coordinator.finish_and_join();
        assert!(abort.is_cancelled());
        assert!(summary.error.is_some());
        assert!(!summary.clean_shutdown);
    }

    #[test]
    fn program_receipt_comparison_requires_same_programdata_identity_and_hash() {
        let receipt = program_receipt_fixture();
        assert!(program_receipts_match(&receipt, &receipt));
        let mut changed_hash = receipt.clone();
        changed_hash.program_data_hash_blake3 = PumpResearchStorageHashV1::from([9; 32]);
        assert!(!program_receipts_match(&receipt, &changed_hash));
        let mut changed_data_identity = receipt.clone();
        changed_data_identity.pump_programdata_pubkey = PumpResearchStoragePubkeyV1::from([8; 32]);
        assert!(!program_receipts_match(&receipt, &changed_data_identity));
    }

    #[test]
    fn programdata_receipt_uses_standalone_no_auth_when_rpc_token_is_absent() {
        let (_, mode) =
            program_data_receipt_rpc_client("https://rpc.nln.clr3.org", None, "x-api-key")
                .expect("construct no-auth ProgramData receipt client");
        assert_eq!(mode, PumpResearchProgramDataRpcAuthModeV1::StandaloneNoAuth);

        let (_, mode) = program_data_receipt_rpc_client(
            "https://rpc.nln.clr3.org",
            Some("research-only-rpc-token"),
            "x-api-key",
        )
        .expect("construct explicit-auth ProgramData receipt client");
        assert_eq!(
            mode,
            PumpResearchProgramDataRpcAuthModeV1::ExplicitStandaloneAuth
        );
    }

    fn operator_preflight_config(
        grpc_endpoint: &str,
        rpc_endpoint: &str,
    ) -> PumpResearchCaptureConfigV1 {
        PumpResearchCaptureConfigV1 {
            primary_provider_id: "operator-preflight-test".to_owned(),
            grpc_endpoint: grpc_endpoint.to_owned(),
            grpc_auth_token_env: None,
            grpc_auth_header: "x-token".to_owned(),
            rpc_endpoint: rpc_endpoint.to_owned(),
            rpc_auth_token_env: None,
            rpc_auth_header: "x-api-key".to_owned(),
            pump_program_id: PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1.to_owned(),
            output_dir: PathBuf::from("ignored-test-output"),
            required_for_run: true,
            queue_capacity: DEFAULT_QUEUE_CAPACITY_V1,
            flush_interval_ms: DEFAULT_FLUSH_INTERVAL_MS_V1,
            segment_max_bytes: DEFAULT_SEGMENT_MAX_BYTES_V1,
            segment_max_duration_ms: DEFAULT_SEGMENT_MAX_DURATION_MS_V1,
            record_max_bytes: PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
        }
    }

    #[test]
    fn operator_preflight_rejects_placeholder_and_inline_endpoint_credentials() {
        assert!(
            validate_operator_preflight_config(&operator_preflight_config(
                "https://your-yellowstone-provider.example",
                "https://rpc.example.net",
            ))
            .is_err()
        );
        assert!(
            validate_operator_preflight_config(&operator_preflight_config(
                "https://user:secret@yellowstone.example.net",
                "https://rpc.example.net",
            ))
            .is_err()
        );
        assert!(
            validate_operator_preflight_config(&operator_preflight_config(
                "https://yellowstone.example.net",
                "https://rpc.example.net?api-key=not-allowed",
            ))
            .is_err()
        );
        assert_eq!(
            validate_operator_preflight_config(&operator_preflight_config(
                "https://yellowstone.example.net",
                "https://rpc.example.net",
            ))
            .expect("approved endpoint references without a credential are allowed"),
            PumpResearchOperatorAuthPresenceV1 {
                grpc_token_present: false,
                rpc_token_present: false,
            }
        );
        assert!(
            validate_operator_preflight_config(&operator_preflight_config(
                "http://yellowstone.example.net",
                "https://rpc.example.net",
            ))
            .is_err()
        );
        assert!(
            validate_operator_preflight_config(&operator_preflight_config(
                "https://yellowstone.example.net/provider-path",
                "https://rpc.example.net",
            ))
            .is_err()
        );
    }

    #[test]
    fn required_ignored_fixture_is_hashed_and_copied_into_full_source_snapshot() {
        let repository = tempdir().expect("temporary Git repository");
        let root = repository.path();
        let init = Command::new("git")
            .args(["init", "--quiet"])
            .current_dir(root)
            .status()
            .expect("initialize temporary Git repository");
        assert!(init.success());
        fs::write(root.join(".gitignore"), "*.json\n").expect("write test .gitignore");
        fs::write(root.join("tracked.rs"), "pub const TRACKED: u8 = 1;\n")
            .expect("write tracked source");
        let add = Command::new("git")
            .args(["add", ".gitignore", "tracked.rs"])
            .current_dir(root)
            .status()
            .expect("add tracked test source");
        assert!(add.success());
        fs::write(
            root.join("ordinary_untracked.rs"),
            "pub const UNTRACKED: u8 = 2;\n",
        )
        .expect("write ordinary untracked source");
        let required_fixture =
            root.join("ghost-core/tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json");
        fs::create_dir_all(required_fixture.parent().expect("fixture parent"))
            .expect("create fixture parent");
        fs::write(&required_fixture, "{\"frozen\":true}\n").expect("write ignored fixture");

        let (inventory, _, _) = collect_untracked_inventory(root).expect("collect inventory");
        let required_path = OPERATOR_PREFLIGHT_REQUIRED_IGNORED_ARTIFACTS_V1[0];
        assert!(inventory.entries.iter().any(|entry| {
            entry.repository_relative_path == required_path
                && entry.source_kind
                    == PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture
        }));
        assert!(inventory.entries.iter().any(|entry| {
            entry.repository_relative_path == "ordinary_untracked.rs"
                && entry.source_kind == PumpResearchOperatorSourceEntryKindV1::Untracked
        }));
        let manifest =
            collect_source_snapshot_manifest(root, &inventory).expect("collect source manifest");
        assert!(manifest.entries.iter().any(|entry| {
            entry.repository_relative_path == required_path
                && entry.source_kind
                    == PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture
        }));
        let bundle = tempdir().expect("temporary bundle directory");
        snapshot_source_tree_from_manifest(root, &manifest, bundle.path())
            .expect("copy full source snapshot");
        assert_eq!(
            fs::read(
                bundle
                    .path()
                    .join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1)
                    .join(required_path),
            )
            .expect("read copied ignored fixture"),
            b"{\"frozen\":true}\n"
        );
        assert!(verify_source_snapshot_contents(bundle.path(), &manifest).is_ok());

        fs::write(&required_fixture, "{\"frozen\":false}\n")
            .expect("mutate live ignored fixture after snapshot");
        let (changed_inventory, _, _) =
            collect_untracked_inventory(root).expect("recollect changed inventory");
        let changed_manifest = collect_source_snapshot_manifest(root, &changed_inventory)
            .expect("recollect changed source manifest");
        assert_ne!(
            operator_digest_bytes(&immutable_json_bytes(&manifest).expect("serialize manifest")),
            operator_digest_bytes(
                &immutable_json_bytes(&changed_manifest).expect("serialize changed manifest"),
            )
        );
    }

    #[test]
    fn external_operator_config_is_required_so_raw_toml_cannot_enter_bundle() {
        let repository = tempdir().expect("temporary repository root");
        let internal_config = repository.path().join("configs/operator.toml");
        fs::create_dir_all(internal_config.parent().expect("internal config parent"))
            .expect("create internal config parent");
        fs::write(
            &internal_config,
            "rpc_endpoint = \"https://rpc.example.net\"\n",
        )
        .expect("write internal config");
        assert!(
            validate_external_operator_config_path(repository.path(), &internal_config).is_err()
        );

        let external = tempdir().expect("temporary external config root");
        let external_config = external.path().join("operator.toml");
        fs::write(
            &external_config,
            "rpc_endpoint = \"https://rpc.example.net\"\n",
        )
        .expect("write external config");
        assert_eq!(
            validate_external_operator_config_path(repository.path(), &external_config)
                .expect("accept external config"),
            fs::canonicalize(&external_config).expect("canonical external config")
        );
    }

    #[test]
    fn sealed_snapshot_cargo_config_strict_allowlist_accepts_exact_repository_config() {
        let temporary = tempdir().expect("temporary isolated build-cwd root");
        let source_snapshot = temporary.path().join("staging/source");
        fs::create_dir_all(source_snapshot.join(".cargo"))
            .expect("create source snapshot cargo config directory");
        fs::write(
            source_snapshot.join(".cargo/config.toml"),
            concat!(
                "[build]\n",
                "rustflags = [\"-C\", \"target-cpu=native\"]\n",
                "jobs = 4\n\n",
                "[profile.release]\n",
                "opt-level = 3\n",
                "lto = true\n",
                "codegen-units = 4\n",
            ),
        )
        .expect("write source snapshot Cargo config");

        let environment = collect_build_environment(
            &source_snapshot,
            operator_digest_bytes(b"cargo-executable"),
            operator_digest_bytes(b"rustc-executable"),
        )
        .expect("collect build environment");
        assert!(matches!(
            environment
                .cargo_config_file_digests
                .get("sealed_snapshot/.cargo/config.toml"),
            Some(Some(_))
        ));
        assert_eq!(environment.cargo_config_file_digests.len(), 2);
        assert_eq!(
            environment.child_environment_semantics,
            OPERATOR_PREFLIGHT_SANITIZED_BUILD_ENVIRONMENT_V1
        );
        assert_eq!(
            environment.cargo_config_scope_semantics,
            OPERATOR_PREFLIGHT_ISOLATED_CARGO_CONFIG_SCOPE_V1
        );
        assert!(environment.rustflags_digest.is_none());
        assert!(environment.cargo_encoded_rustflags_digest.is_none());
        assert!(environment.cargo_build_environment_digests.is_empty());
        assert!(environment
            .cargo_profile_release_environment_digests
            .is_empty());
    }

    #[test]
    fn isolated_build_rejects_any_ancestor_cargo_config_before_cargo_runs() {
        let temporary = tempdir().expect("temporary isolated build-cwd root");
        let staging_root = temporary.path().join("staging");
        let source_snapshot = staging_root.join("source");
        fs::create_dir_all(source_snapshot.join(".cargo"))
            .expect("create snapshot cargo directory");
        fs::create_dir_all(staging_root.join(".cargo")).expect("create ancestor cargo directory");
        fs::write(
            source_snapshot.join(".cargo/config.toml"),
            "[build]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
        )
        .expect("write sealed snapshot config");
        fs::write(
            staging_root.join(".cargo/config.toml"),
            "[build]\nrustc-wrapper = \"/outside/snapshot-wrapper\"\n",
        )
        .expect("write forbidden ancestor config");

        let error = collect_build_environment(
            &source_snapshot,
            operator_digest_bytes(b"cargo-executable"),
            operator_digest_bytes(b"rustc-executable"),
        )
        .expect_err("an ancestor Cargo config must fail before Cargo invocation");
        assert!(error.to_string().contains("forbidden Cargo config"));
    }

    #[test]
    fn sealed_snapshot_cargo_config_rejects_external_compiler_wrapper_and_target_tools() {
        for contents in [
            "[build]\nrustc-wrapper = \"/outside/snapshot-wrapper\"\n",
            "[build]\nrustc = \"/outside/snapshot-rustc\"\n",
            "[target.x86_64-unknown-linux-gnu]\nlinker = \"/outside/snapshot-linker\"\n",
            "[env]\nRUSTC = \"/outside/snapshot-rustc\"\n",
        ] {
            let temporary = tempdir().expect("temporary isolated build-cwd root");
            let source_snapshot = temporary.path().join("staging/source");
            fs::create_dir_all(source_snapshot.join(".cargo"))
                .expect("create source snapshot cargo config directory");
            fs::write(source_snapshot.join(".cargo/config.toml"), contents)
                .expect("write forbidden snapshot config");
            assert!(
                collect_build_environment(
                    &source_snapshot,
                    operator_digest_bytes(b"cargo-executable"),
                    operator_digest_bytes(b"rustc-executable"),
                )
                .is_err(),
                "sealed source config must reject {contents:?}"
            );
        }
    }

    #[test]
    fn sealed_snapshot_cargo_config_rejects_unsealed_rustflag_inputs() {
        for contents in [
            "[build]\nrustflags = [\"-C\", \"linker=/tmp/external-linker\"]\n",
            "[build]\nrustflags = [\"-C\", \"link-arg=/tmp/object.o\"]\n",
            "[build]\nrustflags = [\"-L\", \"native=/tmp/libs\"]\n",
            "[build]\nrustflags = [\"--sysroot\", \"/tmp/sysroot\"]\n",
            "[build]\nrustflags = [\"--extern\", \"dependency=/tmp/dependency.rlib\"]\n",
            "[build]\nrustflags = [\"@/tmp/rustflags.rsp\"]\n",
        ] {
            let temporary = tempdir().expect("temporary isolated build-cwd root");
            let source_snapshot = temporary.path().join("staging/source");
            fs::create_dir_all(source_snapshot.join(".cargo"))
                .expect("create source snapshot cargo config directory");
            fs::write(source_snapshot.join(".cargo/config.toml"), contents)
                .expect("write forbidden snapshot config");

            let error = collect_build_environment(
                &source_snapshot,
                operator_digest_bytes(b"cargo-executable"),
                operator_digest_bytes(b"rustc-executable"),
            )
            .expect_err("unsealed rustflag input must fail before Cargo invocation");
            assert!(
                error.to_string().contains("only admits build.rustflags"),
                "unexpected validation error for {contents:?}: {error:#}"
            );
        }
    }

    #[test]
    fn sealed_snapshot_cargo_config_rejects_target_and_unknown_surfaces() {
        for contents in [
            "[build]\ntarget = \"/tmp/external-target.json\"\n",
            "[target.x86_64-unknown-linux-gnu]\nrustflags = [\"-C\", \"target-cpu=native\"]\n",
            "[alias]\nsealed-build = \"build --release\"\n",
            "[net]\noffline = true\n",
            "[build]\nincremental = false\n",
            "[profile.dev]\nopt-level = 1\n",
            "[profile.release]\nstrip = true\n",
        ] {
            let temporary = tempdir().expect("temporary isolated build-cwd root");
            let source_snapshot = temporary.path().join("staging/source");
            fs::create_dir_all(source_snapshot.join(".cargo"))
                .expect("create source snapshot cargo config directory");
            fs::write(source_snapshot.join(".cargo/config.toml"), contents)
                .expect("write unsupported snapshot config");

            assert!(
                collect_build_environment(
                    &source_snapshot,
                    operator_digest_bytes(b"cargo-executable"),
                    operator_digest_bytes(b"rustc-executable"),
                )
                .is_err(),
                "strict allowlist must reject {contents:?}"
            );
        }
    }

    #[test]
    fn sealed_snapshot_cargo_config_rejects_unapproved_safe_field_values() {
        for contents in [
            "[build]\njobs = 8\n",
            "[build]\nrustflags = [\"-Ctarget-cpu=native\"]\n",
            "[profile.release]\nopt-level = 2\n",
            "[profile.release]\nlto = false\n",
            "[profile.release]\ncodegen-units = 1\n",
        ] {
            let temporary = tempdir().expect("temporary isolated build-cwd root");
            let source_snapshot = temporary.path().join("staging/source");
            fs::create_dir_all(source_snapshot.join(".cargo"))
                .expect("create source snapshot cargo config directory");
            fs::write(source_snapshot.join(".cargo/config.toml"), contents)
                .expect("write unapproved snapshot config");

            assert!(
                collect_build_environment(
                    &source_snapshot,
                    operator_digest_bytes(b"cargo-executable"),
                    operator_digest_bytes(b"rustc-executable"),
                )
                .is_err(),
                "strict allowlist must reject {contents:?}"
            );
        }
    }

    #[test]
    fn fresh_build_rejects_every_unsealed_compiler_and_profile_override() {
        for name in OPERATOR_PREFLIGHT_FRESH_BUILD_DENIED_ENV_V1 {
            let rejected = rejected_fresh_build_environment_overrides(vec![(
                OsString::from(name),
                OsString::from("synthetic-override"),
            )]);
            assert_eq!(rejected, vec![(*name).to_owned()], "must reject {name}");
        }
        let rejected = rejected_fresh_build_environment_overrides(vec![(
            OsString::from("CARGO_PROFILE_RELEASE_LTO"),
            OsString::from("fat"),
        )]);
        assert_eq!(rejected, vec!["CARGO_PROFILE_RELEASE_LTO".to_owned()]);
        assert!(rejected_fresh_build_environment_overrides(vec![(
            OsString::from("PATH"),
            OsString::from("/controlled/bin"),
        )])
        .is_empty());
    }

    #[test]
    fn sanitized_fresh_build_environment_has_only_explicit_safe_inputs() {
        let target = tempdir().expect("temporary fresh build target");
        let repository_root = current_repository_root().expect("resolve repository root");
        let cargo = resolve_executable_from_parent_path("cargo", &repository_root)
            .expect("resolve direct Cargo");
        let rustc = resolve_executable_from_parent_path("rustc", &repository_root)
            .expect("resolve direct rustc");
        let environment = sanitized_fresh_build_environment(
            target.path(),
            &target.path().join("target"),
            &cargo,
            &rustc,
        )
        .expect("construct sanitized child environment");
        let keys: BTreeSet<_> = environment
            .variables
            .keys()
            .map(|key| key.to_string_lossy().into_owned())
            .collect();
        assert_eq!(
            keys,
            BTreeSet::from([
                "CARGO_HOME".to_owned(),
                "CARGO_NET_OFFLINE".to_owned(),
                "CARGO_TARGET_DIR".to_owned(),
                "CARGO_TERM_COLOR".to_owned(),
                "HOME".to_owned(),
                "PATH".to_owned(),
            ])
        );
        assert_ne!(
            cargo.file_name().and_then(|name| name.to_str()),
            Some("rustup"),
            "Cargo provenance must bind the selected direct toolchain binary, not rustup proxy"
        );
        assert_ne!(
            rustc.file_name().and_then(|name| name.to_str()),
            Some("rustup"),
            "rustc provenance must bind the selected direct toolchain binary, not rustup proxy"
        );
        assert!(!keys.contains("GHOST_SEER_GRPC_X_TOKEN"));
        assert!(!keys.contains("GHOST_RPC_AUTH_TOKEN"));
        let cargo_home = PathBuf::from(
            environment
                .variables
                .get(&OsString::from("CARGO_HOME"))
                .expect("sanitized CARGO_HOME"),
        );
        assert!(!cargo_home.join("credentials.toml").exists());
        assert!(!cargo_home.join("config.toml").exists());
    }

    #[test]
    fn credential_scan_blocks_receipt_publication_before_a_final_receipt_exists() {
        let bundle = tempdir().expect("temporary sealed bundle");
        let synthetic_credential = b"pump-research-synthetic-credential".to_vec();
        fs::create_dir(bundle.path().join("release"))
            .expect("create release directory for synthetic bundle");
        write_bytes_create_new(
            &bundle.path().join(OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1),
            &synthetic_credential,
        )
        .expect("write synthetic credential leak");
        let credentials = vec![("gRPC".to_owned(), synthetic_credential)];
        assert!(
            ensure_credential_values_absent_from_sealed_bundle(bundle.path(), &credentials)
                .is_err(),
            "a credential byte match must fail before receipt publication"
        );
        assert!(
            !bundle
                .path()
                .join(OPERATOR_PREFLIGHT_RECEIPT_FILE_V1)
                .exists(),
            "the scanner runs before a final immutable receipt can exist"
        );
    }

    #[test]
    fn invalid_capture_preflight_stops_in_pure_pre_provider_phase() {
        let external = tempdir().expect("temporary external operator config root");
        let config_path = external.path().join("operator.toml");
        fs::write(
            &config_path,
            concat!(
                "primary_provider_id = \"test-provider\"\n",
                "grpc_endpoint = \"https://yellowstone.example.net\"\n",
                "grpc_auth_header = \"x-token\"\n",
                "rpc_endpoint = \"https://rpc.example.net\"\n",
                "rpc_auth_header = \"x-api-key\"\n",
                "pump_program_id = \"6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P\"\n",
                "output_dir = \"/tmp/pump-research-test-output\"\n",
                "required_for_run = true\n",
                "queue_capacity = 1\n",
                "flush_interval_ms = 1\n",
                "segment_max_bytes = 1\n",
                "segment_max_duration_ms = 1\n",
                "record_max_bytes = 16777216\n",
            ),
        )
        .expect("write external operator config");
        let missing_receipt = external.path().join("missing-receipt.json");
        TEST_PROVIDER_IO_PHASE_ENTRIES_V1.store(0, Ordering::Release);
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("build test runtime");
        // Exercise the public async API.  The test-only probe sits immediately
        // before the first ProgramData RPC call; therefore a zero value proves
        // that invalid receipt/config validation cannot even enter the
        // provider-I/O phase, let alone open Yellowstone.
        assert!(runtime
            .block_on(run_capture_from_config_path(&config_path, &missing_receipt))
            .is_err());
        assert_eq!(
            TEST_PROVIDER_IO_PHASE_ENTRIES_V1.load(Ordering::Acquire),
            0,
            "invalid receipt/config must fail before the provider-I/O phase"
        );
    }

    #[test]
    fn operator_preflight_bundle_detects_any_sidecar_tampering() {
        let temporary = tempdir().expect("temporary provenance bundle root");
        let bundle_dir = temporary.path().join("bundle");
        fs::create_dir(&bundle_dir).expect("create provenance bundle");
        let status = b" M off-chain/components/seer/src/research_tape.rs\0";
        let patch = b"diff --git a/a b/a\n";
        let required_fixture =
            "ghost-core/tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json";
        let inventory = PumpResearchOperatorPreflightUntrackedInventoryV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            entries: vec![PumpResearchOperatorPreflightUntrackedEntryV1 {
                repository_relative_path: required_fixture.to_owned(),
                source_kind: PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture,
                digest: operator_digest_bytes(b"frozen-corpus-manifest"),
            }],
        };
        let inventory_bytes = immutable_json_bytes(&inventory).expect("serialize inventory");
        let source_snapshot = PumpResearchOperatorSourceSnapshotManifestV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1.to_owned(),
            entries: vec![PumpResearchOperatorSourceSnapshotEntryV1 {
                repository_relative_path: required_fixture.to_owned(),
                source_kind: PumpResearchOperatorSourceEntryKindV1::RequiredIgnoredFixture,
                digest: operator_digest_bytes(b"frozen-corpus-manifest"),
            }],
        };
        let source_snapshot_bytes =
            immutable_json_bytes(&source_snapshot).expect("serialize source snapshot manifest");
        let redacted_config = b"{\"config\":\"redacted\"}\n";
        let status_digest =
            write_bytes_create_new(&bundle_dir.join(OPERATOR_PREFLIGHT_STATUS_FILE_V1), status)
                .expect("write status");
        let patch_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1),
            patch,
        )
        .expect("write patch");
        let inventory_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1),
            &inventory_bytes,
        )
        .expect("write inventory");
        let source_snapshot_manifest_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1),
            &source_snapshot_bytes,
        )
        .expect("write source snapshot manifest");
        let source_snapshot_file = bundle_dir
            .join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_DIR_V1)
            .join(required_fixture);
        fs::create_dir_all(
            source_snapshot_file
                .parent()
                .expect("source snapshot fixture parent"),
        )
        .expect("create source snapshot fixture parent");
        write_bytes_create_new(&source_snapshot_file, b"frozen-corpus-manifest")
            .expect("write source snapshot fixture");
        let redacted_config_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1),
            redacted_config,
        )
        .expect("write redacted config");
        let release_dir = bundle_dir.join("release");
        fs::create_dir(&release_dir).expect("create release provenance directory");
        let binary_digest =
            write_bytes_create_new(&release_dir.join("pump-research-tape"), b"binary")
                .expect("write release binary");
        let cargo_executable_digest = operator_digest_bytes(b"cargo-executable");
        let rustc_executable_digest = operator_digest_bytes(b"rustc-executable");
        let build_environment = PumpResearchOperatorBuildEnvironmentV1 {
            child_environment_semantics: OPERATOR_PREFLIGHT_SANITIZED_BUILD_ENVIRONMENT_V1
                .to_owned(),
            build_staging_semantics: OPERATOR_PREFLIGHT_ISOLATED_BUILD_STAGING_SEMANTICS_V1
                .to_owned(),
            cargo_target_dir_semantics:
                "fresh_create_new_isolated_staging_target_directory_not_reused_or_persisted_v2"
                    .to_owned(),
            cargo_home_semantics: OPERATOR_PREFLIGHT_SANITIZED_CARGO_HOME_V1.to_owned(),
            cargo_config_scope_semantics: OPERATOR_PREFLIGHT_ISOLATED_CARGO_CONFIG_SCOPE_V1
                .to_owned(),
            cargo_executable_digest: cargo_executable_digest.clone(),
            rustc_executable_digest: rustc_executable_digest.clone(),
            rustflags_digest: None,
            cargo_encoded_rustflags_digest: None,
            cargo_build_environment_digests: BTreeMap::new(),
            cargo_profile_release_environment_digests: BTreeMap::new(),
            cargo_home_digest: None,
            cargo_config_file_digests: BTreeMap::new(),
        };
        let build_environment_digest = write_json_create_new_with_digest(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1),
            &build_environment,
        )
        .expect("write build environment");
        let build_log_digest = write_bytes_create_new(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1),
            b"fresh build output\n",
        )
        .expect("write build log");
        let cargo_lock_digest = operator_digest_bytes(b"cargo-lock");
        let build_receipt = PumpResearchOperatorBuildReceiptV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
            cargo_command: [
                "cargo",
                "build",
                "--locked",
                "--offline",
                "--release",
                "-p",
                "seer",
                "--bin",
                "pump-research-tape",
            ]
            .iter()
            .map(|value| (*value).to_owned())
            .collect(),
            cargo_profile: "release".to_owned(),
            source_snapshot_manifest_digest_before_build: source_snapshot_manifest_digest.clone(),
            source_snapshot_manifest_digest_after_build: source_snapshot_manifest_digest.clone(),
            cargo_lock_digest: cargo_lock_digest.clone(),
            build_environment_digest: build_environment_digest.clone(),
            build_log_digest: build_log_digest.clone(),
            cargo_executable_digest: cargo_executable_digest.clone(),
            rustc_executable_digest: rustc_executable_digest.clone(),
            rustc_version: "rustc test".to_owned(),
            cargo_version: "cargo test".to_owned(),
            release_binary_digest: binary_digest.clone(),
            build_started_wall_ms: 1,
            build_completed_wall_ms: 2,
        };
        let build_receipt_digest = write_json_create_new_with_digest(
            &bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1),
            &build_receipt,
        )
        .expect("write build receipt");
        let mut receipt = PumpResearchOperatorPreflightReceiptV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            receipt_kind: OPERATOR_PREFLIGHT_RECEIPT_KIND_V1.to_owned(),
            created_wall_ms: 1,
            source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1.to_owned(),
            repository_commit: "deadbeef".to_owned(),
            repository_branch: Some("test".to_owned()),
            repository_worktree_state: "dirty".to_owned(),
            git_status_porcelain_file: OPERATOR_PREFLIGHT_STATUS_FILE_V1.to_owned(),
            git_status_porcelain_digest: status_digest,
            tracked_worktree_patch_file: OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1.to_owned(),
            tracked_worktree_patch_digest: patch_digest,
            untracked_inventory_file: OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1.to_owned(),
            untracked_inventory_digest: inventory_digest,
            untracked_entry_count: 1,
            source_snapshot_manifest_file: OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1
                .to_owned(),
            source_snapshot_manifest_digest,
            source_snapshot_entry_count: 1,
            cargo_lock_digest,
            release_binary_file: OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE_V1.to_owned(),
            release_binary_digest: binary_digest,
            build_receipt_file: OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1.to_owned(),
            build_receipt_digest,
            build_log_file: OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1.to_owned(),
            build_log_digest,
            build_environment_file: OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1.to_owned(),
            build_environment_digest,
            build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
            credential_scan_semantics: OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1.to_owned(),
            cargo_executable_digest,
            rustc_executable_digest,
            rustc_version: "rustc test".to_owned(),
            cargo_version: "cargo test".to_owned(),
            config_semantics: OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1.to_owned(),
            config_bytes_digest: operator_digest_bytes(b"capture-config"),
            redacted_config_file: OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1.to_owned(),
            redacted_config_digest,
            artifact_provenance_fingerprint: operator_digest_bytes(b"placeholder"),
        };
        receipt.artifact_provenance_fingerprint =
            receipt_artifact_fingerprint(&receipt).expect("calculate receipt fingerprint");
        let receipt_path = bundle_dir.join(OPERATOR_PREFLIGHT_RECEIPT_FILE_V1);
        write_json_create_new(&receipt_path, &receipt).expect("write receipt");

        let (loaded, _) = load_operator_preflight_receipt(&receipt_path).expect("load receipt");
        verify_operator_preflight_bundle(&receipt_path, &loaded).expect("verify intact bundle");

        let sidecars = [
            bundle_dir.join(OPERATOR_PREFLIGHT_STATUS_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1),
            release_dir.join("pump-research-tape"),
            bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1),
            bundle_dir.join(OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1),
            source_snapshot_file,
        ];
        for sidecar in sidecars {
            let original = fs::read(&sidecar).expect("read intact provenance sidecar");
            fs::write(&sidecar, b"tampered")
                .expect("tamper provenance sidecar in test-only temporary bundle");
            assert!(
                verify_operator_preflight_bundle(&receipt_path, &loaded).is_err(),
                "tampering {} must fail closed",
                sidecar.display()
            );
            fs::write(&sidecar, original).expect("restore provenance sidecar for next mutation");
        }
    }

    #[test]
    fn capture_binding_records_pre_rpc_validation_time_separately_from_write_time() {
        let receipt = PumpResearchOperatorPreflightReceiptV1 {
            schema_version: OPERATOR_PREFLIGHT_SCHEMA_VERSION_V1,
            receipt_kind: OPERATOR_PREFLIGHT_RECEIPT_KIND_V1.to_owned(),
            created_wall_ms: 1,
            source_tree_semantics: OPERATOR_PREFLIGHT_SOURCE_TREE_SEMANTICS_V1.to_owned(),
            repository_commit: "deadbeef".to_owned(),
            repository_branch: None,
            repository_worktree_state: "clean".to_owned(),
            git_status_porcelain_file: OPERATOR_PREFLIGHT_STATUS_FILE_V1.to_owned(),
            git_status_porcelain_digest: operator_digest_bytes(b"status"),
            tracked_worktree_patch_file: OPERATOR_PREFLIGHT_TRACKED_PATCH_FILE_V1.to_owned(),
            tracked_worktree_patch_digest: operator_digest_bytes(b"patch"),
            untracked_inventory_file: OPERATOR_PREFLIGHT_UNTRACKED_INVENTORY_FILE_V1.to_owned(),
            untracked_inventory_digest: operator_digest_bytes(b"inventory"),
            untracked_entry_count: 0,
            source_snapshot_manifest_file: OPERATOR_PREFLIGHT_SOURCE_SNAPSHOT_MANIFEST_FILE_V1
                .to_owned(),
            source_snapshot_manifest_digest: operator_digest_bytes(b"snapshot"),
            source_snapshot_entry_count: 0,
            cargo_lock_digest: operator_digest_bytes(b"lock"),
            release_binary_file: OPERATOR_PREFLIGHT_RELEASE_BINARY_FILE_V1.to_owned(),
            release_binary_digest: operator_digest_bytes(b"binary"),
            build_receipt_file: OPERATOR_PREFLIGHT_BUILD_RECEIPT_FILE_V1.to_owned(),
            build_receipt_digest: operator_digest_bytes(b"build-receipt"),
            build_log_file: OPERATOR_PREFLIGHT_BUILD_LOG_FILE_V1.to_owned(),
            build_log_digest: operator_digest_bytes(b"build-log"),
            build_environment_file: OPERATOR_PREFLIGHT_BUILD_ENVIRONMENT_FILE_V1.to_owned(),
            build_environment_digest: operator_digest_bytes(b"build-environment"),
            build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
            credential_scan_semantics: OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1.to_owned(),
            cargo_executable_digest: operator_digest_bytes(b"cargo-executable"),
            rustc_executable_digest: operator_digest_bytes(b"rustc-executable"),
            rustc_version: "rustc test".to_owned(),
            cargo_version: "cargo test".to_owned(),
            config_semantics: OPERATOR_PREFLIGHT_EXTERNAL_CONFIG_SEMANTICS_V1.to_owned(),
            config_bytes_digest: operator_digest_bytes(b"config"),
            redacted_config_file: OPERATOR_PREFLIGHT_REDACTED_CONFIG_FILE_V1.to_owned(),
            redacted_config_digest: operator_digest_bytes(b"redacted"),
            artifact_provenance_fingerprint: operator_digest_bytes(b"fingerprint"),
        };
        let validated = ValidatedOperatorPreflightV1 {
            receipt,
            receipt_digest: operator_digest_bytes(b"receipt"),
            receipt_validated_wall_ms: 123_456,
        };
        let binding = validated
            .binding_for_run("run-1")
            .expect("current sealed receipt must create an eligible binding");
        assert_eq!(binding.receipt_validated_wall_ms, 123_456);
        assert!(binding.binding_written_wall_ms >= binding.receipt_validated_wall_ms);
        assert!(binding.qualification_provenance_eligible);
        assert_eq!(
            binding.sealed_release_binary_digest,
            Some(binding.release_binary_digest.clone())
        );
    }

    #[test]
    fn debug_build_cannot_be_used_for_operator_preflight_or_capture() {
        if cfg!(debug_assertions) {
            assert!(require_non_debug_operator_bootstrap_binary().is_err());
        }
    }

    struct CaptureEnabledAbSourceWorkloadV1 {
        elapsed: Duration,
        /// Sum of individual source-side hand-off durations. This deliberately
        /// excludes unrelated OS scheduling time and deferred destruction from
        /// the disabled no-sink control path, while the actual writer is live.
        active_elapsed: Duration,
        p99: Duration,
    }

    fn capture_enabled_ab_subscribe_update(sequence: u64) -> SubscribeUpdate {
        let signature_byte = sequence.wrapping_add(1) as u8;
        let signature = vec![signature_byte; 64];
        let mut account_keys: Vec<Pubkey> = (1_u8..=18)
            .map(|value| Pubkey::new_from_array([value; 32]))
            .collect();
        account_keys[8] =
            Pubkey::from_str(ghost_core::transaction_parser::ProgramIds::TOKEN_PROGRAM)
                .expect("frozen Token Program id");
        account_keys[17] = Pubkey::from_str(PUMP_FUN_PROGRAM_ID).expect("frozen Pump program id");

        let mut instruction_data = DISC_BUY.to_vec();
        instruction_data.extend_from_slice(&1_000_000_u64.to_le_bytes());
        instruction_data.extend_from_slice(&50_000_000_u64.to_le_bytes());

        SubscribeUpdate {
            filters: vec!["pump_research_ab".to_owned()],
            update_oneof: Some(UpdateOneof::Transaction(SubscribeUpdateTransaction {
                transaction: Some(SubscribeUpdateTransactionInfo {
                    signature: signature.clone(),
                    is_vote: false,
                    transaction: Some(GrpcTransaction {
                        signatures: vec![signature],
                        message: Some(GrpcMessage {
                            header: None,
                            account_keys: account_keys
                                .iter()
                                .map(|account| account.to_bytes().to_vec())
                                .collect(),
                            recent_blockhash: vec![signature_byte; 32],
                            instructions: vec![CompiledInstruction {
                                program_id_index: 17,
                                accounts: (0_u8..17).collect(),
                                data: instruction_data,
                            }],
                            versioned: false,
                            address_table_lookups: Vec::new(),
                        }),
                    }),
                    meta: Some(TransactionStatusMeta {
                        pre_balances: vec![10_000_000_000; account_keys.len()],
                        post_balances: vec![9_900_000_000; account_keys.len()],
                        ..TransactionStatusMeta::default()
                    }),
                    index: sequence,
                }),
                slot: 1_000_000 + sequence,
            })),
        }
    }

    fn capture_enabled_ab_source_update(sequence: u64) -> PumpResearchSourceUpdateV1 {
        PumpResearchSourceUpdateV1 {
            provider_id: "capture-ab-primary".to_owned(),
            stream_epoch: 1,
            ingress_wall_ts_ms: 1_700_000_000_000_u64.saturating_add(sequence),
            ingress_monotonic_ts_ms: 1_000_u64.saturating_add(sequence),
            update: capture_enabled_ab_subscribe_update(sequence),
        }
    }

    fn capture_enabled_ab_p99(mut samples: Vec<Duration>) -> Duration {
        assert!(!samples.is_empty(), "A/B harness requires latency samples");
        samples.sort_unstable();
        samples[((samples.len() * 99) / 100).min(samples.len() - 1)]
    }

    fn capture_enabled_ab_source_updates(event_count: u64) -> Vec<PumpResearchSourceUpdateV1> {
        (0..event_count)
            .map(capture_enabled_ab_source_update)
            .collect()
    }

    fn run_capture_enabled_ab_source_workload(
        updates: Vec<PumpResearchSourceUpdateV1>,
        sink: Option<&Arc<dyn PumpResearchSourceSinkV1>>,
    ) -> CaptureEnabledAbSourceWorkloadV1 {
        let mut samples = Vec::with_capacity(updates.len());
        let mut active_elapsed = Duration::ZERO;
        let started = Instant::now();

        for source_update in updates {
            // The no-sink control must retain ownership through the measured
            // hand-off, then drop it afterwards. This keeps the comparison
            // about receive-side capture work rather than where the fixture
            // happens to be deallocated.
            let mut source_update = Some(source_update);
            let update_started = Instant::now();
            match sink {
                Some(sink) => {
                    sink.try_capture(
                        source_update
                            .take()
                            .expect("source update must be owned before capture hand-off"),
                    );
                }
                None => {
                    std::hint::black_box(
                        source_update
                            .as_ref()
                            .expect("source update must exist for no-sink control"),
                    );
                }
            }
            let sample = update_started.elapsed();
            active_elapsed = active_elapsed.saturating_add(sample);
            samples.push(sample);
            drop(source_update);
        }

        CaptureEnabledAbSourceWorkloadV1 {
            elapsed: started.elapsed(),
            active_elapsed,
            p99: capture_enabled_ab_p99(samples),
        }
    }

    fn measure_fatal_to_cancel_during_slow_rotation() -> Duration {
        const INJECTED_SLOW_SYNC: Duration = Duration::from_millis(50);
        let directory = tempdir().expect("temporary raw directory for slow-I/O probe");
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let slow_io_probe = install_raw_writer_slow_io_probe(INJECTED_SLOW_SYNC, entered_tx);
        let coordinator = PumpResearchCaptureCoordinatorV1::start(
            directory.path(),
            "capture-ab-slow-io".to_owned(),
            8,
            Duration::from_secs(60),
            // The second source record rotates the first segment and enters
            // the injected close/flush/sync delay while the source is active.
            1,
            Duration::from_secs(60),
        )
        .expect("start slow-I/O capture writer");
        let abort = coordinator.capture_abort();
        let sink = coordinator.source_sink();
        sink.source_stream_established(1);
        sink.try_capture(capture_enabled_ab_source_update(0));
        sink.try_capture(capture_enabled_ab_source_update(1));
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("writer must enter the injected slow rotation/sync window");

        let fatal_recorded = Instant::now();
        assert!(
            coordinator.ingress.record_fatal_capture_error(
                PumpResearchCaptureFatalReasonV1::DropControlLaneSaturated,
            ),
            "the harness must record one receive-side fatal reason"
        );
        while !abort.is_cancelled() && fatal_recorded.elapsed() < Duration::from_secs(2) {
            thread::sleep(Duration::from_millis(1));
        }
        assert!(
            abort.is_cancelled(),
            "writer/control plane must eventually dispatch cancellation after the slow I/O window"
        );
        let fatal_to_cancel = fatal_recorded.elapsed();

        // Do not inject a second delay while completing the synthetic source.
        drop(slow_io_probe);
        sink.source_workers_stopped_cleanly();
        sink.finish_source();
        let summary = coordinator.finish_and_join();
        assert_eq!(summary.accepted_source_records, 2);
        assert!(
            summary.error.is_some() && !summary.clean_shutdown,
            "the injected fatal reason must still make the synthetic capture incomplete"
        );
        fatal_to_cancel
    }

    /// Explicit pre-prospective local qualification gate from Amendment A.
    ///
    /// It is ignored because it intentionally performs release-mode timing,
    /// actual raw writer I/O, and an injected slow rotation/sync. Run it in a
    /// dedicated process with `--ignored --nocapture --test-threads=1`.
    #[test]
    #[ignore = "run explicitly for the Pump Research capture-enabled local A/B gate"]
    fn pr_a_capture_enabled_local_ab_harness() {
        const EVENTS: u64 = 8_192;
        const QUEUE_CAPACITY: usize = 16_384;
        const SOURCE_INGRESS_P99_SLA: Duration = Duration::from_micros(100);

        let disabled =
            run_capture_enabled_ab_source_workload(capture_enabled_ab_source_updates(EVENTS), None);

        let directory = tempdir().expect("temporary raw directory for A/B writer");
        let coordinator = PumpResearchCaptureCoordinatorV1::start(
            directory.path(),
            "capture-ab-enabled".to_owned(),
            QUEUE_CAPACITY,
            Duration::from_secs(60),
            64 * 1024 * 1024,
            Duration::from_secs(60),
        )
        .expect("start bounded capture-enabled raw writer");
        let abort = coordinator.capture_abort();
        let sink = coordinator.source_sink();
        sink.source_stream_established(1);
        let enabled = run_capture_enabled_ab_source_workload(
            capture_enabled_ab_source_updates(EVENTS),
            Some(&sink),
        );
        sink.source_workers_stopped_cleanly();
        sink.finish_source();
        let lifecycle = coordinator.source_lifecycle();
        let summary = coordinator.finish_and_join();

        let disabled_active_throughput = EVENTS as f64 / disabled.active_elapsed.as_secs_f64();
        let enabled_active_throughput = EVENTS as f64 / enabled.active_elapsed.as_secs_f64();
        let fatal_to_cancel = measure_fatal_to_cancel_during_slow_rotation();

        let report = serde_json::json!({
            "schema": "pump_research_capture_enabled_ab_v1",
            "semantics": {
                "disabled": "owned decoded source update reaches the pre-capture no-sink control",
                "enabled": "same source hand-off plus actual PumpResearchCaptureIngressV1, bounded writer, deterministic raw encoding, and filesystem segment publication",
                "reference": "the disabled no-sink arm has no equivalent queue hand-off, so ratios are telemetry only and never a false performance SLA",
                "throughput": "source-side active hand-off time; wall time is scheduling telemetry, not an SLA",
                "parser_worker": "standalone capture does not instantiate a parser worker; frozen parser parity remains a separate required proof",
                "slow_io": "test-only 50ms close/flush/sync rotation delay; measured without asserting a false 5ms cancellation deadline"
            },
            "events": EVENTS,
            "queue_capacity": QUEUE_CAPACITY,
            "disabled": {
                "wall_elapsed_ns": disabled.elapsed.as_nanos() as u64,
                "active_elapsed_ns": disabled.active_elapsed.as_nanos() as u64,
                "active_throughput_events_per_second": disabled_active_throughput,
                "p99_latency_ns": disabled.p99.as_nanos() as u64,
            },
            "enabled": {
                "wall_elapsed_ns": enabled.elapsed.as_nanos() as u64,
                "active_elapsed_ns": enabled.active_elapsed.as_nanos() as u64,
                "active_throughput_events_per_second": enabled_active_throughput,
                "p99_latency_ns": enabled.p99.as_nanos() as u64,
                "source_updates_received": lifecycle.source_updates_received,
                "admitted_source_updates": lifecycle.admitted_source_updates,
                "dropped_source_updates": lifecycle.dropped_source_updates,
                "accepted_source_records": summary.accepted_source_records,
                "persisted_ingress_gap_missing_events": summary.persisted_ingress_gap_missing_events,
                "gap_count": summary.gap_count,
                "segment_count": summary.segments.len(),
                "writer_clean_shutdown": summary.clean_shutdown,
                "writer_error": summary.error,
                "capture_abort_cancelled": abort.is_cancelled(),
            },
            "source_ingress_gate": {
                "enabled_p99_latency_ns": enabled.p99.as_nanos() as u64,
                "enabled_p99_latency_sla_max_ns": SOURCE_INGRESS_P99_SLA.as_nanos() as u64,
                "reference_disabled_p99_latency_ns": disabled.p99.as_nanos() as u64,
            },
            "slow_io": {
                "fatal_to_source_cancel_ns": fatal_to_cancel.as_nanos() as u64,
                "writer_idle_poll_ns": WRITER_IDLE_POLL_V1.as_nanos() as u64,
            },
            "structural_evidence": {
                "receive_disk_io": "not applicable: source ingress uses atomics and bounded try_send only",
                "parser_worker_blocking_waits": "not applicable: standalone capture has no parser worker",
                "silent_loss": lifecycle.source_updates_received != summary.accepted_source_records
                    || lifecycle.dropped_source_updates != 0
                    || summary.persisted_ingress_gap_missing_events != 0,
            }
        });
        println!("PR_A_CAPTURE_ENABLED_AB_REPORT={report}");

        assert!(summary.clean_shutdown && summary.error.is_none());
        assert_eq!(lifecycle.source_updates_received, EVENTS);
        assert_eq!(lifecycle.admitted_source_updates, EVENTS);
        assert_eq!(lifecycle.dropped_source_updates, 0);
        assert_eq!(summary.accepted_source_records, EVENTS);
        assert_eq!(summary.persisted_ingress_gap_missing_events, 0);
        assert_eq!(summary.gap_count, 0);
        assert_eq!(summary.segments.len(), 1);
        assert!(!abort.is_cancelled());
        assert!(
            enabled.p99 <= SOURCE_INGRESS_P99_SLA,
            "capture-enabled source-ingress p99 {:?} exceeds {:?}",
            enabled.p99,
            SOURCE_INGRESS_P99_SLA
        );
    }
}
