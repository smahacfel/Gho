//! Offline, read-only PR-B materialisation of a frozen Pump Research raw V1
//! run.
//!
//! This module has no source connection, no writer, no RPC backfill and no
//! active-runtime wiring. It verifies immutable raw segments before it exposes
//! any record to the parser or certifier.

use crate::{
    binary_parser::{
        BinaryParser, GlobalState, PumpResearchMutationInventoryEntryV1,
        PumpResearchTransactionMutationInventoryV1, DISC_BONDING_CURVE, DISC_GLOBAL_STATE,
    },
    grpc_connection::decode_research_raw_transaction_v1,
    research_tape::{
        operator_digest_bytes, PumpResearchCaptureProvenanceBindingV1,
        PumpResearchOperatorDigestV1, OPERATOR_PREFLIGHT_BINDING_KIND_V1,
        OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1, OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1,
        OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1,
    },
    types::GeyserEvent,
};
use anyhow::{bail, Context, Result};
use futures::{stream, StreamExt};
use ghost_core::pump_research_tape::{
    pump_research_pubkey_from_storage_v1, pump_research_storage_pubkey_v1,
    pump_transition_dependency_v1, EvidenceValueV1, FlagEvidenceSourceV1, FlagEvidenceStatusV1,
    FlagEvidenceV1, ParticipantBalanceProvenanceV1, ParticipantBalanceScopeV1, PumpAccountAnchorV1,
    PumpBirthEvidenceV1, PumpCertifiedMutationV1, PumpConflictReasonV1,
    PumpCreateInitialStateEvidenceV1, PumpCreateKindV1, PumpCurveStateV1, PumpEvidenceSourceV1,
    PumpEvidenceStatusV1, PumpExactResearchTapeManifestV1, PumpInstructionVariantV1,
    PumpMutationKindV1, PumpNonEvaluableReasonV1, PumpPrimaryAccountUpdateEvidenceV1,
    PumpPrimaryBlockMetaEvidenceV1, PumpPrimarySlotEvidenceV1, PumpPrimaryTransactionEvidenceV1,
    PumpProgramDataReceiptV1, PumpRawCoverageGapV1, PumpRawSegmentClosedV1, PumpRawSegmentHeaderV1,
    PumpRawSourceEnvelopeV1, PumpRawSourceRefV1, PumpResearchAccountRoleV1,
    PumpResearchCanonicalTransactionIdentityV1, PumpResearchEventTimeV1,
    PumpResearchQualificationAuditContractV1, PumpResearchQualificationBlockerV1,
    PumpResearchRawCodecV1, PumpResearchRawRecordV1, PumpResearchRequiredEvidenceV1,
    PumpResearchRunCompletionReceiptV1, PumpResearchRunCompletionStatusV1,
    PumpResearchRunStartManifestV1, PumpResearchSegmentReceiptV1,
    PumpResearchSourceInvocationClassV1, PumpResearchStorageHashV1, PumpResearchStoragePubkeyV1,
    PumpResearchStorageSignatureV1, PumpResearchTapeQualificationStatusV1,
    PumpResearchWindowStatusV1, PumpSlotCanonicalityV1, PumpTrajectoryCertificationV1,
    PumpTransactionTrajectoryV1, PumpTransitionDependencyV1, PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1,
    PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1, PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1,
    PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1, PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
};
use ghost_core::{
    account_state_core::{
        types::{AccountStateUpdate, UpdateSource},
        AccountObservationArbiter, AccountObservationArbiterLimitsV1,
        AccountObservationClassificationV1,
    },
    CurveFinality, ObservationProvenanceV1, ObservationSourceFamilyV1, ObservedPumpMutationV1,
    PumpInstructionLimitV1, PumpMutationClaimsV1, PumpMutationFamilyV1, PumpObservationLedgerV1,
    RawProviderRoleV1,
};
use prost::Message;
use reqwest::Url;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use solana_client::{
    client_error::{ClientError, ClientErrorKind},
    nonblocking::rpc_client::RpcClient as AsyncRpcClient,
    rpc_config::RpcBlockConfig,
    rpc_custom_error::JSON_RPC_SERVER_ERROR_SLOT_SKIPPED,
    rpc_request::RpcError,
};
use solana_sdk::{commitment_config::CommitmentConfig, pubkey::Pubkey, signature::Signature};
use solana_transaction_status::{
    option_serializer::OptionSerializer, EncodedTransaction, TransactionDetails, UiConfirmedBlock,
    UiInstruction, UiMessage, UiTransactionEncoding,
};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap, HashSet},
    fs::{self, File, OpenOptions},
    io::{BufRead, BufReader, BufWriter, Read, Write},
    path::{Component, Path, PathBuf},
    str::FromStr,
    sync::Arc,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};

#[cfg(not(unix))]
use std::io::{Seek, SeekFrom};
#[cfg(unix)]
use std::os::unix::{
    fs::{FileExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    io::AsRawFd,
};

const RAW_START_MANIFEST_MAX_BYTES_V1: u64 = 1024 * 1024;
const RAW_COMPLETION_RECEIPT_MAX_BYTES_V1: u64 = 64 * 1024 * 1024;
const RAW_PROVENANCE_BINDING_MAX_BYTES_V1: u64 = 1024 * 1024;
const QUALIFICATION_AUDIT_CONFIG_MAX_BYTES_V1: u64 = 1024 * 1024;
const QUALIFICATION_PREPARATION_RECEIPT_MAX_BYTES_V1: u64 = 4 * 1024 * 1024;
const PROVIDER_SUITABILITY_RECEIPT_MAX_BYTES_V1: u64 = 64 * 1024 * 1024;
const PROVIDER_INDEPENDENCE_ATTESTATION_MAX_BYTES_V1: u64 = 4 * 1024 * 1024;
const GO_D_SOURCE_AUTHORITY_MAX_BYTES_V1: u64 = 4 * 1024 * 1024;
const COMBINED_CERTIFIER_EXECUTABLE_MAX_BYTES_V1: u64 = 256 * 1024 * 1024;
const EXACT_MANIFEST_MAX_BYTES_V1: u64 = 16 * 1024 * 1024;
const GO_D_SOURCE_AUTHORITY_SCHEMA_V1: &str = "pump_research_go_d_source_authority_v1";
const GO_D_SOURCE_AUTHORITY_VERIFIED_V1: &str = "VERIFIED";
const GO_E_EXTERNAL_AUDIT_RETIRED_V1: &str = "RETIRED_NOT_A_GATE";

/// Result returned after a raw run was fully verified and indexed.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpResearchRawIndexSummaryV1 {
    pub run_id: String,
    pub segment_count: usize,
    pub transaction_count: usize,
    pub account_update_count: usize,
    pub slot_update_count: usize,
    pub coverage_gap_count: usize,
}

#[derive(Clone, Debug)]
pub(crate) struct PumpResearchRawTapeIndexV1 {
    pub start_manifest: PumpResearchRunStartManifestV1,
    pub completion_receipt: PumpResearchRunCompletionReceiptV1,
    pub segments: Vec<PumpResearchIndexedSegmentV1>,
    pub transactions: Vec<PumpResearchIndexedTransactionV1>,
    pub account_updates: Vec<PumpResearchIndexedAccountUpdateV1>,
    pub slots: BTreeMap<u64, PumpResearchRawSlotNodeV1>,
    pub block_meta: BTreeMap<u64, Vec<PumpPrimaryBlockMetaEvidenceV1>>,
    pub coverage_gaps: Vec<PumpRawCoverageGapV1>,
    raw_control_authority: PumpResearchRawControlAuthorityV1,
    raw_segment_set_authority: Option<Arc<PumpResearchRawSegmentSetAuthorityV1>>,
    capture_provenance_eligibility: PumpResearchCaptureProvenanceEligibilityV1,
}

#[derive(Clone, Debug)]
struct PumpResearchRawControlAuthorityV1 {
    start_manifest_digest: PumpResearchOperatorDigestV1,
    completion_receipt_digest: PumpResearchOperatorDigestV1,
    provenance_binding_digest: Option<PumpResearchOperatorDigestV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
struct PumpResearchRawSegmentSetEntryV1 {
    segment_index: u64,
    filename: String,
    canonical_source_path: String,
    bytes: u64,
    file_sha256: PumpResearchStorageHashV1,
    file_blake3: PumpResearchStorageHashV1,
}

/// Combined qualification reads raw bytes only through private, unlinked
/// snapshot descriptors. The ordered evidence entries remain sufficient to
/// recompute one deterministic authority digest without serialising handles.
#[derive(Debug)]
struct PumpResearchRawSegmentSetAuthorityV1 {
    entries: Vec<PumpResearchRawSegmentSetEntryV1>,
    aggregate_blake3: String,
    pinned_files: Vec<Arc<File>>,
}

#[derive(Serialize)]
struct PumpResearchRawSegmentSetDigestInputV1<'a> {
    schema_version: u16,
    entries: &'a [PumpResearchRawSegmentSetEntryV1],
}

fn open_regular_nofollow_v1(path: &Path, label: &str) -> Result<File> {
    open_regular_nofollow_with_preopen_hook_v1(path, label, || Ok(()))
}

fn open_regular_nofollow_with_preopen_hook_v1<F>(
    path: &Path,
    label: &str,
    before_open: F,
) -> Result<File>
where
    F: FnOnce() -> Result<()>,
{
    let path_metadata = fs::symlink_metadata(path)
        .with_context(|| format!("inspect {label} {}", path.display()))?;
    if path_metadata.file_type().is_symlink() || !path_metadata.is_file() {
        bail!(
            "{label} {} must be a regular non-symlink file",
            path.display()
        );
    }
    before_open()?;
    let mut options = OpenOptions::new();
    options.read(true);
    #[cfg(unix)]
    options.custom_flags(libc::O_NOFOLLOW | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(path).with_context(|| {
        format!(
            "open {label} {} without following symlinks or blocking on special files",
            path.display()
        )
    })?;
    let opened_metadata = file
        .metadata()
        .with_context(|| format!("inspect opened {label} {}", path.display()))?;
    if !opened_metadata.is_file() {
        bail!("opened {label} {} is not a regular file", path.display());
    }
    Ok(file)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpResearchAuthorityFileKindV1 {
    RawStartManifest,
    RawCompletionReceipt,
    RawProvenanceBinding,
    QualificationAuditConfig,
    QualificationPreparationReceipt,
    ProviderSuitabilityReceipt,
    ProviderIndependenceAttestation,
    GoDSourceAuthority,
    ExactManifest,
}

impl PumpResearchAuthorityFileKindV1 {
    fn label(self) -> &'static str {
        match self {
            Self::RawStartManifest => "raw start manifest",
            Self::RawCompletionReceipt => "raw completion receipt",
            Self::RawProvenanceBinding => "raw provenance binding",
            Self::QualificationAuditConfig => "qualification audit config",
            Self::QualificationPreparationReceipt => "qualification preparation receipt",
            Self::ProviderSuitabilityReceipt => "provider suitability receipt",
            Self::ProviderIndependenceAttestation => "provider-independence attestation",
            Self::GoDSourceAuthority => "GO-D source authority",
            Self::ExactManifest => "exact manifest",
        }
    }

    fn max_bytes(self) -> u64 {
        match self {
            Self::RawStartManifest => RAW_START_MANIFEST_MAX_BYTES_V1,
            Self::RawCompletionReceipt => RAW_COMPLETION_RECEIPT_MAX_BYTES_V1,
            Self::RawProvenanceBinding => RAW_PROVENANCE_BINDING_MAX_BYTES_V1,
            Self::QualificationAuditConfig => QUALIFICATION_AUDIT_CONFIG_MAX_BYTES_V1,
            Self::QualificationPreparationReceipt => QUALIFICATION_PREPARATION_RECEIPT_MAX_BYTES_V1,
            Self::ProviderSuitabilityReceipt => PROVIDER_SUITABILITY_RECEIPT_MAX_BYTES_V1,
            Self::ProviderIndependenceAttestation => PROVIDER_INDEPENDENCE_ATTESTATION_MAX_BYTES_V1,
            Self::GoDSourceAuthority => GO_D_SOURCE_AUTHORITY_MAX_BYTES_V1,
            Self::ExactManifest => EXACT_MANIFEST_MAX_BYTES_V1,
        }
    }
}

#[derive(Debug)]
struct PumpResearchBoundedAuthorityFileV1 {
    bytes: Vec<u8>,
    digest: PumpResearchOperatorDigestV1,
}

/// Operator-approved, hash-pinned authority for the one frozen GO-D tape.
/// It does not claim that an external RPC reproduced the tape. Instead it
/// binds the raw lifecycle/provenance controls and the independently hashed
/// segment set that the offline materializer actually reads.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchGoDSourceAuthorityReceiptV1 {
    schema_version: String,
    source_run_id: String,
    source_storage_format_version: u16,
    go_d_source_authority: String,
    external_go_e_audit: String,
    raw_provenance_binding_sha256: String,
    raw_start_manifest_sha256: String,
    raw_completion_receipt_sha256: String,
    raw_segment_set_blake3: String,
    operator_decision: String,
    created_wall_ms: u64,
}

#[derive(Clone, Debug)]
struct PumpResearchValidatedGoDSourceAuthorityV1 {
    receipt: PumpResearchGoDSourceAuthorityReceiptV1,
    digest: PumpResearchOperatorDigestV1,
    path: PathBuf,
}

#[derive(Clone, Debug, Serialize)]
struct PumpResearchGoDSourceAuthorityReportV1 {
    schema_version: &'static str,
    source_run_id: String,
    #[serde(rename = "GO_D_SOURCE_AUTHORITY")]
    go_d_source_authority: &'static str,
    #[serde(rename = "EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE")]
    external_go_e_audit_not_used_as_gate: bool,
    external_go_e_audit_status: &'static str,
    go_d_source_authority_sha256: String,
    raw_provenance_binding_sha256: String,
    raw_start_manifest_sha256: String,
    raw_completion_receipt_sha256: String,
    raw_segment_set_blake3: String,
}

/// Kernel-bound authority for the executable image mapped by this process.
/// `/proc/self/exe` is intentionally opened once and retained: reopening the
/// pathname returned by `env::current_exe()` would instead attest whichever
/// inode happens to occupy that pathname later.
#[derive(Clone, Debug)]
struct PumpResearchRunningExecutableAuthorityV1 {
    file: Arc<File>,
    digest: PumpResearchOperatorDigestV1,
}

impl PumpResearchRunningExecutableAuthorityV1 {
    fn digest(&self) -> &PumpResearchOperatorDigestV1 {
        &self.digest
    }

    fn revalidate_v1(&self, boundary: &str) -> Result<()> {
        let current = digest_open_regular_file_exact_v1(
            &self.file,
            COMBINED_CERTIFIER_EXECUTABLE_MAX_BYTES_V1,
            &format!("running executable image at {boundary}"),
        )?;
        if current != self.digest {
            bail!("running executable image changed at {boundary}");
        }
        Ok(())
    }
}

#[cfg(target_os = "linux")]
fn capture_running_executable_authority_v1() -> Result<Arc<PumpResearchRunningExecutableAuthorityV1>>
{
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    // `/proc/self/exe` is a kernel-owned magic symlink to the mapped image.
    // Following this one specific symlink is the point of the authority; all
    // ordinary operator-controlled paths continue to use O_NOFOLLOW.
    let file = options
        .open("/proc/self/exe")
        .context("open the kernel-bound running executable image")?;
    let metadata = file
        .metadata()
        .context("inspect the kernel-bound running executable image")?;
    if !metadata.is_file() {
        bail!("the kernel-bound running executable image is not a regular file");
    }
    let digest = digest_open_regular_file_exact_v1(
        &file,
        COMBINED_CERTIFIER_EXECUTABLE_MAX_BYTES_V1,
        "kernel-bound running executable image",
    )?;
    Ok(Arc::new(PumpResearchRunningExecutableAuthorityV1 {
        file: Arc::new(file),
        digest,
    }))
}

#[cfg(not(target_os = "linux"))]
fn capture_running_executable_authority_v1() -> Result<Arc<PumpResearchRunningExecutableAuthorityV1>>
{
    bail!("qualification executable provenance requires Linux /proc/self/exe authority")
}

fn read_bounded_authority_file_v1(
    path: &Path,
    kind: PumpResearchAuthorityFileKindV1,
) -> Result<PumpResearchBoundedAuthorityFileV1> {
    read_bounded_regular_file_with_hooks_v1(
        path,
        kind.label(),
        kind.max_bytes(),
        || Ok(()),
        || Ok(()),
    )
}

fn read_bounded_regular_file_with_hooks_v1<BeforeOpen, AfterRead>(
    path: &Path,
    label: &str,
    max_bytes: u64,
    before_open: BeforeOpen,
    after_read: AfterRead,
) -> Result<PumpResearchBoundedAuthorityFileV1>
where
    BeforeOpen: FnOnce() -> Result<()>,
    AfterRead: FnOnce() -> Result<()>,
{
    let file = open_regular_nofollow_with_preopen_hook_v1(path, label, before_open)?;
    let before = file
        .metadata()
        .with_context(|| format!("inspect opened {label} {}", path.display()))?;
    if !before.is_file() {
        bail!("opened {label} {} is not a regular file", path.display());
    }
    if before.len() > max_bytes {
        bail!(
            "{label} {} is {} bytes, above the {max_bytes}-byte limit",
            path.display(),
            before.len()
        );
    }
    let length = usize::try_from(before.len())
        .with_context(|| format!("{label} {} length does not fit usize", path.display()))?;
    let mut bytes = Vec::new();
    bytes
        .try_reserve_exact(length)
        .with_context(|| format!("reserve {length} bytes for {label} {}", path.display()))?;
    bytes.resize(length, 0);
    if length != 0 {
        #[cfg(unix)]
        file.read_exact_at(&mut bytes, 0)
            .with_context(|| format!("read exactly {label} {}", path.display()))?;
        #[cfg(not(unix))]
        {
            let mut reader = file
                .try_clone()
                .context("clone bounded authority descriptor")?;
            reader.read_exact(&mut bytes)?;
        }
    }
    after_read()?;
    let after = file
        .metadata()
        .with_context(|| format!("inspect {label} {} after read", path.display()))?;
    if !after.is_file() || after.len() != before.len() {
        bail!(
            "{label} {} changed length while being read: {} != {}",
            path.display(),
            after.len(),
            before.len()
        );
    }
    let digest = operator_digest_bytes(&bytes);
    Ok(PumpResearchBoundedAuthorityFileV1 { bytes, digest })
}

fn digest_bounded_authority_file_v1(
    path: &Path,
    kind: PumpResearchAuthorityFileKindV1,
) -> Result<PumpResearchOperatorDigestV1> {
    Ok(read_bounded_authority_file_v1(path, kind)?.digest)
}

fn digest_open_regular_file_exact_v1(
    file: &File,
    max_bytes: u64,
    label: &str,
) -> Result<PumpResearchOperatorDigestV1> {
    let before = file
        .metadata()
        .with_context(|| format!("inspect {label} before bounded digest"))?;
    if !before.is_file() {
        bail!("{label} is not a regular file");
    }
    if before.len() > max_bytes {
        bail!(
            "{label} is {} bytes, above the {max_bytes}-byte limit",
            before.len()
        );
    }
    let expected_bytes = before.len();
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    while offset < expected_bytes {
        let remaining = expected_bytes.saturating_sub(offset);
        let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))
            .context("bounded executable digest chunk length does not fit usize")?;
        #[cfg(unix)]
        file.read_exact_at(&mut buffer[..chunk_len], offset)
            .with_context(|| format!("read exactly {label} bytes at offset {offset}"))?;
        #[cfg(not(unix))]
        {
            let mut reader = file
                .try_clone()
                .with_context(|| format!("clone {label} descriptor"))?;
            reader.seek(SeekFrom::Start(offset))?;
            reader
                .read_exact(&mut buffer[..chunk_len])
                .with_context(|| format!("read exactly {label} bytes at offset {offset}"))?;
        }
        sha256.update(&buffer[..chunk_len]);
        blake3.update(&buffer[..chunk_len]);
        offset = offset
            .checked_add(u64::try_from(chunk_len).context("digest chunk length exceeds u64")?)
            .ok_or_else(|| anyhow::anyhow!("{label} digest offset overflow"))?;
    }
    let after = file
        .metadata()
        .with_context(|| format!("inspect {label} after bounded digest"))?;
    if !after.is_file() || after.len() != expected_bytes {
        bail!(
            "{label} changed length while being digested: {} != {expected_bytes}",
            after.len()
        );
    }
    #[cfg(unix)]
    if after.dev() != before.dev() || after.ino() != before.ino() {
        bail!("{label} descriptor identity changed while being digested");
    }
    Ok(PumpResearchOperatorDigestV1 {
        sha256: format!("{:x}", sha256.finalize()),
        blake3: blake3.finalize().to_hex().to_string(),
        bytes: expected_bytes,
    })
}

fn error_has_io_kind_v1(error: &anyhow::Error, kind: std::io::ErrorKind) -> bool {
    error.chain().any(|cause| {
        cause
            .downcast_ref::<std::io::Error>()
            .is_some_and(|io_error| io_error.kind() == kind)
    })
}

fn hash_open_file_exact_v1(
    file: &File,
    expected_bytes: u64,
    label: &str,
) -> Result<(u64, PumpResearchStorageHashV1, PumpResearchStorageHashV1)> {
    hash_open_file_exact_with_post_read_hook_v1(file, expected_bytes, label, || Ok(()))
}

fn hash_open_file_exact_with_post_read_hook_v1<F>(
    file: &File,
    expected_bytes: u64,
    label: &str,
    after_expected_read: F,
) -> Result<(u64, PumpResearchStorageHashV1, PumpResearchStorageHashV1)>
where
    F: FnOnce() -> Result<()>,
{
    let before = file
        .metadata()
        .with_context(|| format!("inspect {label} before bounded hash"))?;
    if !before.is_file() || before.len() != expected_bytes {
        bail!(
            "{label} size before bounded hash is {}, expected {expected_bytes}",
            before.len()
        );
    }
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    while offset < expected_bytes {
        let remaining = expected_bytes.saturating_sub(offset);
        let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))
            .context("bounded raw segment chunk length does not fit usize")?;
        #[cfg(unix)]
        file.read_exact_at(&mut buffer[..chunk_len], offset)
            .with_context(|| format!("read exactly {label} bytes at offset {offset}"))?;
        #[cfg(not(unix))]
        {
            let mut reader = file.try_clone().context("clone raw segment descriptor")?;
            reader.seek(SeekFrom::Start(offset))?;
            reader
                .read_exact(&mut buffer[..chunk_len])
                .with_context(|| format!("read exactly {label} bytes at offset {offset}"))?;
        }
        sha256.update(&buffer[..chunk_len]);
        blake3.update(&buffer[..chunk_len]);
        offset = offset.saturating_add(u64::try_from(chunk_len).unwrap_or(u64::MAX));
    }
    after_expected_read()?;
    let after = file
        .metadata()
        .with_context(|| format!("inspect {label} after bounded hash"))?;
    if !after.is_file() || after.len() != expected_bytes {
        bail!(
            "{label} size after bounded hash is {}, expected {expected_bytes}",
            after.len()
        );
    }
    let sha256_bytes: [u8; 32] = sha256.finalize().into();
    Ok((
        expected_bytes,
        PumpResearchStorageHashV1::from(sha256_bytes),
        PumpResearchStorageHashV1::from(*blake3.finalize().as_bytes()),
    ))
}

#[cfg(target_os = "linux")]
fn create_anonymous_raw_snapshot_file_v1(parent: &Path) -> Result<File> {
    let parent_metadata = fs::symlink_metadata(parent)
        .with_context(|| format!("inspect private raw snapshot parent {}", parent.display()))?;
    if parent_metadata.file_type().is_symlink() || !parent_metadata.is_dir() {
        bail!(
            "private raw snapshot parent {} must be a real directory",
            parent.display()
        );
    }
    let mut options = OpenOptions::new();
    options
        .read(true)
        .write(true)
        .mode(0o600)
        .custom_flags(libc::O_TMPFILE | libc::O_EXCL | libc::O_CLOEXEC | libc::O_NONBLOCK);
    let file = options.open(parent).with_context(|| {
        format!(
            "create anonymous O_TMPFILE raw snapshot in {}",
            parent.display()
        )
    })?;
    let metadata = file
        .metadata()
        .context("inspect anonymous O_TMPFILE raw snapshot")?;
    if !metadata.is_file() || metadata.nlink() != 0 {
        bail!("private raw snapshot is not an anonymous regular file");
    }
    Ok(file)
}

#[cfg(not(target_os = "linux"))]
fn create_anonymous_raw_snapshot_file_v1(_parent: &Path) -> Result<File> {
    bail!("combined qualification requires Linux O_TMPFILE raw snapshot authority")
}

#[cfg(target_os = "linux")]
fn reopen_anonymous_raw_snapshot_read_only_v1(writable: &File) -> Result<File> {
    let before = writable
        .metadata()
        .context("inspect writable anonymous raw snapshot before read-only reopen")?;
    if !before.is_file() || before.nlink() != 0 {
        bail!("writable private raw snapshot is not an anonymous regular file");
    }
    let proc_path = PathBuf::from(format!("/proc/self/fd/{}", writable.as_raw_fd()));
    let mut options = OpenOptions::new();
    options
        .read(true)
        .custom_flags(libc::O_CLOEXEC | libc::O_NONBLOCK);
    let read_only = options
        .open(&proc_path)
        .context("reopen anonymous raw snapshot through its process-local descriptor")?;
    let after = read_only
        .metadata()
        .context("inspect read-only anonymous raw snapshot")?;
    if !after.is_file()
        || after.nlink() != 0
        || after.dev() != before.dev()
        || after.ino() != before.ino()
        || after.len() != before.len()
    {
        bail!("read-only raw snapshot descriptor does not reference the copied anonymous inode");
    }
    Ok(read_only)
}

#[cfg(not(target_os = "linux"))]
fn reopen_anonymous_raw_snapshot_read_only_v1(_writable: &File) -> Result<File> {
    bail!("combined qualification requires Linux anonymous read-only snapshot reopen")
}

fn copy_raw_segment_to_unlinked_snapshot_v1(
    source_path: &Path,
    snapshot_parent: &Path,
    receipt: &PumpResearchSegmentReceiptV1,
    expected_bytes: u64,
) -> Result<(Arc<File>, u64)> {
    copy_raw_segment_to_unlinked_snapshot_with_post_read_hook_v1(
        source_path,
        snapshot_parent,
        receipt,
        expected_bytes,
        || Ok(()),
    )
}

fn copy_raw_segment_to_unlinked_snapshot_with_post_read_hook_v1<F>(
    source_path: &Path,
    snapshot_parent: &Path,
    receipt: &PumpResearchSegmentReceiptV1,
    expected_bytes: u64,
    after_expected_read: F,
) -> Result<(Arc<File>, u64)>
where
    F: FnOnce() -> Result<()>,
{
    let source = open_regular_nofollow_v1(source_path, "raw segment")?;
    let source_metadata = source
        .metadata()
        .with_context(|| format!("inspect raw segment {} before copy", source_path.display()))?;
    if !source_metadata.is_file() || source_metadata.len() != expected_bytes {
        bail!(
            "raw segment {} size before snapshot copy is {}, expected {expected_bytes}",
            source_path.display(),
            source_metadata.len()
        );
    }
    let mut snapshot = create_anonymous_raw_snapshot_file_v1(snapshot_parent)?;
    let mut sha256 = Sha256::new();
    let mut blake3 = blake3::Hasher::new();
    let mut offset = 0_u64;
    let mut buffer = [0_u8; 1024 * 1024];
    while offset < expected_bytes {
        let remaining = expected_bytes.saturating_sub(offset);
        let chunk_len = usize::try_from(remaining.min(buffer.len() as u64))
            .context("bounded raw snapshot chunk length does not fit usize")?;
        #[cfg(unix)]
        source
            .read_exact_at(&mut buffer[..chunk_len], offset)
            .with_context(|| {
                format!(
                    "read exactly raw segment {} at offset {offset}",
                    source_path.display()
                )
            })?;
        #[cfg(not(unix))]
        {
            let mut reader = source.try_clone().context("clone raw source descriptor")?;
            reader.seek(SeekFrom::Start(offset))?;
            reader
                .read_exact(&mut buffer[..chunk_len])
                .with_context(|| {
                    format!(
                        "read exactly raw segment {} at offset {offset}",
                        source_path.display()
                    )
                })?;
        }
        snapshot
            .write_all(&buffer[..chunk_len])
            .context("copy raw segment into anonymous private snapshot")?;
        sha256.update(&buffer[..chunk_len]);
        blake3.update(&buffer[..chunk_len]);
        offset = offset.saturating_add(u64::try_from(chunk_len).unwrap_or(u64::MAX));
    }
    after_expected_read()?;
    let source_after = source
        .metadata()
        .with_context(|| format!("inspect raw segment {} after copy", source_path.display()))?;
    if !source_after.is_file() || source_after.len() != expected_bytes {
        bail!(
            "raw segment {} size after snapshot copy is {}, expected {expected_bytes}",
            source_path.display(),
            source_after.len()
        );
    }
    let sha256_bytes: [u8; 32] = sha256.finalize().into();
    let copied_sha256 = PumpResearchStorageHashV1::from(sha256_bytes);
    let copied_blake3 = PumpResearchStorageHashV1::from(*blake3.finalize().as_bytes());
    if copied_sha256 != receipt.file_sha256 || copied_blake3 != receipt.file_blake3 {
        bail!(
            "raw segment {} changed while creating the private qualification snapshot",
            source_path.display()
        );
    }
    snapshot
        .sync_all()
        .context("sync anonymous private raw snapshot")?;
    #[cfg(unix)]
    snapshot.set_permissions(fs::Permissions::from_mode(0o400))?;
    let snapshot_metadata = snapshot
        .metadata()
        .context("inspect completed anonymous private raw snapshot")?;
    if !snapshot_metadata.is_file() || snapshot_metadata.len() != expected_bytes {
        bail!(
            "anonymous private raw snapshot size is {}, expected {expected_bytes}",
            snapshot_metadata.len()
        );
    }
    #[cfg(target_os = "linux")]
    if snapshot_metadata.nlink() != 0 {
        bail!("private raw snapshot unexpectedly gained a filesystem link");
    }
    let pinned = reopen_anonymous_raw_snapshot_read_only_v1(&snapshot)?;
    drop(snapshot);
    Ok((Arc::new(pinned), expected_bytes))
}

fn read_frozen_frame_from_open_file_v1(file: &File, offset: u64) -> Result<Vec<u8>> {
    let mut length_bytes = [0_u8; 4];
    #[cfg(unix)]
    file.read_exact_at(&mut length_bytes, offset)
        .with_context(|| format!("read frozen V1 frame length at {offset}"))?;
    #[cfg(not(unix))]
    {
        let mut clone = file.try_clone().context("clone raw snapshot descriptor")?;
        clone.seek(SeekFrom::Start(offset))?;
        clone.read_exact(&mut length_bytes)?;
    }
    let payload_length = u32::from_le_bytes(length_bytes) as usize;
    if payload_length > PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 {
        bail!(
            "frozen V1 frame at offset {offset} declares payload {payload_length} above {}",
            PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1
        );
    }
    let total_length = 4_usize
        .checked_add(payload_length)
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| anyhow::anyhow!("frozen frame length overflow at offset {offset}"))?;
    let mut frame = vec![0_u8; total_length];
    frame[..4].copy_from_slice(&length_bytes);
    #[cfg(unix)]
    file.read_exact_at(&mut frame[4..], offset.saturating_add(4))
        .with_context(|| format!("read frozen V1 frame at {offset}"))?;
    #[cfg(not(unix))]
    {
        let mut clone = file.try_clone().context("clone raw snapshot descriptor")?;
        clone.seek(SeekFrom::Start(offset.saturating_add(4)))?;
        clone.read_exact(&mut frame[4..])?;
    }
    Ok(frame)
}

impl PumpResearchRawSegmentSetAuthorityV1 {
    fn revalidate_v1(
        &self,
        segments: &[PumpResearchIndexedSegmentV1],
        boundary: &str,
    ) -> Result<()> {
        if self.entries.len() != segments.len() || self.pinned_files.len() != segments.len() {
            bail!("raw segment-set authority cardinality changed at {boundary}");
        }
        for ((entry, pinned), segment) in self.entries.iter().zip(&self.pinned_files).zip(segments)
        {
            if entry.segment_index != segment.receipt.segment_index
                || entry.filename != segment.receipt.filename
                || entry.file_sha256 != segment.receipt.file_sha256
                || entry.file_blake3 != segment.receipt.file_blake3
            {
                bail!(
                    "raw segment-set receipt binding changed for segment {} at {boundary}",
                    entry.segment_index
                );
            }
            let canonical_source = fs::canonicalize(&segment.path).with_context(|| {
                format!(
                    "canonicalize raw segment {} at {boundary}",
                    segment.path.display()
                )
            })?;
            if canonical_source.to_string_lossy() != entry.canonical_source_path {
                bail!(
                    "raw segment {} source path changed at {boundary}",
                    entry.segment_index
                );
            }
            let source = open_regular_nofollow_v1(&segment.path, "raw segment")?;
            let source_digest = hash_open_file_exact_v1(
                &source,
                entry.bytes,
                &format!("raw source segment {}", entry.segment_index),
            )
            .with_context(|| {
                format!(
                    "revalidate raw source segment {} at {boundary}",
                    entry.segment_index
                )
            })?;
            let snapshot_digest = hash_open_file_exact_v1(
                pinned,
                entry.bytes,
                &format!("private raw snapshot segment {}", entry.segment_index),
            )
            .with_context(|| {
                format!(
                    "revalidate private raw snapshot segment {} at {boundary}",
                    entry.segment_index
                )
            })?;
            let expected = (entry.bytes, entry.file_sha256, entry.file_blake3);
            if source_digest != expected {
                bail!(
                    "raw source segment {} changed at {boundary}",
                    entry.segment_index
                );
            }
            if snapshot_digest != expected {
                bail!(
                    "private raw snapshot segment {} changed at {boundary}",
                    entry.segment_index
                );
            }
        }
        let aggregate_bytes = serde_json::to_vec(&PumpResearchRawSegmentSetDigestInputV1 {
            schema_version: 1,
            entries: &self.entries,
        })
        .context("serialize raw segment-set authority for revalidation")?;
        let aggregate = blake3::hash(&aggregate_bytes).to_hex().to_string();
        if aggregate != self.aggregate_blake3 {
            bail!("raw segment-set aggregate digest changed at {boundary}");
        }
        Ok(())
    }
}

/// Qualification is stricter than development replay. A missing or legacy
/// sidecar never invalidates immutable raw evidence, but it permanently
/// prevents that run from becoming `Ready` under V1.1.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpResearchCaptureProvenanceEligibilityV1 {
    Eligible,
    Ineligible(PumpResearchCaptureProvenanceIneligibilityReasonV1),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpResearchCaptureProvenanceIneligibilityReasonV1 {
    MissingBinding,
    UnreadableBinding,
    MalformedBinding,
    LegacyOrUnsupportedBinding,
    IneligibleBinding,
    SealedBinaryMismatch,
}

#[derive(Clone, Debug)]
pub(crate) struct PumpResearchIndexedSegmentV1 {
    pub receipt: PumpResearchSegmentReceiptV1,
    pub path: PathBuf,
    pub header: PumpRawSegmentHeaderV1,
    pub file_bytes: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PumpResearchRawRecordPointerV1 {
    pub segment_position: usize,
    /// Byte offset at the beginning of the frozen record frame.
    pub frame_offset: u64,
}

#[derive(Clone, Debug)]
pub(crate) struct PumpResearchIndexedTransactionV1 {
    pub pointer: PumpResearchRawRecordPointerV1,
    pub source: PumpRawSourceEnvelopeV1,
    pub slot: u64,
    pub tx_index: Option<u32>,
    pub signature: PumpResearchStorageSignatureV1,
    pub event_time: PumpResearchEventTimeV1,
    pub block_time: Option<i64>,
}

#[derive(Clone, Debug)]
pub(crate) struct PumpResearchIndexedAccountUpdateV1 {
    pub pointer: PumpResearchRawRecordPointerV1,
    pub source: PumpRawSourceEnvelopeV1,
    pub account_role: PumpResearchAccountRoleV1,
    pub is_startup: bool,
    pub account_pubkey: PumpResearchStoragePubkeyV1,
    pub owner_program: PumpResearchStoragePubkeyV1,
    pub raw_account_data_hash_blake3: PumpResearchStorageHashV1,
    pub slot: u64,
    pub write_version: u64,
    pub txn_signature: Option<PumpResearchStorageSignatureV1>,
    pub event_time: PumpResearchEventTimeV1,
}

/// Raw slot observations remain raw. This only aggregates evidence required by
/// the conservative PR-B canonicality classifier.
#[derive(Clone, Debug, Default)]
pub(crate) struct PumpResearchRawSlotNodeV1 {
    pub parents: BTreeSet<u64>,
    pub saw_processed: bool,
    pub saw_confirmed: bool,
    pub saw_finalized: bool,
}

impl PumpResearchRawTapeIndexV1 {
    pub(crate) fn summary(&self) -> PumpResearchRawIndexSummaryV1 {
        PumpResearchRawIndexSummaryV1 {
            run_id: self.start_manifest.run_id.clone(),
            segment_count: self.segments.len(),
            transaction_count: self.transactions.len(),
            account_update_count: self.account_updates.len(),
            slot_update_count: self.slots.len(),
            coverage_gap_count: self.coverage_gaps.len(),
        }
    }

    pub(crate) fn read_record(
        &self,
        pointer: PumpResearchRawRecordPointerV1,
    ) -> Result<PumpResearchRawRecordV1> {
        let segment = self.segments.get(pointer.segment_position).ok_or_else(|| {
            anyhow::anyhow!(
                "raw record points to missing segment position {}",
                pointer.segment_position
            )
        })?;
        let fallback_file;
        let file = if let Some(authority) = &self.raw_segment_set_authority {
            authority
                .pinned_files
                .get(pointer.segment_position)
                .map(Arc::as_ref)
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "raw snapshot authority has no segment position {}",
                        pointer.segment_position
                    )
                })?
        } else {
            fallback_file = open_regular_nofollow_v1(&segment.path, "raw segment")?;
            &fallback_file
        };
        let frame =
            read_frozen_frame_from_open_file_v1(file, pointer.frame_offset).with_context(|| {
                format!(
                    "read indexed raw record from {} at {}",
                    segment.path.display(),
                    pointer.frame_offset
                )
            })?;
        PumpResearchRawCodecV1::decode_record(&frame)
            .map_err(|error| anyhow::anyhow!(error))
            .with_context(|| format!("decode indexed raw record from {}", segment.path.display()))
    }

    pub(crate) fn read_transaction(
        &self,
        indexed: &PumpResearchIndexedTransactionV1,
    ) -> Result<PumpPrimaryTransactionEvidenceV1> {
        match self.read_record(indexed.pointer)? {
            PumpResearchRawRecordV1::PrimaryTransaction(evidence) => Ok(evidence),
            other => {
                bail!("indexed transaction pointer resolved to unexpected raw record {other:?}")
            }
        }
    }

    pub(crate) fn read_account_update(
        &self,
        indexed: &PumpResearchIndexedAccountUpdateV1,
    ) -> Result<PumpPrimaryAccountUpdateEvidenceV1> {
        match self.read_record(indexed.pointer)? {
            PumpResearchRawRecordV1::PrimaryAccountUpdate(evidence) => Ok(evidence),
            other => bail!("indexed account pointer resolved to unexpected raw record {other:?}"),
        }
    }

    fn seal_raw_segment_set_snapshot_v1(&mut self, output_dir: &Path) -> Result<()> {
        if self.raw_segment_set_authority.is_some() {
            bail!("raw segment-set authority was already sealed");
        }
        let raw_dir = self
            .segments
            .first()
            .and_then(|segment| segment.path.parent())
            .ok_or_else(|| anyhow::anyhow!("indexed raw tape has no segment parent directory"))?;
        let canonical_output = validate_combined_exact_output_path_v1(raw_dir, output_dir)?;
        let snapshot_parent = canonical_output
            .parent()
            .ok_or_else(|| anyhow::anyhow!("canonical exact output has no parent"))?;
        let mut entries = Vec::with_capacity(self.segments.len());
        let mut pinned_files = Vec::with_capacity(self.segments.len());
        for segment in &self.segments {
            let canonical_source = fs::canonicalize(&segment.path)
                .with_context(|| format!("canonicalize raw segment {}", segment.path.display()))?;
            let (pinned, bytes) = copy_raw_segment_to_unlinked_snapshot_v1(
                &segment.path,
                snapshot_parent,
                &segment.receipt,
                segment.file_bytes,
            )?;
            entries.push(PumpResearchRawSegmentSetEntryV1 {
                segment_index: segment.receipt.segment_index,
                filename: segment.receipt.filename.clone(),
                canonical_source_path: canonical_source.to_string_lossy().into_owned(),
                bytes,
                file_sha256: segment.receipt.file_sha256,
                file_blake3: segment.receipt.file_blake3,
            });
            pinned_files.push(pinned);
        }
        let aggregate_bytes = serde_json::to_vec(&PumpResearchRawSegmentSetDigestInputV1 {
            schema_version: 1,
            entries: &entries,
        })
        .context("serialize raw segment-set authority")?;
        let authority = Arc::new(PumpResearchRawSegmentSetAuthorityV1 {
            entries,
            aggregate_blake3: blake3::hash(&aggregate_bytes).to_hex().to_string(),
            pinned_files,
        });
        authority.revalidate_v1(&self.segments, "post-snapshot-seal")?;
        self.raw_segment_set_authority = Some(authority);
        Ok(())
    }

    fn raw_segment_set_authority_v1(&self) -> Result<&PumpResearchRawSegmentSetAuthorityV1> {
        self.raw_segment_set_authority.as_deref().ok_or_else(|| {
            anyhow::anyhow!("combined audit lacks a sealed raw segment-set authority")
        })
    }

    fn raw_segment_set_blake3_v1(&self) -> Option<&str> {
        self.raw_segment_set_authority
            .as_deref()
            .map(|authority| authority.aggregate_blake3.as_str())
    }

    fn has_raw_segment_set_authority_v1(&self) -> bool {
        self.raw_segment_set_authority.is_some()
    }

    fn revalidate_raw_segment_set_v1(&self, boundary: &str) -> Result<()> {
        self.raw_segment_set_authority_v1()?
            .revalidate_v1(&self.segments, boundary)
    }

    fn revalidate_raw_control_authority_v1(
        &self,
        raw_dir: &Path,
        boundary: &str,
    ) -> Result<PumpResearchRawControlAuthorityV1> {
        let current = PumpResearchRawControlAuthorityV1 {
            start_manifest_digest: digest_bounded_authority_file_v1(
                &raw_dir.join("run_start_manifest.json"),
                PumpResearchAuthorityFileKindV1::RawStartManifest,
            )?,
            completion_receipt_digest: digest_bounded_authority_file_v1(
                &raw_dir.join("run_completion_receipt.json"),
                PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
            )?,
            provenance_binding_digest: Some(digest_bounded_authority_file_v1(
                &raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
                PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
            )?),
        };
        if current.start_manifest_digest != self.raw_control_authority.start_manifest_digest
            || current.completion_receipt_digest
                != self.raw_control_authority.completion_receipt_digest
            || current.provenance_binding_digest
                != self.raw_control_authority.provenance_binding_digest
        {
            bail!("raw control authority changed after bounded parse at {boundary}");
        }
        Ok(current)
    }
}

/// Read and validate a closed raw run without changing it. This is the first
/// boundary used by certify; incomplete or partial runs are rejected before
/// the parser sees a transaction.
pub(crate) fn index_pump_research_raw_run_v1(run_dir: &Path) -> Result<PumpResearchRawTapeIndexV1> {
    let start_manifest_path = run_dir.join("run_start_manifest.json");
    let completion_receipt_path = run_dir.join("run_completion_receipt.json");
    let (start_manifest, start_manifest_digest): (
        PumpResearchRunStartManifestV1,
        PumpResearchOperatorDigestV1,
    ) = read_json_with_digest(
        &start_manifest_path,
        PumpResearchAuthorityFileKindV1::RawStartManifest,
    )?;
    let (completion_receipt, completion_receipt_digest): (
        PumpResearchRunCompletionReceiptV1,
        PumpResearchOperatorDigestV1,
    ) = read_json_with_digest(
        &completion_receipt_path,
        PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
    )?;
    let (capture_provenance_eligibility, provenance_binding_digest) =
        assess_capture_provenance_eligibility_v1(run_dir, &start_manifest.run_id);

    if start_manifest.storage_format_version != PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1 {
        bail!(
            "raw start manifest storage format {} is not frozen V1",
            start_manifest.storage_format_version
        );
    }
    if completion_receipt.storage_format_version != PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1 {
        bail!(
            "raw completion receipt storage format {} is not frozen V1",
            completion_receipt.storage_format_version
        );
    }
    if completion_receipt.run_id != start_manifest.run_id {
        bail!(
            "raw completion receipt run_id {} differs from start manifest {}",
            completion_receipt.run_id,
            start_manifest.run_id
        );
    }
    if !matches!(
        completion_receipt.status,
        PumpResearchRunCompletionStatusV1::Complete
            | PumpResearchRunCompletionStatusV1::ProgramVersionBoundary
    ) {
        bail!(
            "raw run {} has incomplete capture status {:?}",
            start_manifest.run_id,
            completion_receipt.status
        );
    }
    if !completion_receipt.clean_shutdown
        || !completion_receipt.source_stream_established
        || !completion_receipt.first_source_update_received
        || !completion_receipt.source_workers_cleanly_stopped
    {
        bail!(
            "raw run {} lacks a clean, established source lifecycle",
            start_manifest.run_id
        );
    }
    if completion_receipt.segment_list.is_empty() {
        bail!(
            "raw run {} has no published segments",
            start_manifest.run_id
        );
    }

    let mut segments = Vec::with_capacity(completion_receipt.segment_list.len());
    let mut transactions = Vec::new();
    let mut account_updates = Vec::new();
    let mut slots = BTreeMap::new();
    let mut block_meta: BTreeMap<u64, Vec<PumpPrimaryBlockMetaEvidenceV1>> = BTreeMap::new();
    let mut coverage_gaps = Vec::new();
    let mut expected_previous_prefix_hash = None;
    let mut previous_segment_index = None;
    let mut previous_stream_epoch = None;
    let mut previous_capture_sequence = None;
    let mut source_record_count = 0u64;

    for receipt in &completion_receipt.segment_list {
        if receipt.segment_index != previous_segment_index.map_or(0, |index| index + 1) {
            bail!(
                "raw completion receipt has non-contiguous segment index {}",
                receipt.segment_index
            );
        }
        previous_segment_index = Some(receipt.segment_index);
        let filename = safe_segment_filename(&receipt.filename)?;
        let path = run_dir.join(filename);
        let segment_position = segments.len();
        let scanned = scan_frozen_segment(
            &path,
            receipt,
            &start_manifest.run_id,
            expected_previous_prefix_hash,
            |pointer, record| match record {
                PumpResearchRawRecordV1::PrimaryTransaction(evidence) => {
                    validate_raw_transaction_source(evidence)?;
                    observe_source_sequence(
                        &mut previous_capture_sequence,
                        &mut source_record_count,
                        &evidence.source,
                    )?;
                    transactions.push(PumpResearchIndexedTransactionV1 {
                        pointer: PumpResearchRawRecordPointerV1 {
                            segment_position,
                            frame_offset: pointer,
                        },
                        source: evidence.source.clone(),
                        slot: evidence.slot,
                        tx_index: evidence.tx_index,
                        signature: evidence.signature,
                        event_time: evidence.event_time,
                        block_time: evidence.block_time,
                    });
                    Ok(())
                }
                PumpResearchRawRecordV1::PrimaryAccountUpdate(evidence) => {
                    validate_raw_account_source(evidence)?;
                    observe_source_sequence(
                        &mut previous_capture_sequence,
                        &mut source_record_count,
                        &evidence.source,
                    )?;
                    account_updates.push(PumpResearchIndexedAccountUpdateV1 {
                        pointer: PumpResearchRawRecordPointerV1 {
                            segment_position,
                            frame_offset: pointer,
                        },
                        source: evidence.source.clone(),
                        account_role: evidence.account_role,
                        is_startup: evidence.is_startup,
                        account_pubkey: evidence.account_pubkey,
                        owner_program: evidence.owner_program,
                        raw_account_data_hash_blake3: evidence.raw_account_data_hash_blake3,
                        slot: evidence.slot,
                        write_version: evidence.write_version,
                        txn_signature: evidence.txn_signature,
                        event_time: evidence.event_time,
                    });
                    Ok(())
                }
                PumpResearchRawRecordV1::PrimarySlotUpdate(evidence) => {
                    validate_source_payload(&evidence.source, &evidence.source_payload, "slot")?;
                    observe_source_sequence(
                        &mut previous_capture_sequence,
                        &mut source_record_count,
                        &evidence.source,
                    )?;
                    record_slot_evidence(&mut slots, evidence);
                    Ok(())
                }
                PumpResearchRawRecordV1::PrimaryBlockMeta(evidence) => {
                    validate_source_payload(
                        &evidence.source,
                        &evidence.source_payload,
                        "block-meta",
                    )?;
                    observe_source_sequence(
                        &mut previous_capture_sequence,
                        &mut source_record_count,
                        &evidence.source,
                    )?;
                    block_meta
                        .entry(evidence.slot)
                        .or_default()
                        .push(evidence.clone());
                    Ok(())
                }
                PumpResearchRawRecordV1::CoverageGap(gap) => {
                    coverage_gaps.push(gap.clone());
                    Ok(())
                }
                PumpResearchRawRecordV1::SegmentClosed(_) => {
                    bail!("segment footer reached through non-footer raw record callback")
                }
            },
        )?;
        if previous_stream_epoch.is_some_and(|previous| scanned.header.stream_epoch < previous) {
            bail!(
                "raw segment {} stream epoch {} regresses below previous epoch {}",
                receipt.segment_index,
                scanned.header.stream_epoch,
                previous_stream_epoch.unwrap_or_default()
            );
        }
        previous_stream_epoch = Some(scanned.header.stream_epoch);
        expected_previous_prefix_hash = Some(scanned.prefix_hash);
        segments.push(PumpResearchIndexedSegmentV1 {
            receipt: receipt.clone(),
            path,
            header: scanned.header,
            file_bytes: scanned.file_bytes,
        });
    }

    if coverage_gaps.len() as u64 != completion_receipt.gap_count {
        bail!(
            "raw completion receipt reports {} gaps but {} gap records were found",
            completion_receipt.gap_count,
            coverage_gaps.len()
        );
    }
    if source_record_count != completion_receipt.persisted_source_record_count {
        bail!(
            "raw completion receipt reports {} persisted source records but {} source records were indexed",
            completion_receipt.persisted_source_record_count,
            source_record_count
        );
    }
    if transactions.is_empty() {
        bail!(
            "raw Pump-only run {} contains no transaction records",
            start_manifest.run_id
        );
    }

    Ok(PumpResearchRawTapeIndexV1 {
        start_manifest,
        completion_receipt,
        segments,
        transactions,
        account_updates,
        slots,
        block_meta,
        coverage_gaps,
        raw_control_authority: PumpResearchRawControlAuthorityV1 {
            start_manifest_digest,
            completion_receipt_digest,
            provenance_binding_digest,
        },
        raw_segment_set_authority: None,
        capture_provenance_eligibility,
    })
}

/// Inspect a raw run without materialising parser or state evidence.
pub fn inspect_pump_research_raw_run_v1(run_dir: &Path) -> Result<PumpResearchRawIndexSummaryV1> {
    Ok(index_pump_research_raw_run_v1(run_dir)?.summary())
}

fn read_json<T>(path: &Path, kind: PumpResearchAuthorityFileKindV1) -> Result<T>
where
    T: serde::de::DeserializeOwned,
{
    Ok(read_json_with_digest(path, kind)?.0)
}

fn read_json_with_digest<T>(
    path: &Path,
    kind: PumpResearchAuthorityFileKindV1,
) -> Result<(T, PumpResearchOperatorDigestV1)>
where
    T: serde::de::DeserializeOwned,
{
    let authority = read_bounded_authority_file_v1(path, kind)?;
    let value = serde_json::from_slice(&authority.bytes)
        .with_context(|| format!("decode JSON {}", path.display()))?;
    Ok((value, authority.digest))
}

fn operator_digest_equal(
    left: &PumpResearchOperatorDigestV1,
    right: &PumpResearchOperatorDigestV1,
) -> bool {
    left == right
}

/// Read the capture-side binding as a qualification gate, never as a raw
/// evidence source. The sidecar was deliberately kept outside frozen V1 raw
/// records; consequently a missing, malformed or pre-correction binding is
/// ineligible rather than a reason to discard the raw segments themselves.
fn assess_capture_provenance_eligibility_v1(
    run_dir: &Path,
    expected_run_id: &str,
) -> (
    PumpResearchCaptureProvenanceEligibilityV1,
    Option<PumpResearchOperatorDigestV1>,
) {
    let path = run_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1);
    let authority = match read_bounded_authority_file_v1(
        &path,
        PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
    ) {
        Ok(authority) => authority,
        Err(error) if error_has_io_kind_v1(&error, std::io::ErrorKind::NotFound) => {
            return (
                PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                    PumpResearchCaptureProvenanceIneligibilityReasonV1::MissingBinding,
                ),
                None,
            )
        }
        Err(_) => {
            return (
                PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                    PumpResearchCaptureProvenanceIneligibilityReasonV1::UnreadableBinding,
                ),
                None,
            )
        }
    };
    let digest = authority.digest;
    let binding: PumpResearchCaptureProvenanceBindingV1 =
        match serde_json::from_slice(&authority.bytes) {
            Ok(binding) => binding,
            Err(_) => {
                return (
                    PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                        PumpResearchCaptureProvenanceIneligibilityReasonV1::MalformedBinding,
                    ),
                    Some(digest),
                )
            }
        };
    if binding.schema_version != 1
        || binding.binding_kind != OPERATOR_PREFLIGHT_BINDING_KIND_V1
        || binding.run_id != expected_run_id
        || binding.build_semantics != OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1
        || binding.credential_scan_semantics != OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1
    {
        return (
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::LegacyOrUnsupportedBinding,
            ),
            Some(digest),
        );
    }
    if !binding.qualification_provenance_eligible {
        return (
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::IneligibleBinding,
            ),
            Some(digest),
        );
    }
    let Some(sealed_release_binary_digest) = binding.sealed_release_binary_digest else {
        return (
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::LegacyOrUnsupportedBinding,
            ),
            Some(digest),
        );
    };
    if !operator_digest_equal(
        &sealed_release_binary_digest,
        &binding.release_binary_digest,
    ) {
        return (
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::SealedBinaryMismatch,
            ),
            Some(digest),
        );
    }
    (
        PumpResearchCaptureProvenanceEligibilityV1::Eligible,
        Some(digest),
    )
}

fn safe_segment_filename(value: &str) -> Result<&Path> {
    let path = Path::new(value);
    if path.as_os_str().is_empty()
        || path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
        || path.extension().and_then(|extension| extension.to_str()) != Some("bin")
    {
        bail!("unsafe raw segment filename {value:?}");
    }
    Ok(path)
}

fn validate_raw_transaction_source(evidence: &PumpPrimaryTransactionEvidenceV1) -> Result<()> {
    validate_source_payload(&evidence.source, &evidence.source_payload, "transaction")
}

fn validate_raw_account_source(evidence: &PumpPrimaryAccountUpdateEvidenceV1) -> Result<()> {
    validate_source_payload(&evidence.source, &evidence.source_payload, "account")?;
    let account_hash =
        PumpResearchStorageHashV1::from(*blake3::hash(&evidence.raw_account_data).as_bytes());
    if account_hash != evidence.raw_account_data_hash_blake3 {
        bail!(
            "raw account data hash mismatch at capture sequence {}",
            evidence.source.capture_sequence
        );
    }
    Ok(())
}

fn validate_source_payload(
    source: &PumpRawSourceEnvelopeV1,
    source_payload: &[u8],
    kind: &'static str,
) -> Result<()> {
    let payload_hash = PumpResearchStorageHashV1::from(*blake3::hash(source_payload).as_bytes());
    if payload_hash != source.payload_hash_blake3 {
        bail!(
            "raw {kind} source payload hash mismatch at capture sequence {}",
            source.capture_sequence
        );
    }
    Ok(())
}

fn observe_source_sequence(
    previous_capture_sequence: &mut Option<u64>,
    source_record_count: &mut u64,
    source: &PumpRawSourceEnvelopeV1,
) -> Result<()> {
    if let Some(previous) = *previous_capture_sequence {
        if source.capture_sequence <= previous {
            bail!(
                "raw source capture sequence {} is not greater than previous {}",
                source.capture_sequence,
                previous
            );
        }
    }
    *previous_capture_sequence = Some(source.capture_sequence);
    *source_record_count = source_record_count.saturating_add(1);
    Ok(())
}

fn record_slot_evidence(
    slots: &mut BTreeMap<u64, PumpResearchRawSlotNodeV1>,
    evidence: &PumpPrimarySlotEvidenceV1,
) {
    let node = slots.entry(evidence.slot).or_default();
    if let Some(parent) = evidence.parent {
        node.parents.insert(parent);
    }
    match evidence.source_status {
        // Frozen protobuf values: Processed=0, Confirmed=1, Finalized=2.
        0 => node.saw_processed = true,
        1 => node.saw_confirmed = true,
        2 => node.saw_finalized = true,
        _ => {}
    }
}

struct PumpResearchScannedSegmentV1 {
    header: PumpRawSegmentHeaderV1,
    prefix_hash: PumpResearchStorageHashV1,
    file_bytes: u64,
}

fn raw_record_stream_epoch(record: &PumpResearchRawRecordV1) -> Option<u64> {
    match record {
        PumpResearchRawRecordV1::PrimaryTransaction(evidence) => Some(evidence.source.stream_epoch),
        PumpResearchRawRecordV1::PrimaryAccountUpdate(evidence) => {
            Some(evidence.source.stream_epoch)
        }
        PumpResearchRawRecordV1::PrimarySlotUpdate(evidence) => Some(evidence.source.stream_epoch),
        PumpResearchRawRecordV1::PrimaryBlockMeta(evidence) => Some(evidence.source.stream_epoch),
        PumpResearchRawRecordV1::CoverageGap(gap) => Some(gap.stream_epoch),
        PumpResearchRawRecordV1::SegmentClosed(_) => None,
    }
}

fn scan_frozen_segment<F>(
    path: &Path,
    receipt: &PumpResearchSegmentReceiptV1,
    expected_run_id: &str,
    expected_previous_prefix_hash: Option<PumpResearchStorageHashV1>,
    mut on_record: F,
) -> Result<PumpResearchScannedSegmentV1>
where
    F: FnMut(u64, &PumpResearchRawRecordV1) -> Result<()>,
{
    let file = open_regular_nofollow_v1(path, "raw segment")?;
    let expected_file_bytes = file
        .metadata()
        .with_context(|| format!("inspect raw segment {} before frozen scan", path.display()))?
        .len();
    // The extra byte makes trailing/growing-file drift observable without
    // permitting an unbounded read-until-EOF scan.
    let mut reader = BufReader::new(file.take(expected_file_bytes.saturating_add(1)));
    let mut offset = 0u64;
    let mut magic = [0u8; PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1.len()];
    reader
        .read_exact(&mut magic)
        .with_context(|| format!("read raw segment magic {}", path.display()))?;
    offset = offset.saturating_add(u64::try_from(magic.len()).unwrap_or(u64::MAX));
    if magic != PUMP_RESEARCH_RAW_SEGMENT_MAGIC_V1 {
        bail!("raw segment {} has invalid V1 magic", path.display());
    }
    let Some((_header_offset, header_frame)) = read_frozen_frame(&mut reader, offset)? else {
        bail!("raw segment {} has no V1 header frame", path.display());
    };
    offset = offset.saturating_add(u64::try_from(header_frame.len()).unwrap_or(u64::MAX));
    let mut header_bytes = magic.to_vec();
    header_bytes.extend_from_slice(&header_frame);
    let header = PumpResearchRawCodecV1::decode_segment_header(&header_bytes)
        .map_err(|error| anyhow::anyhow!(error))
        .with_context(|| format!("decode raw segment header {}", path.display()))?;
    if header.storage_format_version != PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1
        || header.run_id != expected_run_id
        || header.segment_index != receipt.segment_index
        || header.previous_segment_blake3 != expected_previous_prefix_hash
    {
        bail!(
            "raw segment {} header does not match run or chain receipt",
            path.display()
        );
    }

    let mut prefix_hasher = blake3::Hasher::new();
    prefix_hasher.update(&header_bytes);
    let mut file_blake3_hasher = blake3::Hasher::new();
    file_blake3_hasher.update(&header_bytes);
    let mut file_sha256_hasher = Sha256::new();
    file_sha256_hasher.update(&header_bytes);
    let mut accepted_record_count = 0u64;
    let mut data_bytes = u64::try_from(header_bytes.len()).unwrap_or(u64::MAX);
    let mut footer: Option<PumpRawSegmentClosedV1> = None;

    while let Some((frame_offset, frame)) = read_frozen_frame(&mut reader, offset)? {
        offset = offset.saturating_add(u64::try_from(frame.len()).unwrap_or(u64::MAX));
        file_blake3_hasher.update(&frame);
        file_sha256_hasher.update(&frame);
        let record = PumpResearchRawCodecV1::decode_record(&frame)
            .map_err(|error| anyhow::anyhow!(error))
            .with_context(|| format!("decode raw record at {}:{}", path.display(), frame_offset))?;
        if raw_record_stream_epoch(&record)
            .is_some_and(|stream_epoch| stream_epoch != header.stream_epoch)
        {
            bail!(
                "raw segment {} contains a record whose stream epoch differs from header epoch {}",
                path.display(),
                header.stream_epoch
            );
        }
        match &record {
            PumpResearchRawRecordV1::SegmentClosed(closed) => {
                if footer.replace(closed.clone()).is_some() {
                    bail!(
                        "raw segment {} contains more than one footer",
                        path.display()
                    );
                }
                break;
            }
            _ => {
                prefix_hasher.update(&frame);
                accepted_record_count = accepted_record_count.saturating_add(1);
                data_bytes =
                    data_bytes.saturating_add(u64::try_from(frame.len()).unwrap_or(u64::MAX));
                on_record(frame_offset, &record)?;
            }
        }
    }

    let footer = footer
        .ok_or_else(|| anyhow::anyhow!("raw segment {} lacks terminal footer", path.display()))?;
    // A footer must be physically terminal, not merely the last decoded record.
    let mut trailing = [0u8; 1];
    if reader.read(&mut trailing)? != 0 {
        bail!(
            "raw segment {} contains trailing bytes after footer",
            path.display()
        );
    }
    if offset != expected_file_bytes {
        bail!(
            "raw segment {} decoded {offset} bytes but opened size was {expected_file_bytes}",
            path.display()
        );
    }
    let final_metadata = reader
        .get_ref()
        .get_ref()
        .metadata()
        .with_context(|| format!("inspect raw segment {} after frozen scan", path.display()))?;
    if !final_metadata.is_file() || final_metadata.len() != expected_file_bytes {
        bail!(
            "raw segment {} size changed during frozen scan: {} != {expected_file_bytes}",
            path.display(),
            final_metadata.len()
        );
    }
    let prefix_hash = PumpResearchStorageHashV1::from(*prefix_hasher.finalize().as_bytes());
    if footer.storage_format_version != PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1
        || footer.segment_index != receipt.segment_index
        || footer.accepted_record_count != accepted_record_count
        || footer.data_bytes != data_bytes
        || footer.segment_blake3 != prefix_hash
    {
        bail!(
            "raw segment {} footer does not match frozen contents",
            path.display()
        );
    }
    if receipt.accepted_record_count != accepted_record_count {
        bail!(
            "raw segment {} receipt record count mismatch",
            path.display()
        );
    }
    let file_blake3 = PumpResearchStorageHashV1::from(*file_blake3_hasher.finalize().as_bytes());
    let file_sha256_bytes: [u8; 32] = file_sha256_hasher.finalize().into();
    let file_sha256 = PumpResearchStorageHashV1::from(file_sha256_bytes);
    if receipt.file_blake3 != file_blake3 || receipt.file_sha256 != file_sha256 {
        bail!("raw segment {} whole-file digest mismatch", path.display());
    }
    Ok(PumpResearchScannedSegmentV1 {
        header,
        prefix_hash,
        file_bytes: expected_file_bytes,
    })
}

/// Read exactly one frozen V1 length-delimited frame. Offset is the beginning
/// of the frame. EOF before the first length byte is normal only between
/// records.
fn read_frozen_frame<R: Read>(reader: &mut R, offset: u64) -> Result<Option<(u64, Vec<u8>)>> {
    let mut length_bytes = [0u8; 4];
    let first = reader.read(&mut length_bytes[..1])?;
    if first == 0 {
        return Ok(None);
    }
    reader
        .read_exact(&mut length_bytes[1..])
        .context("read frozen V1 frame length")?;
    let payload_length = u32::from_le_bytes(length_bytes) as usize;
    if payload_length > PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1 {
        bail!(
            "frozen V1 frame at offset {offset} declares payload {payload_length} above {}",
            PUMP_RESEARCH_RAW_RECORD_MAX_BYTES_V1
        );
    }
    let total_length = 4usize
        .checked_add(payload_length)
        .and_then(|value| value.checked_add(32))
        .ok_or_else(|| anyhow::anyhow!("frozen frame length overflow at offset {offset}"))?;
    let mut frame = vec![0u8; total_length];
    frame[..4].copy_from_slice(&length_bytes);
    reader
        .read_exact(&mut frame[4..])
        .with_context(|| format!("read frozen V1 frame at offset {offset}"))?;
    Ok(Some((offset, frame)))
}

/// Materialised slot status.  This is deliberately derived only from the
/// immutable raw SlotUpdate/BlockMeta evidence; no RPC response can promote a
/// raw `Unresolved` slot into a canonical one.
#[derive(Clone, Debug, Default)]
pub(crate) struct PumpResearchSlotCanonicalityIndexV1 {
    by_slot: BTreeMap<u64, PumpSlotCanonicalityV1>,
}

impl PumpResearchSlotCanonicalityIndexV1 {
    pub(crate) fn classify(&self, slot: u64) -> PumpSlotCanonicalityV1 {
        self.by_slot
            .get(&slot)
            .copied()
            .unwrap_or(PumpSlotCanonicalityV1::Unresolved)
    }
}

fn build_slot_canonicality_index(
    raw: &PumpResearchRawTapeIndexV1,
) -> PumpResearchSlotCanonicalityIndexV1 {
    let mut nodes = raw.slots.clone();
    for (slot, block_meta) in &raw.block_meta {
        let node = nodes.entry(*slot).or_default();
        for meta in block_meta {
            node.parents.insert(meta.parent_slot);
        }
    }

    let mut by_slot = BTreeMap::new();
    let highest_finalized = nodes
        .iter()
        .filter_map(|(slot, node)| node.saw_finalized.then_some(*slot))
        .max();
    let rooted_lineage = highest_finalized
        .and_then(|root| complete_parent_lineage(&nodes, root))
        .unwrap_or_default();
    // Coverage evidence is local. A gap far away from a slot (and from the
    // finalized lineage used to prove a competing fork dead) must not turn a
    // whole otherwise intact run into an artificial unresolved tail. A gap
    // with no usable boundary is intentionally treated by `gap_affects_slot`
    // as affecting every slot, which remains fail-closed.
    let rooted_lineage_gap_free = rooted_lineage
        .iter()
        .all(|slot| !gap_affects_slot(raw, *slot));

    for (slot, node) in &nodes {
        let slot_gap_free = !gap_affects_slot(raw, *slot);
        let canonicality = if node.saw_finalized && slot_gap_free {
            PumpSlotCanonicalityV1::RootedCanonical
        } else if slot_gap_free
            && rooted_lineage_gap_free
            && highest_finalized.is_some_and(|root| *slot <= root)
            && !rooted_lineage.is_empty()
            && !rooted_lineage.contains(slot)
            && !node.saw_finalized
        {
            // A later finalized root has a complete preserved parent lineage
            // through this capture range and the observed fork slot is not on
            // it.  This is the only PR-B route to `Dead`.
            PumpSlotCanonicalityV1::Dead
        } else {
            PumpSlotCanonicalityV1::Unresolved
        };
        by_slot.insert(*slot, canonicality);
    }

    PumpResearchSlotCanonicalityIndexV1 { by_slot }
}

/// Return a lineage only when every parent edge is unambiguous and preserved.
/// Reaching a parent which predates the captured graph is acceptable: it is a
/// boundary of this raw run, not evidence that the child forked.
fn complete_parent_lineage(
    nodes: &BTreeMap<u64, PumpResearchRawSlotNodeV1>,
    root: u64,
) -> Option<BTreeSet<u64>> {
    let mut lineage = BTreeSet::new();
    let mut current = root;
    loop {
        if !lineage.insert(current) {
            return None;
        }
        let node = nodes.get(&current)?;
        if node.parents.is_empty() {
            return Some(lineage);
        }
        if node.parents.len() != 1 {
            return None;
        }
        let parent = *node.parents.iter().next()?;
        if !nodes.contains_key(&parent) {
            return Some(lineage);
        }
        current = parent;
    }
}

fn gap_affects_slot(raw: &PumpResearchRawTapeIndexV1, slot: u64) -> bool {
    raw.coverage_gaps
        .iter()
        .any(|gap| coverage_gap_affects_slot(gap, slot))
}

fn gap_affects_stream_epoch_slot(
    raw: &PumpResearchRawTapeIndexV1,
    stream_epoch: u64,
    slot: u64,
) -> bool {
    raw.coverage_gaps
        .iter()
        .any(|gap| gap.stream_epoch == stream_epoch && coverage_gap_affects_slot(gap, slot))
}

fn coverage_gap_affects_slot(gap: &PumpRawCoverageGapV1, slot: u64) -> bool {
    let boundaries = [
        gap.before.slot,
        gap.after.slot,
        gap.first_dropped.slot,
        gap.last_dropped.slot,
    ];
    let mut known = boundaries.into_iter().flatten();
    let Some(first) = known.next() else {
        // Without a slot boundary, it is not safe to assert that any
        // particular mutation escaped the local-loss episode.
        return true;
    };
    let (mut low, mut high) = (first, first);
    for boundary_slot in known {
        low = low.min(boundary_slot);
        high = high.max(boundary_slot);
    }
    (low..=high).contains(&slot)
}

#[derive(Clone, Debug)]
struct PumpResearchAcceptedCurveAnchorV1 {
    anchor: PumpAccountAnchorV1,
    source_capture_sequence: u64,
    source_transaction_index: Option<u32>,
    is_startup: bool,
}

#[derive(Clone, Debug)]
struct PumpResearchAcceptedGlobalAnchorV1 {
    source_ref: PumpRawSourceRefV1,
    account_pubkey: Pubkey,
    slot: u64,
    write_version: u64,
    txn_signature: Option<Signature>,
    source_capture_sequence: u64,
    source_transaction_index: Option<u32>,
    is_startup: bool,
    state: GlobalState,
}

#[derive(Clone, Debug, Default)]
struct PumpResearchAccountAnchorIndexV1 {
    curves: HashMap<Pubkey, Vec<PumpResearchAcceptedCurveAnchorV1>>,
    final_by_signature: HashMap<(Signature, Pubkey), Vec<PumpResearchAcceptedCurveAnchorV1>>,
    global: Vec<PumpResearchAcceptedGlobalAnchorV1>,
    curve_conflicts: HashSet<Pubkey>,
    global_conflict: bool,
}

fn build_account_anchor_index(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
) -> Result<PumpResearchAccountAnchorIndexV1> {
    let mut arbiters: HashMap<Pubkey, AccountObservationArbiter> = HashMap::new();
    let mut index = PumpResearchAccountAnchorIndexV1::default();
    let canonical_global = Pubkey::from_str(PUMP_RESEARCH_PUMP_GLOBAL_BASE58_V1)
        .context("parse frozen canonical Pump Global pubkey")?;
    let transaction_indices: HashMap<Signature, u32> = raw
        .transactions
        .iter()
        .filter_map(|transaction| {
            (canonicality.classify(transaction.slot) == PumpSlotCanonicalityV1::RootedCanonical)
                .then(|| {
                    let signature = Signature::from(transaction.signature.into_inner());
                    transaction.tx_index.map(|tx_index| (signature, tx_index))
                })
                .flatten()
        })
        .collect();

    for indexed in &raw.account_updates {
        if canonicality.classify(indexed.slot) != PumpSlotCanonicalityV1::RootedCanonical
            || gap_affects_slot(raw, indexed.slot)
        {
            continue;
        }
        let account = raw.read_account_update(indexed)?;
        let account_pubkey = pump_research_pubkey_from_storage_v1(account.account_pubkey);
        let expected_owner =
            pump_research_pubkey_from_storage_v1(raw.start_manifest.pump_program_id);
        if account.account_role == PumpResearchAccountRoleV1::TransitionDependencyGlobal
            && account_pubkey != canonical_global
        {
            // The V1 dependency closure permits only the canonical Pump
            // Global account. A discriminator match on another Pump-owned
            // account is not historical Create-state authority.
            index.global_conflict = true;
            continue;
        }
        if pump_research_pubkey_from_storage_v1(account.owner_program) != expected_owner {
            match account.account_role {
                PumpResearchAccountRoleV1::BondingCurve => {
                    index.curve_conflicts.insert(account_pubkey);
                }
                PumpResearchAccountRoleV1::TransitionDependencyGlobal => {
                    index.global_conflict = true;
                }
            }
            continue;
        }

        let curve_state = match account.account_role {
            PumpResearchAccountRoleV1::BondingCurve => {
                match strict_curve_state(&account.raw_account_data) {
                    Some(state) => Some(state),
                    None => {
                        // The exact account payload exists but its frozen state
                        // layout cannot be decoded. It must not become an anchor.
                        None
                    }
                }
            }
            PumpResearchAccountRoleV1::TransitionDependencyGlobal => None,
        };
        let global_state = match account.account_role {
            PumpResearchAccountRoleV1::TransitionDependencyGlobal => {
                strict_global_state(&account.raw_account_data)
            }
            PumpResearchAccountRoleV1::BondingCurve => None,
        };

        if curve_state.is_none() && global_state.is_none() {
            continue;
        }

        let arbiter = arbiters.entry(account_pubkey).or_insert_with(|| {
            AccountObservationArbiter::with_limits(AccountObservationArbiterLimitsV1 {
                // Offline materialisation has a finite, closed input. Keep a
                // large explicit bound rather than letting normal live limits
                // create an artificial historical gap for a hot curve.
                max_versions_per_account: 16_384,
                max_unique_observations_per_version: 8,
                max_identity_conflicts_per_account: 128,
                max_identity_transitions_per_account: 16,
            })
        });
        let account_data_hash =
            blake3::Hash::from(account.raw_account_data_hash_blake3.into_inner())
                .to_hex()
                .to_string();
        let update = AccountStateUpdate {
            pool_amm_id: account_pubkey,
            base_mint: Pubkey::default(),
            bonding_curve: account_pubkey,
            sol_reserves: curve_state.map_or(0, |state| state.real_quote_reserves),
            token_reserves: curve_state.map_or(0, |state| state.real_token_reserves),
            is_complete: curve_state.map_or(0, |state| u8::from(state.complete)),
            slot: account.slot,
            write_version: Some(account.write_version),
            source_account_pubkey: Some(account_pubkey),
            source_account_owner_or_program: Some(expected_owner),
            account_data_len: Some(
                u64::try_from(account.raw_account_data.len()).unwrap_or(u64::MAX),
            ),
            account_data_hash: Some(account_data_hash),
            receive_ts_ms: account
                .event_time
                .ingress_wall_ts_ms
                .unwrap_or(account.source.capture_sequence),
            receive_seq: account.source.capture_sequence,
            curve_finality: CurveFinality::Finalized,
            source: UpdateSource::WalReplay,
            provider_id: Some(account.source.provider_id.clone()),
            provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            txn_signature: account
                .txn_signature
                .map(|signature| Signature::from(signature.into_inner())),
        };
        let decision = arbiter.arbitrate(&update);
        if matches!(
            decision.classification,
            AccountObservationClassificationV1::SameVersionDifferentHashConflict
                | AccountObservationClassificationV1::AccountIdentityConflict
        ) {
            match account.account_role {
                PumpResearchAccountRoleV1::BondingCurve => {
                    index.curve_conflicts.insert(account_pubkey);
                }
                PumpResearchAccountRoleV1::TransitionDependencyGlobal => {
                    index.global_conflict = true;
                }
            }
        }
        if !decision.canonical_apply {
            continue;
        }

        let source_ref = source_ref_for_account(raw, indexed);
        let source_transaction_index = account.txn_signature.and_then(|signature| {
            transaction_indices
                .get(&Signature::from(signature.into_inner()))
                .copied()
        });
        if let Some(state) = curve_state {
            let anchor = PumpAccountAnchorV1 {
                source_ref: source_ref.clone(),
                account_pubkey: account.account_pubkey,
                slot: account.slot,
                write_version: account.write_version,
                txn_signature: account.txn_signature,
                raw_account_data_hash_blake3: account.raw_account_data_hash_blake3,
                state,
            };
            let accepted = PumpResearchAcceptedCurveAnchorV1 {
                anchor,
                source_capture_sequence: account.source.capture_sequence,
                source_transaction_index,
                is_startup: account.is_startup,
            };
            if let Some(signature) = accepted.anchor.txn_signature {
                index
                    .final_by_signature
                    .entry((Signature::from(signature.into_inner()), account_pubkey))
                    .or_default()
                    .push(accepted.clone());
            }
            index
                .curves
                .entry(account_pubkey)
                .or_default()
                .push(accepted);
        }
        if let Some(state) = global_state {
            index.global.push(PumpResearchAcceptedGlobalAnchorV1 {
                source_ref,
                account_pubkey,
                slot: account.slot,
                write_version: account.write_version,
                txn_signature: account
                    .txn_signature
                    .map(|signature| Signature::from(signature.into_inner())),
                source_capture_sequence: account.source.capture_sequence,
                source_transaction_index,
                is_startup: account.is_startup,
                state,
            });
        }
    }

    for anchors in index.curves.values_mut() {
        anchors.sort_by_key(anchor_sort_key);
    }
    for anchors in index.final_by_signature.values_mut() {
        anchors.sort_by_key(anchor_sort_key);
    }
    index.global.sort_by_key(global_anchor_sort_key);
    Ok(index)
}

fn strict_curve_state(data: &[u8]) -> Option<PumpCurveStateV1> {
    // The V1 curve layout is frozen at the account boundary. Do not reuse a
    // lenient runtime decoder here: an exact anchor needs the expected
    // discriminator, exact fixed field range, and a valid bool byte.
    const CURVE_ACCOUNT_BYTES_V1: usize = 49;
    if data.len() != CURVE_ACCOUNT_BYTES_V1 || data[..8] != DISC_BONDING_CURVE {
        return None;
    }
    let read_u64 = |offset: usize| -> Option<u64> {
        Some(u64::from_le_bytes(
            data.get(offset..offset + 8)?.try_into().ok()?,
        ))
    };
    let complete = match data[48] {
        0 => false,
        1 => true,
        _ => return None,
    };
    Some(PumpCurveStateV1 {
        virtual_token_reserves: read_u64(8)?,
        virtual_quote_reserves: read_u64(16)?,
        real_token_reserves: read_u64(24)?,
        real_quote_reserves: read_u64(32)?,
        complete,
    })
}

fn strict_global_state(data: &[u8]) -> Option<GlobalState> {
    // The V1 Global dependency is equally layout-strict. A later program
    // layout change must make Create fallback non-evaluable, never silently
    // reinterpret bytes with an older decoder.
    const GLOBAL_ACCOUNT_BYTES_V1: usize = 113;
    if data.len() != GLOBAL_ACCOUNT_BYTES_V1 || data[..8] != DISC_GLOBAL_STATE {
        return None;
    }
    let authority: [u8; 32] = data[8..40].try_into().ok()?;
    let initialized = match data[40] {
        0 => false,
        1 => true,
        _ => return None,
    };
    let fee_recipient: [u8; 32] = data[41..73].try_into().ok()?;
    let read_u64 = |offset: usize| -> Option<u64> {
        Some(u64::from_le_bytes(
            data.get(offset..offset + 8)?.try_into().ok()?,
        ))
    };
    Some(GlobalState {
        authority,
        initialized,
        fee_recipient,
        initial_virtual_token_reserves: read_u64(73)?,
        initial_virtual_sol_reserves: read_u64(81)?,
        initial_real_token_reserves: read_u64(89)?,
        token_total_supply: read_u64(97)?,
        fee_basis_points: read_u64(105)?,
    })
}

fn source_ref_for_transaction(
    raw: &PumpResearchRawTapeIndexV1,
    indexed: &PumpResearchIndexedTransactionV1,
) -> PumpRawSourceRefV1 {
    PumpRawSourceRefV1 {
        run_id: raw.start_manifest.run_id.clone(),
        segment_index: raw.segments[indexed.pointer.segment_position]
            .receipt
            .segment_index,
        capture_sequence: indexed.source.capture_sequence,
        record_payload_hash_blake3: indexed.source.payload_hash_blake3,
    }
}

fn source_ref_for_account(
    raw: &PumpResearchRawTapeIndexV1,
    indexed: &PumpResearchIndexedAccountUpdateV1,
) -> PumpRawSourceRefV1 {
    PumpRawSourceRefV1 {
        run_id: raw.start_manifest.run_id.clone(),
        segment_index: raw.segments[indexed.pointer.segment_position]
            .receipt
            .segment_index,
        capture_sequence: indexed.source.capture_sequence,
        record_payload_hash_blake3: indexed.source.payload_hash_blake3,
    }
}

fn anchor_sort_key(anchor: &PumpResearchAcceptedCurveAnchorV1) -> (u64, u32, u64, u64) {
    (
        anchor.anchor.slot,
        anchor.source_transaction_index.unwrap_or(u32::MAX),
        anchor.anchor.write_version,
        anchor.source_capture_sequence,
    )
}

fn global_anchor_sort_key(anchor: &PumpResearchAcceptedGlobalAnchorV1) -> (u64, u32, u64, u64) {
    (
        anchor.slot,
        anchor.source_transaction_index.unwrap_or(u32::MAX),
        anchor.write_version,
        anchor.source_capture_sequence,
    )
}

/// One durable status row.  These JSONL records explain why a trajectory,
/// fork, or unrepresentable transaction did not enter an exact window rather
/// than silently removing it from the research denominator.
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpResearchCoverageRecordKindV1 {
    Slot,
    Trajectory,
    TransactionWithoutTrajectory,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PumpResearchCoverageStatusV1 {
    pub schema_version: u16,
    pub kind: PumpResearchCoverageRecordKindV1,
    pub slot: u64,
    pub signature: Option<PumpResearchStorageSignatureV1>,
    pub bonding_curve: Option<PumpResearchStoragePubkeyV1>,
    pub canonicality: PumpSlotCanonicalityV1,
    pub certification: Option<PumpTrajectoryCertificationV1>,
    pub mutation_count: u32,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct PumpResearchCertificationSummaryV1 {
    pub source_run_id: String,
    pub qualification_status: PumpResearchTapeQualificationStatusV1,
    pub rooted_canonical_slots: u64,
    pub dead_fork_slots: u64,
    pub unresolved_slots: u64,
    pub transaction_count: u64,
    pub trajectory_count: u64,
    pub exact_trajectory_count: u64,
    pub successful_rooted_mutation_count: u64,
    pub exact_rooted_mutation_count: u64,
    pub birth_count: u64,
    pub output_dir: PathBuf,
}

impl Default for PumpResearchCertificationSummaryV1 {
    fn default() -> Self {
        Self {
            source_run_id: String::new(),
            qualification_status: PumpResearchTapeQualificationStatusV1::Unqualified,
            rooted_canonical_slots: 0,
            dead_fork_slots: 0,
            unresolved_slots: 0,
            transaction_count: 0,
            trajectory_count: 0,
            exact_trajectory_count: 0,
            successful_rooted_mutation_count: 0,
            exact_rooted_mutation_count: 0,
            birth_count: 0,
            output_dir: PathBuf::new(),
        }
    }
}

const DEFAULT_QUALIFICATION_CONCURRENCY_V1: usize = 8;
const DEFAULT_QUALIFICATION_RETRIES_V1: u32 = 2;
const DEFAULT_QUALIFICATION_TIMEOUT_MS_V1: u64 = 20_000;

/// Read-only input for the optional independent source-completeness audit.
/// It intentionally is not part of `SeerConfig` or the primary capture
/// configuration. The endpoint can only verify a closed raw tape; its data is
/// never inserted into raw evidence, anchors, inventory or trajectories.
#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PumpResearchQualificationAuditConfigV1 {
    pub audit_provider_id: String,
    pub audit_rpc_endpoint: String,
    /// Optional complete URL path loaded from a dedicated operator env var.
    /// This supports providers whose credential is encoded in the endpoint
    /// path without ever persisting that path in TOML or qualification JSON.
    #[serde(default)]
    pub audit_rpc_endpoint_path_env: Option<String>,
    #[serde(default)]
    pub audit_rpc_auth_token_env: Option<String>,
    #[serde(default = "default_audit_rpc_auth_header")]
    pub audit_rpc_auth_header: String,
    #[serde(default = "default_qualification_concurrency")]
    pub bounded_concurrency: usize,
    #[serde(default = "default_qualification_retries")]
    pub bounded_retry_count: u32,
    #[serde(default = "default_qualification_timeout_ms")]
    pub request_timeout_ms: u64,
}

impl PumpResearchQualificationAuditConfigV1 {
    pub fn load(path: &Path) -> Result<Self> {
        let authority = read_bounded_authority_file_v1(
            path,
            PumpResearchAuthorityFileKindV1::QualificationAuditConfig,
        )?;
        Self::from_bytes(path, &authority.bytes)
    }

    fn from_bytes(path: &Path, bytes: &[u8]) -> Result<Self> {
        let text = std::str::from_utf8(bytes).with_context(|| {
            format!("qualification audit config {} is not UTF-8", path.display())
        })?;
        let config: Self = toml::from_str(text)
            .with_context(|| format!("parse qualification audit config {}", path.display()))?;
        config.validate()?;
        Ok(config)
    }

    fn validate(&self) -> Result<()> {
        validate_nonempty_trimmed("audit_provider_id", &self.audit_provider_id)?;
        validate_root_https_origin("audit_rpc_endpoint", &self.audit_rpc_endpoint)?;
        validate_nonempty_trimmed("audit_rpc_auth_header", &self.audit_rpc_auth_header)?;
        if let Some(variable) = &self.audit_rpc_auth_token_env {
            validate_nonempty_trimmed("audit_rpc_auth_token_env", variable)?;
        }
        if let Some(variable) = &self.audit_rpc_endpoint_path_env {
            validate_nonempty_trimmed("audit_rpc_endpoint_path_env", variable)?;
        }
        if self.audit_rpc_endpoint_path_env.is_some() && self.audit_rpc_auth_token_env.is_some() {
            bail!("qualification audit config must choose endpoint-path auth or header auth, not both");
        }
        if self.bounded_concurrency == 0 || self.bounded_concurrency > 64 {
            bail!("bounded_concurrency must be in 1..=64");
        }
        if self.bounded_retry_count > 8 {
            bail!("bounded_retry_count must be at most 8");
        }
        if self.request_timeout_ms == 0 || self.request_timeout_ms > 120_000 {
            bail!("request_timeout_ms must be in 1..=120000");
        }
        Ok(())
    }

    fn resolve_auth_token(&self) -> Result<Option<String>> {
        match &self.audit_rpc_auth_token_env {
            Some(name) => std::env::var(name)
                .with_context(|| {
                    format!("read audit RPC credential from environment variable {name}")
                })
                .map(Some),
            None => Ok(None),
        }
    }

    fn endpoint_with_path_credential(&self, path_credential: Option<&str>) -> Result<String> {
        let Some(path_credential) = path_credential else {
            return Ok(self.audit_rpc_endpoint.clone());
        };
        if path_credential.is_empty()
            || !path_credential.starts_with('/')
            || path_credential.len() > 2048
            || path_credential.contains(['?', '#', '\\'])
            || path_credential.contains("//")
            || path_credential
                .split('/')
                .any(|segment| matches!(segment, "." | ".."))
        {
            bail!("audit RPC endpoint-path credential has an invalid absolute path shape");
        }
        let mut endpoint = Url::parse(&self.audit_rpc_endpoint)
            .context("parse root audit RPC endpoint for path credential")?;
        endpoint.set_path(path_credential);
        Ok(endpoint.into())
    }

    fn resolve_connection(&self) -> Result<PumpResearchResolvedAuditConnectionV1> {
        let endpoint_path_credential = match &self.audit_rpc_endpoint_path_env {
            Some(name) => Some(std::env::var(name).with_context(|| {
                format!("read audit RPC endpoint-path credential from environment variable {name}")
            })?),
            None => None,
        };
        let auth_token = self.resolve_auth_token()?;
        let endpoint = self.endpoint_with_path_credential(endpoint_path_credential.as_deref())?;
        Ok(PumpResearchResolvedAuditConnectionV1 {
            endpoint,
            endpoint_path_credential,
            auth_token,
        })
    }
}

#[derive(Clone)]
struct PumpResearchResolvedAuditConnectionV1 {
    endpoint: String,
    endpoint_path_credential: Option<String>,
    auth_token: Option<String>,
}

const fn default_qualification_concurrency() -> usize {
    DEFAULT_QUALIFICATION_CONCURRENCY_V1
}

const fn default_qualification_retries() -> u32 {
    DEFAULT_QUALIFICATION_RETRIES_V1
}

const fn default_qualification_timeout_ms() -> u64 {
    DEFAULT_QUALIFICATION_TIMEOUT_MS_V1
}

fn default_audit_rpc_auth_header() -> String {
    "x-api-key".to_owned()
}

fn qualification_audit_auth_mode_v1(
    config: &PumpResearchQualificationAuditConfigV1,
) -> &'static str {
    if config.audit_rpc_endpoint_path_env.is_some() {
        "explicit_endpoint_path_auth_no_legacy_fallback"
    } else if config.audit_rpc_auth_token_env.is_some() {
        "explicit_per_client_auth"
    } else {
        "standalone_no_auth_no_legacy_fallback"
    }
}

const PROVIDER_SUITABILITY_BURST_SLOT_COUNT_V1: usize = 16;
const PROVIDER_SUITABILITY_MAX_RAW_REPRESENTATIVE_SCAN_V1: usize = 250_000;
const PROVIDER_SUITABILITY_MAX_CONSECUTIVE_UNAVAILABLE_V1: u32 = 3;
const PROVIDER_SUITABILITY_MAX_PROVIDER_WALL_MS_V1: u64 = 420_000;
const PROVIDER_SUITABILITY_RECEIPT_FILE_V1: &str = "provider_suitability_receipt_v1.json";
const PROVIDER_INDEPENDENCE_ATTESTATION_VERSION_V1: &str = "v1";
const PROVIDER_INDEPENDENCE_ATTESTATION_STATUS_V1: &str = "verified_independent";

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpResearchProviderSuitabilityStatusV1 {
    ReadyForFullAudit,
    BlockedNoQualificationRange,
    BlockedMissingRawRepresentative,
    BlockedAuditUnavailable,
    BlockedSampleMismatch,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PumpResearchProviderSuitabilitySummaryV1 {
    pub source_run_id: String,
    pub status: PumpResearchProviderSuitabilityStatusV1,
    pub output_dir: PathBuf,
    pub receipt_path: PathBuf,
    pub sample_slot_count: usize,
    pub attempted_slot_count: usize,
    pub matched_slot_count: usize,
    pub unavailable_slot_count: usize,
    pub total_request_attempt_count: u64,
    pub provider_elapsed_ms: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PumpResearchProviderSuitabilityFetchStatusV1 {
    Block,
    Skipped,
    Unavailable,
    NotAttemptedCircuitBreaker,
    NotAttemptedWallDeadline,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchProviderSuitabilitySlotFindingV1 {
    schema_version: u16,
    slot: u64,
    selection_roles: Vec<String>,
    fetch_status: PumpResearchProviderSuitabilityFetchStatusV1,
    request_attempt_count: u32,
    request_elapsed_ms: u64,
    raw_identity_count: u32,
    audit_identity_count: u32,
    raw_failed_transaction_count: u32,
    audit_failed_transaction_count: u32,
    raw_invocation_class_counts: BTreeMap<String, u64>,
    audit_invocation_class_counts: BTreeMap<String, u64>,
    raw_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    audit_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    identity_multiset_matches: bool,
    invocation_class_counts_match: bool,
    failed_status_multiset_matches: bool,
    audit_error: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchProviderSuitabilityReceiptV1 {
    schema_version: u16,
    kind: String,
    created_wall_ms: u64,
    source_run_id: String,
    status: PumpResearchProviderSuitabilityStatusV1,
    preparation_receipt_digest: PumpResearchOperatorDigestV1,
    audit_config_digest: PumpResearchOperatorDigestV1,
    executable_digest: PumpResearchOperatorDigestV1,
    raw_binding_digest: PumpResearchOperatorDigestV1,
    raw_start_manifest_digest: PumpResearchOperatorDigestV1,
    raw_completion_receipt_digest: PumpResearchOperatorDigestV1,
    audit_provider_id: String,
    audit_rpc_endpoint_blake3: String,
    audit_auth_mode: String,
    provider_identity_independence_verified: bool,
    qualification_stream_epoch: Option<u64>,
    qualification_start_slot: Option<u64>,
    qualification_end_slot: Option<u64>,
    qualification_blocker: Option<PumpResearchQualificationBlockerV1>,
    bounded_concurrency: usize,
    bounded_retry_count: u32,
    request_timeout_ms: u64,
    burst_slot_target_count: usize,
    max_raw_representative_scan: usize,
    max_consecutive_unavailable: u32,
    max_provider_wall_ms: u64,
    raw_representative_transactions_examined: usize,
    missing_raw_representative_roles: Vec<String>,
    sample_slot_count: usize,
    attempted_slot_count: usize,
    matched_slot_count: usize,
    unavailable_slot_count: usize,
    total_request_attempt_count: u64,
    provider_elapsed_ms: u64,
    slot_findings: Vec<PumpResearchProviderSuitabilitySlotFindingV1>,
    provider_io_performed: bool,
    raw_write_attempt_count: u64,
    exact_output_created: bool,
    certify_started: bool,
    export_started: bool,
    strategy_started: bool,
}

#[derive(Clone, Debug)]
struct PumpResearchProviderSuitabilityPlanV1 {
    qualification_range: Option<PumpResearchQualifiedSlotRangeV1>,
    qualification_blocker: Option<PumpResearchQualificationBlockerV1>,
    slots: BTreeMap<u64, BTreeSet<String>>,
    raw_representative_transactions_examined: usize,
    missing_raw_representative_roles: Vec<String>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpResearchQualifiedSlotRangeV1 {
    stream_epoch: u64,
    start_slot: u64,
    end_slot: u64,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum PumpResearchQualificationRangeSelectionV1 {
    Ready(PumpResearchQualifiedSlotRangeV1),
    Blocked(PumpResearchQualificationBlockerV1),
}

#[derive(Clone, Debug)]
struct PumpResearchValidatedSuitabilityPreparationV1 {
    digest: PumpResearchOperatorDigestV1,
    planned_exact_output: PathBuf,
}

/// Human-reviewed provider independence is an external authority input.  The
/// certifier can verify its exact bytes and bindings, but must never infer
/// physical independence from a different provider id or hostname.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchProviderIndependenceIdentityV1 {
    provider_id: String,
    service_type: String,
    entity_name: String,
    infrastructure_type: String,
    network_autonomous_system: String,
    primary_datacenter_location: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchProviderIndependenceAssertionsV1 {
    distinct_legal_entities: bool,
    distinct_infrastructure_operators: bool,
    distinct_network_routing_paths: bool,
    distinct_ingest_architecture: bool,
    zero_shared_credential_domain: bool,
    independent_retention_and_indexing: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchProviderIndependenceReviewerSignoffV1 {
    reviewer_id: String,
    operator_assertion: String,
    evidence_references: Vec<String>,
    created_wall_ms: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchProviderIndependenceBindingsV1 {
    audit_rpc_endpoint_blake3: String,
    audit_config_digest: PumpResearchOperatorDigestV1,
    provider_suitability_receipt_digest: PumpResearchOperatorDigestV1,
    provider_suitability_executable_digest: PumpResearchOperatorDigestV1,
    combined_certifier_executable_digest: PumpResearchOperatorDigestV1,
    raw_binding_digest: PumpResearchOperatorDigestV1,
    raw_start_manifest_digest: PumpResearchOperatorDigestV1,
    raw_completion_receipt_digest: PumpResearchOperatorDigestV1,
    qualification_stream_epoch: u64,
    qualification_start_slot: u64,
    qualification_end_slot: u64,
    planned_exact_output: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct PumpResearchProviderIndependenceAttestationV1 {
    attestation_version: String,
    source_run_id: String,
    /// Kept as a human-readable anchor in addition to the complete digest in
    /// `evidence_bindings`; it must name the exact approved bounded probe.
    go_e0_receipt_sha256: String,
    primary_provider: PumpResearchProviderIndependenceIdentityV1,
    audit_provider: PumpResearchProviderIndependenceIdentityV1,
    independence_assertions: PumpResearchProviderIndependenceAssertionsV1,
    attestation_status: String,
    reviewer_signoff: PumpResearchProviderIndependenceReviewerSignoffV1,
    evidence_bindings: PumpResearchProviderIndependenceBindingsV1,
}

#[derive(Clone, Debug)]
struct PumpResearchValidatedProviderIndependenceV1 {
    attestation_digest: PumpResearchOperatorDigestV1,
    attestation_path: PathBuf,
    audit_config_digest: PumpResearchOperatorDigestV1,
    audit_config_path: PathBuf,
    provider_suitability_receipt_digest: PumpResearchOperatorDigestV1,
    provider_suitability_receipt_path: PathBuf,
    running_executable_authority: Arc<PumpResearchRunningExecutableAuthorityV1>,
    raw_binding_digest: PumpResearchOperatorDigestV1,
    raw_binding_path: PathBuf,
    raw_start_manifest_digest: PumpResearchOperatorDigestV1,
    raw_start_manifest_path: PathBuf,
    raw_completion_receipt_digest: PumpResearchOperatorDigestV1,
    raw_completion_receipt_path: PathBuf,
    audit_rpc_endpoint_blake3: String,
    planned_exact_output: PathBuf,
}

/// One local-only authority object owns the exact resolved provider
/// connection from attestation validation through the full audit.  Neither the
/// audit loop nor the exact writer may resolve credential environment again.
struct PumpResearchValidatedCombinedAuditAuthorityV1 {
    audit_config: PumpResearchQualificationAuditConfigV1,
    resolved_connection: PumpResearchResolvedAuditConnectionV1,
    audit_rpc_endpoint_blake3: String,
    provider_independence: PumpResearchValidatedProviderIndependenceV1,
}

fn validate_nonempty_trimmed(name: &str, value: &str) -> Result<()> {
    if value.trim().is_empty() || value.trim() != value {
        bail!("{name} must be non-empty and have no surrounding whitespace");
    }
    Ok(())
}

fn validate_root_https_origin(name: &str, value: &str) -> Result<()> {
    let parsed = Url::parse(value).with_context(|| format!("parse {name} as HTTPS URL"))?;
    if parsed.scheme() != "https"
        || parsed.host_str().is_none()
        || !parsed.username().is_empty()
        || parsed.password().is_some()
        || parsed.path() != "/"
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        bail!("{name} must be a root-only HTTPS origin without userinfo, path, query or fragment");
    }
    Ok(())
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchQualificationSlotFindingV1 {
    schema_version: u16,
    slot: u64,
    raw_identity_count: u32,
    audit_identity_count: u32,
    raw_failed_transaction_count: u32,
    audit_failed_transaction_count: u32,
    raw_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    audit_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    raw_invocation_class_counts: BTreeMap<String, u64>,
    audit_invocation_class_counts: BTreeMap<String, u64>,
    identity_multiset_matches: bool,
    invocation_class_counts_match: bool,
    failed_status_multiset_matches: bool,
    audit_error: Option<String>,
    status: PumpResearchQualificationSlotStatusV1,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum PumpResearchQualificationSlotStatusV1 {
    Matched,
    SourceCoverageUnproven,
    SourceFilterCpiCoverageUnproven,
    AuditUnavailable,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchQualificationReportV1 {
    schema_version: u16,
    contract: PumpResearchQualificationAuditContractV1,
    source_run_id: String,
    audit_provider_id: String,
    audit_rpc_endpoint_blake3: String,
    raw_segment_set_blake3: String,
    qualification_stream_epoch: Option<u64>,
    qualification_start_slot: Option<u64>,
    qualification_end_slot: Option<u64>,
    rooted_slot_count: u64,
    dead_slot_count: u64,
    unresolved_slot_count: u64,
    /// Backward-compatible alias for `raw_failed_transaction_count`.
    failed_transaction_count: u64,
    raw_failed_transaction_count: u64,
    audit_failed_transaction_count: u64,
    raw_transaction_count: u64,
    audit_transaction_count: u64,
    raw_invocation_class_counts: BTreeMap<String, u64>,
    audit_invocation_class_counts: BTreeMap<String, u64>,
    provider_identity_independence_verified: bool,
    provider_independence_attestation_digest: PumpResearchOperatorDigestV1,
    exact_rooted_mutation_numerator: u64,
    exact_rooted_mutation_denominator: u64,
    program_receipts_match: bool,
    global_dependency_anchor_count: u64,
    status: PumpResearchTapeQualificationStatusV1,
}

#[derive(Clone, Debug)]
struct PumpResearchQualificationResultV1 {
    status: PumpResearchTapeQualificationStatusV1,
    slot_findings: Vec<PumpResearchQualificationSlotFindingV1>,
    report: PumpResearchQualificationReportV1,
}

/// One independently structural-scanned Pump transaction.  This remains
/// audit-only information: it is never fed back into the raw tape, parser
/// inventory, account anchors or exact trajectory certifier.
#[derive(Clone, Debug)]
struct PumpResearchAuditTransactionV1 {
    identity: PumpResearchCanonicalTransactionIdentityV1,
    invocation_class_counts: BTreeMap<String, u64>,
    failed: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct PumpResearchAuditComparisonV1 {
    raw_identity_count: u32,
    audit_identity_count: u32,
    raw_failed_transaction_count: u32,
    audit_failed_transaction_count: u32,
    raw_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    audit_only_identities: Vec<PumpResearchCanonicalTransactionIdentityV1>,
    raw_invocation_class_counts: BTreeMap<String, u64>,
    audit_invocation_class_counts: BTreeMap<String, u64>,
    identity_multiset_matches: bool,
    invocation_class_counts_match: bool,
    failed_status_multiset_matches: bool,
}

#[derive(Clone, Debug)]
enum PumpResearchAuditSlotFetchV1 {
    Block(Vec<PumpResearchAuditTransactionV1>),
    Skipped,
    Unavailable(String),
}

#[derive(Clone, Debug)]
struct PumpResearchAuditSlotFetchMetricsV1 {
    result: PumpResearchAuditSlotFetchV1,
    attempt_count: u32,
    elapsed_ms: u64,
}

const PUMP_RESEARCH_AUDITED_CLASSES_V1: [PumpResearchSourceInvocationClassV1; 4] = [
    PumpResearchSourceInvocationClassV1::DirectTopLevel,
    PumpResearchSourceInvocationClassV1::InnerCpi,
    PumpResearchSourceInvocationClassV1::RouterToPumpCpi,
    PumpResearchSourceInvocationClassV1::V0LoadedAddress,
];

fn qualification_audit_rpc_client_with_connection_v1(
    config: &PumpResearchQualificationAuditConfigV1,
    timeout: Duration,
    connection: &PumpResearchResolvedAuditConnectionV1,
) -> Result<Arc<AsyncRpcClient>> {
    let client = match connection.auth_token.as_deref() {
        Some(token) => crate::rpc_http_client::new_async_rpc_client_with_explicit_auth(
            connection.endpoint.clone(),
            &config.audit_rpc_auth_header,
            token,
        )
        .map_err(anyhow::Error::msg)?,
        None => crate::rpc_http_client::new_async_rpc_client_without_legacy_auth_with_timeout(
            connection.endpoint.clone(),
            timeout,
        )
        .map_err(anyhow::Error::msg)?,
    };
    Ok(Arc::new(client))
}

/// Perform the independent finalized-block comparison without ever opening a
/// raw segment for writing.  Audit observations deliberately have no authority
/// beyond qualification: an RPC block can prove that primary source evidence
/// is incomplete, but can never fill a raw record, mutation inventory, anchor
/// or canonicality hole.
async fn run_independent_source_completeness_audit_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    authority: &PumpResearchValidatedCombinedAuditAuthorityV1,
) -> Result<PumpResearchQualificationResultV1> {
    let config = &authority.audit_config;
    let resolved_connection = &authority.resolved_connection;
    config.validate()?;
    if config.audit_provider_id == raw.start_manifest.primary_provider_id {
        bail!(
            "qualification audit_provider_id {} must differ from primary_provider_id {}",
            config.audit_provider_id,
            raw.start_manifest.primary_provider_id
        );
    }

    let canonicality_counts = canonicality_counts(canonicality);
    let range_selection = qualification_range_selection_v1(raw, canonicality);
    let (qualification_range, initial_blocker) = match range_selection {
        PumpResearchQualificationRangeSelectionV1::Ready(range) => (Some(range), None),
        PumpResearchQualificationRangeSelectionV1::Blocked(blocker) => (None, Some(blocker)),
    };
    let mut report = PumpResearchQualificationReportV1 {
        schema_version: 1,
        contract: PumpResearchQualificationAuditContractV1 {
            schema_version: 1,
            primary_provider_must_differ: true,
            raw_tape_read_only: true,
            audit_must_compare_failed_transactions: true,
            audited_invocation_classes: PUMP_RESEARCH_AUDITED_CLASSES_V1.to_vec(),
        },
        source_run_id: raw.start_manifest.run_id.clone(),
        audit_provider_id: config.audit_provider_id.clone(),
        audit_rpc_endpoint_blake3: authority.audit_rpc_endpoint_blake3.clone(),
        raw_segment_set_blake3: raw
            .raw_segment_set_blake3_v1()
            .ok_or_else(|| anyhow::anyhow!("full audit lacks raw segment-set authority"))?
            .to_owned(),
        qualification_stream_epoch: qualification_range.map(|range| range.stream_epoch),
        qualification_start_slot: qualification_range.map(|range| range.start_slot),
        qualification_end_slot: qualification_range.map(|range| range.end_slot),
        rooted_slot_count: canonicality_counts.0,
        dead_slot_count: canonicality_counts.1,
        unresolved_slot_count: canonicality_counts.2,
        failed_transaction_count: 0,
        raw_failed_transaction_count: 0,
        audit_failed_transaction_count: 0,
        raw_transaction_count: 0,
        audit_transaction_count: 0,
        raw_invocation_class_counts: empty_invocation_class_counts(),
        audit_invocation_class_counts: empty_invocation_class_counts(),
        provider_identity_independence_verified: true,
        provider_independence_attestation_digest: authority
            .provider_independence
            .attestation_digest
            .clone(),
        exact_rooted_mutation_numerator: 0,
        exact_rooted_mutation_denominator: 0,
        program_receipts_match: raw.completion_receipt.status
            == PumpResearchRunCompletionStatusV1::Complete,
        global_dependency_anchor_count: 0,
        status: PumpResearchTapeQualificationStatusV1::Blocked(
            initial_blocker
                .unwrap_or(PumpResearchQualificationBlockerV1::SourceEvidenceInsufficient),
        ),
    };

    let Some(qualification_range) = qualification_range else {
        return Ok(PumpResearchQualificationResultV1 {
            status: report.status,
            slot_findings: Vec::new(),
            report,
        });
    };

    let raw_by_slot =
        collect_rooted_raw_audit_transactions_v1(raw, canonicality, qualification_range)?;
    for transactions in raw_by_slot.values() {
        for transaction in transactions {
            report.raw_transaction_count = report.raw_transaction_count.saturating_add(1);
            if transaction.failed {
                report.failed_transaction_count = report.failed_transaction_count.saturating_add(1);
                report.raw_failed_transaction_count =
                    report.raw_failed_transaction_count.saturating_add(1);
            }
            add_invocation_class_counts(
                &mut report.raw_invocation_class_counts,
                &transaction.invocation_class_counts,
            );
        }
    }

    // The provider must never observe fingerprints from a segment set that
    // differs from the source paths or the private descriptors retained for
    // exact materialisation.
    raw.revalidate_raw_segment_set_v1("after-audit-fingerprints-before-provider-io")?;

    let timeout = Duration::from_millis(config.request_timeout_ms);
    let client =
        qualification_audit_rpc_client_with_connection_v1(config, timeout, resolved_connection)?;
    let mut fetched_slots: Vec<(u64, PumpResearchAuditSlotFetchV1)> = stream::iter(
        (qualification_range.start_slot..=qualification_range.end_slot).map(|slot| {
            let client = Arc::clone(&client);
            async move {
                let fetched = fetch_finalized_audit_slot_v1(
                    client,
                    slot,
                    config.bounded_retry_count,
                    timeout,
                )
                .await;
                (slot, fetched)
            }
        }),
    )
    .buffer_unordered(config.bounded_concurrency)
    .collect()
    .await;
    fetched_slots.sort_by_key(|(slot, _)| *slot);

    let mut slot_findings = Vec::with_capacity(fetched_slots.len());
    let mut saw_source_coverage_failure = false;
    let mut saw_filter_coverage_failure = false;
    for (slot, fetched) in fetched_slots {
        let raw_transactions = raw_by_slot.get(&slot).cloned().unwrap_or_default();
        let mut raw_class_counts = empty_invocation_class_counts();
        for transaction in &raw_transactions {
            add_invocation_class_counts(
                &mut raw_class_counts,
                &transaction.invocation_class_counts,
            );
        }
        let status = match fetched {
            PumpResearchAuditSlotFetchV1::Block(transactions) => {
                for transaction in &transactions {
                    report.audit_transaction_count =
                        report.audit_transaction_count.saturating_add(1);
                    if transaction.failed {
                        report.audit_failed_transaction_count =
                            report.audit_failed_transaction_count.saturating_add(1);
                    }
                    add_invocation_class_counts(
                        &mut report.audit_invocation_class_counts,
                        &transaction.invocation_class_counts,
                    );
                }
                let comparison =
                    compare_audit_transaction_multisets_v1(&raw_transactions, &transactions);
                let status = qualification_slot_status_from_comparison_v1(&comparison);
                if status == PumpResearchQualificationSlotStatusV1::SourceCoverageUnproven {
                    saw_source_coverage_failure = true;
                } else if status
                    == PumpResearchQualificationSlotStatusV1::SourceFilterCpiCoverageUnproven
                {
                    saw_filter_coverage_failure = true;
                }
                slot_findings.push(PumpResearchQualificationSlotFindingV1 {
                    schema_version: 1,
                    slot,
                    raw_identity_count: comparison.raw_identity_count,
                    audit_identity_count: comparison.audit_identity_count,
                    raw_failed_transaction_count: comparison.raw_failed_transaction_count,
                    audit_failed_transaction_count: comparison.audit_failed_transaction_count,
                    raw_only_identities: comparison.raw_only_identities,
                    audit_only_identities: comparison.audit_only_identities,
                    raw_invocation_class_counts: comparison.raw_invocation_class_counts,
                    audit_invocation_class_counts: comparison.audit_invocation_class_counts,
                    identity_multiset_matches: comparison.identity_multiset_matches,
                    invocation_class_counts_match: comparison.invocation_class_counts_match,
                    failed_status_multiset_matches: comparison.failed_status_multiset_matches,
                    audit_error: None,
                    status,
                });
                continue;
            }
            PumpResearchAuditSlotFetchV1::Skipped => {
                let status = if raw_transactions.is_empty() {
                    PumpResearchQualificationSlotStatusV1::Matched
                } else {
                    saw_source_coverage_failure = true;
                    PumpResearchQualificationSlotStatusV1::SourceCoverageUnproven
                };
                status
            }
            PumpResearchAuditSlotFetchV1::Unavailable(reason) => {
                saw_source_coverage_failure = true;
                slot_findings.push(PumpResearchQualificationSlotFindingV1 {
                    schema_version: 1,
                    slot,
                    raw_identity_count: u32::try_from(raw_transactions.len()).unwrap_or(u32::MAX),
                    audit_identity_count: 0,
                    raw_failed_transaction_count: raw_transactions
                        .iter()
                        .filter(|transaction| transaction.failed)
                        .count()
                        .try_into()
                        .unwrap_or(u32::MAX),
                    audit_failed_transaction_count: 0,
                    raw_only_identities: sorted_audit_identities(
                        raw_transactions.iter().map(|tx| tx.identity),
                    ),
                    audit_only_identities: Vec::new(),
                    raw_invocation_class_counts: raw_class_counts,
                    audit_invocation_class_counts: empty_invocation_class_counts(),
                    identity_multiset_matches: false,
                    invocation_class_counts_match: false,
                    failed_status_multiset_matches: false,
                    audit_error: Some(redacted_audit_error(config, resolved_connection, &reason)),
                    status: PumpResearchQualificationSlotStatusV1::AuditUnavailable,
                });
                continue;
            }
        };
        slot_findings.push(PumpResearchQualificationSlotFindingV1 {
            schema_version: 1,
            slot,
            raw_identity_count: u32::try_from(raw_transactions.len()).unwrap_or(u32::MAX),
            audit_identity_count: 0,
            raw_failed_transaction_count: raw_transactions
                .iter()
                .filter(|transaction| transaction.failed)
                .count()
                .try_into()
                .unwrap_or(u32::MAX),
            audit_failed_transaction_count: 0,
            raw_only_identities: sorted_audit_identities(
                raw_transactions.iter().map(|tx| tx.identity),
            ),
            audit_only_identities: Vec::new(),
            raw_invocation_class_counts: raw_class_counts,
            audit_invocation_class_counts: empty_invocation_class_counts(),
            identity_multiset_matches: raw_transactions.is_empty(),
            invocation_class_counts_match: raw_transactions.is_empty(),
            failed_status_multiset_matches: raw_transactions.is_empty(),
            audit_error: None,
            status,
        });
    }

    let every_supported_class_observed = PUMP_RESEARCH_AUDITED_CLASSES_V1.iter().all(|class| {
        let name = pump_research_invocation_class_name(*class);
        report
            .raw_invocation_class_counts
            .get(name)
            .copied()
            .unwrap_or(0)
            > 0
            && report.raw_invocation_class_counts.get(name)
                == report.audit_invocation_class_counts.get(name)
    });
    if !every_supported_class_observed {
        saw_filter_coverage_failure = true;
    }
    report.status = qualification_status_from_audit_failures_v1(
        saw_source_coverage_failure,
        saw_filter_coverage_failure,
    );
    Ok(PumpResearchQualificationResultV1 {
        status: report.status,
        slot_findings,
        report,
    })
}

fn canonicality_counts(canonicality: &PumpResearchSlotCanonicalityIndexV1) -> (u64, u64, u64) {
    canonicality
        .by_slot
        .values()
        .fold((0, 0, 0), |counts, status| match status {
            PumpSlotCanonicalityV1::RootedCanonical => {
                (counts.0.saturating_add(1), counts.1, counts.2)
            }
            PumpSlotCanonicalityV1::Dead => (counts.0, counts.1.saturating_add(1), counts.2),
            PumpSlotCanonicalityV1::Unresolved => (counts.0, counts.1, counts.2.saturating_add(1)),
        })
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct PumpResearchStreamEpochBoundaryV1 {
    stream_epoch: u64,
    first_block_meta_slot: Option<u64>,
    last_block_meta_slot: Option<u64>,
}

fn qualification_range_selection_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
) -> PumpResearchQualificationRangeSelectionV1 {
    let mut observed_epochs: BTreeMap<u64, (Option<(u64, u64)>, Option<(u64, u64)>)> = raw
        .segments
        .iter()
        .map(|segment| (segment.header.stream_epoch, (None, None)))
        .collect();
    for evidence in raw.block_meta.values().flatten() {
        let Some((first, last)) = observed_epochs.get_mut(&evidence.source.stream_epoch) else {
            return PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            );
        };
        let observation = (evidence.source.capture_sequence, evidence.slot);
        if first.is_none_or(|current| observation.0 < current.0) {
            *first = Some(observation);
        }
        if last.is_none_or(|current| observation.0 > current.0) {
            *last = Some(observation);
        }
    }
    let boundaries: Vec<_> = observed_epochs
        .into_iter()
        .map(
            |(stream_epoch, (first, last))| PumpResearchStreamEpochBoundaryV1 {
                stream_epoch,
                first_block_meta_slot: first.map(|observation| observation.1),
                last_block_meta_slot: last.map(|observation| observation.1),
            },
        )
        .collect();
    qualification_range_selection_with_boundaries_v1(
        &boundaries,
        canonicality,
        |stream_epoch, slot| gap_affects_stream_epoch_slot(raw, stream_epoch, slot),
    )
}

fn qualification_range_selection_with_boundaries_v1<F>(
    boundaries: &[PumpResearchStreamEpochBoundaryV1],
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    mut gap_affects: F,
) -> PumpResearchQualificationRangeSelectionV1
where
    F: FnMut(u64, u64) -> bool,
{
    if boundaries.is_empty() {
        return PumpResearchQualificationRangeSelectionV1::Blocked(
            PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
        );
    }
    let mut best = None;
    for boundary in boundaries {
        let (Some(first_block_meta_slot), Some(last_block_meta_slot)) = (
            boundary.first_block_meta_slot,
            boundary.last_block_meta_slot,
        ) else {
            return PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            );
        };
        let Some(epoch_start) = first_block_meta_slot.checked_add(1) else {
            return PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            );
        };
        if epoch_start > last_block_meta_slot {
            return PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            );
        }

        let mut current_start = None;
        let mut current_end = None;
        for (slot, status) in canonicality
            .by_slot
            .range(epoch_start..=last_block_meta_slot)
        {
            let eligible = *status == PumpSlotCanonicalityV1::RootedCanonical
                && !gap_affects(boundary.stream_epoch, *slot);
            if eligible {
                let contiguous = current_end
                    .and_then(|previous: u64| previous.checked_add(1))
                    .is_some_and(|expected| expected == *slot);
                if current_start.is_some() && !contiguous {
                    best = select_preferred_qualification_range(
                        best,
                        PumpResearchQualifiedSlotRangeV1 {
                            stream_epoch: boundary.stream_epoch,
                            start_slot: current_start.unwrap_or(*slot),
                            end_slot: current_end.unwrap_or(*slot),
                        },
                    );
                    current_start = None;
                }
                current_start.get_or_insert(*slot);
                current_end = Some(*slot);
            } else if let (Some(start_slot), Some(end_slot)) =
                (current_start.take(), current_end.take())
            {
                best = select_preferred_qualification_range(
                    best,
                    PumpResearchQualifiedSlotRangeV1 {
                        stream_epoch: boundary.stream_epoch,
                        start_slot,
                        end_slot,
                    },
                );
            }
        }
        if let (Some(start_slot), Some(end_slot)) = (current_start, current_end) {
            best = select_preferred_qualification_range(
                best,
                PumpResearchQualifiedSlotRangeV1 {
                    stream_epoch: boundary.stream_epoch,
                    start_slot,
                    end_slot,
                },
            );
        }
    }
    best.map(PumpResearchQualificationRangeSelectionV1::Ready)
        .unwrap_or(PumpResearchQualificationRangeSelectionV1::Blocked(
            PumpResearchQualificationBlockerV1::SourceEvidenceInsufficient,
        ))
}

fn select_preferred_qualification_range(
    current: Option<PumpResearchQualifiedSlotRangeV1>,
    candidate: PumpResearchQualifiedSlotRangeV1,
) -> Option<PumpResearchQualifiedSlotRangeV1> {
    match current {
        None => Some(candidate),
        Some(existing) => {
            let existing_len = existing
                .end_slot
                .saturating_sub(existing.start_slot)
                .saturating_add(1);
            let candidate_len = candidate
                .end_slot
                .saturating_sub(candidate.start_slot)
                .saturating_add(1);
            let candidate_wins_tie = (
                candidate.start_slot,
                candidate.stream_epoch,
                candidate.end_slot,
            ) < (
                existing.start_slot,
                existing.stream_epoch,
                existing.end_slot,
            );
            if candidate_len > existing_len || (candidate_len == existing_len && candidate_wins_tie)
            {
                Some(candidate)
            } else {
                Some(existing)
            }
        }
    }
}

fn collect_rooted_raw_audit_transactions_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    qualification_range: PumpResearchQualifiedSlotRangeV1,
) -> Result<BTreeMap<u64, Vec<PumpResearchAuditTransactionV1>>> {
    let mut by_slot: BTreeMap<u64, Vec<PumpResearchAuditTransactionV1>> = BTreeMap::new();
    for indexed in &raw.transactions {
        if indexed.source.stream_epoch != qualification_range.stream_epoch
            || !(qualification_range.start_slot..=qualification_range.end_slot)
                .contains(&indexed.slot)
            || canonicality.classify(indexed.slot) != PumpSlotCanonicalityV1::RootedCanonical
        {
            continue;
        }
        let transaction = raw.read_transaction(indexed)?;
        let Some(scanned) = scan_raw_pump_transaction_for_audit_v1(&transaction)? else {
            continue;
        };
        if scanned.identity.slot != indexed.slot
            || Some(scanned.identity.tx_index) != indexed.tx_index
            || scanned.identity.signature != indexed.signature
        {
            bail!(
                "raw audit identity differs from indexed frozen transaction at capture sequence {}",
                indexed.source.capture_sequence
            );
        }
        by_slot.entry(indexed.slot).or_default().push(scanned);
    }
    Ok(by_slot)
}

fn scan_raw_pump_transaction_for_audit_v1(
    evidence: &PumpPrimaryTransactionEvidenceV1,
) -> Result<Option<PumpResearchAuditTransactionV1>> {
    use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

    let update = SubscribeUpdateTransaction::decode(evidence.source_payload.as_slice())
        .context("decode frozen raw SubscribeUpdateTransaction for audit")?;
    if update.slot != evidence.slot {
        bail!("raw transaction audit payload slot differs from frozen evidence");
    }
    let info = update
        .transaction
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("raw transaction audit payload has no transaction info"))?;
    let transaction = info
        .transaction
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("raw transaction audit payload has no transaction"))?;
    let meta = info
        .meta
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("raw transaction audit payload has no metadata"))?;
    let message = transaction
        .message
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("raw transaction audit payload has no message"))?;
    let static_count = message.account_keys.len();
    let mut account_keys = Vec::with_capacity(
        static_count + meta.loaded_writable_addresses.len() + meta.loaded_readonly_addresses.len(),
    );
    for bytes in message
        .account_keys
        .iter()
        .chain(meta.loaded_writable_addresses.iter())
        .chain(meta.loaded_readonly_addresses.iter())
    {
        account_keys.push(
            Pubkey::try_from(bytes.as_slice())
                .context("decode raw transaction audit account key")?,
        );
    }
    let pump_program = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
        .context("parse frozen Pump program ID")?;
    let mut invocation_class_counts = empty_invocation_class_counts();
    for instruction in &message.instructions {
        let program_id = audit_program_id(&account_keys, u32::from(instruction.program_id_index))?;
        if program_id == pump_program {
            add_invocation_class(
                &mut invocation_class_counts,
                PumpResearchSourceInvocationClassV1::DirectTopLevel,
            );
            if usize::try_from(instruction.program_id_index)
                .ok()
                .is_some_and(|index| index >= static_count)
            {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::V0LoadedAddress,
                );
            }
        }
    }
    for group in &meta.inner_instructions {
        let parent = message
            .instructions
            .get(
                usize::try_from(group.index)
                    .map_err(|_| anyhow::anyhow!("raw inner instruction index overflow"))?,
            )
            .ok_or_else(|| {
                anyhow::anyhow!("raw inner instruction references missing outer instruction")
            })?;
        let parent_program = audit_program_id(&account_keys, u32::from(parent.program_id_index))?;
        for instruction in &group.instructions {
            let program_id =
                audit_program_id(&account_keys, u32::from(instruction.program_id_index))?;
            if program_id != pump_program {
                continue;
            }
            add_invocation_class(
                &mut invocation_class_counts,
                PumpResearchSourceInvocationClassV1::InnerCpi,
            );
            if parent_program != pump_program {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::RouterToPumpCpi,
                );
            }
            if usize::try_from(instruction.program_id_index)
                .ok()
                .is_some_and(|index| index >= static_count)
            {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::V0LoadedAddress,
                );
            }
        }
    }
    if invocation_class_counts.values().all(|count| *count == 0) {
        return Ok(None);
    }
    let signature_bytes: [u8; 64] = info
        .signature
        .as_slice()
        .try_into()
        .map_err(|_| anyhow::anyhow!("raw transaction audit signature is not 64 bytes"))?;
    let tx_index = u32::try_from(info.index)
        .map_err(|_| anyhow::anyhow!("raw transaction audit index does not fit u32"))?;
    Ok(Some(PumpResearchAuditTransactionV1 {
        identity: PumpResearchCanonicalTransactionIdentityV1 {
            slot: evidence.slot,
            tx_index,
            signature: PumpResearchStorageSignatureV1::from(signature_bytes),
        },
        invocation_class_counts,
        failed: meta.err.is_some(),
    }))
}

async fn fetch_finalized_audit_slot_v1(
    client: Arc<AsyncRpcClient>,
    slot: u64,
    retries: u32,
    timeout: Duration,
) -> PumpResearchAuditSlotFetchV1 {
    fetch_finalized_audit_slot_with_metrics_v1(client, slot, retries, timeout, None)
        .await
        .result
}

async fn fetch_finalized_audit_slot_with_metrics_v1(
    client: Arc<AsyncRpcClient>,
    slot: u64,
    retries: u32,
    timeout: Duration,
    provider_deadline: Option<Instant>,
) -> PumpResearchAuditSlotFetchMetricsV1 {
    let started = Instant::now();
    let block_config = RpcBlockConfig {
        encoding: Some(UiTransactionEncoding::Json),
        transaction_details: Some(TransactionDetails::Full),
        rewards: Some(false),
        commitment: Some(CommitmentConfig::finalized()),
        max_supported_transaction_version: Some(0),
    };
    let mut last_error = None;
    let mut attempt_count = 0_u32;
    for _attempt in 0..=retries {
        let attempt_timeout = match provider_deadline {
            Some(deadline) => match deadline.checked_duration_since(Instant::now()) {
                Some(remaining) if !remaining.is_zero() => timeout.min(remaining),
                _ => {
                    last_error = Some(
                        "GO-E0 hard provider wall deadline was exhausted before the next request"
                            .to_owned(),
                    );
                    break;
                }
            },
            None => timeout,
        };
        attempt_count = attempt_count.saturating_add(1);
        let response = tokio::time::timeout(
            attempt_timeout,
            client.get_block_with_config(slot, block_config),
        )
        .await;
        match response {
            Ok(Ok(block)) => match scan_finalized_audit_block_v1(slot, block) {
                Ok(transactions) => {
                    return PumpResearchAuditSlotFetchMetricsV1 {
                        result: PumpResearchAuditSlotFetchV1::Block(transactions),
                        attempt_count,
                        elapsed_ms: duration_millis_u64(started.elapsed()),
                    }
                }
                Err(error) => {
                    return PumpResearchAuditSlotFetchMetricsV1 {
                        result: PumpResearchAuditSlotFetchV1::Unavailable(error.to_string()),
                        attempt_count,
                        elapsed_ms: duration_millis_u64(started.elapsed()),
                    }
                }
            },
            Ok(Err(error)) if audit_rpc_error_is_explicit_skipped_slot(&error) => {
                return PumpResearchAuditSlotFetchMetricsV1 {
                    result: PumpResearchAuditSlotFetchV1::Skipped,
                    attempt_count,
                    elapsed_ms: duration_millis_u64(started.elapsed()),
                };
            }
            Ok(Err(error)) => last_error = Some(error.to_string()),
            Err(_) => {
                if provider_deadline.is_some_and(|deadline| Instant::now() >= deadline) {
                    last_error = Some(
                        "GO-E0 hard provider wall deadline expired during getBlock".to_owned(),
                    );
                    break;
                }
                last_error = Some(format!(
                    "getBlock finalized request timed out after {attempt_timeout:?}"
                ));
            }
        }
    }
    PumpResearchAuditSlotFetchMetricsV1 {
        result: PumpResearchAuditSlotFetchV1::Unavailable(
            last_error
                .unwrap_or_else(|| "getBlock finalized request failed without an error".to_owned()),
        ),
        attempt_count,
        elapsed_ms: duration_millis_u64(started.elapsed()),
    }
}

fn duration_millis_u64(duration: Duration) -> u64 {
    u64::try_from(duration.as_millis()).unwrap_or(u64::MAX)
}

fn audit_rpc_error_is_explicit_skipped_slot(error: &ClientError) -> bool {
    matches!(
        &error.kind,
        ClientErrorKind::RpcError(RpcError::RpcResponseError { code, .. })
            if *code == JSON_RPC_SERVER_ERROR_SLOT_SKIPPED
    )
}

fn scan_finalized_audit_block_v1(
    slot: u64,
    block: UiConfirmedBlock,
) -> Result<Vec<PumpResearchAuditTransactionV1>> {
    let transactions = block
        .transactions
        .ok_or_else(|| anyhow::anyhow!("finalized audit block {slot} omitted transactions"))?;
    let pump_program = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
        .context("parse frozen Pump program ID")?;
    let mut found = Vec::new();
    for (position, transaction) in transactions.iter().enumerate() {
        let tx_index = u32::try_from(position).map_err(|_| {
            anyhow::anyhow!("finalized audit block {slot} has too many transactions")
        })?;
        let Some(scanned) =
            scan_finalized_audit_transaction_v1(slot, tx_index, transaction, pump_program)?
        else {
            continue;
        };
        found.push(scanned);
    }
    Ok(found)
}

fn scan_finalized_audit_transaction_v1(
    slot: u64,
    tx_index: u32,
    transaction: &solana_transaction_status::EncodedTransactionWithStatusMeta,
    pump_program: Pubkey,
) -> Result<Option<PumpResearchAuditTransactionV1>> {
    let EncodedTransaction::Json(ui_transaction) = &transaction.transaction else {
        bail!("finalized audit transaction {slot}:{tx_index} is not JSON encoded");
    };
    let UiMessage::Raw(message) = &ui_transaction.message else {
        bail!("finalized audit transaction {slot}:{tx_index} does not contain a raw message");
    };
    let meta = transaction.meta.as_ref().ok_or_else(|| {
        anyhow::anyhow!("finalized audit transaction {slot}:{tx_index} has no metadata")
    })?;
    let static_count = message.account_keys.len();
    let uses_address_lookup = message
        .address_table_lookups
        .as_ref()
        .is_some_and(|lookups| !lookups.is_empty());
    let mut account_keys = Vec::with_capacity(static_count);
    for key in &message.account_keys {
        account_keys.push(Pubkey::from_str(key).with_context(|| {
            format!("parse finalized audit static account key at {slot}:{tx_index}")
        })?);
    }
    match &meta.loaded_addresses {
        OptionSerializer::Some(loaded) => {
            for key in loaded.writable.iter().chain(loaded.readonly.iter()) {
                account_keys.push(Pubkey::from_str(key).with_context(|| {
                    format!("parse finalized audit loaded account key at {slot}:{tx_index}")
                })?);
            }
        }
        OptionSerializer::None | OptionSerializer::Skip if uses_address_lookup => {
            bail!("finalized audit v0 transaction {slot}:{tx_index} omits loaded addresses");
        }
        OptionSerializer::None | OptionSerializer::Skip => {}
    }
    let inner_groups = match &meta.inner_instructions {
        OptionSerializer::Some(groups) => groups,
        OptionSerializer::None | OptionSerializer::Skip => {
            bail!("finalized audit transaction {slot}:{tx_index} omits inner instructions")
        }
    };
    let mut invocation_class_counts = empty_invocation_class_counts();
    for instruction in &message.instructions {
        let program_id = audit_program_id(&account_keys, u32::from(instruction.program_id_index))?;
        if program_id == pump_program {
            add_invocation_class(
                &mut invocation_class_counts,
                PumpResearchSourceInvocationClassV1::DirectTopLevel,
            );
            if usize::try_from(instruction.program_id_index)
                .ok()
                .is_some_and(|index| index >= static_count)
            {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::V0LoadedAddress,
                );
            }
        }
    }
    for group in inner_groups {
        let parent = message.instructions.get(usize::from(group.index)).ok_or_else(|| {
            anyhow::anyhow!(
                "finalized audit inner instruction references missing outer instruction at {slot}:{tx_index}"
            )
        })?;
        let parent_program = audit_program_id(&account_keys, u32::from(parent.program_id_index))?;
        for instruction in &group.instructions {
            let UiInstruction::Compiled(instruction) = instruction else {
                bail!(
                    "finalized audit inner instruction at {slot}:{tx_index} is not compiled JSON"
                );
            };
            let program_id =
                audit_program_id(&account_keys, u32::from(instruction.program_id_index))?;
            if program_id != pump_program {
                continue;
            }
            add_invocation_class(
                &mut invocation_class_counts,
                PumpResearchSourceInvocationClassV1::InnerCpi,
            );
            if parent_program != pump_program {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::RouterToPumpCpi,
                );
            }
            if usize::try_from(instruction.program_id_index)
                .ok()
                .is_some_and(|index| index >= static_count)
            {
                add_invocation_class(
                    &mut invocation_class_counts,
                    PumpResearchSourceInvocationClassV1::V0LoadedAddress,
                );
            }
        }
    }
    if invocation_class_counts.values().all(|count| *count == 0) {
        return Ok(None);
    }
    let signature = ui_transaction.signatures.first().ok_or_else(|| {
        anyhow::anyhow!("finalized audit transaction {slot}:{tx_index} has no signature")
    })?;
    let signature = Signature::from_str(signature)
        .with_context(|| format!("parse finalized audit signature at {slot}:{tx_index}"))?;
    let signature_bytes: [u8; 64] = signature
        .as_ref()
        .try_into()
        .map_err(|_| anyhow::anyhow!("finalized audit signature is not 64 bytes"))?;
    Ok(Some(PumpResearchAuditTransactionV1 {
        identity: PumpResearchCanonicalTransactionIdentityV1 {
            slot,
            tx_index,
            signature: PumpResearchStorageSignatureV1::from(signature_bytes),
        },
        invocation_class_counts,
        failed: meta.err.is_some(),
    }))
}

fn audit_program_id(account_keys: &[Pubkey], program_id_index: u32) -> Result<Pubkey> {
    let index = usize::try_from(program_id_index)
        .map_err(|_| anyhow::anyhow!("program ID index does not fit usize"))?;
    account_keys.get(index).copied().ok_or_else(|| {
        anyhow::anyhow!("program ID index {program_id_index} is outside account keys")
    })
}

fn empty_invocation_class_counts() -> BTreeMap<String, u64> {
    PUMP_RESEARCH_AUDITED_CLASSES_V1
        .iter()
        .map(|class| (pump_research_invocation_class_name(*class).to_owned(), 0))
        .collect()
}

fn pump_research_invocation_class_name(class: PumpResearchSourceInvocationClassV1) -> &'static str {
    match class {
        PumpResearchSourceInvocationClassV1::DirectTopLevel => "direct_top_level",
        PumpResearchSourceInvocationClassV1::InnerCpi => "inner_cpi",
        PumpResearchSourceInvocationClassV1::RouterToPumpCpi => "router_to_pump_cpi",
        PumpResearchSourceInvocationClassV1::V0LoadedAddress => "v0_loaded_address",
    }
}

fn add_invocation_class(
    counts: &mut BTreeMap<String, u64>,
    class: PumpResearchSourceInvocationClassV1,
) {
    let count = counts
        .entry(pump_research_invocation_class_name(class).to_owned())
        .or_default();
    *count = count.saturating_add(1);
}

fn add_invocation_class_counts(
    destination: &mut BTreeMap<String, u64>,
    source: &BTreeMap<String, u64>,
) {
    for (class, count) in source {
        let destination_count = destination.entry(class.clone()).or_default();
        *destination_count = destination_count.saturating_add(*count);
    }
}

fn audit_identity_multiset_difference(
    raw_transactions: &[PumpResearchAuditTransactionV1],
    audit_transactions: &[PumpResearchAuditTransactionV1],
) -> (
    Vec<PumpResearchCanonicalTransactionIdentityV1>,
    Vec<PumpResearchCanonicalTransactionIdentityV1>,
) {
    let mut raw_counts: HashMap<PumpResearchCanonicalTransactionIdentityV1, u32> = HashMap::new();
    let mut audit_counts: HashMap<PumpResearchCanonicalTransactionIdentityV1, u32> = HashMap::new();
    for transaction in raw_transactions {
        let count = raw_counts.entry(transaction.identity).or_default();
        *count = count.saturating_add(1);
    }
    for transaction in audit_transactions {
        let count = audit_counts.entry(transaction.identity).or_default();
        *count = count.saturating_add(1);
    }
    let mut raw_only = Vec::new();
    let mut audit_only = Vec::new();
    for (identity, raw_count) in &raw_counts {
        let audit_count = audit_counts.get(identity).copied().unwrap_or_default();
        for _ in audit_count..*raw_count {
            raw_only.push(*identity);
        }
    }
    for (identity, audit_count) in &audit_counts {
        let raw_count = raw_counts.get(identity).copied().unwrap_or_default();
        for _ in raw_count..*audit_count {
            audit_only.push(*identity);
        }
    }
    (
        sorted_audit_identities(raw_only),
        sorted_audit_identities(audit_only),
    )
}

fn sorted_audit_identities<I>(identities: I) -> Vec<PumpResearchCanonicalTransactionIdentityV1>
where
    I: IntoIterator<Item = PumpResearchCanonicalTransactionIdentityV1>,
{
    let mut identities: Vec<_> = identities.into_iter().collect();
    identities.sort_by_key(|identity| {
        (
            identity.slot,
            identity.tx_index,
            identity.signature.into_inner(),
        )
    });
    identities
}

fn add_provider_suitability_slot_role(
    slots: &mut BTreeMap<u64, BTreeSet<String>>,
    slot: u64,
    role: &str,
) {
    slots.entry(slot).or_default().insert(role.to_owned());
}

fn evenly_spaced_slots(start_slot: u64, end_slot: u64, count: usize) -> Vec<u64> {
    if count == 0 || start_slot > end_slot {
        return Vec::new();
    }
    if count == 1 || start_slot == end_slot {
        return vec![start_slot];
    }
    let span = u128::from(end_slot - start_slot);
    let denominator = u128::try_from(count - 1).unwrap_or(u128::MAX);
    (0..count)
        .filter_map(|index| {
            let numerator = u128::try_from(index).ok()?;
            let offset = span.saturating_mul(numerator) / denominator;
            u64::try_from(offset)
                .ok()
                .and_then(|offset| start_slot.checked_add(offset))
        })
        .collect()
}

fn build_provider_suitability_plan_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
) -> Result<PumpResearchProviderSuitabilityPlanV1> {
    let qualification_range = match qualification_range_selection_v1(raw, canonicality) {
        PumpResearchQualificationRangeSelectionV1::Ready(range) => range,
        PumpResearchQualificationRangeSelectionV1::Blocked(blocker) => {
            return Ok(PumpResearchProviderSuitabilityPlanV1 {
                qualification_range: None,
                qualification_blocker: Some(blocker),
                slots: BTreeMap::new(),
                raw_representative_transactions_examined: 0,
                missing_raw_representative_roles: vec!["qualification_range".to_owned()],
            })
        }
    };
    let start_slot = qualification_range.start_slot;
    let end_slot = qualification_range.end_slot;

    let mut slots = BTreeMap::new();
    add_provider_suitability_slot_role(&mut slots, start_slot, "qualification_first");
    add_provider_suitability_slot_role(
        &mut slots,
        start_slot + (end_slot - start_slot) / 2,
        "qualification_midpoint",
    );
    add_provider_suitability_slot_role(&mut slots, end_slot, "qualification_last");
    for slot in evenly_spaced_slots(
        start_slot,
        end_slot,
        PROVIDER_SUITABILITY_BURST_SLOT_COUNT_V1,
    ) {
        add_provider_suitability_slot_role(&mut slots, slot, "bounded_burst");
    }

    let required_roles = [
        "direct_top_level",
        "inner_cpi",
        "router_to_pump_cpi",
        "v0_loaded_address",
        "failed_pump_transaction",
    ];
    let mut representatives: BTreeMap<&'static str, Option<u64>> = required_roles
        .iter()
        .copied()
        .map(|role| (role, None))
        .collect();
    let mut examined = 0_usize;
    for indexed in &raw.transactions {
        if examined >= PROVIDER_SUITABILITY_MAX_RAW_REPRESENTATIVE_SCAN_V1
            || representatives.values().all(Option::is_some)
        {
            break;
        }
        if indexed.source.stream_epoch != qualification_range.stream_epoch
            || !(start_slot..=end_slot).contains(&indexed.slot)
            || canonicality.classify(indexed.slot) != PumpSlotCanonicalityV1::RootedCanonical
        {
            continue;
        }
        examined = examined.saturating_add(1);
        let transaction = raw.read_transaction(indexed)?;
        let Some(scanned) = scan_raw_pump_transaction_for_audit_v1(&transaction)? else {
            continue;
        };
        for role in required_roles.iter().take(4) {
            if representatives.get(role).copied().flatten().is_none()
                && scanned
                    .invocation_class_counts
                    .get(*role)
                    .copied()
                    .unwrap_or_default()
                    > 0
            {
                representatives.insert(role, Some(indexed.slot));
            }
        }
        if scanned.failed
            && representatives
                .get("failed_pump_transaction")
                .copied()
                .flatten()
                .is_none()
        {
            representatives.insert("failed_pump_transaction", Some(indexed.slot));
        }
    }

    let mut missing_roles = Vec::new();
    for role in required_roles {
        match representatives.get(role).copied().flatten() {
            Some(slot) => add_provider_suitability_slot_role(&mut slots, slot, role),
            None => missing_roles.push(role.to_owned()),
        }
    }
    Ok(PumpResearchProviderSuitabilityPlanV1 {
        qualification_range: Some(qualification_range),
        qualification_blocker: None,
        slots,
        raw_representative_transactions_examined: examined,
        missing_raw_representative_roles: missing_roles,
    })
}

fn collect_provider_suitability_raw_transactions_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    qualification_range: PumpResearchQualifiedSlotRangeV1,
    sample_slots: &BTreeSet<u64>,
) -> Result<BTreeMap<u64, Vec<PumpResearchAuditTransactionV1>>> {
    let mut by_slot: BTreeMap<u64, Vec<PumpResearchAuditTransactionV1>> = BTreeMap::new();
    for indexed in &raw.transactions {
        if indexed.source.stream_epoch != qualification_range.stream_epoch
            || !sample_slots.contains(&indexed.slot)
            || canonicality.classify(indexed.slot) != PumpSlotCanonicalityV1::RootedCanonical
        {
            continue;
        }
        let transaction = raw.read_transaction(indexed)?;
        if let Some(scanned) = scan_raw_pump_transaction_for_audit_v1(&transaction)? {
            by_slot.entry(indexed.slot).or_default().push(scanned);
        }
    }
    for transactions in by_slot.values_mut() {
        transactions.sort_by_key(|transaction| transaction.identity.tx_index);
    }
    Ok(by_slot)
}

fn audit_failure_multiset_matches(
    raw_transactions: &[PumpResearchAuditTransactionV1],
    audit_transactions: &[PumpResearchAuditTransactionV1],
) -> bool {
    let mut raw_counts = HashMap::new();
    let mut audit_counts = HashMap::new();
    for transaction in raw_transactions {
        let count = raw_counts
            .entry((transaction.identity, transaction.failed))
            .or_insert(0_u32);
        *count = count.saturating_add(1);
    }
    for transaction in audit_transactions {
        let count = audit_counts
            .entry((transaction.identity, transaction.failed))
            .or_insert(0_u32);
        *count = count.saturating_add(1);
    }
    raw_counts == audit_counts
}

fn compare_audit_transaction_multisets_v1(
    raw_transactions: &[PumpResearchAuditTransactionV1],
    audit_transactions: &[PumpResearchAuditTransactionV1],
) -> PumpResearchAuditComparisonV1 {
    let (raw_identity_count, raw_failed_transaction_count, raw_invocation_class_counts) =
        audit_transaction_counts(raw_transactions);
    let (audit_identity_count, audit_failed_transaction_count, audit_invocation_class_counts) =
        audit_transaction_counts(audit_transactions);
    let (raw_only_identities, audit_only_identities) =
        audit_identity_multiset_difference(raw_transactions, audit_transactions);
    let identity_multiset_matches =
        raw_only_identities.is_empty() && audit_only_identities.is_empty();
    let invocation_class_counts_match =
        raw_invocation_class_counts == audit_invocation_class_counts;
    let failed_status_multiset_matches =
        audit_failure_multiset_matches(raw_transactions, audit_transactions);

    PumpResearchAuditComparisonV1 {
        raw_identity_count,
        audit_identity_count,
        raw_failed_transaction_count,
        audit_failed_transaction_count,
        raw_only_identities,
        audit_only_identities,
        raw_invocation_class_counts,
        audit_invocation_class_counts,
        identity_multiset_matches,
        invocation_class_counts_match,
        failed_status_multiset_matches,
    }
}

fn qualification_slot_status_from_comparison_v1(
    comparison: &PumpResearchAuditComparisonV1,
) -> PumpResearchQualificationSlotStatusV1 {
    if !comparison.identity_multiset_matches || !comparison.failed_status_multiset_matches {
        PumpResearchQualificationSlotStatusV1::SourceCoverageUnproven
    } else if !comparison.invocation_class_counts_match {
        PumpResearchQualificationSlotStatusV1::SourceFilterCpiCoverageUnproven
    } else {
        PumpResearchQualificationSlotStatusV1::Matched
    }
}

fn qualification_status_from_audit_failures_v1(
    saw_source_coverage_failure: bool,
    saw_filter_coverage_failure: bool,
) -> PumpResearchTapeQualificationStatusV1 {
    if saw_source_coverage_failure {
        PumpResearchTapeQualificationStatusV1::Blocked(
            PumpResearchQualificationBlockerV1::SourceCoverageUnproven,
        )
    } else if saw_filter_coverage_failure {
        PumpResearchTapeQualificationStatusV1::Blocked(
            PumpResearchQualificationBlockerV1::SourceFilterCpiCoverageUnproven,
        )
    } else {
        PumpResearchTapeQualificationStatusV1::Ready
    }
}

fn audit_transaction_counts(
    transactions: &[PumpResearchAuditTransactionV1],
) -> (u32, u32, BTreeMap<String, u64>) {
    let mut class_counts = empty_invocation_class_counts();
    let mut failed_count = 0_u32;
    for transaction in transactions {
        if transaction.failed {
            failed_count = failed_count.saturating_add(1);
        }
        add_invocation_class_counts(&mut class_counts, &transaction.invocation_class_counts);
    }
    (
        u32::try_from(transactions.len()).unwrap_or(u32::MAX),
        failed_count,
        class_counts,
    )
}

fn redacted_audit_error(
    config: &PumpResearchQualificationAuditConfigV1,
    connection: &PumpResearchResolvedAuditConnectionV1,
    error: &str,
) -> String {
    let mut redacted = error.replace(&connection.endpoint, "<redacted-audit-endpoint>");
    redacted = redacted.replace(&config.audit_rpc_endpoint, "<redacted-audit-origin>");
    if let Ok(parsed) = Url::parse(&config.audit_rpc_endpoint) {
        if let Some(host) = parsed.host_str() {
            redacted = redacted.replace(host, "<redacted-audit-host>");
        }
    }
    if let Some(path_credential) = &connection.endpoint_path_credential {
        redacted = redacted.replace(path_credential, "<redacted-audit-path-credential>");
    }
    if let Some(auth_token) = &connection.auth_token {
        redacted = redacted.replace(auth_token, "<redacted-audit-credential>");
    }
    redacted
}

fn value_string<'a>(value: &'a serde_json::Value, field: &str) -> Result<&'a str> {
    value
        .get(field)
        .and_then(serde_json::Value::as_str)
        .ok_or_else(|| anyhow::anyhow!("qualification preparation snapshot lacks string {field}"))
}

fn value_bool(value: &serde_json::Value, field: &str) -> Result<bool> {
    value
        .get(field)
        .and_then(serde_json::Value::as_bool)
        .ok_or_else(|| anyhow::anyhow!("qualification preparation snapshot lacks bool {field}"))
}

fn validate_provider_suitability_preparation_v1(
    preparation_receipt_path: &Path,
    expected_preparation_sha256: &str,
    audit_config_digest: &PumpResearchOperatorDigestV1,
    audit_provider_id: &str,
    raw: &PumpResearchRawTapeIndexV1,
    run_dir: &Path,
) -> Result<PumpResearchValidatedSuitabilityPreparationV1> {
    if expected_preparation_sha256.len() != 64
        || !expected_preparation_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
    {
        bail!("expected qualification preparation SHA-256 must contain 64 hex characters");
    }
    let preparation_authority = read_bounded_authority_file_v1(
        preparation_receipt_path,
        PumpResearchAuthorityFileKindV1::QualificationPreparationReceipt,
    )?;
    let digest = preparation_authority.digest.clone();
    if !digest
        .sha256
        .eq_ignore_ascii_case(expected_preparation_sha256)
    {
        bail!(
            "qualification preparation snapshot SHA-256 {} differs from expected {}",
            digest.sha256,
            expected_preparation_sha256
        );
    }
    let value: serde_json::Value = serde_json::from_slice(&preparation_authority.bytes)
        .with_context(|| {
            format!(
                "decode JSON qualification preparation receipt {}",
                preparation_receipt_path.display()
            )
        })?;
    if value
        .get("schema_version")
        .and_then(serde_json::Value::as_u64)
        != Some(1)
        || value_string(&value, "kind")? != "pump_research_qualification_preparation_v1"
        || value_string(&value, "status")? != "HOLD_PROVIDER_IO_AND_CERTIFY"
        || value_string(&value, "raw_run_id")? != raw.start_manifest.run_id
        || value_string(&value, "audit_provider_id")? != audit_provider_id
        || audit_provider_id == raw.start_manifest.primary_provider_id
    {
        bail!("qualification preparation snapshot identity/status is invalid");
    }
    if value_string(&value, "audit_config_sha256")? != audit_config_digest.sha256 {
        bail!("qualification preparation snapshot audit config digest has drifted");
    }
    let control_paths = [
        (
            "raw_binding_sha256",
            run_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
        ),
        (
            "raw_start_manifest_sha256",
            run_dir.join("run_start_manifest.json"),
            PumpResearchAuthorityFileKindV1::RawStartManifest,
        ),
        (
            "raw_completion_receipt_sha256",
            run_dir.join("run_completion_receipt.json"),
            PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
        ),
    ];
    for (field, path, kind) in control_paths {
        if value_string(&value, field)? != digest_bounded_authority_file_v1(&path, kind)?.sha256 {
            bail!("qualification preparation snapshot {field} no longer matches raw evidence");
        }
    }
    for field in [
        "provider_io_performed",
        "certify_started",
        "qualification_audit_started",
        "export_started",
        "strategy_started",
        "physical_provider_capacity_and_retention_verified",
    ] {
        if value_bool(&value, field)? {
            bail!("qualification preparation snapshot unexpectedly has {field}=true");
        }
    }
    if !value_bool(&value, "planned_output_dir_absent")? {
        bail!("qualification preparation snapshot did not prove planned output absence");
    }
    let planned_output = PathBuf::from(value_string(&value, "planned_output_dir")?);
    if planned_output.exists() {
        bail!(
            "planned exact output {} exists before provider suitability",
            planned_output.display()
        );
    }
    Ok(PumpResearchValidatedSuitabilityPreparationV1 {
        digest,
        planned_exact_output: planned_output,
    })
}

fn wall_clock_ms_v1() -> Result<u64> {
    let duration = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .context("system clock is before Unix epoch")?;
    u64::try_from(duration.as_millis()).context("wall clock milliseconds overflow u64")
}

fn bytes_contain(haystack: &[u8], needle: &[u8]) -> bool {
    !needle.is_empty()
        && haystack
            .windows(needle.len())
            .any(|window| window == needle)
}

fn canonical_create_new_output_path(path: &Path, label: &str) -> Result<PathBuf> {
    if path.exists() {
        bail!("{label} {} already exists", path.display());
    }
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| anyhow::anyhow!("{label} {} has no parent directory", path.display()))?;
    let canonical_parent = fs::canonicalize(parent)
        .with_context(|| format!("canonicalize {label} parent {}", parent.display()))?;
    if !canonical_parent.is_dir() {
        bail!(
            "{label} parent {} is not a directory",
            canonical_parent.display()
        );
    }
    let name = path
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("{label} {} has no file name", path.display()))?;
    Ok(canonical_parent.join(name))
}

fn validate_combined_exact_output_path_v1(run_dir: &Path, output_dir: &Path) -> Result<PathBuf> {
    let canonical_raw = fs::canonicalize(run_dir)
        .with_context(|| format!("canonicalize raw run directory {}", run_dir.display()))?;
    if !canonical_raw.is_dir() {
        bail!(
            "raw run directory {} is not a directory",
            canonical_raw.display()
        );
    }
    let canonical_output = canonical_create_new_output_path(output_dir, "exact output")?;
    if canonical_output.starts_with(&canonical_raw) || canonical_raw.starts_with(&canonical_output)
    {
        bail!(
            "exact output {} must be outside and disjoint from immutable raw evidence {}",
            canonical_output.display(),
            canonical_raw.display()
        );
    }
    Ok(canonical_output)
}

fn validate_provider_suitability_output_path_v1(
    run_dir: &Path,
    planned_exact_output: &Path,
    output_dir: &Path,
) -> Result<PathBuf> {
    let canonical_raw = fs::canonicalize(run_dir)
        .with_context(|| format!("canonicalize raw run directory {}", run_dir.display()))?;
    let canonical_output =
        canonical_create_new_output_path(output_dir, "provider suitability output")?;
    let canonical_planned_exact =
        canonical_create_new_output_path(planned_exact_output, "planned exact output")?;
    if canonical_output.starts_with(&canonical_raw) || canonical_raw.starts_with(&canonical_output)
    {
        bail!(
            "provider suitability output {} must be outside immutable raw evidence {}",
            canonical_output.display(),
            canonical_raw.display()
        );
    }
    if canonical_output == canonical_planned_exact
        || canonical_output.starts_with(&canonical_planned_exact)
        || canonical_planned_exact.starts_with(&canonical_output)
    {
        bail!(
            "provider suitability output {} must be disjoint from planned exact output {}",
            canonical_output.display(),
            canonical_planned_exact.display()
        );
    }
    Ok(canonical_output)
}

fn validate_attested_output_before_snapshot_v1(
    source_run_id: &str,
    canonical_output: &Path,
    attestation_path: &Path,
    expected_attestation_sha256: &str,
    running_executable_authority: &PumpResearchRunningExecutableAuthorityV1,
) -> Result<()> {
    validate_expected_sha256_v1(
        expected_attestation_sha256,
        "expected provider-independence attestation SHA-256",
    )?;
    let attestation_authority = read_bounded_authority_file_v1(
        attestation_path,
        PumpResearchAuthorityFileKindV1::ProviderIndependenceAttestation,
    )?;
    let attestation_digest = attestation_authority.digest.clone();
    if !attestation_digest
        .sha256
        .eq_ignore_ascii_case(expected_attestation_sha256)
    {
        bail!(
            "provider-independence attestation SHA-256 {} differs from expected {}",
            attestation_digest.sha256,
            expected_attestation_sha256
        );
    }
    let attestation: PumpResearchProviderIndependenceAttestationV1 =
        serde_json::from_slice(&attestation_authority.bytes).with_context(|| {
            format!(
                "decode provider-independence attestation {} before raw snapshot",
                attestation_path.display()
            )
        })?;
    if attestation.attestation_version != PROVIDER_INDEPENDENCE_ATTESTATION_VERSION_V1
        || attestation.source_run_id != source_run_id
        || attestation.attestation_status != PROVIDER_INDEPENDENCE_ATTESTATION_STATUS_V1
        || !attestation.independence_assertions.distinct_legal_entities
        || !attestation
            .independence_assertions
            .distinct_infrastructure_operators
        || !attestation
            .independence_assertions
            .distinct_network_routing_paths
        || !attestation
            .independence_assertions
            .distinct_ingest_architecture
        || !attestation
            .independence_assertions
            .zero_shared_credential_domain
        || !attestation
            .independence_assertions
            .independent_retention_and_indexing
    {
        bail!("provider-independence attestation lacks an approved physical-independence decision");
    }
    let attested_output = canonical_create_new_output_path(
        Path::new(&attestation.evidence_bindings.planned_exact_output),
        "attested planned exact output",
    )?;
    if attested_output != canonical_output {
        bail!(
            "attested planned exact output {} differs from requested {}",
            attested_output.display(),
            canonical_output.display()
        );
    }
    if attestation
        .evidence_bindings
        .combined_certifier_executable_digest
        != *running_executable_authority.digest()
    {
        bail!("provider-independence attestation running executable binding is stale");
    }
    running_executable_authority.revalidate_v1("before raw snapshot")?;
    Ok(())
}

fn validate_expected_sha256_v1(value: &str, label: &str) -> Result<()> {
    if value.len() != 64 || !value.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        bail!("{label} must contain exactly 64 hexadecimal characters");
    }
    Ok(())
}

impl PumpResearchValidatedGoDSourceAuthorityV1 {
    fn report_v1(&self) -> PumpResearchGoDSourceAuthorityReportV1 {
        PumpResearchGoDSourceAuthorityReportV1 {
            schema_version: GO_D_SOURCE_AUTHORITY_SCHEMA_V1,
            source_run_id: self.receipt.source_run_id.clone(),
            go_d_source_authority: GO_D_SOURCE_AUTHORITY_VERIFIED_V1,
            external_go_e_audit_not_used_as_gate: true,
            external_go_e_audit_status: GO_E_EXTERNAL_AUDIT_RETIRED_V1,
            go_d_source_authority_sha256: self.digest.sha256.clone(),
            raw_provenance_binding_sha256: self.receipt.raw_provenance_binding_sha256.clone(),
            raw_start_manifest_sha256: self.receipt.raw_start_manifest_sha256.clone(),
            raw_completion_receipt_sha256: self.receipt.raw_completion_receipt_sha256.clone(),
            raw_segment_set_blake3: self.receipt.raw_segment_set_blake3.clone(),
        }
    }

    fn revalidate_v1(
        &self,
        raw: &PumpResearchRawTapeIndexV1,
        raw_dir: &Path,
        boundary: &str,
    ) -> Result<()> {
        let current = digest_bounded_authority_file_v1(
            &self.path,
            PumpResearchAuthorityFileKindV1::GoDSourceAuthority,
        )?;
        if current != self.digest {
            bail!("GO-D source authority changed at {boundary}");
        }
        raw.revalidate_raw_control_authority_v1(raw_dir, boundary)?;
        raw.revalidate_raw_segment_set_v1(boundary)?;
        if raw.raw_segment_set_blake3_v1() != Some(self.receipt.raw_segment_set_blake3.as_str()) {
            bail!("GO-D raw segment-set authority changed at {boundary}");
        }
        Ok(())
    }
}

fn validate_go_d_source_authority_v1(
    raw: &PumpResearchRawTapeIndexV1,
    raw_dir: &Path,
    authority_path: &Path,
    expected_authority_sha256: &str,
) -> Result<PumpResearchValidatedGoDSourceAuthorityV1> {
    validate_expected_sha256_v1(
        expected_authority_sha256,
        "expected GO-D source-authority SHA-256",
    )?;
    let authority = read_bounded_authority_file_v1(
        authority_path,
        PumpResearchAuthorityFileKindV1::GoDSourceAuthority,
    )?;
    if !authority
        .digest
        .sha256
        .eq_ignore_ascii_case(expected_authority_sha256)
    {
        bail!(
            "GO-D source-authority SHA-256 {} differs from expected {}",
            authority.digest.sha256,
            expected_authority_sha256
        );
    }
    let receipt: PumpResearchGoDSourceAuthorityReceiptV1 = serde_json::from_slice(&authority.bytes)
        .with_context(|| format!("decode GO-D source authority {}", authority_path.display()))?;
    if receipt.schema_version != GO_D_SOURCE_AUTHORITY_SCHEMA_V1
        || receipt.go_d_source_authority != GO_D_SOURCE_AUTHORITY_VERIFIED_V1
        || receipt.external_go_e_audit != GO_E_EXTERNAL_AUDIT_RETIRED_V1
        || receipt.source_run_id != raw.start_manifest.run_id
        || receipt.source_storage_format_version != raw.start_manifest.storage_format_version
    {
        bail!("GO-D source authority does not identify this frozen raw tape");
    }
    if receipt.operator_decision.trim().is_empty() || receipt.created_wall_ms == 0 {
        bail!("GO-D source authority lacks a non-empty operator decision and creation time");
    }
    for (label, value) in [
        (
            "raw_provenance_binding_sha256",
            receipt.raw_provenance_binding_sha256.as_str(),
        ),
        (
            "raw_start_manifest_sha256",
            receipt.raw_start_manifest_sha256.as_str(),
        ),
        (
            "raw_completion_receipt_sha256",
            receipt.raw_completion_receipt_sha256.as_str(),
        ),
        (
            "raw_segment_set_blake3",
            receipt.raw_segment_set_blake3.as_str(),
        ),
    ] {
        validate_expected_sha256_v1(value, label)?;
    }
    if !matches!(
        raw.capture_provenance_eligibility,
        PumpResearchCaptureProvenanceEligibilityV1::Eligible
    ) || raw.completion_receipt.status != PumpResearchRunCompletionStatusV1::Complete
        || !raw.completion_receipt.clean_shutdown
        || !raw.completion_receipt.source_stream_established
        || !raw.completion_receipt.first_source_update_received
        || !raw.completion_receipt.source_workers_cleanly_stopped
        || raw.completion_receipt.capture_failure.is_some()
        || raw.completion_receipt.source_lifecycle_error.is_some()
        || raw.completion_receipt.received_source_update_count
            != raw.completion_receipt.admitted_source_update_count
        || raw.completion_receipt.admitted_source_update_count
            != raw.completion_receipt.persisted_source_record_count
        || raw.completion_receipt.dropped_source_update_count != 0
        || raw.completion_receipt.gap_count != 0
        || raw
            .completion_receipt
            .persisted_ingress_gap_missing_event_count
            != 0
        || !raw.coverage_gaps.is_empty()
    {
        bail!(
            "GO-D source authority requires eligible provenance and clean zero-loss raw lifecycle"
        );
    }
    let controls =
        raw.revalidate_raw_control_authority_v1(raw_dir, "GO-D-source-authority-validation")?;
    let binding = controls
        .provenance_binding_digest
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("GO-D raw tape lacks provenance binding authority"))?;
    if !binding
        .sha256
        .eq_ignore_ascii_case(&receipt.raw_provenance_binding_sha256)
        || !controls
            .start_manifest_digest
            .sha256
            .eq_ignore_ascii_case(&receipt.raw_start_manifest_sha256)
        || !controls
            .completion_receipt_digest
            .sha256
            .eq_ignore_ascii_case(&receipt.raw_completion_receipt_sha256)
    {
        bail!("GO-D source authority control-file binding is stale");
    }
    if raw.raw_segment_set_blake3_v1() != Some(receipt.raw_segment_set_blake3.as_str()) {
        bail!("GO-D source authority raw segment-set binding is stale");
    }
    let validated = PumpResearchValidatedGoDSourceAuthorityV1 {
        receipt,
        digest: authority.digest,
        path: authority_path.to_path_buf(),
    };
    validated.revalidate_v1(raw, raw_dir, "after-GO-D-authority-validation")?;
    Ok(validated)
}

fn validate_provider_suitability_receipt_for_combined_audit_v1(
    receipt: &PumpResearchProviderSuitabilityReceiptV1,
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    audit_config: &PumpResearchQualificationAuditConfigV1,
    resolved_audit_endpoint: &str,
    audit_config_digest: &PumpResearchOperatorDigestV1,
    raw_binding_digest: &PumpResearchOperatorDigestV1,
    raw_start_manifest_digest: &PumpResearchOperatorDigestV1,
    raw_completion_receipt_digest: &PumpResearchOperatorDigestV1,
) -> Result<PumpResearchQualifiedSlotRangeV1> {
    let PumpResearchQualificationRangeSelectionV1::Ready(expected_range) =
        qualification_range_selection_v1(raw, canonicality)
    else {
        bail!("combined audit has no eligible epoch-aware qualification range");
    };
    let expected_plan = build_provider_suitability_plan_v1(raw, canonicality)?;
    if expected_plan.qualification_range != Some(expected_range)
        || expected_plan.qualification_blocker.is_some()
    {
        bail!("provider suitability plan differs from the qualification authority range");
    }
    if receipt.schema_version != 1
        || receipt.kind != "pump_research_provider_suitability_v1"
        || receipt.status != PumpResearchProviderSuitabilityStatusV1::ReadyForFullAudit
        || receipt.source_run_id != raw.start_manifest.run_id
        || receipt.audit_provider_id != audit_config.audit_provider_id
        || receipt.audit_config_digest != *audit_config_digest
        || receipt.audit_rpc_endpoint_blake3
            != blake3::hash(resolved_audit_endpoint.as_bytes())
                .to_hex()
                .to_string()
        || receipt.raw_binding_digest != *raw_binding_digest
        || receipt.raw_start_manifest_digest != *raw_start_manifest_digest
        || receipt.raw_completion_receipt_digest != *raw_completion_receipt_digest
        || receipt.qualification_stream_epoch != Some(expected_range.stream_epoch)
        || receipt.qualification_start_slot != Some(expected_range.start_slot)
        || receipt.qualification_end_slot != Some(expected_range.end_slot)
        || receipt.qualification_blocker.is_some()
        || receipt.audit_auth_mode != qualification_audit_auth_mode_v1(audit_config)
        || receipt.provider_identity_independence_verified
        || receipt.bounded_concurrency != audit_config.bounded_concurrency
        || receipt.bounded_retry_count != audit_config.bounded_retry_count
        || receipt.request_timeout_ms != audit_config.request_timeout_ms
        || receipt.burst_slot_target_count != PROVIDER_SUITABILITY_BURST_SLOT_COUNT_V1
        || receipt.max_raw_representative_scan
            != PROVIDER_SUITABILITY_MAX_RAW_REPRESENTATIVE_SCAN_V1
        || receipt.max_consecutive_unavailable
            != PROVIDER_SUITABILITY_MAX_CONSECUTIVE_UNAVAILABLE_V1
        || receipt.max_provider_wall_ms != PROVIDER_SUITABILITY_MAX_PROVIDER_WALL_MS_V1
        || receipt.raw_representative_transactions_examined
            != expected_plan.raw_representative_transactions_examined
        || receipt.missing_raw_representative_roles
            != expected_plan.missing_raw_representative_roles
    {
        bail!("provider suitability receipt identity, inputs or qualification range are invalid");
    }
    if !receipt.provider_io_performed
        || receipt.sample_slot_count != expected_plan.slots.len()
        || receipt.sample_slot_count == 0
        || receipt.attempted_slot_count != receipt.sample_slot_count
        || receipt.matched_slot_count != receipt.sample_slot_count
        || receipt.unavailable_slot_count != 0
        || !receipt.missing_raw_representative_roles.is_empty()
        || receipt.raw_write_attempt_count != 0
        || receipt.exact_output_created
        || receipt.certify_started
        || receipt.export_started
        || receipt.strategy_started
        || receipt.slot_findings.len() != receipt.sample_slot_count
        || receipt.slot_findings.iter().any(|finding| {
            finding.fetch_status != PumpResearchProviderSuitabilityFetchStatusV1::Block
                || !finding.identity_multiset_matches
                || !finding.invocation_class_counts_match
                || !finding.failed_status_multiset_matches
                || finding.audit_error.is_some()
        })
    {
        bail!("provider suitability receipt is not a complete ReadyForFullAudit proof");
    }

    let mut finding_by_slot = BTreeMap::new();
    for finding in &receipt.slot_findings {
        if finding_by_slot.insert(finding.slot, finding).is_some() {
            bail!(
                "provider suitability receipt contains duplicate finding for slot {}",
                finding.slot
            );
        }
    }
    let expected_sample_slots: BTreeSet<_> = expected_plan.slots.keys().copied().collect();
    if finding_by_slot.keys().copied().collect::<BTreeSet<_>>() != expected_sample_slots {
        bail!("provider suitability receipt sample slots differ from the deterministic plan");
    }
    let raw_by_slot = collect_provider_suitability_raw_transactions_v1(
        raw,
        canonicality,
        expected_range,
        &expected_sample_slots,
    )?;
    let mut total_request_attempt_count = 0_u64;
    for (slot, expected_roles) in &expected_plan.slots {
        let finding = finding_by_slot.get(slot).copied().ok_or_else(|| {
            anyhow::anyhow!("missing provider suitability finding for slot {slot}")
        })?;
        let expected_roles: Vec<_> = expected_roles.iter().cloned().collect();
        let raw_transactions = raw_by_slot.get(slot).map(Vec::as_slice).unwrap_or_default();
        let (raw_count, raw_failed, raw_classes) = audit_transaction_counts(raw_transactions);
        if finding.schema_version != 1
            || finding.selection_roles != expected_roles
            || finding.request_attempt_count == 0
            || finding.request_attempt_count > audit_config.bounded_retry_count.saturating_add(1)
            || finding.raw_identity_count != raw_count
            || finding.audit_identity_count != raw_count
            || finding.raw_failed_transaction_count != raw_failed
            || finding.audit_failed_transaction_count != raw_failed
            || finding.raw_invocation_class_counts != raw_classes
            || finding.audit_invocation_class_counts != raw_classes
            || !finding.raw_only_identities.is_empty()
            || !finding.audit_only_identities.is_empty()
        {
            bail!(
                "provider suitability finding for slot {slot} differs from the deterministic raw plan"
            );
        }
        total_request_attempt_count =
            total_request_attempt_count.saturating_add(u64::from(finding.request_attempt_count));
    }
    if receipt.total_request_attempt_count != total_request_attempt_count {
        bail!("provider suitability receipt request-attempt total is inconsistent with findings");
    }
    Ok(expected_range)
}

#[allow(clippy::too_many_arguments)]
fn validate_provider_independence_attestation_v1(
    raw: &PumpResearchRawTapeIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    output_dir: &Path,
    audit_config_path: &Path,
    provider_suitability_receipt_path: &Path,
    attestation_path: &Path,
    expected_attestation_sha256: &str,
    running_executable_authority: Arc<PumpResearchRunningExecutableAuthorityV1>,
) -> Result<PumpResearchValidatedCombinedAuditAuthorityV1> {
    validate_expected_sha256_v1(
        expected_attestation_sha256,
        "expected provider-independence attestation SHA-256",
    )?;
    let audit_config_authority = read_bounded_authority_file_v1(
        audit_config_path,
        PumpResearchAuthorityFileKindV1::QualificationAuditConfig,
    )?;
    let audit_config = PumpResearchQualificationAuditConfigV1::from_bytes(
        audit_config_path,
        &audit_config_authority.bytes,
    )?;
    let audit_config_digest = audit_config_authority.digest;
    let resolved_connection = audit_config.resolve_connection()?;

    let raw_binding_path = raw
        .segments
        .first()
        .and_then(|segment| segment.path.parent())
        .ok_or_else(|| anyhow::anyhow!("indexed raw tape has no segment parent directory"))?
        .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1);
    let raw_dir = raw_binding_path
        .parent()
        .ok_or_else(|| anyhow::anyhow!("raw binding has no parent directory"))?;
    let raw_start_manifest_path = raw_dir.join("run_start_manifest.json");
    let raw_completion_receipt_path = raw_dir.join("run_completion_receipt.json");
    let current_raw_controls =
        raw.revalidate_raw_control_authority_v1(raw_dir, "combined-authority-validation")?;
    let raw_binding_digest = current_raw_controls
        .provenance_binding_digest
        .ok_or_else(|| anyhow::anyhow!("combined audit lacks a raw provenance binding digest"))?;
    let raw_start_manifest_digest = current_raw_controls.start_manifest_digest;
    let raw_completion_receipt_digest = current_raw_controls.completion_receipt_digest;

    let suitability_authority = read_bounded_authority_file_v1(
        provider_suitability_receipt_path,
        PumpResearchAuthorityFileKindV1::ProviderSuitabilityReceipt,
    )?;
    let provider_suitability_receipt_digest = suitability_authority.digest.clone();
    let suitability: PumpResearchProviderSuitabilityReceiptV1 =
        serde_json::from_slice(&suitability_authority.bytes).with_context(|| {
            format!(
                "decode provider suitability receipt {}",
                provider_suitability_receipt_path.display()
            )
        })?;
    let qualification_range = validate_provider_suitability_receipt_for_combined_audit_v1(
        &suitability,
        raw,
        canonicality,
        &audit_config,
        &resolved_connection.endpoint,
        &audit_config_digest,
        &raw_binding_digest,
        &raw_start_manifest_digest,
        &raw_completion_receipt_digest,
    )?;

    let attestation_authority = read_bounded_authority_file_v1(
        attestation_path,
        PumpResearchAuthorityFileKindV1::ProviderIndependenceAttestation,
    )?;
    let attestation_digest = attestation_authority.digest.clone();
    if !attestation_digest
        .sha256
        .eq_ignore_ascii_case(expected_attestation_sha256)
    {
        bail!(
            "provider-independence attestation SHA-256 {} differs from expected {}",
            attestation_digest.sha256,
            expected_attestation_sha256
        );
    }
    let attestation: PumpResearchProviderIndependenceAttestationV1 =
        serde_json::from_slice(&attestation_authority.bytes).with_context(|| {
            format!(
                "decode provider-independence attestation {}",
                attestation_path.display()
            )
        })?;
    running_executable_authority.revalidate_v1("combined-authority-validation")?;
    let combined_certifier_executable_digest = running_executable_authority.digest().clone();
    let canonical_output = canonical_create_new_output_path(output_dir, "exact output")?;
    let canonical_output_text = canonical_output
        .to_str()
        .ok_or_else(|| anyhow::anyhow!("canonical exact output path is not UTF-8"))?;
    let endpoint_digest = blake3::hash(resolved_connection.endpoint.as_bytes())
        .to_hex()
        .to_string();

    if attestation.attestation_version != PROVIDER_INDEPENDENCE_ATTESTATION_VERSION_V1
        || attestation.source_run_id != raw.start_manifest.run_id
        || attestation.go_e0_receipt_sha256 != provider_suitability_receipt_digest.sha256
        || attestation.primary_provider.provider_id != raw.start_manifest.primary_provider_id
        || attestation.audit_provider.provider_id != audit_config.audit_provider_id
        || attestation.primary_provider.provider_id == attestation.audit_provider.provider_id
        || attestation.primary_provider.entity_name == attestation.audit_provider.entity_name
        || attestation.evidence_bindings.audit_rpc_endpoint_blake3 != endpoint_digest
        || attestation.evidence_bindings.audit_config_digest != audit_config_digest
        || attestation
            .evidence_bindings
            .provider_suitability_receipt_digest
            != provider_suitability_receipt_digest
        || attestation
            .evidence_bindings
            .provider_suitability_executable_digest
            != suitability.executable_digest
        || attestation
            .evidence_bindings
            .combined_certifier_executable_digest
            != combined_certifier_executable_digest
        || attestation.evidence_bindings.raw_binding_digest != raw_binding_digest
        || attestation.evidence_bindings.raw_start_manifest_digest != raw_start_manifest_digest
        || attestation.evidence_bindings.raw_completion_receipt_digest
            != raw_completion_receipt_digest
        || attestation.evidence_bindings.qualification_stream_epoch
            != qualification_range.stream_epoch
        || attestation.evidence_bindings.qualification_start_slot != qualification_range.start_slot
        || attestation.evidence_bindings.qualification_end_slot != qualification_range.end_slot
        || attestation.evidence_bindings.planned_exact_output != canonical_output_text
    {
        bail!("provider-independence attestation bindings do not match combined audit inputs");
    }
    for (label, value) in [
        (
            "primary provider service_type",
            attestation.primary_provider.service_type.as_str(),
        ),
        (
            "primary provider entity_name",
            attestation.primary_provider.entity_name.as_str(),
        ),
        (
            "primary provider infrastructure_type",
            attestation.primary_provider.infrastructure_type.as_str(),
        ),
        (
            "primary provider network_autonomous_system",
            attestation
                .primary_provider
                .network_autonomous_system
                .as_str(),
        ),
        (
            "primary provider datacenter location",
            attestation
                .primary_provider
                .primary_datacenter_location
                .as_str(),
        ),
        (
            "audit provider service_type",
            attestation.audit_provider.service_type.as_str(),
        ),
        (
            "audit provider entity_name",
            attestation.audit_provider.entity_name.as_str(),
        ),
        (
            "audit provider infrastructure_type",
            attestation.audit_provider.infrastructure_type.as_str(),
        ),
        (
            "audit provider network_autonomous_system",
            attestation
                .audit_provider
                .network_autonomous_system
                .as_str(),
        ),
        (
            "audit provider datacenter location",
            attestation
                .audit_provider
                .primary_datacenter_location
                .as_str(),
        ),
        (
            "provider-independence reviewer_id",
            attestation.reviewer_signoff.reviewer_id.as_str(),
        ),
        (
            "provider-independence operator_assertion",
            attestation.reviewer_signoff.operator_assertion.as_str(),
        ),
    ] {
        validate_nonempty_trimmed(label, value)?;
    }
    if attestation.primary_provider.service_type != "yellowstone_grpc_geyser"
        || attestation.audit_provider.service_type != "json_rpc_read_only"
        || attestation.attestation_status != PROVIDER_INDEPENDENCE_ATTESTATION_STATUS_V1
        || attestation.reviewer_signoff.created_wall_ms == 0
        || attestation.reviewer_signoff.operator_assertion.len() > 4096
        || attestation.reviewer_signoff.evidence_references.is_empty()
        || attestation.reviewer_signoff.evidence_references.len() > 16
        || attestation
            .reviewer_signoff
            .evidence_references
            .iter()
            .any(|reference| {
                reference.trim().is_empty()
                    || reference.trim() != reference
                    || reference.len() > 2048
            })
        || !attestation.independence_assertions.distinct_legal_entities
        || !attestation
            .independence_assertions
            .distinct_infrastructure_operators
        || !attestation
            .independence_assertions
            .distinct_network_routing_paths
        || !attestation
            .independence_assertions
            .distinct_ingest_architecture
        || !attestation
            .independence_assertions
            .zero_shared_credential_domain
        || !attestation
            .independence_assertions
            .independent_retention_and_indexing
    {
        bail!("provider-independence attestation lacks an approved physical-independence decision");
    }
    if bytes_contain(
        &attestation_authority.bytes,
        resolved_connection.endpoint.as_bytes(),
    ) || bytes_contain(
        &attestation_authority.bytes,
        audit_config.audit_rpc_endpoint.as_bytes(),
    ) {
        bail!("provider-independence attestation must not persist the literal audit endpoint");
    }
    if let Some(host) = Url::parse(&audit_config.audit_rpc_endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
    {
        if bytes_contain(&attestation_authority.bytes, host.as_bytes()) {
            bail!("provider-independence attestation must not persist the literal audit hostname");
        }
    }
    if let Some(token) = resolved_connection.auth_token.as_deref() {
        if bytes_contain(&attestation_authority.bytes, token.as_bytes()) {
            bail!("provider-independence attestation must not persist the audit credential");
        }
    }
    if let Some(path_credential) = resolved_connection.endpoint_path_credential.as_deref() {
        if bytes_contain(&attestation_authority.bytes, path_credential.as_bytes()) {
            bail!("provider-independence attestation must not persist the audit endpoint-path credential");
        }
    }

    Ok(PumpResearchValidatedCombinedAuditAuthorityV1 {
        audit_config,
        resolved_connection,
        audit_rpc_endpoint_blake3: endpoint_digest.clone(),
        provider_independence: PumpResearchValidatedProviderIndependenceV1 {
            attestation_digest,
            attestation_path: attestation_path.to_path_buf(),
            audit_config_digest,
            audit_config_path: audit_config_path.to_path_buf(),
            provider_suitability_receipt_digest,
            provider_suitability_receipt_path: provider_suitability_receipt_path.to_path_buf(),
            running_executable_authority,
            raw_binding_digest,
            raw_binding_path,
            raw_start_manifest_digest,
            raw_start_manifest_path,
            raw_completion_receipt_digest,
            raw_completion_receipt_path,
            audit_rpc_endpoint_blake3: endpoint_digest,
            planned_exact_output: canonical_output,
        },
    })
}

impl PumpResearchValidatedProviderIndependenceV1 {
    fn revalidate_before_exact_output_v1(
        &self,
        output_dir: &Path,
        audited_endpoint_blake3: &str,
    ) -> Result<()> {
        if audited_endpoint_blake3 != self.audit_rpc_endpoint_blake3 {
            bail!(
                "full audit endpoint digest differs from validated provider-independence authority"
            );
        }
        let stable_inputs = [
            (
                "provider-independence attestation",
                self.attestation_path.as_path(),
                &self.attestation_digest,
                PumpResearchAuthorityFileKindV1::ProviderIndependenceAttestation,
            ),
            (
                "qualification audit config",
                self.audit_config_path.as_path(),
                &self.audit_config_digest,
                PumpResearchAuthorityFileKindV1::QualificationAuditConfig,
            ),
            (
                "provider suitability receipt",
                self.provider_suitability_receipt_path.as_path(),
                &self.provider_suitability_receipt_digest,
                PumpResearchAuthorityFileKindV1::ProviderSuitabilityReceipt,
            ),
            (
                "raw provenance binding",
                self.raw_binding_path.as_path(),
                &self.raw_binding_digest,
                PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
            ),
            (
                "raw start manifest",
                self.raw_start_manifest_path.as_path(),
                &self.raw_start_manifest_digest,
                PumpResearchAuthorityFileKindV1::RawStartManifest,
            ),
            (
                "raw completion receipt",
                self.raw_completion_receipt_path.as_path(),
                &self.raw_completion_receipt_digest,
                PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
            ),
        ];
        for (label, path, expected, kind) in stable_inputs {
            let current = digest_bounded_authority_file_v1(path, kind)?;
            if &current != expected {
                bail!("{label} changed after provider-independence validation");
            }
        }
        self.running_executable_authority
            .revalidate_v1("before exact output")?;
        let current_output = canonical_create_new_output_path(output_dir, "exact output")?;
        if current_output != self.planned_exact_output {
            bail!("exact output path changed after provider-independence validation");
        }
        Ok(())
    }
}

fn publish_provider_suitability_receipt_v1(
    output_dir: &Path,
    receipt: &PumpResearchProviderSuitabilityReceiptV1,
    config: &PumpResearchQualificationAuditConfigV1,
    connection: &PumpResearchResolvedAuditConnectionV1,
) -> Result<PathBuf> {
    if output_dir.exists() {
        bail!(
            "provider suitability output {} already exists",
            output_dir.display()
        );
    }
    let parent = output_dir.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "provider suitability output {} has no parent",
            output_dir.display()
        )
    })?;
    fs::create_dir_all(parent)
        .with_context(|| format!("create provider suitability parent {}", parent.display()))?;
    let name = output_dir
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or_else(|| anyhow::anyhow!("provider suitability output name is not UTF-8"))?;
    let partial_dir = parent.join(format!(".{name}.partial"));
    if partial_dir.exists() {
        bail!(
            "provider suitability partial output {} already exists",
            partial_dir.display()
        );
    }
    fs::create_dir(&partial_dir).with_context(|| {
        format!(
            "create provider suitability partial directory {}",
            partial_dir.display()
        )
    })?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&partial_dir, fs::Permissions::from_mode(0o700))?;
    }
    let receipt_path = partial_dir.join(PROVIDER_SUITABILITY_RECEIPT_FILE_V1);
    let receipt_bytes =
        serde_json::to_vec_pretty(receipt).context("serialize provider suitability receipt")?;
    if bytes_contain(&receipt_bytes, connection.endpoint.as_bytes())
        || bytes_contain(&receipt_bytes, config.audit_rpc_endpoint.as_bytes())
    {
        bail!("provider suitability receipt would persist the literal audit endpoint");
    }
    if let Some(host) = Url::parse(&config.audit_rpc_endpoint)
        .ok()
        .and_then(|url| url.host_str().map(str::to_owned))
    {
        if bytes_contain(&receipt_bytes, host.as_bytes()) {
            bail!("provider suitability receipt would persist the literal audit hostname");
        }
    }
    if let Some(token) = connection.auth_token.as_deref() {
        if bytes_contain(&receipt_bytes, token.as_bytes()) {
            bail!("provider suitability receipt would persist the audit credential");
        }
    }
    if let Some(path_credential) = connection.endpoint_path_credential.as_deref() {
        if bytes_contain(&receipt_bytes, path_credential.as_bytes()) {
            bail!("provider suitability receipt would persist the audit endpoint-path credential");
        }
    }
    write_json_create_new_and_sync(&receipt_path, &receipt_bytes)?;
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&receipt_path, fs::Permissions::from_mode(0o600))?;
    }
    File::open(&partial_dir)?.sync_all()?;
    fs::rename(&partial_dir, output_dir).with_context(|| {
        format!(
            "publish provider suitability output {} -> {}",
            partial_dir.display(),
            output_dir.display()
        )
    })?;
    File::open(parent)?.sync_all()?;
    Ok(output_dir.join(PROVIDER_SUITABILITY_RECEIPT_FILE_V1))
}

/// Run the bounded GO-E0 provider-capability check. It reads and verifies the
/// closed raw tape, but never creates an exact output and never writes raw.
#[allow(clippy::too_many_arguments)]
pub async fn probe_pump_research_qualification_provider_v1(
    run_dir: &Path,
    audit_config_path: &Path,
    preparation_receipt_path: &Path,
    expected_preparation_sha256: &str,
    output_dir: &Path,
) -> Result<PumpResearchProviderSuitabilitySummaryV1> {
    let running_executable_authority = capture_running_executable_authority_v1()?;
    let audit_config_authority = read_bounded_authority_file_v1(
        audit_config_path,
        PumpResearchAuthorityFileKindV1::QualificationAuditConfig,
    )?;
    let audit_config = PumpResearchQualificationAuditConfigV1::from_bytes(
        audit_config_path,
        &audit_config_authority.bytes,
    )?;
    audit_config.validate()?;
    if audit_config.bounded_concurrency != 1 {
        bail!("GO-E0 requires bounded_concurrency = 1");
    }
    let raw = index_pump_research_raw_run_v1(run_dir)?;
    if audit_config.audit_provider_id == raw.start_manifest.primary_provider_id {
        bail!("GO-E0 audit provider must differ from the primary provider");
    }
    let audit_config_digest = audit_config_authority.digest;
    let validated_preparation = validate_provider_suitability_preparation_v1(
        preparation_receipt_path,
        expected_preparation_sha256,
        &audit_config_digest,
        &audit_config.audit_provider_id,
        &raw,
        run_dir,
    )?;
    let canonical_output_dir = validate_provider_suitability_output_path_v1(
        run_dir,
        &validated_preparation.planned_exact_output,
        output_dir,
    )?;
    let preparation_receipt_digest = validated_preparation.digest;
    let executable_digest = running_executable_authority.digest().clone();
    let current_raw_controls =
        raw.revalidate_raw_control_authority_v1(run_dir, "GO-E0-authority-validation")?;
    let raw_binding_digest = current_raw_controls
        .provenance_binding_digest
        .ok_or_else(|| anyhow::anyhow!("GO-E0 lacks a raw provenance binding digest"))?;
    let raw_start_manifest_digest = current_raw_controls.start_manifest_digest;
    let raw_completion_receipt_digest = current_raw_controls.completion_receipt_digest;
    let canonicality = build_slot_canonicality_index(&raw);
    let plan = build_provider_suitability_plan_v1(&raw, &canonicality)?;
    let sample_slots: BTreeSet<u64> = plan.slots.keys().copied().collect();
    let raw_by_slot = match plan.qualification_range {
        Some(qualification_range) => collect_provider_suitability_raw_transactions_v1(
            &raw,
            &canonicality,
            qualification_range,
            &sample_slots,
        )?,
        None => BTreeMap::new(),
    };

    let provider_started = Instant::now();
    let provider_deadline = provider_started
        .checked_add(Duration::from_millis(
            PROVIDER_SUITABILITY_MAX_PROVIDER_WALL_MS_V1,
        ))
        .ok_or_else(|| anyhow::anyhow!("GO-E0 provider deadline overflow"))?;
    let timeout = Duration::from_millis(audit_config.request_timeout_ms);
    let mut provider_io_performed = false;
    let mut attempted_slot_count = 0_usize;
    let mut matched_slot_count = 0_usize;
    let mut unavailable_slot_count = 0_usize;
    let mut total_request_attempt_count = 0_u64;
    let mut consecutive_unavailable = 0_u32;
    let mut saw_sample_mismatch = false;
    let mut findings = Vec::with_capacity(plan.slots.len());
    let resolved_connection = audit_config.resolve_connection()?;
    let client = if plan.qualification_range.is_some() {
        Some(qualification_audit_rpc_client_with_connection_v1(
            &audit_config,
            timeout,
            &resolved_connection,
        )?)
    } else {
        None
    };

    for (slot, roles) in &plan.slots {
        let raw_transactions = raw_by_slot.get(slot).cloned().unwrap_or_default();
        let (raw_count, raw_failed, raw_classes) = audit_transaction_counts(&raw_transactions);
        let stop_status =
            if consecutive_unavailable >= PROVIDER_SUITABILITY_MAX_CONSECUTIVE_UNAVAILABLE_V1 {
                Some(PumpResearchProviderSuitabilityFetchStatusV1::NotAttemptedCircuitBreaker)
            } else if Instant::now() >= provider_deadline {
                Some(PumpResearchProviderSuitabilityFetchStatusV1::NotAttemptedWallDeadline)
            } else {
                None
            };
        if let Some(fetch_status) = stop_status {
            unavailable_slot_count = unavailable_slot_count.saturating_add(1);
            findings.push(PumpResearchProviderSuitabilitySlotFindingV1 {
                schema_version: 1,
                slot: *slot,
                selection_roles: roles.iter().cloned().collect(),
                fetch_status,
                request_attempt_count: 0,
                request_elapsed_ms: 0,
                raw_identity_count: raw_count,
                audit_identity_count: 0,
                raw_failed_transaction_count: raw_failed,
                audit_failed_transaction_count: 0,
                raw_invocation_class_counts: raw_classes,
                audit_invocation_class_counts: empty_invocation_class_counts(),
                raw_only_identities: sorted_audit_identities(
                    raw_transactions
                        .iter()
                        .map(|transaction| transaction.identity),
                ),
                audit_only_identities: Vec::new(),
                identity_multiset_matches: false,
                invocation_class_counts_match: false,
                failed_status_multiset_matches: false,
                audit_error: Some(match fetch_status {
                    PumpResearchProviderSuitabilityFetchStatusV1::NotAttemptedWallDeadline => {
                        "GO-E0 bounded provider wall deadline stopped further requests".to_owned()
                    }
                    _ => "GO-E0 bounded circuit breaker stopped further requests".to_owned(),
                }),
            });
            continue;
        }

        let audit_client = client.as_ref().ok_or_else(|| {
            anyhow::anyhow!("GO-E0 sample slots exist without an audit RPC client")
        })?;
        let metrics = fetch_finalized_audit_slot_with_metrics_v1(
            Arc::clone(audit_client),
            *slot,
            audit_config.bounded_retry_count,
            timeout,
            Some(provider_deadline),
        )
        .await;
        provider_io_performed = true;
        attempted_slot_count = attempted_slot_count.saturating_add(1);
        total_request_attempt_count =
            total_request_attempt_count.saturating_add(u64::from(metrics.attempt_count));
        let mut finding = PumpResearchProviderSuitabilitySlotFindingV1 {
            schema_version: 1,
            slot: *slot,
            selection_roles: roles.iter().cloned().collect(),
            fetch_status: PumpResearchProviderSuitabilityFetchStatusV1::Unavailable,
            request_attempt_count: metrics.attempt_count,
            request_elapsed_ms: metrics.elapsed_ms,
            raw_identity_count: raw_count,
            audit_identity_count: 0,
            raw_failed_transaction_count: raw_failed,
            audit_failed_transaction_count: 0,
            raw_invocation_class_counts: raw_classes.clone(),
            audit_invocation_class_counts: empty_invocation_class_counts(),
            raw_only_identities: sorted_audit_identities(
                raw_transactions
                    .iter()
                    .map(|transaction| transaction.identity),
            ),
            audit_only_identities: Vec::new(),
            identity_multiset_matches: false,
            invocation_class_counts_match: false,
            failed_status_multiset_matches: false,
            audit_error: None,
        };
        match metrics.result {
            PumpResearchAuditSlotFetchV1::Block(audit_transactions) => {
                consecutive_unavailable = 0;
                let comparison =
                    compare_audit_transaction_multisets_v1(&raw_transactions, &audit_transactions);
                finding.fetch_status = PumpResearchProviderSuitabilityFetchStatusV1::Block;
                finding.raw_identity_count = comparison.raw_identity_count;
                finding.audit_identity_count = comparison.audit_identity_count;
                finding.raw_failed_transaction_count = comparison.raw_failed_transaction_count;
                finding.audit_failed_transaction_count = comparison.audit_failed_transaction_count;
                finding.raw_invocation_class_counts = comparison.raw_invocation_class_counts;
                finding.audit_invocation_class_counts = comparison.audit_invocation_class_counts;
                finding.raw_only_identities = comparison.raw_only_identities;
                finding.audit_only_identities = comparison.audit_only_identities;
                finding.identity_multiset_matches = comparison.identity_multiset_matches;
                finding.invocation_class_counts_match = comparison.invocation_class_counts_match;
                finding.failed_status_multiset_matches = comparison.failed_status_multiset_matches;
                if finding.identity_multiset_matches
                    && finding.invocation_class_counts_match
                    && finding.failed_status_multiset_matches
                {
                    matched_slot_count = matched_slot_count.saturating_add(1);
                } else {
                    saw_sample_mismatch = true;
                }
            }
            PumpResearchAuditSlotFetchV1::Skipped => {
                consecutive_unavailable = 0;
                finding.fetch_status = PumpResearchProviderSuitabilityFetchStatusV1::Skipped;
                finding.identity_multiset_matches = raw_transactions.is_empty();
                finding.invocation_class_counts_match =
                    raw_classes == empty_invocation_class_counts();
                finding.failed_status_multiset_matches = raw_transactions.is_empty();
                if finding.identity_multiset_matches
                    && finding.invocation_class_counts_match
                    && finding.failed_status_multiset_matches
                {
                    matched_slot_count = matched_slot_count.saturating_add(1);
                } else {
                    saw_sample_mismatch = true;
                }
            }
            PumpResearchAuditSlotFetchV1::Unavailable(error) => {
                consecutive_unavailable = consecutive_unavailable.saturating_add(1);
                unavailable_slot_count = unavailable_slot_count.saturating_add(1);
                finding.audit_error = Some(redacted_audit_error(
                    &audit_config,
                    &resolved_connection,
                    &error,
                ));
            }
        }
        findings.push(finding);
    }

    let status = if plan.qualification_range.is_none() {
        PumpResearchProviderSuitabilityStatusV1::BlockedNoQualificationRange
    } else if unavailable_slot_count > 0 || attempted_slot_count < plan.slots.len() {
        PumpResearchProviderSuitabilityStatusV1::BlockedAuditUnavailable
    } else if saw_sample_mismatch {
        PumpResearchProviderSuitabilityStatusV1::BlockedSampleMismatch
    } else if !plan.missing_raw_representative_roles.is_empty() {
        PumpResearchProviderSuitabilityStatusV1::BlockedMissingRawRepresentative
    } else {
        PumpResearchProviderSuitabilityStatusV1::ReadyForFullAudit
    };
    let provider_elapsed_ms = duration_millis_u64(provider_started.elapsed());
    let receipt = PumpResearchProviderSuitabilityReceiptV1 {
        schema_version: 1,
        kind: "pump_research_provider_suitability_v1".to_owned(),
        created_wall_ms: wall_clock_ms_v1()?,
        source_run_id: raw.start_manifest.run_id.clone(),
        status,
        preparation_receipt_digest: preparation_receipt_digest.clone(),
        audit_config_digest: audit_config_digest.clone(),
        executable_digest: executable_digest.clone(),
        raw_binding_digest: raw_binding_digest.clone(),
        raw_start_manifest_digest: raw_start_manifest_digest.clone(),
        raw_completion_receipt_digest: raw_completion_receipt_digest.clone(),
        audit_provider_id: audit_config.audit_provider_id.clone(),
        audit_rpc_endpoint_blake3: blake3::hash(resolved_connection.endpoint.as_bytes())
            .to_hex()
            .to_string(),
        audit_auth_mode: qualification_audit_auth_mode_v1(&audit_config).to_owned(),
        provider_identity_independence_verified: false,
        qualification_stream_epoch: plan.qualification_range.map(|range| range.stream_epoch),
        qualification_start_slot: plan.qualification_range.map(|range| range.start_slot),
        qualification_end_slot: plan.qualification_range.map(|range| range.end_slot),
        qualification_blocker: plan.qualification_blocker,
        bounded_concurrency: audit_config.bounded_concurrency,
        bounded_retry_count: audit_config.bounded_retry_count,
        request_timeout_ms: audit_config.request_timeout_ms,
        burst_slot_target_count: PROVIDER_SUITABILITY_BURST_SLOT_COUNT_V1,
        max_raw_representative_scan: PROVIDER_SUITABILITY_MAX_RAW_REPRESENTATIVE_SCAN_V1,
        max_consecutive_unavailable: PROVIDER_SUITABILITY_MAX_CONSECUTIVE_UNAVAILABLE_V1,
        max_provider_wall_ms: PROVIDER_SUITABILITY_MAX_PROVIDER_WALL_MS_V1,
        raw_representative_transactions_examined: plan.raw_representative_transactions_examined,
        missing_raw_representative_roles: plan.missing_raw_representative_roles,
        sample_slot_count: plan.slots.len(),
        attempted_slot_count,
        matched_slot_count,
        unavailable_slot_count,
        total_request_attempt_count,
        provider_elapsed_ms,
        slot_findings: findings,
        provider_io_performed,
        raw_write_attempt_count: 0,
        exact_output_created: false,
        certify_started: false,
        export_started: false,
        strategy_started: false,
    };
    let raw_binding_path = run_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1);
    let raw_start_manifest_path = run_dir.join("run_start_manifest.json");
    let raw_completion_receipt_path = run_dir.join("run_completion_receipt.json");
    let stable_inputs = [
        (
            "qualification preparation snapshot",
            preparation_receipt_path,
            &preparation_receipt_digest,
            PumpResearchAuthorityFileKindV1::QualificationPreparationReceipt,
        ),
        (
            "qualification audit config",
            audit_config_path,
            &audit_config_digest,
            PumpResearchAuthorityFileKindV1::QualificationAuditConfig,
        ),
        (
            "raw provenance binding",
            raw_binding_path.as_path(),
            &raw_binding_digest,
            PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
        ),
        (
            "raw start manifest",
            raw_start_manifest_path.as_path(),
            &raw_start_manifest_digest,
            PumpResearchAuthorityFileKindV1::RawStartManifest,
        ),
        (
            "raw completion receipt",
            raw_completion_receipt_path.as_path(),
            &raw_completion_receipt_digest,
            PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
        ),
    ];
    for (label, path, expected, kind) in stable_inputs {
        let current = digest_bounded_authority_file_v1(path, kind)?;
        if &current != expected {
            bail!("{label} changed during the GO-E0 provider probe");
        }
    }
    running_executable_authority.revalidate_v1("after GO-E0 provider probe")?;
    if validated_preparation.planned_exact_output.exists() {
        bail!(
            "planned exact output {} appeared during the GO-E0 provider probe",
            validated_preparation.planned_exact_output.display()
        );
    }
    let receipt_path = publish_provider_suitability_receipt_v1(
        &canonical_output_dir,
        &receipt,
        &audit_config,
        &resolved_connection,
    )?;
    Ok(PumpResearchProviderSuitabilitySummaryV1 {
        source_run_id: receipt.source_run_id,
        status,
        output_dir: canonical_output_dir,
        receipt_path,
        sample_slot_count: receipt.sample_slot_count,
        attempted_slot_count,
        matched_slot_count,
        unavailable_slot_count,
        total_request_attempt_count,
        provider_elapsed_ms,
    })
}

/// Certify one complete raw run into standalone JSONL output.  The materialiser
/// is deliberately synchronous and offline: it opens no RPC/Yellowstone
/// client, never writes raw segments, and does not enter the active Seer
/// runtime.
pub fn certify_pump_research_raw_run_v1(
    run_dir: &Path,
    output_dir: &Path,
) -> Result<PumpResearchCertificationSummaryV1> {
    let raw = index_pump_research_raw_run_v1(run_dir)?;
    let canonicality = build_slot_canonicality_index(&raw);
    certify_indexed_pump_research_raw_run_v1(raw, canonicality, None, None, None, output_dir)
}

/// Materialize the operator-approved frozen GO-D tape without consulting an
/// external RPC. The hash-pinned source authority can promote only the exact
/// run/control/segment set it names, and the same anonymous snapshot bytes are
/// used from indexing through final publication.
pub fn certify_pump_research_verified_go_d_v1(
    run_dir: &Path,
    output_dir: &Path,
    source_authority_path: &Path,
    expected_source_authority_sha256: &str,
) -> Result<PumpResearchCertificationSummaryV1> {
    let canonical_output = validate_combined_exact_output_path_v1(run_dir, output_dir)?;
    let mut raw = index_pump_research_raw_run_v1(run_dir)?;
    raw.seal_raw_segment_set_snapshot_v1(&canonical_output)?;
    let source_authority = validate_go_d_source_authority_v1(
        &raw,
        run_dir,
        source_authority_path,
        expected_source_authority_sha256,
    )?;
    let canonicality = build_slot_canonicality_index(&raw);
    certify_indexed_pump_research_raw_run_v1(
        raw,
        canonicality,
        None,
        None,
        Some(&source_authority),
        &canonical_output,
    )
}

/// Certify with the optional independent, read-only source-completeness audit.
/// The audit is deliberately performed before any exact output directory is
/// created, so an unavailable or malformed external source cannot leave a
/// half-published exact artifact behind.
pub async fn certify_pump_research_raw_run_with_qualification_audit_v1(
    run_dir: &Path,
    output_dir: &Path,
    audit_config_path: &Path,
    provider_suitability_receipt_path: &Path,
    provider_independence_attestation_path: &Path,
    expected_provider_independence_sha256: &str,
) -> Result<PumpResearchCertificationSummaryV1> {
    // Pin the kernel-mapped executable image before any raw indexing. Every
    // later executable check rehashes this same descriptor, never a mutable
    // pathname returned by `env::current_exe()`.
    let running_executable_authority = capture_running_executable_authority_v1()?;
    // Validate the immutable-raw/output boundary before indexing and before
    // allocating any private snapshot path. A malformed CLI output can never
    // create even a temporary entry beneath raw evidence.
    let canonical_output = validate_combined_exact_output_path_v1(run_dir, output_dir)?;
    let mut raw = index_pump_research_raw_run_v1(run_dir)?;
    // Bind the hash-pinned attestation to this exact create-new output without
    // resolving endpoint credentials and before the 6.45 GB-class snapshot
    // copy. Full authority validation still runs again after sealing.
    validate_attested_output_before_snapshot_v1(
        &raw.start_manifest.run_id,
        &canonical_output,
        provider_independence_attestation_path,
        expected_provider_independence_sha256,
        &running_executable_authority,
    )?;
    raw.seal_raw_segment_set_snapshot_v1(&canonical_output)?;
    let canonicality = build_slot_canonicality_index(&raw);
    let authority = validate_provider_independence_attestation_v1(
        &raw,
        &canonicality,
        &canonical_output,
        audit_config_path,
        provider_suitability_receipt_path,
        provider_independence_attestation_path,
        expected_provider_independence_sha256,
        running_executable_authority,
    )?;
    let audit =
        run_independent_source_completeness_audit_v1(&raw, &canonicality, &authority).await?;
    certify_indexed_pump_research_raw_run_v1(
        raw,
        canonicality,
        Some(audit),
        Some(&authority.provider_independence),
        None,
        &canonical_output,
    )
}

/// Preserve the distinction between evidence materialisation and promotion.
/// A source-completeness audit can describe an old raw run, but cannot repair
/// a capture whose sealed preflight/binding predates the current provenance
/// contract. This order is intentionally fail-closed and is shared by normal
/// certify and certify-with-audit.
fn qualification_status_with_capture_provenance_v1(
    capture_provenance: PumpResearchCaptureProvenanceEligibilityV1,
    program_version_boundary: bool,
    audit_status: Option<PumpResearchTapeQualificationStatusV1>,
    verified_go_d_source_authority: bool,
) -> PumpResearchTapeQualificationStatusV1 {
    if !matches!(
        capture_provenance,
        PumpResearchCaptureProvenanceEligibilityV1::Eligible
    ) {
        return PumpResearchTapeQualificationStatusV1::Blocked(
            PumpResearchQualificationBlockerV1::CaptureProvenanceUnqualified,
        );
    }
    if program_version_boundary {
        return PumpResearchTapeQualificationStatusV1::Blocked(
            PumpResearchQualificationBlockerV1::ProgramVersionBoundary,
        );
    }
    if verified_go_d_source_authority {
        return PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape;
    }
    audit_status.unwrap_or(PumpResearchTapeQualificationStatusV1::Unqualified)
}

fn certify_indexed_pump_research_raw_run_v1(
    raw: PumpResearchRawTapeIndexV1,
    canonicality: PumpResearchSlotCanonicalityIndexV1,
    qualification: Option<PumpResearchQualificationResultV1>,
    provider_independence: Option<&PumpResearchValidatedProviderIndependenceV1>,
    go_d_source_authority: Option<&PumpResearchValidatedGoDSourceAuthorityV1>,
    output_dir: &Path,
) -> Result<PumpResearchCertificationSummaryV1> {
    certify_indexed_pump_research_raw_run_with_final_check_hook_v1(
        raw,
        canonicality,
        qualification,
        provider_independence,
        go_d_source_authority,
        output_dir,
        |_| Ok(()),
    )
}

fn certify_indexed_pump_research_raw_run_with_final_check_hook_v1<F>(
    raw: PumpResearchRawTapeIndexV1,
    canonicality: PumpResearchSlotCanonicalityIndexV1,
    mut qualification: Option<PumpResearchQualificationResultV1>,
    provider_independence: Option<&PumpResearchValidatedProviderIndependenceV1>,
    go_d_source_authority: Option<&PumpResearchValidatedGoDSourceAuthorityV1>,
    output_dir: &Path,
    before_final_raw_check: F,
) -> Result<PumpResearchCertificationSummaryV1>
where
    F: FnOnce(&PumpResearchRawTapeIndexV1) -> Result<()>,
{
    if qualification.is_some() != provider_independence.is_some() {
        bail!("qualified certification requires a validated provider-independence authority");
    }
    if go_d_source_authority.is_some() && qualification.is_some() {
        bail!("GO-D frozen-tape authority and retired GO-E audit authority are mutually exclusive");
    }
    if let Some(result) = &qualification {
        let raw_segment_set_blake3 = raw.raw_segment_set_blake3_v1().ok_or_else(|| {
            anyhow::anyhow!("qualified exact output lacks raw segment-set authority")
        })?;
        if result.report.raw_segment_set_blake3 != raw_segment_set_blake3 {
            bail!("qualification report raw segment-set digest differs from exact authority");
        }
    }
    if raw.has_raw_segment_set_authority_v1() {
        raw.revalidate_raw_segment_set_v1("after-provider-audit-before-account-anchors")?;
    }
    let anchors = build_account_anchor_index(&raw, &canonicality)?;
    let program_version_boundary =
        raw.completion_receipt.status == PumpResearchRunCompletionStatusV1::ProgramVersionBoundary;
    if let Some(provider_independence) = provider_independence {
        let audit_endpoint_blake3 = qualification
            .as_ref()
            .map(|result| result.report.audit_rpc_endpoint_blake3.as_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "provider-independence authority exists without a qualification audit result"
                )
            })?;
        provider_independence
            .revalidate_before_exact_output_v1(output_dir, audit_endpoint_blake3)?;
    }
    let raw_dir = raw
        .segments
        .first()
        .and_then(|segment| segment.path.parent())
        .ok_or_else(|| anyhow::anyhow!("indexed raw tape has no segment parent directory"))?;
    if let Some(go_d_source_authority) = go_d_source_authority {
        if !raw.has_raw_segment_set_authority_v1() {
            bail!("verified GO-D exact output lacks a sealed raw segment-set authority");
        }
        go_d_source_authority.revalidate_v1(&raw, raw_dir, "before-GO-D-exact-output")?;
    }
    let mut output = PumpResearchExactOutputWriterV1::create(output_dir)?;

    let mut summary = PumpResearchCertificationSummaryV1 {
        source_run_id: raw.start_manifest.run_id.clone(),
        output_dir: output_dir.to_path_buf(),
        ..PumpResearchCertificationSummaryV1::default()
    };
    for (slot, status) in &canonicality.by_slot {
        match status {
            PumpSlotCanonicalityV1::RootedCanonical => {
                summary.rooted_canonical_slots = summary.rooted_canonical_slots.saturating_add(1)
            }
            PumpSlotCanonicalityV1::Dead => {
                summary.dead_fork_slots = summary.dead_fork_slots.saturating_add(1)
            }
            PumpSlotCanonicalityV1::Unresolved => {
                summary.unresolved_slots = summary.unresolved_slots.saturating_add(1)
            }
        }
        output.write_coverage(&PumpResearchCoverageStatusV1 {
            schema_version: 1,
            kind: PumpResearchCoverageRecordKindV1::Slot,
            slot: *slot,
            signature: None,
            bonding_curve: None,
            canonicality: *status,
            certification: None,
            mutation_count: 0,
        })?;
    }

    let parser = BinaryParser::new(false);
    for indexed in &raw.transactions {
        summary.transaction_count = summary.transaction_count.saturating_add(1);
        let transaction = raw.read_transaction(indexed)?;
        let event = decode_research_raw_transaction_v1(
            &transaction.source_payload,
            transaction.slot,
            transaction.block_time,
            &transaction.source.provider_id,
            transaction.event_time,
        )
        .map_err(|error| anyhow::anyhow!(error))
        .with_context(|| {
            format!(
                "replay raw transaction at capture sequence {}",
                transaction.source.capture_sequence
            )
        })?;
        validate_replayed_transaction_identity(indexed, &event)?;
        let mut inventory = parser
            .parse_research_mutation_inventory_v1(&event)
            .map_err(|error| anyhow::anyhow!(error))
            .with_context(|| {
                format!(
                    "parse research mutation inventory at capture sequence {}",
                    transaction.source.capture_sequence
                )
            })?;
        if !replay_transaction_through_observation_ledger(&inventory, &transaction, &event)? {
            inventory.inventory_complete = false;
        }
        let mut trajectories = materialize_transaction_trajectories(
            &raw,
            indexed,
            &transaction,
            &event,
            &inventory,
            &anchors,
            &canonicality,
            program_version_boundary,
        )?;
        let participant_balances =
            participant_balance_evidence_by_locator(&transaction.source_payload, &inventory);
        for trajectory in &mut trajectories {
            for mutation in &mut trajectory.mutations {
                if let Some(evidence) = participant_balances.get(&mutation.locator) {
                    apply_participant_balance_evidence(mutation, evidence);
                }
            }
        }
        if trajectories.is_empty() && !inventory.mutations.is_empty() {
            output.write_coverage(&PumpResearchCoverageStatusV1 {
                schema_version: 1,
                kind: PumpResearchCoverageRecordKindV1::TransactionWithoutTrajectory,
                slot: transaction.slot,
                signature: Some(transaction.signature),
                bonding_curve: None,
                canonicality: canonicality.classify(transaction.slot),
                certification: Some(PumpTrajectoryCertificationV1::NonEvaluable(
                    PumpNonEvaluableReasonV1::IncompleteMutationInventory,
                )),
                mutation_count: u32::try_from(inventory.mutations.len()).unwrap_or(u32::MAX),
            })?;
        }
        for trajectory in trajectories.drain(..) {
            let mutation_count = u32::try_from(trajectory.mutations.len()).unwrap_or(u32::MAX);
            let rooted =
                canonicality.classify(trajectory.slot) == PumpSlotCanonicalityV1::RootedCanonical;
            let successful_mutations = trajectory
                .mutations
                .iter()
                .filter(|mutation| mutation.success)
                .count() as u64;
            if rooted {
                summary.successful_rooted_mutation_count = summary
                    .successful_rooted_mutation_count
                    .saturating_add(successful_mutations);
                if trajectory.certification == PumpTrajectoryCertificationV1::Exact {
                    summary.exact_rooted_mutation_count = summary
                        .exact_rooted_mutation_count
                        .saturating_add(successful_mutations);
                }
            }
            summary.trajectory_count = summary.trajectory_count.saturating_add(1);
            if trajectory.certification == PumpTrajectoryCertificationV1::Exact {
                summary.exact_trajectory_count = summary.exact_trajectory_count.saturating_add(1);
            }
            if let Some(birth) = birth_from_trajectory(&trajectory, &inventory) {
                output.write_birth(&birth)?;
                summary.birth_count = summary.birth_count.saturating_add(1);
            }
            output.write_coverage(&PumpResearchCoverageStatusV1 {
                schema_version: 1,
                kind: PumpResearchCoverageRecordKindV1::Trajectory,
                slot: trajectory.slot,
                signature: Some(trajectory.signature),
                bonding_curve: Some(trajectory.bonding_curve),
                canonicality: canonicality.classify(trajectory.slot),
                certification: Some(trajectory.certification),
                mutation_count,
            })?;
            output.write_trajectory(&trajectory)?;
        }
    }

    let qualification_status = qualification_status_with_capture_provenance_v1(
        raw.capture_provenance_eligibility,
        program_version_boundary,
        qualification.as_ref().map(|result| result.status),
        go_d_source_authority.is_some(),
    );
    summary.qualification_status = qualification_status;
    if let Some(result) = qualification.as_mut() {
        result.report.exact_rooted_mutation_numerator = summary.exact_rooted_mutation_count;
        result.report.exact_rooted_mutation_denominator = summary.successful_rooted_mutation_count;
        result.report.global_dependency_anchor_count =
            u64::try_from(anchors.global.len()).unwrap_or(u64::MAX);
        result.report.status = qualification_status;
        output.write_qualification(result)?;
    }
    if let Some(go_d_source_authority) = go_d_source_authority {
        output.write_go_d_source_authority(&go_d_source_authority.report_v1())?;
    }
    let manifest = exact_manifest_from_raw(&raw, qualification_status, go_d_source_authority)?;
    before_final_raw_check(&raw)?;
    output.finish(&manifest, || {
        if raw.has_raw_segment_set_authority_v1() {
            raw.revalidate_raw_segment_set_v1("after-materialization-before-exact-publish")?;
        }
        if let Some(provider_independence) = provider_independence {
            let audit_endpoint_blake3 = qualification
                .as_ref()
                .map(|result| result.report.audit_rpc_endpoint_blake3.as_str())
                .ok_or_else(|| {
                    anyhow::anyhow!(
                        "provider-independence authority exists without final audit result"
                    )
                })?;
            provider_independence
                .revalidate_before_exact_output_v1(output_dir, audit_endpoint_blake3)?;
        }
        if let Some(go_d_source_authority) = go_d_source_authority {
            go_d_source_authority.revalidate_v1(
                &raw,
                raw_dir,
                "after-materialization-before-GO-D-exact-publish",
            )?;
        }
        Ok(())
    })?;
    Ok(summary)
}

fn validate_replayed_transaction_identity(
    indexed: &PumpResearchIndexedTransactionV1,
    event: &GeyserEvent,
) -> Result<()> {
    let GeyserEvent::Transaction {
        signature,
        slot,
        tx_index,
        ..
    } = event
    else {
        bail!("offline transaction decoder returned a non-transaction event");
    };
    if *signature != Signature::from(indexed.signature.into_inner())
        || *slot != Some(indexed.slot)
        || *tx_index != indexed.tx_index
    {
        bail!("offline replay identity differs from frozen transaction evidence");
    }
    Ok(())
}

/// Replay the existing structural authority with the research inventory.  It
/// cannot certify reserve economics, but it guards against accidentally
/// treating a parser-only row as canonical primary raw evidence.
fn replay_transaction_through_observation_ledger(
    inventory: &PumpResearchTransactionMutationInventoryV1,
    transaction: &PumpPrimaryTransactionEvidenceV1,
    event: &GeyserEvent,
) -> Result<bool> {
    let (success, error) = transaction_execution_status(event)?;
    let declared_count = u32::try_from(inventory.mutations.len()).ok();
    let mut ledger = PumpObservationLedgerV1::default();
    for entry in &inventory.mutations {
        let family = match entry.kind {
            PumpMutationKindV1::Create => PumpMutationFamilyV1::InitializePool,
            PumpMutationKindV1::Trade => PumpMutationFamilyV1::Trade,
            _ => continue,
        };
        let (Some(locator), Some(order)) = (entry.locator.clone(), entry.order.clone()) else {
            return Ok(false);
        };
        let instruction_limit = match (entry.side, entry.instruction_limit_lamports) {
            (Some(ghost_core::PumpTradeSideV1::Buy), Some(limit)) => {
                Some(PumpInstructionLimitV1::MaxWalletDebitLamports(limit))
            }
            (Some(ghost_core::PumpTradeSideV1::Sell), Some(limit)) => {
                Some(PumpInstructionLimitV1::MinWalletCreditLamports(limit))
            }
            _ => None,
        };
        let observation = ObservedPumpMutationV1 {
            mutation_family: family,
            signature: Signature::from(transaction.signature.into_inner()),
            locator_hint: Some(locator),
            canonical_order: Some(order),
            raw_transaction_mutation_count: declared_count,
            claims: PumpMutationClaimsV1 {
                curve: entry.bonding_curve,
                mint: entry.mint,
                side: entry.side,
                success: Some(success),
                error_code: error.clone(),
                token_amount_units: entry.token_amount_units,
                instruction_limit,
                ..PumpMutationClaimsV1::default()
            },
            raw_provider_role: Some(RawProviderRoleV1::PrimaryAuthority),
            provenance: ObservationProvenanceV1 {
                source_family: ObservationSourceFamilyV1::RawYellowstone,
                source_id: "pump_research_tape_v1_offline_replay".to_owned(),
                provider_id: transaction.source.provider_id.clone(),
                schema_id: "yellowstone_subscribe_update_transaction_v1".to_owned(),
                payload_hash_blake3: transaction.source.payload_hash_blake3.into_inner(),
                received_at_monotonic_ns: 0,
            },
        };
        let decision = ledger.observe(observation, 0);
        if !decision.observation_decision.did_canonical_apply() {
            return Ok(false);
        }
    }
    let snapshot = ledger.snapshot();
    let observed_structural = inventory
        .mutations
        .iter()
        .filter(|entry| {
            matches!(
                entry.kind,
                PumpMutationKindV1::Create | PumpMutationKindV1::Trade
            )
        })
        .count();
    Ok(snapshot.primary_evidence_complete
        && u32::try_from(observed_structural).ok() == declared_count)
}

struct PumpResearchExactOutputWriterV1 {
    final_root: PathBuf,
    partial_root: PathBuf,
    births: BufWriter<File>,
    trajectories: BufWriter<File>,
    coverage: BufWriter<File>,
}

impl PumpResearchExactOutputWriterV1 {
    fn create(root: &Path) -> Result<Self> {
        if root.exists() {
            bail!(
                "exact output directory {} already exists; certify never overwrites an artifact",
                root.display()
            );
        }
        let parent = root.parent().ok_or_else(|| {
            anyhow::anyhow!("exact output directory {} has no parent", root.display())
        })?;
        let name = root
            .file_name()
            .and_then(|name| name.to_str())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "exact output directory {} has no UTF-8 final name",
                    root.display()
                )
            })?;
        let partial_root = parent.join(format!(".{name}.partial"));
        if partial_root.exists() {
            bail!(
                "exact partial directory {} already exists; inspect or retain the interrupted artifact before retrying",
                partial_root.display()
            );
        }
        fs::create_dir_all(parent)
            .with_context(|| format!("create exact output parent {}", parent.display()))?;
        fs::create_dir(&partial_root).with_context(|| {
            format!(
                "create exact partial output directory {}",
                partial_root.display()
            )
        })?;
        let write_root = partial_root.clone();
        let open = move |name: &str| -> Result<BufWriter<File>> {
            let path = write_root.join(name);
            Ok(BufWriter::new(
                File::options()
                    .write(true)
                    .create_new(true)
                    .open(&path)
                    .with_context(|| format!("create exact artifact {}", path.display()))?,
            ))
        };
        Ok(Self {
            final_root: root.to_path_buf(),
            partial_root,
            births: open("births_v1.jsonl")?,
            trajectories: open("trajectories_v1.jsonl")?,
            coverage: open("coverage_v1.jsonl")?,
        })
    }

    fn write_birth(&mut self, birth: &PumpBirthEvidenceV1) -> Result<()> {
        write_jsonl(&mut self.births, birth)
    }

    fn write_trajectory(&mut self, trajectory: &PumpTransactionTrajectoryV1) -> Result<()> {
        write_jsonl(&mut self.trajectories, trajectory)
    }

    fn write_coverage(&mut self, coverage: &PumpResearchCoverageStatusV1) -> Result<()> {
        write_jsonl(&mut self.coverage, coverage)
    }

    fn write_qualification(&mut self, result: &PumpResearchQualificationResultV1) -> Result<()> {
        let qualification_dir = self.partial_root.join("qualification");
        fs::create_dir(&qualification_dir).with_context(|| {
            format!(
                "create exact qualification directory {}",
                qualification_dir.display()
            )
        })?;
        let findings_path = qualification_dir.join("source_completeness_v1.jsonl");
        let mut findings = BufWriter::new(
            File::options()
                .write(true)
                .create_new(true)
                .open(&findings_path)
                .with_context(|| {
                    format!("create qualification findings {}", findings_path.display())
                })?,
        );
        for finding in &result.slot_findings {
            write_jsonl(&mut findings, finding)?;
        }
        sync_jsonl_writer(findings)?;
        write_json_create_new_and_sync(
            &qualification_dir.join("qualification_report_v1.json"),
            &serde_json::to_vec_pretty(&result.report).context("serialize qualification report")?,
        )?;
        File::open(&qualification_dir)
            .with_context(|| {
                format!(
                    "open qualification directory {}",
                    qualification_dir.display()
                )
            })?
            .sync_all()
            .with_context(|| {
                format!(
                    "sync qualification directory {}",
                    qualification_dir.display()
                )
            })?;
        Ok(())
    }

    fn write_go_d_source_authority(
        &mut self,
        report: &PumpResearchGoDSourceAuthorityReportV1,
    ) -> Result<()> {
        let authority_dir = self.partial_root.join("authority");
        fs::create_dir(&authority_dir).with_context(|| {
            format!(
                "create exact GO-D authority directory {}",
                authority_dir.display()
            )
        })?;
        write_json_create_new_and_sync(
            &authority_dir.join("go_d_source_authority_v1.json"),
            &serde_json::to_vec_pretty(report).context("serialize GO-D source authority report")?,
        )?;
        File::open(&authority_dir)?.sync_all()?;
        Ok(())
    }

    fn finish<F>(self, manifest: &PumpExactResearchTapeManifestV1, before_publish: F) -> Result<()>
    where
        F: FnOnce() -> Result<()>,
    {
        let Self {
            final_root,
            partial_root,
            births,
            trajectories,
            coverage,
        } = self;
        sync_jsonl_writer(births)?;
        sync_jsonl_writer(trajectories)?;
        sync_jsonl_writer(coverage)?;
        let manifest_path = partial_root.join("manifest.json");
        let manifest_bytes =
            serde_json::to_vec_pretty(manifest).context("serialize exact tape manifest")?;
        let mut manifest_file = File::options()
            .write(true)
            .create_new(true)
            .open(&manifest_path)
            .with_context(|| format!("create exact manifest {}", manifest_path.display()))?;
        manifest_file
            .write_all(&manifest_bytes)
            .with_context(|| format!("write exact manifest {}", manifest_path.display()))?;
        manifest_file.write_all(b"\n")?;
        manifest_file.sync_all()?;
        File::open(&partial_root)?.sync_all()?;
        before_publish()?;
        fs::rename(&partial_root, &final_root).with_context(|| {
            format!(
                "atomically publish exact artifact {} -> {}",
                partial_root.display(),
                final_root.display()
            )
        })?;
        let parent = final_root.parent().ok_or_else(|| {
            anyhow::anyhow!(
                "published exact output {} has no parent",
                final_root.display()
            )
        })?;
        File::open(parent)
            .with_context(|| format!("open exact output parent {}", parent.display()))?
            .sync_all()
            .with_context(|| format!("sync exact output parent {}", parent.display()))?;
        Ok(())
    }
}

fn write_jsonl<T: Serialize>(writer: &mut BufWriter<File>, value: &T) -> Result<()> {
    serde_json::to_writer(&mut *writer, value).context("serialize exact JSONL row")?;
    writer
        .write_all(b"\n")
        .context("write exact JSONL newline")?;
    Ok(())
}

fn sync_jsonl_writer(writer: BufWriter<File>) -> Result<()> {
    let mut file = writer
        .into_inner()
        .map_err(|error| anyhow::anyhow!("flush exact JSONL writer: {}", error.error()))?;
    file.flush()?;
    file.sync_all()?;
    Ok(())
}

fn exact_manifest_from_raw(
    raw: &PumpResearchRawTapeIndexV1,
    qualification_status: PumpResearchTapeQualificationStatusV1,
    go_d_source_authority: Option<&PumpResearchValidatedGoDSourceAuthorityV1>,
) -> Result<PumpExactResearchTapeManifestV1> {
    Ok(PumpExactResearchTapeManifestV1 {
        schema_version: 1,
        source_run_id: raw.start_manifest.run_id.clone(),
        source_storage_format_version: raw.start_manifest.storage_format_version,
        // Independent source-completeness audit is a separate, read-only
        // qualification input. A normal certify is intentionally useful but
        // never claims PUMP_RESEARCH_TAPE_V1_READY. The caller has already
        // given ProgramData version-boundary evidence priority over an audit
        // result, so the manifest cannot claim Ready across an upgrade.
        qualification_status,
        source_raw_segment_set_blake3: raw
            .raw_segment_set_blake3_v1()
            .unwrap_or_default()
            .to_owned(),
        go_d_source_authority: go_d_source_authority
            .map(|_| GO_D_SOURCE_AUTHORITY_VERIFIED_V1.to_owned())
            .unwrap_or_default(),
        external_go_e_audit_not_used_as_gate: go_d_source_authority.is_some(),
        go_d_source_authority_sha256: go_d_source_authority
            .map(|authority| authority.digest.sha256.clone())
            .unwrap_or_default(),
        source_descriptor_sha256: raw
            .start_manifest
            .source_proto_descriptor_hash
            .strip_prefix("sha256:")
            .unwrap_or(&raw.start_manifest.source_proto_descriptor_hash)
            .to_owned(),
        program_start_receipt: program_start_receipt_from_raw(raw),
        program_completion_receipt: program_completion_receipt_from_raw(raw)?,
    })
}

fn program_start_receipt_from_raw(raw: &PumpResearchRawTapeIndexV1) -> PumpProgramDataReceiptV1 {
    PumpProgramDataReceiptV1 {
        pump_program_id: raw.start_manifest.pump_program_id,
        pump_program_account_owner: raw.start_manifest.pump_program_account_owner,
        pump_programdata_pubkey: raw.start_manifest.pump_programdata_pubkey,
        program_data_owner: raw.start_manifest.program_data_owner,
        program_data_hash_algorithm: raw.start_manifest.program_data_hash_algorithm.clone(),
        program_data_hash_blake3: raw.start_manifest.program_data_hash_at_start,
        program_deployment_slot: raw.start_manifest.program_deployment_slot_at_start,
        observed_context_slot: raw.start_manifest.program_observed_context_slot_at_start,
        commitment: raw.start_manifest.program_receipt_commitment.clone(),
    }
}

fn program_completion_receipt_from_raw(
    raw: &PumpResearchRawTapeIndexV1,
) -> Result<Option<PumpProgramDataReceiptV1>> {
    let receipt = &raw.completion_receipt;
    let fields = (
        receipt.pump_program_id_at_completion,
        receipt.pump_program_account_owner_at_completion,
        receipt.pump_programdata_pubkey_at_completion,
        receipt.program_data_owner_at_completion,
        receipt.program_data_hash_at_completion,
        receipt.program_observed_context_slot_at_completion,
        receipt.program_receipt_commitment_at_completion.as_ref(),
    );
    match fields {
        (
            Some(pump_program_id),
            Some(pump_program_account_owner),
            Some(pump_programdata_pubkey),
            Some(program_data_owner),
            Some(program_data_hash_blake3),
            Some(observed_context_slot),
            Some(commitment),
        ) => Ok(Some(PumpProgramDataReceiptV1 {
            pump_program_id,
            pump_program_account_owner,
            pump_programdata_pubkey,
            program_data_owner,
            program_data_hash_algorithm: raw.start_manifest.program_data_hash_algorithm.clone(),
            program_data_hash_blake3,
            program_deployment_slot: receipt.program_deployment_slot_at_completion,
            observed_context_slot,
            commitment: commitment.clone(),
        })),
        (None, None, None, None, None, None, None) => Ok(None),
        _ => bail!("raw completion ProgramData receipt is partially populated"),
    }
}

fn materialize_transaction_trajectories(
    raw: &PumpResearchRawTapeIndexV1,
    indexed: &PumpResearchIndexedTransactionV1,
    transaction: &PumpPrimaryTransactionEvidenceV1,
    event: &GeyserEvent,
    inventory: &PumpResearchTransactionMutationInventoryV1,
    anchors: &PumpResearchAccountAnchorIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    program_version_boundary: bool,
) -> Result<Vec<PumpTransactionTrajectoryV1>> {
    let (success, error) = transaction_execution_status(event)?;
    if transaction.tx_index.is_none() {
        return Ok(Vec::new());
    }
    let signature = Signature::from(transaction.signature.into_inner());
    let mut by_curve: BTreeMap<Pubkey, Vec<PumpResearchMutationInventoryEntryV1>> = BTreeMap::new();
    for entry in &inventory.mutations {
        if let Some(curve) = entry.bonding_curve {
            by_curve.entry(curve).or_default().push(entry.clone());
        }
    }

    let mut trajectories = Vec::with_capacity(by_curve.len());
    for (curve, mut entries) in by_curve {
        entries.sort_by(|left, right| left.order.cmp(&right.order));
        let Some(mint) = entries.first().and_then(|entry| entry.mint) else {
            continue;
        };
        if entries.iter().any(|entry| entry.mint != Some(mint)) {
            if let Some(trajectory) = trajectory_from_entries(
                raw,
                indexed,
                transaction,
                curve,
                mint,
                &entries,
                success,
                error.clone(),
                None,
                None,
                PumpTrajectoryCertificationV1::Conflict(
                    PumpConflictReasonV1::MintCurveIdentityConflict,
                ),
            ) {
                trajectories.push(trajectory);
            }
            continue;
        }
        if let Some(trajectory) = certify_curve_trajectory(
            raw,
            indexed,
            transaction,
            inventory,
            curve,
            mint,
            &entries,
            success,
            error.clone(),
            anchors,
            canonicality,
            program_version_boundary,
            signature,
        )? {
            trajectories.push(trajectory);
        }
    }
    Ok(trajectories)
}

fn transaction_execution_status(event: &GeyserEvent) -> Result<(bool, Option<String>)> {
    match event {
        GeyserEvent::Transaction {
            success,
            error_code,
            ..
        } => Ok((*success, error_code.clone())),
        _ => bail!("research inventory source did not decode to a transaction"),
    }
}

#[allow(clippy::too_many_arguments)]
fn certify_curve_trajectory(
    raw: &PumpResearchRawTapeIndexV1,
    indexed: &PumpResearchIndexedTransactionV1,
    transaction: &PumpPrimaryTransactionEvidenceV1,
    inventory: &PumpResearchTransactionMutationInventoryV1,
    curve: Pubkey,
    mint: Pubkey,
    entries: &[PumpResearchMutationInventoryEntryV1],
    success: bool,
    error: Option<String>,
    anchors: &PumpResearchAccountAnchorIndexV1,
    canonicality: &PumpResearchSlotCanonicalityIndexV1,
    program_version_boundary: bool,
    signature: Signature,
) -> Result<Option<PumpTransactionTrajectoryV1>> {
    let tx_index = transaction
        .tx_index
        .ok_or_else(|| anyhow::anyhow!("curve certification requires a raw transaction index"))?;
    let slot_status = canonicality.classify(transaction.slot);
    let pre_anchor =
        select_curve_pre_anchor(anchors, curve, transaction.slot, transaction.tx_index);
    let final_anchor = select_curve_final_anchor(anchors, signature, curve);

    let immediate = if program_version_boundary {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::ProgramVersionBoundary,
        ))
    } else if slot_status == PumpSlotCanonicalityV1::Dead {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::NonCanonicalFork,
        ))
    } else if slot_status == PumpSlotCanonicalityV1::Unresolved {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::UnresolvedCanonicality,
        ))
    } else if gap_affects_slot(raw, transaction.slot) {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::CoverageGap,
        ))
    } else if !success {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::FailedTransaction,
        ))
    } else if !inventory.inventory_complete {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::IncompleteMutationInventory,
        ))
    } else if inventory.has_unattributed_unknown_mutation
        || entries
            .iter()
            .any(|entry| entry.kind == PumpMutationKindV1::UnknownMutation)
    {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::UnknownMutation,
        ))
    } else if anchors.curve_conflicts.contains(&curve) {
        Some(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::AccountProviderConflict,
        ))
    } else if entries.iter().any(|entry| entry.direct_event_conflict) {
        Some(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::DirectEventStateMismatch,
        ))
    } else if entries.iter().any(|entry| entry.direct_event_ambiguous) {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::AmbiguousOrder,
        ))
    } else if entries
        .iter()
        .any(|entry| entry.locator.is_none() || entry.order.is_none())
    {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::AmbiguousOrder,
        ))
    } else if entries
        .windows(2)
        .any(|pair| pair[0].order.as_ref() >= pair[1].order.as_ref())
    {
        Some(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::AmbiguousOrder,
        ))
    } else {
        None
    };
    if let Some(certification) = immediate {
        return Ok(trajectory_from_entries(
            raw,
            indexed,
            transaction,
            curve,
            mint,
            entries,
            success,
            error,
            pre_anchor.map(|anchor| anchor.anchor.clone()),
            final_anchor.map(|anchor| anchor.anchor.clone()),
            certification,
        ));
    }

    let Some(first) = entries.first() else {
        return Ok(None);
    };
    let starts_with_create = first.kind == PumpMutationKindV1::Create;
    let initial = if starts_with_create {
        genesis_curve_state(raw, anchors, transaction, first)
    } else {
        pre_anchor
            .as_ref()
            .map(|anchor| Ok(anchor.anchor.state))
            .unwrap_or_else(|| {
                Err(PumpTrajectoryCertificationV1::NonEvaluable(
                    PumpNonEvaluableReasonV1::MissingPreAnchor,
                ))
            })
    };
    let mut current = match initial {
        Ok(state) => state,
        Err(certification) => {
            return Ok(trajectory_from_entries(
                raw,
                indexed,
                transaction,
                curve,
                mint,
                entries,
                success,
                error,
                pre_anchor.map(|anchor| anchor.anchor.clone()),
                final_anchor.map(|anchor| anchor.anchor.clone()),
                certification,
            ));
        }
    };
    let Some(final_anchor) = final_anchor else {
        let reason = if anchors.curves.get(&curve).is_some_and(|items| {
            items.iter().any(|anchor| {
                anchor.anchor.slot == transaction.slot && anchor.anchor.txn_signature.is_none()
            })
        }) {
            PumpNonEvaluableReasonV1::MissingFinalTxnSignature
        } else {
            PumpNonEvaluableReasonV1::MissingFinalAnchor
        };
        return Ok(trajectory_from_entries(
            raw,
            indexed,
            transaction,
            curve,
            mint,
            entries,
            success,
            error,
            pre_anchor.map(|anchor| anchor.anchor.clone()),
            None,
            PumpTrajectoryCertificationV1::NonEvaluable(reason),
        ));
    };

    let mut certified = Vec::with_capacity(entries.len());
    for (entry_index, entry) in entries.iter().enumerate() {
        let state_before = if entry_index == 0 && starts_with_create {
            None
        } else {
            Some(current)
        };
        let transition = if entry_index == 0 && starts_with_create {
            // `current` was constructed directly from create evidence or the
            // historical effective Global predecessor; a Create itself has no
            // pre-existing curve account state.
            Ok((current, None))
        } else {
            apply_curve_transition(current, entry)
        };
        let (state_after, curve_quote_lamports) = match transition {
            Ok(value) => value,
            Err(certification) => {
                return Ok(trajectory_from_entries(
                    raw,
                    indexed,
                    transaction,
                    curve,
                    mint,
                    entries,
                    success,
                    error,
                    pre_anchor.map(|anchor| anchor.anchor.clone()),
                    Some(final_anchor.anchor.clone()),
                    certification,
                ));
            }
        };
        if !direct_event_matches(entry, state_after) {
            return Ok(trajectory_from_entries(
                raw,
                indexed,
                transaction,
                curve,
                mint,
                entries,
                success,
                error,
                pre_anchor.map(|anchor| anchor.anchor.clone()),
                Some(final_anchor.anchor.clone()),
                PumpTrajectoryCertificationV1::Conflict(
                    PumpConflictReasonV1::DirectEventStateMismatch,
                ),
            ));
        }
        let Some(mutation) = certified_mutation_from_entry(
            entry,
            success,
            error.clone(),
            state_before,
            Some(state_after),
            curve_quote_lamports,
        ) else {
            return Ok(trajectory_from_entries(
                raw,
                indexed,
                transaction,
                curve,
                mint,
                entries,
                success,
                error,
                pre_anchor.map(|anchor| anchor.anchor.clone()),
                Some(final_anchor.anchor.clone()),
                PumpTrajectoryCertificationV1::NonEvaluable(
                    PumpNonEvaluableReasonV1::AmbiguousOrder,
                ),
            ));
        };
        certified.push(mutation);
        current = state_after;
    }
    if current != final_anchor.anchor.state {
        return Ok(trajectory_from_entries(
            raw,
            indexed,
            transaction,
            curve,
            mint,
            entries,
            success,
            error,
            pre_anchor.map(|anchor| anchor.anchor.clone()),
            Some(final_anchor.anchor.clone()),
            PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::FinalStateMismatch),
        ));
    }

    let source_ref = source_ref_for_transaction(raw, indexed);
    Ok(Some(PumpTransactionTrajectoryV1 {
        source_ref,
        signature: transaction.signature,
        slot: transaction.slot,
        tx_index,
        event_time: transaction.event_time,
        mint: pump_research_storage_pubkey_v1(mint),
        bonding_curve: pump_research_storage_pubkey_v1(curve),
        pre_anchor: pre_anchor.map(|anchor| anchor.anchor.clone()),
        mutations: certified,
        final_anchor: Some(final_anchor.anchor.clone()),
        certification: PumpTrajectoryCertificationV1::Exact,
    }))
}

fn select_curve_pre_anchor(
    anchors: &PumpResearchAccountAnchorIndexV1,
    curve: Pubkey,
    transaction_slot: u64,
    transaction_index: Option<u32>,
) -> Option<&PumpResearchAcceptedCurveAnchorV1> {
    let transaction_index = transaction_index?;
    anchors.curves.get(&curve)?.iter().rev().find(|anchor| {
        anchor.anchor.slot < transaction_slot
            || (anchor.anchor.slot == transaction_slot
                && anchor
                    .source_transaction_index
                    .is_some_and(|anchor_index| anchor_index < transaction_index))
    })
}

fn select_curve_final_anchor(
    anchors: &PumpResearchAccountAnchorIndexV1,
    signature: Signature,
    curve: Pubkey,
) -> Option<&PumpResearchAcceptedCurveAnchorV1> {
    let candidates = anchors.final_by_signature.get(&(signature, curve))?;
    (candidates.len() == 1).then(|| &candidates[0])
}

fn select_global_pre_anchor(
    anchors: &PumpResearchAccountAnchorIndexV1,
    transaction_slot: u64,
    transaction_index: u32,
) -> Option<&PumpResearchAcceptedGlobalAnchorV1> {
    anchors.global.iter().rev().find(|anchor| {
        anchor.slot < transaction_slot
            || (anchor.slot == transaction_slot
                && anchor
                    .source_transaction_index
                    .is_some_and(|anchor_index| anchor_index < transaction_index))
    })
}

fn genesis_curve_state(
    raw: &PumpResearchRawTapeIndexV1,
    anchors: &PumpResearchAccountAnchorIndexV1,
    transaction: &PumpPrimaryTransactionEvidenceV1,
    create: &PumpResearchMutationInventoryEntryV1,
) -> std::result::Result<PumpCurveStateV1, PumpTrajectoryCertificationV1> {
    if let Some(state) = direct_event_complete_state(create) {
        return Ok(state);
    }
    let dependency = pump_transition_dependency_v1(
        create.instruction_variant,
        Some(PumpCreateInitialStateEvidenceV1::RequiresPumpGlobalFallback),
    );
    if dependency != PumpTransitionDependencyV1::PumpGlobal || anchors.global_conflict {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::TransitionDependencyUncaptured,
        ));
    }
    let Some(transaction_index) = transaction.tx_index else {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::AmbiguousOrder,
        ));
    };
    let Some(global) = select_global_pre_anchor(anchors, transaction.slot, transaction_index)
    else {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::TransitionDependencyUncaptured,
        ));
    };
    // The V1 frozen dependency closure permits exactly this program-versioned
    // Create fallback.  Pump Global has no real-quote field; the Pump Create
    // transition begins its curve-side real quote reserve at zero.  The final
    // primary AccountUpdate remains the bit-exact proof of that semantics.
    let state = PumpCurveStateV1 {
        virtual_quote_reserves: global.state.initial_virtual_sol_reserves,
        virtual_token_reserves: global.state.initial_virtual_token_reserves,
        real_quote_reserves: 0,
        real_token_reserves: global.state.initial_real_token_reserves,
        complete: false,
    };
    if !global.state.initialized {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::TransitionDependencyUncaptured,
        ));
    }
    // A direct Create event may supply a partial tuple. It is not a substitute
    // for Global, but every supplied component must agree with this genesis
    // transition.
    if !direct_event_matches(create, state) {
        return Err(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::DirectEventStateMismatch,
        ));
    }
    // Keep the raw argument explicit: no current RPC/global state is ever
    // consulted by this fallback.  This statement also protects against an
    // accidental future replacement of the predecessor selector above.
    let _ = raw;
    Ok(state)
}

fn direct_event_complete_state(
    entry: &PumpResearchMutationInventoryEntryV1,
) -> Option<PumpCurveStateV1> {
    let state = entry.direct_event_state.as_ref()?;
    Some(PumpCurveStateV1 {
        virtual_quote_reserves: state.virtual_quote_reserves?,
        virtual_token_reserves: state.virtual_token_reserves?,
        real_quote_reserves: state.real_quote_reserves?,
        real_token_reserves: state.real_token_reserves?,
        complete: state.complete?,
    })
}

fn direct_event_matches(
    entry: &PumpResearchMutationInventoryEntryV1,
    state: PumpCurveStateV1,
) -> bool {
    let Some(direct) = &entry.direct_event_state else {
        return true;
    };
    direct
        .virtual_quote_reserves
        .is_none_or(|value| value == state.virtual_quote_reserves)
        && direct
            .virtual_token_reserves
            .is_none_or(|value| value == state.virtual_token_reserves)
        && direct
            .real_quote_reserves
            .is_none_or(|value| value == state.real_quote_reserves)
        && direct
            .real_token_reserves
            .is_none_or(|value| value == state.real_token_reserves)
        && direct.complete.is_none_or(|value| value == state.complete)
}

fn apply_curve_transition(
    current: PumpCurveStateV1,
    entry: &PumpResearchMutationInventoryEntryV1,
) -> std::result::Result<(PumpCurveStateV1, Option<u64>), PumpTrajectoryCertificationV1> {
    if !entry.instruction_payload_exact {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::UnsupportedVariant,
        ));
    }
    if entry.kind != PumpMutationKindV1::Trade {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::UnsupportedVariant,
        ));
    }
    let Some(amount) = entry.token_amount_units else {
        return Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::AmbiguousAmount,
        ));
    };
    match entry.instruction_variant {
        PumpInstructionVariantV1::LegacyBuy | PumpInstructionVariantV1::BuyV2 => {
            transition_buy(current, amount)
        }
        PumpInstructionVariantV1::LegacySell | PumpInstructionVariantV1::SellV2 => {
            transition_sell(current, amount)
        }
        PumpInstructionVariantV1::BuyExactQuoteInV2 => Err(
            PumpTrajectoryCertificationV1::NonEvaluable(PumpNonEvaluableReasonV1::AmbiguousAmount),
        ),
        _ => Err(PumpTrajectoryCertificationV1::NonEvaluable(
            PumpNonEvaluableReasonV1::UnsupportedVariant,
        )),
    }
}

fn transition_buy(
    current: PumpCurveStateV1,
    amount: u64,
) -> std::result::Result<(PumpCurveStateV1, Option<u64>), PumpTrajectoryCertificationV1> {
    if amount == 0
        || amount >= current.virtual_token_reserves
        || amount > current.real_token_reserves
    {
        return Err(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ));
    }
    let virtual_token_reserves = current.virtual_token_reserves - amount;
    let invariant = u128::from(current.virtual_quote_reserves)
        .checked_mul(u128::from(current.virtual_token_reserves))
        .ok_or(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ))?;
    let denominator = u128::from(virtual_token_reserves);
    let virtual_quote_reserves_u128 =
        invariant / denominator + u128::from(invariant % denominator != 0);
    let virtual_quote_reserves = u64::try_from(virtual_quote_reserves_u128).map_err(|_| {
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch)
    })?;
    let curve_quote = virtual_quote_reserves
        .checked_sub(current.virtual_quote_reserves)
        .ok_or(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ))?;
    let real_token_reserves = current.real_token_reserves.checked_sub(amount).ok_or(
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch),
    )?;
    let real_quote_reserves = current.real_quote_reserves.checked_add(curve_quote).ok_or(
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch),
    )?;
    Ok((
        PumpCurveStateV1 {
            virtual_quote_reserves,
            virtual_token_reserves,
            real_quote_reserves,
            real_token_reserves,
            complete: current.complete || real_token_reserves == 0,
        },
        Some(curve_quote),
    ))
}

fn transition_sell(
    current: PumpCurveStateV1,
    amount: u64,
) -> std::result::Result<(PumpCurveStateV1, Option<u64>), PumpTrajectoryCertificationV1> {
    if amount == 0 || current.complete {
        return Err(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ));
    }
    let virtual_token_reserves = current.virtual_token_reserves.checked_add(amount).ok_or(
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch),
    )?;
    let invariant = u128::from(current.virtual_quote_reserves)
        .checked_mul(u128::from(current.virtual_token_reserves))
        .ok_or(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ))?;
    let denominator = u128::from(virtual_token_reserves);
    let virtual_quote_reserves_u128 =
        invariant / denominator + u128::from(invariant % denominator != 0);
    let virtual_quote_reserves = u64::try_from(virtual_quote_reserves_u128).map_err(|_| {
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch)
    })?;
    let curve_quote = current
        .virtual_quote_reserves
        .checked_sub(virtual_quote_reserves)
        .ok_or(PumpTrajectoryCertificationV1::Conflict(
            PumpConflictReasonV1::ConservationMismatch,
        ))?;
    let real_token_reserves = current.real_token_reserves.checked_add(amount).ok_or(
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch),
    )?;
    let real_quote_reserves = current.real_quote_reserves.checked_sub(curve_quote).ok_or(
        PumpTrajectoryCertificationV1::Conflict(PumpConflictReasonV1::ConservationMismatch),
    )?;
    Ok((
        PumpCurveStateV1 {
            virtual_quote_reserves,
            virtual_token_reserves,
            real_quote_reserves,
            real_token_reserves,
            complete: false,
        },
        Some(curve_quote),
    ))
}

#[allow(clippy::too_many_arguments)]
fn trajectory_from_entries(
    raw: &PumpResearchRawTapeIndexV1,
    indexed: &PumpResearchIndexedTransactionV1,
    transaction: &PumpPrimaryTransactionEvidenceV1,
    curve: Pubkey,
    mint: Pubkey,
    entries: &[PumpResearchMutationInventoryEntryV1],
    success: bool,
    error: Option<String>,
    pre_anchor: Option<PumpAccountAnchorV1>,
    final_anchor: Option<PumpAccountAnchorV1>,
    certification: PumpTrajectoryCertificationV1,
) -> Option<PumpTransactionTrajectoryV1> {
    let tx_index = transaction.tx_index?;
    let mutations = entries
        .iter()
        .map(|entry| certified_mutation_from_entry(entry, success, error.clone(), None, None, None))
        .collect::<Option<Vec<_>>>()?;
    Some(PumpTransactionTrajectoryV1 {
        source_ref: source_ref_for_transaction(raw, indexed),
        signature: transaction.signature,
        slot: transaction.slot,
        tx_index,
        event_time: transaction.event_time,
        mint: pump_research_storage_pubkey_v1(mint),
        bonding_curve: pump_research_storage_pubkey_v1(curve),
        pre_anchor,
        mutations,
        final_anchor,
        certification,
    })
}

fn certified_mutation_from_entry(
    entry: &PumpResearchMutationInventoryEntryV1,
    success: bool,
    error: Option<String>,
    state_before: Option<PumpCurveStateV1>,
    state_after: Option<PumpCurveStateV1>,
    curve_quote_lamports: Option<u64>,
) -> Option<PumpCertifiedMutationV1> {
    let locator = entry.locator.clone()?;
    let order = entry.order.clone()?;
    Some(PumpCertifiedMutationV1 {
        locator,
        order,
        kind: entry.kind,
        instruction_variant: entry.instruction_variant,
        success,
        error,
        participant: evidence_pubkey(entry.participant, PumpEvidenceSourceV1::WitnessOnly),
        side: entry.side,
        // A trade amount is preserved from the raw instruction tree, not from
        // a Create payload. `WitnessOnly` is deliberately conservative until
        // a future frozen evidence-source enum adds an instruction-payload
        // category; it must never be relabelled as Create evidence.
        token_amount_units: evidence_value(
            entry.token_amount_units,
            PumpEvidenceSourceV1::WitnessOnly,
        ),
        curve_quote_lamports: evidence_value(
            curve_quote_lamports,
            PumpEvidenceSourceV1::ProgramEvent,
        ),
        instruction_limit_lamports: entry.instruction_limit_lamports,
        wallet_quote_delta_lamports: None,
        protocol_fee_lamports: None,
        creator_fee_lamports: None,
        state_before,
        state_after,
        participant_token_account: evidence_pubkey(None, PumpEvidenceSourceV1::TransactionMeta),
        participant_token_balance_before_units: evidence_value(
            None,
            PumpEvidenceSourceV1::TransactionMeta,
        ),
        participant_token_balance_after_units: evidence_value(
            None,
            PumpEvidenceSourceV1::TransactionMeta,
        ),
        participant_balance_scope: ParticipantBalanceScopeV1::Unknown,
        participant_balance_provenance: ParticipantBalanceProvenanceV1::Unknown,
    })
}

fn evidence_value<T>(value: Option<T>, source: PumpEvidenceSourceV1) -> EvidenceValueV1<T> {
    let known = value.is_some();
    EvidenceValueV1 {
        status: if known {
            PumpEvidenceStatusV1::Known
        } else {
            PumpEvidenceStatusV1::Unknown
        },
        value,
        source: known.then_some(source),
    }
}

fn evidence_pubkey(
    value: Option<Pubkey>,
    source: PumpEvidenceSourceV1,
) -> EvidenceValueV1<PumpResearchStoragePubkeyV1> {
    evidence_value(value.map(pump_research_storage_pubkey_v1), source)
}

#[derive(Clone, Debug)]
struct PumpResearchParticipantBalanceEvidenceV1 {
    token_account: EvidenceValueV1<PumpResearchStoragePubkeyV1>,
    before_units: EvidenceValueV1<u64>,
    after_units: EvidenceValueV1<u64>,
    scope: ParticipantBalanceScopeV1,
    provenance: ParticipantBalanceProvenanceV1,
}

impl PumpResearchParticipantBalanceEvidenceV1 {
    fn unknown() -> Self {
        Self {
            token_account: evidence_pubkey(None, PumpEvidenceSourceV1::TransactionMeta),
            before_units: evidence_value(None, PumpEvidenceSourceV1::TransactionMeta),
            after_units: evidence_value(None, PumpEvidenceSourceV1::TransactionMeta),
            scope: ParticipantBalanceScopeV1::Unknown,
            provenance: ParticipantBalanceProvenanceV1::Unknown,
        }
    }
}

fn apply_participant_balance_evidence(
    mutation: &mut PumpCertifiedMutationV1,
    evidence: &PumpResearchParticipantBalanceEvidenceV1,
) {
    mutation.participant_token_account = evidence.token_account.clone();
    mutation.participant_token_balance_before_units = evidence.before_units.clone();
    mutation.participant_token_balance_after_units = evidence.after_units.clone();
    mutation.participant_balance_scope = evidence.scope;
    mutation.participant_balance_provenance = evidence.provenance.clone();
}

/// Materialise the optional *concrete trade token-account* balance evidence.
/// This intentionally does not create a wallet-total inventory and does not
/// replay SPL transfers. Any account reuse, extra touch, missing metadata or
/// non-ATA proof failure remains `Unknown`.
fn participant_balance_evidence_by_locator(
    source_payload: &[u8],
    inventory: &PumpResearchTransactionMutationInventoryV1,
) -> HashMap<ghost_core::RawPumpMutationLocatorV1, PumpResearchParticipantBalanceEvidenceV1> {
    use yellowstone_grpc_proto::prelude::SubscribeUpdateTransaction;

    let mut result = HashMap::new();
    let Ok(update) = SubscribeUpdateTransaction::decode(source_payload) else {
        return result;
    };
    let Some(transaction_info) = update.transaction.as_ref() else {
        return result;
    };
    let (Some(transaction), Some(meta)) = (
        transaction_info.transaction.as_ref(),
        transaction_info.meta.as_ref(),
    ) else {
        return result;
    };
    let Some(message) = transaction.message.as_ref() else {
        return result;
    };

    let mut account_keys = Vec::with_capacity(
        message.account_keys.len()
            + meta.loaded_writable_addresses.len()
            + meta.loaded_readonly_addresses.len(),
    );
    for bytes in message
        .account_keys
        .iter()
        .chain(meta.loaded_writable_addresses.iter())
        .chain(meta.loaded_readonly_addresses.iter())
    {
        let Ok(pubkey) = Pubkey::try_from(bytes.as_slice()) else {
            return result;
        };
        account_keys.push(pubkey);
    }
    let mut account_reference_count: HashMap<u32, u32> = HashMap::new();
    for instruction in &message.instructions {
        for index in &instruction.accounts {
            let count = account_reference_count
                .entry(u32::from(*index))
                .or_default();
            *count = count.saturating_add(1);
        }
    }
    for group in &meta.inner_instructions {
        for instruction in &group.instructions {
            for index in &instruction.accounts {
                let count = account_reference_count
                    .entry(u32::from(*index))
                    .or_default();
                *count = count.saturating_add(1);
            }
        }
    }
    let mut mutation_count_by_token_account: HashMap<u32, u32> = HashMap::new();
    for entry in &inventory.mutations {
        if let Some(index) = entry.participant_token_account_message_index {
            let count = mutation_count_by_token_account.entry(index).or_default();
            *count = count.saturating_add(1);
        }
    }

    for entry in &inventory.mutations {
        let Some(locator) = entry.locator.clone() else {
            continue;
        };
        let evidence = participant_balance_for_entry(
            entry,
            &account_keys,
            &account_reference_count,
            &mutation_count_by_token_account,
            &meta.pre_token_balances,
            &meta.post_token_balances,
        );
        result.insert(locator, evidence);
    }
    result
}

fn participant_balance_for_entry(
    entry: &PumpResearchMutationInventoryEntryV1,
    account_keys: &[Pubkey],
    account_reference_count: &HashMap<u32, u32>,
    mutation_count_by_token_account: &HashMap<u32, u32>,
    pre_token_balances: &[yellowstone_grpc_proto::prelude::TokenBalance],
    post_token_balances: &[yellowstone_grpc_proto::prelude::TokenBalance],
) -> PumpResearchParticipantBalanceEvidenceV1 {
    if entry.kind != PumpMutationKindV1::Trade {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    }
    let (
        Some(participant),
        Some(mint),
        Some(token_account),
        Some(message_account_index),
        Some(instruction_account_position),
        Some(token_program),
    ) = (
        entry.participant,
        entry.mint,
        entry.participant_token_account,
        entry.participant_token_account_message_index,
        entry.participant_token_account_instruction_position,
        entry.token_program,
    )
    else {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    };
    if account_keys
        .get(usize::try_from(message_account_index).unwrap_or(usize::MAX))
        .copied()
        != Some(token_account)
        || mutation_count_by_token_account
            .get(&message_account_index)
            .copied()
            != Some(1)
        || account_reference_count.get(&message_account_index).copied() != Some(1)
    {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    }
    let Ok(associated_token_program) =
        Pubkey::from_str("ATokenGPvbdGVxr1b2hvZbsiqW5xWH25efTNsLJA8knL")
    else {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    };
    let (expected_ata, _) = Pubkey::find_program_address(
        &[participant.as_ref(), token_program.as_ref(), mint.as_ref()],
        &associated_token_program,
    );
    if expected_ata != token_account {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    }

    let pre: Vec<_> = pre_token_balances
        .iter()
        .filter(|balance| balance.account_index == message_account_index)
        .collect();
    let post: Vec<_> = post_token_balances
        .iter()
        .filter(|balance| balance.account_index == message_account_index)
        .collect();
    let ([pre], [post]) = (pre.as_slice(), post.as_slice()) else {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    };
    let (Some(pre_amount), Some(post_amount)) = (
        pre.ui_token_amount
            .as_ref()
            .and_then(|amount| amount.amount.parse::<u64>().ok()),
        post.ui_token_amount
            .as_ref()
            .and_then(|amount| amount.amount.parse::<u64>().ok()),
    ) else {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    };
    if pre.mint != mint.to_string()
        || post.mint != mint.to_string()
        || pre.owner != participant.to_string()
        || post.owner != participant.to_string()
        || pre.program_id != token_program.to_string()
        || post.program_id != token_program.to_string()
    {
        return PumpResearchParticipantBalanceEvidenceV1::unknown();
    }
    PumpResearchParticipantBalanceEvidenceV1 {
        token_account: evidence_pubkey(Some(token_account), PumpEvidenceSourceV1::TransactionMeta),
        before_units: evidence_value(Some(pre_amount), PumpEvidenceSourceV1::TransactionMeta),
        after_units: evidence_value(Some(post_amount), PumpEvidenceSourceV1::TransactionMeta),
        scope: ParticipantBalanceScopeV1::CanonicalTradeTokenAccount,
        provenance: ParticipantBalanceProvenanceV1::TransactionMetaAndCanonicalAtaProof {
            message_account_index,
            instruction_account_position,
            token_program: pump_research_storage_pubkey_v1(token_program),
        },
    }
}

fn birth_from_trajectory(
    trajectory: &PumpTransactionTrajectoryV1,
    inventory: &PumpResearchTransactionMutationInventoryV1,
) -> Option<PumpBirthEvidenceV1> {
    let create = inventory.mutations.iter().find(|entry| {
        entry.kind == PumpMutationKindV1::Create
            && entry.bonding_curve
                == Some(pump_research_pubkey_from_storage_v1(
                    trajectory.bonding_curve,
                ))
            && entry.mint == Some(pump_research_pubkey_from_storage_v1(trajectory.mint))
    })?;
    let locator = create.locator.clone()?;
    let order = create.order.clone()?;
    let quote_mint = create.quote_mint?;
    let initial_state = trajectory
        .mutations
        .iter()
        .find(|mutation| mutation.locator == locator)
        .and_then(|mutation| mutation.state_after);
    // The state is known only on an exact trajectory. `state_after` is empty
    // for every non-evaluable/conflicting path today, but this explicit gate
    // preserves the V1 birth contract if a future diagnostic representation
    // carries a provisional state alongside a non-exact certification.
    let initial_state = (trajectory.certification == PumpTrajectoryCertificationV1::Exact)
        .then_some(initial_state)
        .flatten();
    let create_source = match create.instruction_variant {
        PumpInstructionVariantV1::Create => PumpEvidenceSourceV1::CreatePayload,
        PumpInstructionVariantV1::CreateV2 => PumpEvidenceSourceV1::CreateV2Payload,
        _ => return None,
    };
    Some(PumpBirthEvidenceV1 {
        candidate_id: format!(
            "{}:{}:{}:{}",
            trajectory.source_ref.run_id,
            Signature::from(trajectory.signature.into_inner()),
            locator.outer_instruction_index,
            locator.semantic_event_ordinal
        ),
        create_kind: match create.instruction_variant {
            PumpInstructionVariantV1::Create => PumpCreateKindV1::Create,
            PumpInstructionVariantV1::CreateV2 => PumpCreateKindV1::CreateV2,
            _ => return None,
        },
        locator,
        order,
        event_time: trajectory.event_time,
        mint: trajectory.mint,
        bonding_curve: trajectory.bonding_curve,
        quote_mint: pump_research_storage_pubkey_v1(quote_mint),
        protocol_creator: evidence_pubkey(create.protocol_creator, create_source),
        create_user: evidence_pubkey(create.create_user, create_source),
        // A complete Create event is program-event evidence. The only allowed
        // fallback is the historical effective Pump Global predecessor, which
        // is account-layout evidence. Never label that fallback as an event.
        initial_state: evidence_value(
            initial_state,
            if direct_event_complete_state(create).is_some() {
                PumpEvidenceSourceV1::ProgramEvent
            } else {
                PumpEvidenceSourceV1::AccountLayout
            },
        ),
        token_total_supply: evidence_value(
            create.token_total_supply,
            PumpEvidenceSourceV1::ProgramEvent,
        ),
        mayhem: flag_evidence(create.create_mayhem, create.instruction_variant),
        cashback: flag_evidence(create.create_cashback, create.instruction_variant),
    })
}

fn flag_evidence(value: Option<bool>, variant: PumpInstructionVariantV1) -> FlagEvidenceV1 {
    let source = match variant {
        PumpInstructionVariantV1::Create => Some(FlagEvidenceSourceV1::CreatePayload),
        PumpInstructionVariantV1::CreateV2 => Some(FlagEvidenceSourceV1::CreateV2Payload),
        _ => None,
    };
    FlagEvidenceV1 {
        value,
        status: match value {
            Some(true) => FlagEvidenceStatusV1::KnownTrue,
            Some(false) => FlagEvidenceStatusV1::KnownFalse,
            None => FlagEvidenceStatusV1::Unknown,
        },
        source: value.is_some().then_some(source).flatten(),
    }
}

/// The only time axes accepted by the generic offline window exporter. They
/// deliberately remain distinct: a chain timestamp is never manufactured
/// from ingress time and monotonic time is never an epoch timestamp.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PumpResearchWindowTimeAxisV1 {
    Chain,
    Observed,
}

impl PumpResearchWindowTimeAxisV1 {
    pub fn parse_cli(value: &str) -> Result<Self> {
        match value {
            "chain" => Ok(Self::Chain),
            "observed" => Ok(Self::Observed),
            _ => bail!("time axis must be exactly 'chain' or 'observed', got {value:?}"),
        }
    }

    fn timestamp(self, event_time: PumpResearchEventTimeV1) -> Option<u64> {
        match self {
            Self::Chain => event_time.chain_event_ts_ms,
            Self::Observed => event_time.ingress_wall_ts_ms,
        }
    }
}

/// Immutable request material recorded beside a generic exported experiment.
/// It has no strategy name, feature, score, PnL or execution assumptions.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchWindowExportRequestV1 {
    pub time_axis: PumpResearchWindowTimeAxisV1,
    pub observation_ms: u64,
    pub forward_ms: u64,
    pub required_evidence: Option<PumpResearchRequiredEvidenceV1>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchExportedWindowV1 {
    pub schema_version: u16,
    pub source_run_id: String,
    pub candidate_id: String,
    pub mint: PumpResearchStoragePubkeyV1,
    pub bonding_curve: PumpResearchStoragePubkeyV1,
    pub time_axis: PumpResearchWindowTimeAxisV1,
    pub birth_time_ms: Option<u64>,
    pub observation_end_ms: Option<u64>,
    pub window_end_ms: Option<u64>,
    pub trajectory_count: u32,
    pub status: PumpResearchWindowStatusV1,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct PumpResearchWindowExportSummaryV1 {
    pub source_run_id: String,
    pub output_dir: PathBuf,
    pub exported_birth_count: u64,
    pub complete_window_count: u64,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
struct PumpResearchWindowExportManifestV1 {
    schema_version: u16,
    source_run_id: String,
    qualification_status: PumpResearchTapeQualificationStatusV1,
    #[serde(rename = "GO_D_SOURCE_AUTHORITY")]
    go_d_source_authority: String,
    #[serde(rename = "EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE")]
    external_go_e_audit_not_used_as_gate: bool,
    go_d_source_authority_sha256: String,
    request: PumpResearchWindowExportRequestV1,
    exported_birth_count: u64,
    complete_window_count: u64,
}

fn has_verified_go_d_export_authority_v1(
    qualification_status: &PumpResearchTapeQualificationStatusV1,
    go_d_source_authority: &str,
    external_go_e_audit_not_used_as_gate: bool,
    go_d_source_authority_sha256: &str,
) -> bool {
    qualification_status == &PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape
        && go_d_source_authority == GO_D_SOURCE_AUTHORITY_VERIFIED_V1
        && external_go_e_audit_not_used_as_gate
        && go_d_source_authority_sha256.len() == 64
        && go_d_source_authority_sha256
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit())
}

/// Create a strategy-neutral launch window artifact from the hash-pinned GO-D
/// frozen tape. The exporter is read-only with respect to `exact`; retired
/// GO-E `Ready` outputs are not a gate and cannot substitute for GO-D source
/// authority.
pub fn export_pump_research_windows_v1(
    tape_dir: &Path,
    request: PumpResearchWindowExportRequestV1,
    output_dir: &Path,
) -> Result<PumpResearchWindowExportSummaryV1> {
    if request.observation_ms == 0 || request.forward_ms == 0 {
        bail!("observation_ms and forward_ms must both be greater than zero");
    }
    if output_dir.exists() {
        bail!(
            "window output directory {} already exists; export never overwrites an experiment artifact",
            output_dir.display()
        );
    }

    let manifest: PumpExactResearchTapeManifestV1 = read_json(
        &tape_dir.join("manifest.json"),
        PumpResearchAuthorityFileKindV1::ExactManifest,
    )?;
    if manifest.schema_version != 1 || manifest.source_storage_format_version != 1 {
        bail!(
            "exact tape {} is not a supported V1 artifact",
            tape_dir.display()
        );
    }
    if !has_verified_go_d_export_authority_v1(
        &manifest.qualification_status,
        &manifest.go_d_source_authority,
        manifest.external_go_e_audit_not_used_as_gate,
        &manifest.go_d_source_authority_sha256,
    ) {
        bail!(
            "exact tape {} is {:?}; export-window requires VERIFIED GO-D authority with EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE=true",
            tape_dir.display(),
            manifest.qualification_status
        );
    }
    let births: Vec<PumpBirthEvidenceV1> = read_jsonl(&tape_dir.join("births_v1.jsonl"))?;
    let trajectories: Vec<PumpTransactionTrajectoryV1> =
        read_jsonl(&tape_dir.join("trajectories_v1.jsonl"))?;
    if births.is_empty() {
        bail!("exact tape {} contains no birth rows", tape_dir.display());
    }
    if trajectories.is_empty() {
        bail!(
            "exact tape {} contains no trajectory rows",
            tape_dir.display()
        );
    }

    let observed_times: Vec<u64> = trajectories
        .iter()
        .filter_map(|trajectory| request.time_axis.timestamp(trajectory.event_time))
        .collect();
    let Some(first_available_time_ms) = observed_times.iter().copied().min() else {
        bail!(
            "exact tape {} has no {:?} timestamp for any trajectory",
            tape_dir.display(),
            request.time_axis
        );
    };
    let last_available_time_ms = observed_times
        .iter()
        .copied()
        .max()
        .ok_or_else(|| anyhow::anyhow!("timestamp maximum disappeared"))?;

    let parent = output_dir.parent().ok_or_else(|| {
        anyhow::anyhow!(
            "window output directory {} has no parent",
            output_dir.display()
        )
    })?;
    let name = output_dir
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or_else(|| {
            anyhow::anyhow!(
                "window output directory {} has no UTF-8 final name",
                output_dir.display()
            )
        })?;
    let partial_output_dir = parent.join(format!(".{name}.partial"));
    if partial_output_dir.exists() {
        bail!(
            "window partial directory {} already exists; inspect or retain the interrupted artifact before retrying",
            partial_output_dir.display()
        );
    }
    fs::create_dir_all(parent)
        .with_context(|| format!("create window output parent {}", parent.display()))?;
    fs::create_dir(&partial_output_dir).with_context(|| {
        format!(
            "create window partial output directory {}",
            partial_output_dir.display()
        )
    })?;
    let windows_path = partial_output_dir.join("windows_v1.jsonl");
    let mut windows = BufWriter::new(
        File::options()
            .write(true)
            .create_new(true)
            .open(&windows_path)
            .with_context(|| format!("create window artifact {}", windows_path.display()))?,
    );

    let mut summary = PumpResearchWindowExportSummaryV1 {
        source_run_id: manifest.source_run_id.clone(),
        output_dir: output_dir.to_path_buf(),
        ..PumpResearchWindowExportSummaryV1::default()
    };
    for birth in &births {
        let exported = materialize_window_for_birth(
            &manifest.source_run_id,
            birth,
            &trajectories,
            &request,
            first_available_time_ms,
            last_available_time_ms,
        );
        if exported.status == PumpResearchWindowStatusV1::Complete {
            summary.complete_window_count = summary.complete_window_count.saturating_add(1);
        }
        summary.exported_birth_count = summary.exported_birth_count.saturating_add(1);
        write_jsonl(&mut windows, &exported)?;
    }
    sync_jsonl_writer(windows)?;
    let output_manifest = PumpResearchWindowExportManifestV1 {
        schema_version: 1,
        source_run_id: manifest.source_run_id,
        qualification_status: manifest.qualification_status,
        go_d_source_authority: GO_D_SOURCE_AUTHORITY_VERIFIED_V1.to_owned(),
        external_go_e_audit_not_used_as_gate: true,
        go_d_source_authority_sha256: manifest.go_d_source_authority_sha256,
        request,
        exported_birth_count: summary.exported_birth_count,
        complete_window_count: summary.complete_window_count,
    };
    write_json_create_new_and_sync(
        &partial_output_dir.join("manifest.json"),
        &serde_json::to_vec_pretty(&output_manifest).context("serialize window manifest")?,
    )?;
    File::open(&partial_output_dir)
        .with_context(|| {
            format!(
                "open window partial output directory {}",
                partial_output_dir.display()
            )
        })?
        .sync_all()
        .with_context(|| {
            format!(
                "sync window partial output directory {}",
                partial_output_dir.display()
            )
        })?;
    fs::rename(&partial_output_dir, output_dir).with_context(|| {
        format!(
            "atomically publish window artifact {} -> {}",
            partial_output_dir.display(),
            output_dir.display()
        )
    })?;
    File::open(parent)
        .with_context(|| format!("open window output parent {}", parent.display()))?
        .sync_all()
        .with_context(|| format!("sync window output parent {}", parent.display()))?;
    Ok(summary)
}

fn materialize_window_for_birth(
    source_run_id: &str,
    birth: &PumpBirthEvidenceV1,
    all_trajectories: &[PumpTransactionTrajectoryV1],
    request: &PumpResearchWindowExportRequestV1,
    first_available_time_ms: u64,
    last_available_time_ms: u64,
) -> PumpResearchExportedWindowV1 {
    let birth_time_ms = request.time_axis.timestamp(birth.event_time);
    let observation_end_ms =
        birth_time_ms.and_then(|time| time.checked_add(request.observation_ms));
    let window_end_ms = observation_end_ms.and_then(|time| time.checked_add(request.forward_ms));
    let mut in_window: Vec<&PumpTransactionTrajectoryV1> = Vec::new();
    let status = match (birth_time_ms, window_end_ms) {
        (None, _) => PumpResearchWindowStatusV1::MissingBirth,
        (Some(birth_time_ms), _) if birth_time_ms < first_available_time_ms => {
            PumpResearchWindowStatusV1::TruncatedAtRunStart
        }
        (_, None) => PumpResearchWindowStatusV1::TruncatedAtRunEnd,
        (Some(_birth_time_ms), Some(window_end_ms)) if window_end_ms > last_available_time_ms => {
            PumpResearchWindowStatusV1::TruncatedAtRunEnd
        }
        (Some(birth_time_ms), Some(window_end_ms)) => {
            in_window = all_trajectories
                .iter()
                .filter(|trajectory| {
                    trajectory.mint == birth.mint
                        && trajectory.bonding_curve == birth.bonding_curve
                        && request
                            .time_axis
                            .timestamp(trajectory.event_time)
                            .is_some_and(|time| (birth_time_ms..=window_end_ms).contains(&time))
                })
                .collect();
            classify_window_trajectories(&in_window, request.required_evidence)
        }
    };
    PumpResearchExportedWindowV1 {
        schema_version: 1,
        source_run_id: source_run_id.to_owned(),
        candidate_id: birth.candidate_id.clone(),
        mint: birth.mint,
        bonding_curve: birth.bonding_curve,
        time_axis: request.time_axis,
        birth_time_ms,
        observation_end_ms,
        window_end_ms,
        trajectory_count: u32::try_from(in_window.len()).unwrap_or(u32::MAX),
        status,
    }
}

fn classify_window_trajectories(
    trajectories: &[&PumpTransactionTrajectoryV1],
    required_evidence: Option<PumpResearchRequiredEvidenceV1>,
) -> PumpResearchWindowStatusV1 {
    if trajectories.is_empty() {
        return PumpResearchWindowStatusV1::MissingBirth;
    }
    for trajectory in trajectories {
        match trajectory.certification {
            PumpTrajectoryCertificationV1::Exact => {}
            PumpTrajectoryCertificationV1::Conflict(_) => {
                return PumpResearchWindowStatusV1::NonExactMutation
            }
            PumpTrajectoryCertificationV1::NonEvaluable(PumpNonEvaluableReasonV1::CoverageGap) => {
                return PumpResearchWindowStatusV1::CoverageGap
            }
            PumpTrajectoryCertificationV1::NonEvaluable(
                PumpNonEvaluableReasonV1::ProcessBoundary,
            ) => return PumpResearchWindowStatusV1::ProcessBoundary,
            PumpTrajectoryCertificationV1::NonEvaluable(
                PumpNonEvaluableReasonV1::NonCanonicalFork,
            ) => return PumpResearchWindowStatusV1::NonCanonicalFork,
            PumpTrajectoryCertificationV1::NonEvaluable(
                PumpNonEvaluableReasonV1::UnresolvedCanonicality,
            ) => return PumpResearchWindowStatusV1::UnresolvedCanonicality,
            PumpTrajectoryCertificationV1::NonEvaluable(
                PumpNonEvaluableReasonV1::ProgramVersionBoundary,
            ) => return PumpResearchWindowStatusV1::ProgramVersionBoundary,
            PumpTrajectoryCertificationV1::NonEvaluable(_) => {
                return PumpResearchWindowStatusV1::NonExactMutation
            }
        }
    }
    if trajectories.iter().any(|trajectory| {
        trajectory.mutations.iter().any(|mutation| {
            matches!(
                mutation.kind,
                PumpMutationKindV1::Complete | PumpMutationKindV1::Migrate
            )
        })
    }) {
        return PumpResearchWindowStatusV1::TerminalBeforeWindowEnd;
    }
    if let Some(required_evidence) = required_evidence {
        let sufficient = trajectories.iter().all(|trajectory| {
            trajectory
                .mutations
                .iter()
                .filter(|mutation| mutation.kind == PumpMutationKindV1::Trade)
                .all(|mutation| participant_balance_is_complete(mutation, required_evidence))
        });
        if !sufficient {
            return PumpResearchWindowStatusV1::MissingRequiredEvidence(required_evidence);
        }
    }
    PumpResearchWindowStatusV1::Complete
}

fn participant_balance_is_complete(
    mutation: &PumpCertifiedMutationV1,
    required_evidence: PumpResearchRequiredEvidenceV1,
) -> bool {
    match required_evidence {
        PumpResearchRequiredEvidenceV1::ParticipantBalance => {
            mutation.participant_token_account.status == PumpEvidenceStatusV1::Known
                && mutation.participant_token_balance_before_units.status
                    == PumpEvidenceStatusV1::Known
                && mutation.participant_token_balance_after_units.status
                    == PumpEvidenceStatusV1::Known
                && mutation.participant_balance_scope
                    == ParticipantBalanceScopeV1::CanonicalTradeTokenAccount
                && matches!(
                    mutation.participant_balance_provenance,
                    ParticipantBalanceProvenanceV1::TransactionMetaAndCanonicalAtaProof { .. }
                )
        }
    }
}

fn read_jsonl<T>(path: &Path) -> Result<Vec<T>>
where
    T: serde::de::DeserializeOwned,
{
    let file = File::open(path).with_context(|| format!("open JSONL {}", path.display()))?;
    let reader = BufReader::new(file);
    let mut values = Vec::new();
    for (line_number, line) in reader.lines().enumerate() {
        let line = line.with_context(|| format!("read JSONL {}", path.display()))?;
        if line.trim().is_empty() {
            bail!(
                "JSONL {} has an empty line at {}",
                path.display(),
                line_number + 1
            );
        }
        values.push(serde_json::from_str(&line).with_context(|| {
            format!("decode JSONL {} line {}", path.display(), line_number + 1)
        })?);
    }
    Ok(values)
}

fn write_json_create_new_and_sync(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = File::options()
        .write(true)
        .create_new(true)
        .open(path)
        .with_context(|| format!("create {}", path.display()))?;
    file.write_all(bytes)
        .with_context(|| format!("write {}", path.display()))?;
    file.write_all(b"\n")
        .with_context(|| format!("write newline to {}", path.display()))?;
    file.sync_all()
        .with_context(|| format!("sync {}", path.display()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::pump_quote::{
        quote_exact_base_in_sell, quote_exact_base_out, FeeRounding, ProgramFeeRule,
        ProgramFeeSchedule, ProgramFeeScheduleEvidenceV1, PumpReserveState, PumpRouteVariant,
    };
    use ghost_core::pump_research_tape::{
        PumpRawCoverageBoundaryV1, PumpRawCoverageGapReasonV1, PumpResearchProviderRoleV1,
    };
    use solana_sdk::{hash::Hash, message::MessageHeader, signature::Signature};
    use solana_transaction_status::{
        EncodedTransactionWithStatusMeta, UiAddressTableLookup, UiCompiledInstruction,
        UiInnerInstructions, UiLoadedAddresses, UiRawMessage, UiTransaction,
        UiTransactionStatusMeta,
    };
    use std::{net::TcpListener, process::Command, sync::mpsc, thread};
    use tempfile::tempdir;

    #[cfg(unix)]
    fn replace_regular_file_with_fifo_v1(path: &Path, backup: &Path) {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        fs::rename(path, backup).expect("move regular fixture behind FIFO replacement");
        let c_path =
            CString::new(path.as_os_str().as_bytes()).expect("FIFO fixture path contains no NUL");
        // SAFETY: c_path is a valid, NUL-terminated pathname within a
        // temporary test directory. The call creates only an owner-private
        // FIFO used to exercise nonblocking authority reads.
        let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
        assert_eq!(result, 0, "create FIFO authority replacement");
    }

    #[cfg(unix)]
    fn restore_regular_file_after_fifo_v1(path: &Path, backup: &Path) {
        fs::remove_file(path).expect("remove FIFO authority replacement");
        fs::rename(backup, path).expect("restore regular authority fixture");
    }

    struct TestEnvironmentVariableV1 {
        name: &'static str,
        previous: Option<std::ffi::OsString>,
    }

    impl TestEnvironmentVariableV1 {
        fn set(name: &'static str, value: &str) -> Self {
            let previous = std::env::var_os(name);
            std::env::set_var(name, value);
            Self { name, previous }
        }
    }

    impl Drop for TestEnvironmentVariableV1 {
        fn drop(&mut self) {
            match self.previous.take() {
                Some(value) => std::env::set_var(self.name, value),
                None => std::env::remove_var(self.name),
            }
        }
    }

    fn rooted_canonicality(
        slots: impl IntoIterator<Item = u64>,
    ) -> PumpResearchSlotCanonicalityIndexV1 {
        PumpResearchSlotCanonicalityIndexV1 {
            by_slot: slots
                .into_iter()
                .map(|slot| (slot, PumpSlotCanonicalityV1::RootedCanonical))
                .collect(),
        }
    }

    fn stream_epoch_boundary(
        stream_epoch: u64,
        first_block_meta_slot: Option<u64>,
        last_block_meta_slot: Option<u64>,
    ) -> PumpResearchStreamEpochBoundaryV1 {
        PumpResearchStreamEpochBoundaryV1 {
            stream_epoch,
            first_block_meta_slot,
            last_block_meta_slot,
        }
    }

    fn ready_range(
        stream_epoch: u64,
        start_slot: u64,
        end_slot: u64,
    ) -> PumpResearchQualificationRangeSelectionV1 {
        PumpResearchQualificationRangeSelectionV1::Ready(PumpResearchQualifiedSlotRangeV1 {
            stream_epoch,
            start_slot,
            end_slot,
        })
    }

    #[derive(Clone, Copy, Debug)]
    enum EpochRecordFixtureKind {
        Transaction,
        AccountUpdate,
        SlotUpdate,
        BlockMeta,
        CoverageGap,
    }

    fn epoch_source_fixture(stream_epoch: u64, capture_sequence: u64) -> PumpRawSourceEnvelopeV1 {
        PumpRawSourceEnvelopeV1 {
            provider_id: "epoch-corruption-fixture".to_owned(),
            provider_role: PumpResearchProviderRoleV1::PrimaryAuthority,
            stream_epoch,
            capture_sequence,
            payload_hash_blake3: PumpResearchStorageHashV1::from(*blake3::hash(&[]).as_bytes()),
        }
    }

    fn epoch_record_fixture(
        kind: EpochRecordFixtureKind,
        stream_epoch: u64,
        capture_sequence: u64,
    ) -> PumpResearchRawRecordV1 {
        let source = epoch_source_fixture(stream_epoch, capture_sequence);
        match kind {
            EpochRecordFixtureKind::Transaction => {
                PumpResearchRawRecordV1::PrimaryTransaction(PumpPrimaryTransactionEvidenceV1 {
                    source,
                    slot: 100 + capture_sequence,
                    tx_index: Some(0),
                    signature: PumpResearchStorageSignatureV1::from([capture_sequence as u8; 64]),
                    event_time: PumpResearchEventTimeV1::default(),
                    block_time: None,
                    source_payload: Vec::new(),
                })
            }
            EpochRecordFixtureKind::AccountUpdate => {
                PumpResearchRawRecordV1::PrimaryAccountUpdate(PumpPrimaryAccountUpdateEvidenceV1 {
                    source,
                    account_role: PumpResearchAccountRoleV1::BondingCurve,
                    is_startup: false,
                    account_pubkey: PumpResearchStoragePubkeyV1::from([3; 32]),
                    owner_program: PumpResearchStoragePubkeyV1::from([4; 32]),
                    raw_account_data: Vec::new(),
                    raw_account_data_hash_blake3: PumpResearchStorageHashV1::from(
                        *blake3::hash(&[]).as_bytes(),
                    ),
                    slot: 100 + capture_sequence,
                    write_version: capture_sequence,
                    txn_signature: None,
                    event_time: PumpResearchEventTimeV1::default(),
                    source_payload: Vec::new(),
                })
            }
            EpochRecordFixtureKind::SlotUpdate => {
                PumpResearchRawRecordV1::PrimarySlotUpdate(PumpPrimarySlotEvidenceV1 {
                    source,
                    slot: 100 + capture_sequence,
                    parent: Some(99 + capture_sequence),
                    source_status: 0,
                    event_time: PumpResearchEventTimeV1::default(),
                    source_payload: Vec::new(),
                })
            }
            EpochRecordFixtureKind::BlockMeta => {
                PumpResearchRawRecordV1::PrimaryBlockMeta(PumpPrimaryBlockMetaEvidenceV1 {
                    source,
                    slot: 100 + capture_sequence,
                    parent_slot: 99 + capture_sequence,
                    block_time: None,
                    event_time: PumpResearchEventTimeV1::default(),
                    source_payload: Vec::new(),
                })
            }
            EpochRecordFixtureKind::CoverageGap => {
                PumpResearchRawRecordV1::CoverageGap(PumpRawCoverageGapV1 {
                    gap_id_blake3: PumpResearchStorageHashV1::from([stream_epoch as u8; 32]),
                    provider_id: "epoch-corruption-fixture".to_owned(),
                    stream_epoch,
                    episode_sequence: capture_sequence,
                    reason: PumpRawCoverageGapReasonV1::IngressQueueSaturated,
                    before: PumpRawCoverageBoundaryV1::default(),
                    after: PumpRawCoverageBoundaryV1::default(),
                    missing_event_count: 1,
                    first_dropped: PumpRawCoverageBoundaryV1::default(),
                    last_dropped: PumpRawCoverageBoundaryV1::default(),
                    queue_high_water: 1,
                    started_at_wall_ms: 1,
                    ended_at_wall_ms: 2,
                    recovered: false,
                })
            }
        }
    }

    /// One deterministic frozen Yellowstone transaction covers every
    /// representative role required by provider suitability. The Pump program
    /// is referenced through loaded addresses both directly and as an inner
    /// instruction under a distinct router, and the transaction is failed.
    fn provider_suitability_transaction_fixture(
        stream_epoch: u64,
        capture_sequence: u64,
        slot: u64,
    ) -> PumpResearchRawRecordV1 {
        use yellowstone_grpc_proto::prelude::{
            CompiledInstruction as GrpcCompiledInstruction,
            InnerInstruction as GrpcInnerInstruction, InnerInstructions as GrpcInnerInstructions,
            Message as GrpcMessage, SubscribeUpdateTransaction, SubscribeUpdateTransactionInfo,
            Transaction as GrpcTransaction, TransactionError as GrpcTransactionError,
            TransactionStatusMeta as GrpcTransactionStatusMeta,
        };

        let router = Pubkey::new_unique();
        let pump = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
            .expect("frozen Pump program id");
        let signature_bytes = [capture_sequence as u8; 64];
        let update = SubscribeUpdateTransaction {
            transaction: Some(SubscribeUpdateTransactionInfo {
                signature: signature_bytes.to_vec(),
                is_vote: false,
                transaction: Some(GrpcTransaction {
                    signatures: vec![signature_bytes.to_vec()],
                    message: Some(GrpcMessage {
                        header: None,
                        account_keys: vec![router.to_bytes().to_vec()],
                        recent_blockhash: vec![0; 32],
                        instructions: vec![
                            GrpcCompiledInstruction {
                                program_id_index: 0,
                                accounts: Vec::new(),
                                data: Vec::new(),
                            },
                            GrpcCompiledInstruction {
                                program_id_index: 1,
                                accounts: Vec::new(),
                                data: Vec::new(),
                            },
                        ],
                        versioned: true,
                        address_table_lookups: Vec::new(),
                    }),
                }),
                meta: Some(GrpcTransactionStatusMeta {
                    err: Some(GrpcTransactionError { err: vec![1] }),
                    inner_instructions: vec![GrpcInnerInstructions {
                        index: 0,
                        instructions: vec![GrpcInnerInstruction {
                            program_id_index: 1,
                            accounts: Vec::new(),
                            data: Vec::new(),
                            stack_height: Some(2),
                        }],
                    }],
                    loaded_writable_addresses: vec![pump.to_bytes().to_vec()],
                    ..GrpcTransactionStatusMeta::default()
                }),
                index: 0,
            }),
            slot,
        };
        let source_payload = update.encode_to_vec();
        let mut source = epoch_source_fixture(stream_epoch, capture_sequence);
        source.payload_hash_blake3 =
            PumpResearchStorageHashV1::from(*blake3::hash(source_payload.as_slice()).as_bytes());
        PumpResearchRawRecordV1::PrimaryTransaction(PumpPrimaryTransactionEvidenceV1 {
            source,
            slot,
            tx_index: Some(0),
            signature: PumpResearchStorageSignatureV1::from(signature_bytes),
            event_time: PumpResearchEventTimeV1::default(),
            block_time: None,
            source_payload,
        })
    }

    fn record_capture_sequence(record: &PumpResearchRawRecordV1) -> Option<u64> {
        match record {
            PumpResearchRawRecordV1::PrimaryTransaction(evidence) => {
                Some(evidence.source.capture_sequence)
            }
            PumpResearchRawRecordV1::PrimaryAccountUpdate(evidence) => {
                Some(evidence.source.capture_sequence)
            }
            PumpResearchRawRecordV1::PrimarySlotUpdate(evidence) => {
                Some(evidence.source.capture_sequence)
            }
            PumpResearchRawRecordV1::PrimaryBlockMeta(evidence) => {
                Some(evidence.source.capture_sequence)
            }
            PumpResearchRawRecordV1::CoverageGap(_) | PumpResearchRawRecordV1::SegmentClosed(_) => {
                None
            }
        }
    }

    fn write_frozen_segment_fixture(
        raw_dir: &Path,
        run_id: &str,
        segment_index: u64,
        stream_epoch: u64,
        previous_segment_blake3: Option<PumpResearchStorageHashV1>,
        records: &[PumpResearchRawRecordV1],
        clean_shutdown: bool,
    ) -> (PumpResearchSegmentReceiptV1, PumpResearchStorageHashV1) {
        let header = PumpRawSegmentHeaderV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            run_id: run_id.to_owned(),
            segment_index,
            stream_epoch,
            opened_wall_ts_ms: 1,
            opened_monotonic_ts_ms: 1,
            previous_segment_blake3,
        };
        let header_bytes = PumpResearchRawCodecV1::encode_segment_header(&header)
            .expect("encode valid epoch fixture header");
        let mut bytes = header_bytes.clone();
        let mut prefix_hasher = blake3::Hasher::new();
        prefix_hasher.update(&header_bytes);
        let mut first_capture_sequence = None;
        let mut last_capture_sequence = None;
        for record in records {
            let frame = PumpResearchRawCodecV1::encode_record(record)
                .expect("encode valid epoch fixture record");
            prefix_hasher.update(&frame);
            bytes.extend_from_slice(&frame);
            if let Some(sequence) = record_capture_sequence(record) {
                first_capture_sequence.get_or_insert(sequence);
                last_capture_sequence = Some(sequence);
            }
        }
        let prefix_hash = PumpResearchStorageHashV1::from(*prefix_hasher.finalize().as_bytes());
        let footer = PumpRawSegmentClosedV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            segment_index,
            accepted_record_count: records.len() as u64,
            data_bytes: bytes.len() as u64,
            segment_blake3: prefix_hash,
            closed_wall_ts_ms: 2,
            clean_shutdown,
        };
        bytes.extend_from_slice(
            &PumpResearchRawCodecV1::encode_record(&PumpResearchRawRecordV1::SegmentClosed(footer))
                .expect("encode valid epoch fixture footer"),
        );
        let filename = format!("segment_{segment_index:05}.bin");
        fs::write(raw_dir.join(&filename), &bytes).expect("write epoch fixture segment");
        let file_sha256: [u8; 32] = Sha256::digest(&bytes).into();
        let receipt = PumpResearchSegmentReceiptV1 {
            segment_index,
            filename,
            file_sha256: PumpResearchStorageHashV1::from(file_sha256),
            file_blake3: PumpResearchStorageHashV1::from(*blake3::hash(&bytes).as_bytes()),
            first_capture_sequence,
            last_capture_sequence,
            accepted_record_count: records.len() as u64,
        };
        (receipt, prefix_hash)
    }

    fn epoch_fixture_start_manifest(run_id: &str) -> PumpResearchRunStartManifestV1 {
        PumpResearchRunStartManifestV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            schema_version: 1,
            run_id: run_id.to_owned(),
            repository_commit: "epoch-corruption-fixture".to_owned(),
            binary_hash_blake3: PumpResearchStorageHashV1::from([0; 32]),
            config_hash_blake3: PumpResearchStorageHashV1::from([0; 32]),
            raw_event_schema_version: 1,
            decoder_version: "epoch-corruption-fixture".to_owned(),
            primary_provider_id: "epoch-corruption-fixture".to_owned(),
            primary_provider_role: PumpResearchProviderRoleV1::PrimaryAuthority,
            commitment: "processed".to_owned(),
            subscription_request_fingerprint_blake3: PumpResearchStorageHashV1::from([0; 32]),
            stream_epoch: 0,
            capture_started_wall_ms: 1,
            capture_started_monotonic_ms: 1,
            time_contract_version: 1,
            required_for_run: true,
            source_proto_schema_version: "fixture".to_owned(),
            source_proto_descriptor_hash: "sha256:fixture".to_owned(),
            source_proto_crate: "fixture".to_owned(),
            source_proto_crate_version: "fixture".to_owned(),
            source_client_crate: "fixture".to_owned(),
            source_client_version: "fixture".to_owned(),
            source_capture_semantics: "decoded_protobuf_schema_lossless_v1".to_owned(),
            pump_program_id: PumpResearchStoragePubkeyV1::from([1; 32]),
            pump_program_account_owner: PumpResearchStoragePubkeyV1::from([2; 32]),
            pump_programdata_pubkey: PumpResearchStoragePubkeyV1::from([3; 32]),
            program_data_owner: PumpResearchStoragePubkeyV1::from([2; 32]),
            program_data_hash_algorithm: "blake3-256".to_owned(),
            program_data_hash_at_start: PumpResearchStorageHashV1::from([4; 32]),
            program_deployment_slot_at_start: Some(5),
            program_observed_context_slot_at_start: 6,
            program_receipt_commitment: "finalized".to_owned(),
        }
    }

    fn write_epoch_raw_run_fixture(
        raw_dir: &Path,
        run_id: &str,
        segments: Vec<(u64, Vec<PumpResearchRawRecordV1>)>,
    ) {
        let segment_count = segments.len();
        let mut receipts = Vec::with_capacity(segment_count);
        let mut previous_prefix = None;
        let mut source_record_count = 0_u64;
        let mut gap_count = 0_u64;
        for (position, (stream_epoch, records)) in segments.into_iter().enumerate() {
            for record in &records {
                if record_capture_sequence(record).is_some() {
                    source_record_count = source_record_count.saturating_add(1);
                }
                if matches!(record, PumpResearchRawRecordV1::CoverageGap(_)) {
                    gap_count = gap_count.saturating_add(1);
                }
            }
            let segment_index = position as u64;
            let clean_shutdown = position + 1 == segment_count;
            let (receipt, prefix) = write_frozen_segment_fixture(
                raw_dir,
                run_id,
                segment_index,
                stream_epoch,
                previous_prefix,
                &records,
                clean_shutdown,
            );
            receipts.push(receipt);
            previous_prefix = Some(prefix);
        }
        let completion = PumpResearchRunCompletionReceiptV1 {
            storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            run_id: run_id.to_owned(),
            capture_ended_wall_ms: 2,
            pump_program_id_at_completion: None,
            pump_program_account_owner_at_completion: None,
            pump_programdata_pubkey_at_completion: None,
            program_data_owner_at_completion: None,
            program_data_hash_at_completion: None,
            program_deployment_slot_at_completion: None,
            program_observed_context_slot_at_completion: None,
            program_receipt_commitment_at_completion: None,
            segment_list: receipts,
            gap_count,
            source_stream_established: true,
            first_source_update_received: source_record_count > 0,
            source_workers_cleanly_stopped: true,
            received_source_update_count: source_record_count,
            admitted_source_update_count: source_record_count,
            persisted_source_record_count: source_record_count,
            dropped_source_update_count: gap_count,
            persisted_ingress_gap_episode_count: gap_count,
            persisted_ingress_gap_missing_event_count: gap_count,
            source_lifecycle_error: None,
            capture_failure: None,
            clean_shutdown: true,
            status: PumpResearchRunCompletionStatusV1::Complete,
        };
        fs::write(
            raw_dir.join("run_start_manifest.json"),
            serde_json::to_vec(&epoch_fixture_start_manifest(run_id))
                .expect("serialize epoch fixture start manifest"),
        )
        .expect("write epoch fixture start manifest");
        fs::write(
            raw_dir.join("run_completion_receipt.json"),
            serde_json::to_vec(&completion).expect("serialize epoch fixture completion receipt"),
        )
        .expect("write epoch fixture completion receipt");
    }

    fn audit_status_meta(
        inner_instructions: OptionSerializer<Vec<UiInnerInstructions>>,
        loaded_addresses: OptionSerializer<UiLoadedAddresses>,
    ) -> UiTransactionStatusMeta {
        UiTransactionStatusMeta {
            err: None,
            status: Ok(()),
            fee: 0,
            pre_balances: Vec::new(),
            post_balances: Vec::new(),
            inner_instructions,
            log_messages: OptionSerializer::Skip,
            pre_token_balances: OptionSerializer::Skip,
            post_token_balances: OptionSerializer::Skip,
            rewards: OptionSerializer::Skip,
            loaded_addresses,
            return_data: OptionSerializer::Skip,
            compute_units_consumed: OptionSerializer::Skip,
        }
    }

    fn audit_transaction(
        account_keys: Vec<Pubkey>,
        instructions: Vec<UiCompiledInstruction>,
        address_table_lookups: Option<Vec<UiAddressTableLookup>>,
        meta: UiTransactionStatusMeta,
    ) -> EncodedTransactionWithStatusMeta {
        EncodedTransactionWithStatusMeta {
            transaction: EncodedTransaction::Json(UiTransaction {
                signatures: vec![Signature::new_unique().to_string()],
                message: UiMessage::Raw(UiRawMessage {
                    header: MessageHeader::default(),
                    account_keys: account_keys
                        .into_iter()
                        .map(|key| key.to_string())
                        .collect(),
                    recent_blockhash: Hash::default().to_string(),
                    instructions,
                    address_table_lookups,
                }),
            }),
            meta: Some(meta),
            version: None,
        }
    }

    fn compiled_instruction(program_id_index: u8) -> UiCompiledInstruction {
        UiCompiledInstruction {
            program_id_index,
            accounts: Vec::new(),
            data: String::new(),
            stack_height: None,
        }
    }

    #[test]
    fn qualification_audit_config_loads_no_auth_root_https_without_provider_io() {
        let temporary = tempdir().expect("temporary audit config");
        let path = temporary.path().join("audit.toml");
        fs::write(
            &path,
            r#"
audit_provider_id = "independent-public-finalized-rpc"
audit_rpc_endpoint = "https://api.mainnet-beta.solana.com"
audit_rpc_auth_header = "x-api-key"
bounded_concurrency = 1
bounded_retry_count = 2
request_timeout_ms = 30000
"#,
        )
        .expect("write valid no-auth audit config");

        let config = PumpResearchQualificationAuditConfigV1::load(&path)
            .expect("load local-only audit config");
        assert_eq!(config.audit_provider_id, "independent-public-finalized-rpc");
        assert_eq!(
            config.audit_rpc_endpoint,
            "https://api.mainnet-beta.solana.com"
        );
        assert_eq!(config.audit_rpc_auth_token_env, None);
        assert_eq!(config.audit_rpc_endpoint_path_env, None);
        assert_eq!(config.bounded_concurrency, 1);
        assert_eq!(config.bounded_retry_count, 2);
        assert_eq!(config.request_timeout_ms, 30_000);
        assert_eq!(
            config.resolve_auth_token().expect("no-auth resolution"),
            None
        );
    }

    #[test]
    fn qualification_audit_endpoint_path_credential_stays_out_of_config_origin() {
        let config = PumpResearchQualificationAuditConfigV1 {
            audit_provider_id: "spectrum-audit".to_owned(),
            audit_rpc_endpoint: "https://spectrum.example".to_owned(),
            audit_rpc_endpoint_path_env: Some("SYNTHETIC_SPECTRUM_PATH".to_owned()),
            audit_rpc_auth_token_env: None,
            audit_rpc_auth_header: "x-api-key".to_owned(),
            bounded_concurrency: 1,
            bounded_retry_count: 2,
            request_timeout_ms: 30_000,
        };
        config.validate().expect("valid endpoint-path auth config");
        let path_credential = "/synthetic-secret/solana/mainnet/shared/pruned/rpc";
        let resolved = config
            .endpoint_with_path_credential(Some(path_credential))
            .expect("resolve credentialized endpoint in memory");
        assert_eq!(
            resolved,
            "https://spectrum.example/synthetic-secret/solana/mainnet/shared/pruned/rpc"
        );
        assert!(!config.audit_rpc_endpoint.contains("synthetic-secret"));
        for invalid in ["relative", "/../secret", "/double//slash", "/secret?query"] {
            assert!(config.endpoint_with_path_credential(Some(invalid)).is_err());
        }
    }

    #[test]
    fn qualification_audit_config_rejects_non_root_or_credentialed_url() {
        for endpoint in [
            "https://audit.example/path",
            "https://user@audit.example",
            "https://audit.example?api-key=secret",
        ] {
            let temporary = tempdir().expect("temporary invalid audit config");
            let path = temporary.path().join("audit.toml");
            fs::write(
                &path,
                format!("audit_provider_id = \"independent\"\naudit_rpc_endpoint = {endpoint:?}\n"),
            )
            .expect("write invalid audit config");
            let error = PumpResearchQualificationAuditConfigV1::load(&path)
                .expect_err("non-root or credentialed audit URL must fail closed");
            assert!(error.to_string().contains("root-only HTTPS origin"));
        }
    }

    fn fixture_fee_schedule() -> ProgramFeeSchedule {
        ProgramFeeSchedule {
            fee_schedule_id: "research-state-parity-fixture".to_owned(),
            effective_slot: 1,
            evidence: ProgramFeeScheduleEvidenceV1::CanonicalFixture {
                fixture_id: "research-state-parity".to_owned(),
                transaction_signature: "unit-test".to_owned(),
                observed_slot: 1,
            },
            // Reserve transition is independent of the fee, but the existing
            // quote API intentionally requires an explicit valid schedule.
            rules: vec![ProgramFeeRule {
                component_id: "zero-fee-for-reserve-parity".to_owned(),
                numerator: 0,
                denominator: 1,
                rounding: FeeRounding::Floor,
            }],
        }
    }

    #[test]
    fn finalized_audit_scans_direct_top_level_pump_instruction() {
        let pump = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
            .expect("frozen Pump program id");
        let transaction = audit_transaction(
            vec![pump],
            vec![compiled_instruction(0)],
            None,
            audit_status_meta(
                OptionSerializer::Some(Vec::new()),
                OptionSerializer::Some(UiLoadedAddresses {
                    writable: Vec::new(),
                    readonly: Vec::new(),
                }),
            ),
        );

        let scanned = scan_finalized_audit_transaction_v1(41, 0, &transaction, pump)
            .expect("scan direct Pump transaction")
            .expect("direct Pump invocation must be preserved");
        assert_eq!(scanned.identity.slot, 41);
        assert_eq!(scanned.identity.tx_index, 0);
        assert_eq!(scanned.invocation_class_counts["direct_top_level"], 1);
        assert_eq!(scanned.invocation_class_counts["inner_cpi"], 0);
        assert_eq!(scanned.invocation_class_counts["router_to_pump_cpi"], 0);
        assert_eq!(scanned.invocation_class_counts["v0_loaded_address"], 0);
    }

    #[test]
    fn finalized_audit_scans_router_inner_v0_pump_instruction() {
        let router = Pubkey::new_unique();
        let pump = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
            .expect("frozen Pump program id");
        let lookup_table = Pubkey::new_unique();
        let transaction = audit_transaction(
            vec![router],
            vec![compiled_instruction(0)],
            Some(vec![UiAddressTableLookup {
                account_key: lookup_table.to_string(),
                writable_indexes: vec![0],
                readonly_indexes: Vec::new(),
            }]),
            audit_status_meta(
                OptionSerializer::Some(vec![UiInnerInstructions {
                    index: 0,
                    instructions: vec![UiInstruction::Compiled(compiled_instruction(1))],
                }]),
                OptionSerializer::Some(UiLoadedAddresses {
                    writable: vec![pump.to_string()],
                    readonly: Vec::new(),
                }),
            ),
        );

        let scanned = scan_finalized_audit_transaction_v1(42, 7, &transaction, pump)
            .expect("scan v0 router Pump transaction")
            .expect("inner Pump invocation must be preserved");
        assert_eq!(scanned.identity.slot, 42);
        assert_eq!(scanned.identity.tx_index, 7);
        assert_eq!(scanned.invocation_class_counts["direct_top_level"], 0);
        assert_eq!(scanned.invocation_class_counts["inner_cpi"], 1);
        assert_eq!(scanned.invocation_class_counts["router_to_pump_cpi"], 1);
        assert_eq!(scanned.invocation_class_counts["v0_loaded_address"], 1);
    }

    #[test]
    fn finalized_audit_rejects_missing_inner_instruction_data() {
        let pump = Pubkey::from_str(PUMP_RESEARCH_PUMP_PROGRAM_ID_BASE58_V1)
            .expect("frozen Pump program id");
        let transaction = audit_transaction(
            vec![pump],
            vec![compiled_instruction(0)],
            None,
            audit_status_meta(
                OptionSerializer::None,
                OptionSerializer::Some(UiLoadedAddresses {
                    writable: Vec::new(),
                    readonly: Vec::new(),
                }),
            ),
        );

        let error = scan_finalized_audit_transaction_v1(43, 0, &transaction, pump)
            .expect_err("missing inner instruction data must block qualification");
        assert!(error.to_string().contains("omits inner instructions"));
    }

    #[test]
    fn audit_identity_difference_is_a_multiset_not_a_set() {
        let identity = PumpResearchCanonicalTransactionIdentityV1 {
            slot: 44,
            tx_index: 3,
            signature: PumpResearchStorageSignatureV1::from([7; 64]),
        };
        let transaction = PumpResearchAuditTransactionV1 {
            identity,
            invocation_class_counts: empty_invocation_class_counts(),
            failed: false,
        };
        let (raw_only, audit_only) = audit_identity_multiset_difference(
            &[transaction.clone(), transaction.clone()],
            &[transaction],
        );
        assert_eq!(raw_only, vec![identity]);
        assert!(audit_only.is_empty());
    }

    #[test]
    fn provider_suitability_sampling_is_bounded_and_includes_range_edges() {
        assert_eq!(evenly_spaced_slots(100, 115, 4), vec![100, 105, 110, 115]);
        assert_eq!(evenly_spaced_slots(7, 7, 16), vec![7]);
        assert!(evenly_spaced_slots(9, 8, 16).is_empty());
        assert!(evenly_spaced_slots(1, 2, 0).is_empty());
    }

    #[test]
    fn full_audit_and_provider_suitability_share_failed_status_authority() {
        let identity = PumpResearchCanonicalTransactionIdentityV1 {
            slot: 45,
            tx_index: 4,
            signature: PumpResearchStorageSignatureV1::from([8; 64]),
        };
        let successful = PumpResearchAuditTransactionV1 {
            identity,
            invocation_class_counts: empty_invocation_class_counts(),
            failed: false,
        };
        let failed = PumpResearchAuditTransactionV1 {
            failed: true,
            ..successful.clone()
        };
        assert!(audit_failure_multiset_matches(
            std::slice::from_ref(&successful),
            std::slice::from_ref(&successful),
        ));
        let comparison = compare_audit_transaction_multisets_v1(&[successful], &[failed]);
        assert!(comparison.identity_multiset_matches);
        assert!(comparison.invocation_class_counts_match);
        assert!(!comparison.failed_status_multiset_matches);
        assert_eq!(comparison.raw_failed_transaction_count, 0);
        assert_eq!(comparison.audit_failed_transaction_count, 1);
        assert_eq!(
            qualification_slot_status_from_comparison_v1(&comparison),
            PumpResearchQualificationSlotStatusV1::SourceCoverageUnproven
        );
        assert_eq!(
            qualification_status_from_audit_failures_v1(true, false),
            PumpResearchTapeQualificationStatusV1::Blocked(
                PumpResearchQualificationBlockerV1::SourceCoverageUnproven,
            )
        );
        assert_ne!(
            qualification_status_from_audit_failures_v1(true, false),
            PumpResearchTapeQualificationStatusV1::Ready
        );
    }

    #[test]
    fn provider_suitability_error_redaction_removes_endpoint_and_host() {
        let config = PumpResearchQualificationAuditConfigV1 {
            audit_provider_id: "independent".to_owned(),
            audit_rpc_endpoint: "https://private-audit.example".to_owned(),
            audit_rpc_endpoint_path_env: None,
            audit_rpc_auth_token_env: None,
            audit_rpc_auth_header: "x-api-key".to_owned(),
            bounded_concurrency: 1,
            bounded_retry_count: 2,
            request_timeout_ms: 30_000,
        };
        let connection = PumpResearchResolvedAuditConnectionV1 {
            endpoint: config.audit_rpc_endpoint.clone(),
            endpoint_path_credential: None,
            auth_token: None,
        };
        let redacted = redacted_audit_error(
            &config,
            &connection,
            "request to https://private-audit.example failed on private-audit.example",
        );
        assert!(!redacted.contains("private-audit.example"));
        assert!(redacted.contains("<redacted-audit-endpoint>"));
    }

    #[test]
    fn provider_suitability_output_is_disjoint_from_raw_and_planned_exact() {
        let temporary = tempdir().expect("temporary provider suitability paths");
        let raw = temporary.path().join("run/raw");
        let operator_logs = temporary.path().join("operator-logs");
        std::fs::create_dir_all(&raw).expect("create raw directory");
        std::fs::create_dir_all(&operator_logs).expect("create operator log directory");
        let planned_exact = temporary.path().join("run/exact-go-e-v1");

        let safe = validate_provider_suitability_output_path_v1(
            &raw,
            &planned_exact,
            &operator_logs.join("go-e0"),
        )
        .expect("operator log output remains separate");
        assert_eq!(safe, operator_logs.join("go-e0"));

        assert!(validate_provider_suitability_output_path_v1(
            &raw,
            &planned_exact,
            &raw.join("go-e0"),
        )
        .is_err());
        assert!(
            validate_provider_suitability_output_path_v1(&raw, &planned_exact, &planned_exact,)
                .is_err()
        );
    }

    fn write_provider_audit_authority_raw_fixture_v1(raw_dir: &Path, run_id: &str) {
        let mut slot_100 = epoch_record_fixture(EpochRecordFixtureKind::SlotUpdate, 1, 1);
        let mut slot_101 = epoch_record_fixture(EpochRecordFixtureKind::SlotUpdate, 1, 2);
        let mut slot_102 = epoch_record_fixture(EpochRecordFixtureKind::SlotUpdate, 1, 4);
        for (record, slot, parent) in [
            (&mut slot_100, 100, Some(99)),
            (&mut slot_101, 101, Some(100)),
            (&mut slot_102, 102, Some(101)),
        ] {
            let PumpResearchRawRecordV1::PrimarySlotUpdate(evidence) = record else {
                unreachable!("slot fixture must be a slot update")
            };
            evidence.slot = slot;
            evidence.parent = parent;
            evidence.source_status = 2;
        }
        let mut first_block = epoch_record_fixture(EpochRecordFixtureKind::BlockMeta, 1, 0);
        let PumpResearchRawRecordV1::PrimaryBlockMeta(first_block_evidence) = &mut first_block
        else {
            unreachable!("block fixture must be block meta")
        };
        first_block_evidence.slot = 100;
        first_block_evidence.parent_slot = 99;
        let transaction = provider_suitability_transaction_fixture(1, 3, 101);
        let mut last_block = epoch_record_fixture(EpochRecordFixtureKind::BlockMeta, 1, 5);
        let PumpResearchRawRecordV1::PrimaryBlockMeta(last_block_evidence) = &mut last_block else {
            unreachable!("block fixture must be block meta")
        };
        last_block_evidence.slot = 102;
        last_block_evidence.parent_slot = 101;
        write_epoch_raw_run_fixture(
            raw_dir,
            run_id,
            vec![(
                1,
                vec![
                    first_block,
                    slot_100,
                    slot_101,
                    transaction,
                    slot_102,
                    last_block,
                ],
            )],
        );
    }

    fn replace_first_segment_with_coherent_variant_v1(raw_dir: &Path, run_id: &str) {
        let replacement = tempdir().expect("temporary coherent replacement segment");
        let replacement_record = provider_suitability_transaction_fixture(1, 0, 9_999);
        let _ = write_frozen_segment_fixture(
            replacement.path(),
            run_id,
            0,
            1,
            None,
            &[replacement_record],
            true,
        );
        fs::rename(
            replacement.path().join("segment_00000.bin"),
            raw_dir.join("segment_00000.bin"),
        )
        .expect("atomically replace source segment with coherent alternative bytes");
    }

    fn raw_drift_audit_authority_fixture_v1(
        raw: &PumpResearchRawTapeIndexV1,
        endpoint: String,
        output_dir: &Path,
    ) -> PumpResearchValidatedCombinedAuditAuthorityV1 {
        let audit_config = PumpResearchQualificationAuditConfigV1 {
            audit_provider_id: "fixture-independent-audit".to_owned(),
            audit_rpc_endpoint: endpoint.clone(),
            audit_rpc_endpoint_path_env: None,
            audit_rpc_auth_token_env: None,
            audit_rpc_auth_header: default_audit_rpc_auth_header(),
            bounded_concurrency: 1,
            bounded_retry_count: 0,
            request_timeout_ms: 1_000,
        };
        let anchor_path = raw.segments[0].path.clone();
        let anchor_digest = operator_digest_bytes(
            &fs::read(&anchor_path).expect("read fixture anchor file for digest"),
        );
        let planned_exact_output = canonical_create_new_output_path(output_dir, "fixture exact")
            .expect("canonical fixture output");
        let running_executable_authority = capture_running_executable_authority_v1()
            .expect("capture running test executable authority");
        let provider_independence = PumpResearchValidatedProviderIndependenceV1 {
            attestation_digest: anchor_digest.clone(),
            attestation_path: anchor_path.clone(),
            audit_config_digest: anchor_digest.clone(),
            audit_config_path: anchor_path.clone(),
            provider_suitability_receipt_digest: anchor_digest.clone(),
            provider_suitability_receipt_path: anchor_path.clone(),
            running_executable_authority,
            raw_binding_digest: anchor_digest.clone(),
            raw_binding_path: anchor_path.clone(),
            raw_start_manifest_digest: anchor_digest.clone(),
            raw_start_manifest_path: anchor_path.clone(),
            raw_completion_receipt_digest: anchor_digest,
            raw_completion_receipt_path: anchor_path,
            audit_rpc_endpoint_blake3: blake3::hash(endpoint.as_bytes()).to_hex().to_string(),
            planned_exact_output,
        };
        PumpResearchValidatedCombinedAuditAuthorityV1 {
            audit_config,
            resolved_connection: PumpResearchResolvedAuditConnectionV1 {
                endpoint: endpoint.clone(),
                endpoint_path_credential: None,
                auth_token: None,
            },
            audit_rpc_endpoint_blake3: blake3::hash(endpoint.as_bytes()).to_hex().to_string(),
            provider_independence,
        }
    }

    #[test]
    fn raw_segment_set_snapshot_control_is_hash_bound_and_manifested() {
        let temporary = tempdir().expect("temporary raw segment-set control");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture");
        let run_id = "raw-segment-set-control";
        write_epoch_raw_run_fixture(
            &raw_dir,
            run_id,
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        let output_dir = temporary.path().join("exact");
        let mut raw = index_pump_research_raw_run_v1(&raw_dir).expect("index control raw");
        raw.seal_raw_segment_set_snapshot_v1(&output_dir)
            .expect("seal control raw snapshot");
        raw.revalidate_raw_segment_set_v1("control")
            .expect("hash-equivalent source and snapshot must pass");
        let digest = raw
            .raw_segment_set_blake3_v1()
            .expect("sealed aggregate digest")
            .to_owned();
        assert_eq!(digest.len(), 64);
        assert!(!temporary
            .path()
            .read_dir()
            .expect("read temporary root")
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("raw-snapshot")));
        let manifest = exact_manifest_from_raw(
            &raw,
            PumpResearchTapeQualificationStatusV1::Unqualified,
            None,
        )
        .expect("build control exact manifest");
        assert_eq!(manifest.source_raw_segment_set_blake3, digest);
        let writer = PumpResearchExactOutputWriterV1::create(&output_dir)
            .expect("create control exact partial");
        writer
            .finish(&manifest, || {
                raw.revalidate_raw_segment_set_v1("control-before-publish")
            })
            .expect("publish control exact artifact");
        assert!(output_dir.is_dir());
        assert!(!temporary.path().join(".exact.partial").exists());
    }

    #[test]
    fn verified_go_d_materialization_is_offline_hash_pinned_and_explicit() {
        let temporary = tempdir().expect("temporary verified GO-D fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create verified GO-D raw fixture");
        let run_id = "verified-go-d-fixture";
        write_provider_audit_authority_raw_fixture_v1(&raw_dir, run_id);
        fs::write(
            raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            serde_json::to_vec(&current_capture_provenance_binding_for_test(run_id))
                .expect("serialize current capture binding"),
        )
        .expect("write current capture binding");

        let probe_output = temporary.path().join("probe-exact");
        let mut indexed = index_pump_research_raw_run_v1(&raw_dir).expect("index GO-D fixture");
        indexed
            .seal_raw_segment_set_snapshot_v1(&probe_output)
            .expect("seal GO-D fixture segment authority");
        let controls = indexed
            .revalidate_raw_control_authority_v1(&raw_dir, "test-authority-build")
            .expect("read GO-D fixture controls");
        let authority = PumpResearchGoDSourceAuthorityReceiptV1 {
            schema_version: GO_D_SOURCE_AUTHORITY_SCHEMA_V1.to_owned(),
            source_run_id: run_id.to_owned(),
            source_storage_format_version: PUMP_RESEARCH_STORAGE_FORMAT_VERSION_V1,
            go_d_source_authority: GO_D_SOURCE_AUTHORITY_VERIFIED_V1.to_owned(),
            external_go_e_audit: GO_E_EXTERNAL_AUDIT_RETIRED_V1.to_owned(),
            raw_provenance_binding_sha256: controls
                .provenance_binding_digest
                .expect("fixture binding digest")
                .sha256,
            raw_start_manifest_sha256: controls.start_manifest_digest.sha256,
            raw_completion_receipt_sha256: controls.completion_receipt_digest.sha256,
            raw_segment_set_blake3: indexed
                .raw_segment_set_blake3_v1()
                .expect("fixture segment-set digest")
                .to_owned(),
            operator_decision: "fixture GO-D is verified; GO-E is not a gate".to_owned(),
            created_wall_ms: 1,
        };
        drop(indexed);
        let authority_path = temporary.path().join("go-d-authority.json");
        let authority_bytes =
            serde_json::to_vec_pretty(&authority).expect("serialize GO-D authority fixture");
        fs::write(&authority_path, &authority_bytes).expect("write GO-D authority fixture");
        let authority_sha256 = operator_digest_bytes(&authority_bytes).sha256;

        let output_dir = temporary.path().join("exact-go-d");
        let summary = certify_pump_research_verified_go_d_v1(
            &raw_dir,
            &output_dir,
            &authority_path,
            &authority_sha256,
        )
        .expect("materialize verified GO-D without provider I/O");
        assert_eq!(
            summary.qualification_status,
            PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape
        );
        let manifest: PumpExactResearchTapeManifestV1 = read_json(
            &output_dir.join("manifest.json"),
            PumpResearchAuthorityFileKindV1::ExactManifest,
        )
        .expect("read verified GO-D exact manifest");
        assert_eq!(
            manifest.qualification_status,
            PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape
        );
        assert_eq!(
            manifest.go_d_source_authority,
            GO_D_SOURCE_AUTHORITY_VERIFIED_V1
        );
        assert!(manifest.external_go_e_audit_not_used_as_gate);
        assert_eq!(manifest.go_d_source_authority_sha256, authority_sha256);
        let report: serde_json::Value = serde_json::from_slice(
            &fs::read(
                output_dir
                    .join("authority")
                    .join("go_d_source_authority_v1.json"),
            )
            .expect("read GO-D authority report"),
        )
        .expect("decode GO-D authority report");
        assert_eq!(report["GO_D_SOURCE_AUTHORITY"], "VERIFIED");
        assert_eq!(report["EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE"], true);

        let stale_output = temporary.path().join("stale-exact-go-d");
        let mut stale_authority = authority;
        stale_authority.raw_segment_set_blake3 = "00".repeat(32);
        let stale_path = temporary.path().join("stale-go-d-authority.json");
        let stale_bytes =
            serde_json::to_vec_pretty(&stale_authority).expect("serialize stale authority");
        fs::write(&stale_path, &stale_bytes).expect("write stale authority");
        let stale_error = certify_pump_research_verified_go_d_v1(
            &raw_dir,
            &stale_output,
            &stale_path,
            &operator_digest_bytes(&stale_bytes).sha256,
        )
        .expect_err("stale segment-set authority must fail closed");
        assert!(stale_error
            .to_string()
            .contains("raw segment-set binding is stale"));
        assert!(!stale_output.exists());
    }

    #[cfg(unix)]
    #[test]
    fn raw_index_rejects_segment_symlink_before_frozen_scan() {
        use std::os::unix::fs::symlink;

        let temporary = tempdir().expect("temporary raw symlink fixture");
        let run_id = "raw-segment-symlink";
        write_epoch_raw_run_fixture(
            temporary.path(),
            run_id,
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        let segment = temporary.path().join("segment_00000.bin");
        let target = temporary.path().join("segment_target.bin");
        fs::rename(&segment, &target).expect("move real segment behind symlink");
        symlink(&target, &segment).expect("create segment symlink");
        let error = index_pump_research_raw_run_v1(temporary.path())
            .expect_err("raw segment symlink must fail before frozen scan");
        assert!(error.to_string().contains("regular non-symlink file"));
    }

    #[cfg(unix)]
    #[test]
    fn raw_open_regular_to_fifo_race_is_nonblocking_and_rejected() {
        use std::{ffi::CString, os::unix::ffi::OsStrExt};

        let temporary = tempdir().expect("temporary regular-to-FIFO race fixture");
        let path = temporary.path().join("segment.bin");
        fs::write(&path, b"regular-before-open").expect("write regular precheck file");
        let writer_path = path.clone();
        let mut delayed_writer = None;
        let started = Instant::now();
        let error =
            open_regular_nofollow_with_preopen_hook_v1(&path, "raw segment race fixture", || {
                fs::remove_file(&path).expect("remove regular file before FIFO replacement");
                let c_path = CString::new(path.as_os_str().as_bytes())
                    .expect("FIFO fixture path contains no NUL");
                // SAFETY: c_path is a valid, NUL-terminated pathname and the
                // call only creates a test FIFO with owner-only permissions.
                let result = unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) };
                assert_eq!(result, 0, "create FIFO replacement");
                delayed_writer = Some(thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    let mut options = OpenOptions::new();
                    options
                        .write(true)
                        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
                    let _ = options.open(writer_path);
                }));
                Ok(())
            })
            .expect_err("post-precheck FIFO must be rejected without blocking");
        let open_elapsed = started.elapsed();
        delayed_writer
            .expect("delayed FIFO writer was installed")
            .join()
            .expect("join delayed FIFO writer");
        assert!(
            open_elapsed < Duration::from_millis(750),
            "O_NONBLOCK open must reject FIFO before delayed writer; elapsed={open_elapsed:?}"
        );
        assert!(error.to_string().contains("is not a regular file"));
    }

    #[cfg(unix)]
    #[test]
    fn bounded_authority_reader_rejects_regular_to_fifo_race_without_blocking() {
        let temporary = tempdir().expect("temporary authority FIFO race fixture");
        let path = temporary.path().join("run_start_manifest.json");
        let backup = temporary.path().join("run_start_manifest.backup");
        fs::write(&path, b"{}").expect("write regular authority fixture");
        let writer_path = path.clone();
        let mut delayed_writer = None;
        let started = Instant::now();
        let error = read_bounded_regular_file_with_hooks_v1(
            &path,
            "raw start manifest race fixture",
            RAW_START_MANIFEST_MAX_BYTES_V1,
            || {
                replace_regular_file_with_fifo_v1(&path, &backup);
                delayed_writer = Some(thread::spawn(move || {
                    thread::sleep(Duration::from_secs(1));
                    let mut options = OpenOptions::new();
                    options
                        .write(true)
                        .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
                    let _ = options.open(writer_path);
                }));
                Ok(())
            },
            || Ok(()),
        )
        .expect_err("authority FIFO replacement must be rejected without blocking");
        let elapsed = started.elapsed();
        delayed_writer
            .expect("delayed authority FIFO writer was installed")
            .join()
            .expect("join delayed authority FIFO writer");
        restore_regular_file_after_fifo_v1(&path, &backup);
        assert!(
            elapsed < Duration::from_millis(750),
            "bounded authority open waited for FIFO peer: {elapsed:?}"
        );
        assert!(error.to_string().contains("is not a regular file"));
    }

    #[test]
    fn bounded_authority_reader_rejects_growth_and_per_kind_size_limit() {
        let temporary = tempdir().expect("temporary bounded authority fixture");
        let path = temporary.path().join("authority.json");
        fs::write(&path, b"{}").expect("write bounded authority fixture");
        let started = Instant::now();
        let error = read_bounded_regular_file_with_hooks_v1(
            &path,
            "growing authority fixture",
            RAW_START_MANIFEST_MAX_BYTES_V1,
            || Ok(()),
            || {
                let mut append = OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .expect("open authority fixture for append");
                append
                    .write_all(b"growth-after-bound")
                    .expect("append authority growth");
                Ok(())
            },
        )
        .expect_err("authority growth must fail after exact bounded read");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error
            .to_string()
            .contains("changed length while being read"));

        let oversized = temporary.path().join("oversized-start-manifest.json");
        fs::write(
            &oversized,
            vec![0_u8; (RAW_START_MANIFEST_MAX_BYTES_V1 + 1) as usize],
        )
        .expect("write oversized authority fixture");
        let limit_error = read_bounded_authority_file_v1(
            &oversized,
            PumpResearchAuthorityFileKindV1::RawStartManifest,
        )
        .expect_err("per-kind authority limit must fail before allocation/read");
        assert!(limit_error.to_string().contains("above the"));
    }

    #[test]
    fn indexed_raw_control_authority_rejects_parse_to_digest_snapshot_drift() {
        let temporary = tempdir().expect("temporary raw control authority fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw control fixture directory");
        write_epoch_raw_run_fixture(
            &raw_dir,
            "raw-control-authority",
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        fs::write(
            raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            b"fixture-binding",
        )
        .expect("write raw control binding fixture");
        let raw = index_pump_research_raw_run_v1(&raw_dir).expect("index raw control fixture");
        let paths = [
            raw_dir.join("run_start_manifest.json"),
            raw_dir.join("run_completion_receipt.json"),
            raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
        ];
        for path in paths {
            let original = fs::read(&path).expect("read original raw control fixture");
            let mut drifted = original.clone();
            drifted.extend_from_slice(b"\n ");
            fs::write(&path, drifted).expect("write same-parse different-digest control fixture");
            let error = raw
                .revalidate_raw_control_authority_v1(&raw_dir, "test-parse-digest-boundary")
                .expect_err("raw control digest drift after parse must fail");
            assert!(error.to_string().contains("changed after bounded parse"));
            fs::write(&path, original).expect("restore raw control fixture");
            raw.revalidate_raw_control_authority_v1(&raw_dir, "test-restored-control")
                .expect("restored raw control authority passes");
        }
    }

    #[test]
    fn bounded_hash_rejects_growth_after_expected_bytes() {
        let temporary = tempdir().expect("temporary growing-file fixture");
        let path = temporary.path().join("segment.bin");
        fs::write(&path, vec![7_u8; 1024 * 1024]).expect("write bounded source fixture");
        let file = open_regular_nofollow_v1(&path, "growing raw segment")
            .expect("open growing-file control");
        let expected_bytes = file.metadata().expect("source metadata").len();
        let started = Instant::now();
        let error = hash_open_file_exact_with_post_read_hook_v1(
            &file,
            expected_bytes,
            "growing raw segment",
            || {
                let mut append = OpenOptions::new()
                    .append(true)
                    .open(&path)
                    .expect("open growing source for append");
                append
                    .write_all(b"growth-after-exact-bound")
                    .expect("append deterministic growth");
                append.sync_all().expect("sync deterministic growth");
                Ok(())
            },
        )
        .expect_err("growth beyond the pinned size must fail");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("size after bounded hash"));
    }

    #[test]
    fn bounded_snapshot_copy_rejects_growth_after_expected_bytes() {
        let temporary = tempdir().expect("temporary growing-copy fixture");
        let source_path = temporary.path().join("segment.bin");
        let source_bytes = vec![9_u8; 1024 * 1024];
        fs::write(&source_path, &source_bytes).expect("write bounded copy source");
        let sha256: [u8; 32] = Sha256::digest(&source_bytes).into();
        let receipt = PumpResearchSegmentReceiptV1 {
            segment_index: 0,
            filename: "segment.bin".to_owned(),
            file_sha256: PumpResearchStorageHashV1::from(sha256),
            file_blake3: PumpResearchStorageHashV1::from(*blake3::hash(&source_bytes).as_bytes()),
            first_capture_sequence: None,
            last_capture_sequence: None,
            accepted_record_count: 0,
        };
        let expected_bytes = u64::try_from(source_bytes.len()).expect("fixture length fits u64");
        let started = Instant::now();
        let error = copy_raw_segment_to_unlinked_snapshot_with_post_read_hook_v1(
            &source_path,
            temporary.path(),
            &receipt,
            expected_bytes,
            || {
                let mut append = OpenOptions::new()
                    .append(true)
                    .open(&source_path)
                    .expect("open copy source for deterministic growth");
                append
                    .write_all(b"growth-after-copy-bound")
                    .expect("append copy growth");
                append.sync_all().expect("sync copy growth");
                Ok(())
            },
        )
        .expect_err("snapshot copy must reject growth beyond indexed size");
        assert!(started.elapsed() < Duration::from_secs(1));
        assert!(error.to_string().contains("size after snapshot copy"));
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn anonymous_raw_snapshot_has_no_pathname_cleanup_surface_on_error() {
        let temporary = tempdir().expect("temporary anonymous snapshot fixture");
        let source_path = temporary.path().join("segment.bin");
        let foreign_dir = temporary.path().join(".pump-research-raw-snapshot-foreign");
        fs::create_dir(&foreign_dir).expect("create foreign directory fixture");
        fs::write(foreign_dir.join("foreign-marker"), b"not-owned").expect("write foreign marker");
        let source_bytes = b"anonymous-snapshot-source";
        fs::write(&source_path, source_bytes).expect("write anonymous snapshot source");
        let sha256: [u8; 32] = Sha256::digest(source_bytes).into();
        let receipt = PumpResearchSegmentReceiptV1 {
            segment_index: 0,
            filename: "segment.bin".to_owned(),
            file_sha256: PumpResearchStorageHashV1::from(sha256),
            file_blake3: PumpResearchStorageHashV1::from(*blake3::hash(source_bytes).as_bytes()),
            first_capture_sequence: None,
            last_capture_sequence: None,
            accepted_record_count: 0,
        };
        let before = directory_entry_digest_snapshot_v1(temporary.path());
        let error = copy_raw_segment_to_unlinked_snapshot_with_post_read_hook_v1(
            &source_path,
            temporary.path(),
            &receipt,
            u64::try_from(source_bytes.len()).expect("fixture length fits u64"),
            || bail!("synthetic anonymous snapshot failure"),
        )
        .expect_err("anonymous snapshot hook failure must propagate");
        assert!(error
            .to_string()
            .contains("synthetic anonymous snapshot failure"));
        assert_eq!(directory_entry_digest_snapshot_v1(temporary.path()), before);
        assert_eq!(
            fs::read(foreign_dir.join("foreign-marker")).expect("foreign marker survives"),
            b"not-owned"
        );

        let (pinned, bytes) = copy_raw_segment_to_unlinked_snapshot_v1(
            &source_path,
            temporary.path(),
            &receipt,
            u64::try_from(source_bytes.len()).expect("fixture length fits u64"),
        )
        .expect("successful anonymous snapshot");
        assert_eq!(bytes, source_bytes.len() as u64);
        assert_eq!(
            pinned.metadata().expect("anonymous metadata").nlink(),
            0,
            "O_TMPFILE snapshot must have no pathname"
        );
        let mut pinned_ref: &File = pinned.as_ref();
        let write_error = pinned_ref
            .write_all(b"forbidden")
            .expect_err("pinned anonymous authority descriptor must be read-only");
        assert_eq!(write_error.raw_os_error(), Some(libc::EBADF));
        assert_eq!(directory_entry_digest_snapshot_v1(temporary.path()), before);
    }

    fn directory_entry_digest_snapshot_v1(path: &Path) -> BTreeMap<String, String> {
        let mut snapshot = BTreeMap::new();
        for entry in fs::read_dir(path).expect("read fixture directory") {
            let entry = entry.expect("fixture directory entry");
            let name = entry.file_name().to_string_lossy().into_owned();
            let metadata = fs::symlink_metadata(entry.path()).expect("fixture entry metadata");
            let value = if metadata.is_file() {
                operator_digest_bytes(&fs::read(entry.path()).expect("read fixture regular file"))
                    .sha256
            } else if metadata.is_dir() {
                "directory".to_owned()
            } else if metadata.file_type().is_symlink() {
                "symlink".to_owned()
            } else {
                "special".to_owned()
            };
            snapshot.insert(name, value);
        }
        snapshot
    }

    #[tokio::test]
    async fn public_combined_output_inside_raw_fails_before_any_side_effect_or_provider_io() {
        let temporary = tempdir().expect("temporary output-inside-raw fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture directory");
        write_epoch_raw_run_fixture(
            &raw_dir,
            "output-inside-raw",
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        let before = directory_entry_digest_snapshot_v1(&raw_dir);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind zero-I/O probe");
        listener
            .set_nonblocking(true)
            .expect("set zero-I/O probe nonblocking");
        let audit_config_path = temporary.path().join("audit.toml");
        fs::write(
            &audit_config_path,
            format!(
                "audit_provider_id = \"fixture-audit\"\naudit_rpc_endpoint = \"https://{}\"\nbounded_concurrency = 1\nbounded_retry_count = 0\nrequest_timeout_ms = 1000\n",
                listener.local_addr().expect("zero-I/O address")
            ),
        )
        .expect("write zero-I/O audit config");
        let output_dir = raw_dir.join("exact-invalid");
        let error = tokio::time::timeout(
            Duration::from_secs(2),
            certify_pump_research_raw_run_with_qualification_audit_v1(
                &raw_dir,
                &output_dir,
                &audit_config_path,
                &temporary.path().join("missing-suitability.json"),
                &temporary.path().join("missing-attestation.json"),
                &"00".repeat(32),
            ),
        )
        .await
        .expect("output/raw boundary must terminate locally")
        .expect_err("output inside immutable raw must be rejected");
        assert!(error
            .to_string()
            .contains("outside and disjoint from immutable raw evidence"));
        assert_eq!(directory_entry_digest_snapshot_v1(&raw_dir), before);
        assert!(!output_dir.exists());
        assert!(!raw_dir
            .read_dir()
            .expect("read raw after rejection")
            .flatten()
            .any(|entry| entry.file_name().to_string_lossy().contains("raw-snapshot")));
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
    }

    #[tokio::test]
    async fn raw_segment_drift_before_provider_io_fails_with_zero_requests() {
        let temporary = tempdir().expect("temporary pre-provider drift fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture");
        let run_id = "raw-drift-before-provider";
        write_provider_audit_authority_raw_fixture_v1(&raw_dir, run_id);
        let output_dir = temporary.path().join("exact");
        let mut raw = index_pump_research_raw_run_v1(&raw_dir).expect("index raw fixture");
        raw.seal_raw_segment_set_snapshot_v1(&output_dir)
            .expect("seal raw snapshot");
        let canonicality = build_slot_canonicality_index(&raw);
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind request counter");
        listener
            .set_nonblocking(true)
            .expect("set request counter nonblocking");
        let endpoint = format!(
            "https://{}/",
            listener.local_addr().expect("request counter address")
        );
        let authority = raw_drift_audit_authority_fixture_v1(&raw, endpoint, &output_dir);
        replace_first_segment_with_coherent_variant_v1(&raw_dir, run_id);
        let error = run_independent_source_completeness_audit_v1(&raw, &canonicality, &authority)
            .await
            .expect_err("source drift must fail before provider I/O");
        assert!(error
            .to_string()
            .contains("after-audit-fingerprints-before-provider-io"));
        assert!(matches!(
            listener.accept(),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
        ));
        assert!(!output_dir.exists());
    }

    #[test]
    fn raw_segment_drift_after_audit_fails_before_exact_writer() {
        let temporary = tempdir().expect("temporary post-audit drift fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture");
        let run_id = "raw-drift-after-audit";
        write_epoch_raw_run_fixture(
            &raw_dir,
            run_id,
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        let output_dir = temporary.path().join("exact");
        let mut raw = index_pump_research_raw_run_v1(&raw_dir).expect("index raw fixture");
        raw.seal_raw_segment_set_snapshot_v1(&output_dir)
            .expect("seal raw snapshot");
        let canonicality = build_slot_canonicality_index(&raw);
        replace_first_segment_with_coherent_variant_v1(&raw_dir, run_id);
        let error = certify_indexed_pump_research_raw_run_v1(
            raw,
            canonicality,
            None,
            None,
            None,
            &output_dir,
        )
        .expect_err("post-audit drift must fail before exact writer");
        assert!(error
            .to_string()
            .contains("after-provider-audit-before-account-anchors"));
        assert!(!output_dir.exists());
        assert!(!temporary.path().join(".exact.partial").exists());
    }

    #[test]
    fn raw_segment_drift_during_materialization_blocks_final_publish() {
        let temporary = tempdir().expect("temporary materialization drift fixture");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture");
        let run_id = "raw-drift-during-materialization";
        write_epoch_raw_run_fixture(
            &raw_dir,
            run_id,
            vec![(1, vec![provider_suitability_transaction_fixture(1, 0, 101)])],
        );
        let output_dir = temporary.path().join("exact");
        let partial_dir = temporary.path().join(".exact.partial");
        let mut raw = index_pump_research_raw_run_v1(&raw_dir).expect("index raw fixture");
        raw.seal_raw_segment_set_snapshot_v1(&output_dir)
            .expect("seal raw snapshot");
        let canonicality = build_slot_canonicality_index(&raw);
        let error = certify_indexed_pump_research_raw_run_with_final_check_hook_v1(
            raw,
            canonicality,
            None,
            None,
            None,
            &output_dir,
            |_| {
                assert!(partial_dir.is_dir());
                replace_first_segment_with_coherent_variant_v1(&raw_dir, run_id);
                Ok(())
            },
        )
        .expect_err("materialization-time drift must block final rename");
        assert!(error
            .to_string()
            .contains("after-materialization-before-exact-publish"));
        assert!(!output_dir.exists());
        assert!(partial_dir.is_dir());
    }

    async fn assert_combined_authority_fixture_v1(
        verify_endpoint_drift: bool,
        verify_public_pre_io_boundary: bool,
        verify_public_fifo_authorities: bool,
        verify_late_fifo_revalidation: bool,
        attested_running_executable_digest_override: Option<PumpResearchOperatorDigestV1>,
    ) {
        let temporary = tempdir().expect("temporary provider-independence authority");
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind request-count probe");
        listener
            .set_nonblocking(true)
            .expect("set request-count probe nonblocking");
        let endpoint_origin = format!(
            "https://{}",
            listener.local_addr().expect("request-count probe address")
        );
        let endpoint_path_env = if verify_endpoint_drift {
            "PUMP_RESEARCH_G5_1_TEST_ENDPOINT_PATH_DRIFT"
        } else {
            "PUMP_RESEARCH_G5_1_TEST_ENDPOINT_PATH_PUBLIC"
        };
        let _endpoint_guard =
            TestEnvironmentVariableV1::set(endpoint_path_env, "/validated-authority-a");
        let raw_dir = temporary.path().join("raw");
        fs::create_dir(&raw_dir).expect("create raw fixture directory");
        let run_id = "provider-independence-fixture";
        write_provider_audit_authority_raw_fixture_v1(&raw_dir, run_id);
        fs::write(
            raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            b"fixture-binding",
        )
        .expect("write raw binding fixture");
        let mut raw =
            index_pump_research_raw_run_v1(&raw_dir).expect("index raw authority fixture");
        let canonicality = build_slot_canonicality_index(&raw);
        assert_eq!(
            qualification_range_selection_v1(&raw, &canonicality),
            ready_range(1, 101, 102)
        );

        let audit_config_path = temporary.path().join("audit.toml");
        let audit_config_bytes = format!(
            r#"
audit_provider_id = "fixture-independent-audit"
audit_rpc_endpoint = {endpoint_origin:?}
audit_rpc_endpoint_path_env = {endpoint_path_env:?}
bounded_concurrency = 1
bounded_retry_count = 2
request_timeout_ms = 30000
"#
        );
        fs::write(&audit_config_path, audit_config_bytes.as_bytes())
            .expect("write audit config fixture");
        let audit_config_digest = operator_digest_bytes(audit_config_bytes.as_bytes());
        let audit_config = PumpResearchQualificationAuditConfigV1::from_bytes(
            &audit_config_path,
            audit_config_bytes.as_bytes(),
        )
        .expect("parse audit config fixture");
        let resolved_connection = audit_config
            .resolve_connection()
            .expect("resolve fixture authority A");
        let expected_plan =
            build_provider_suitability_plan_v1(&raw, &canonicality).expect("build exact plan");
        let expected_range = expected_plan
            .qualification_range
            .expect("fixture has qualification range");
        let sample_slots: BTreeSet<_> = expected_plan.slots.keys().copied().collect();
        let raw_by_slot = collect_provider_suitability_raw_transactions_v1(
            &raw,
            &canonicality,
            expected_range,
            &sample_slots,
        )
        .expect("collect deterministic raw samples");
        let raw_binding_digest = digest_bounded_authority_file_v1(
            &raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            PumpResearchAuthorityFileKindV1::RawProvenanceBinding,
        )
        .expect("digest fixture binding");
        let raw_start_manifest_digest = digest_bounded_authority_file_v1(
            &raw_dir.join("run_start_manifest.json"),
            PumpResearchAuthorityFileKindV1::RawStartManifest,
        )
        .expect("digest fixture start manifest");
        let raw_completion_receipt_digest = digest_bounded_authority_file_v1(
            &raw_dir.join("run_completion_receipt.json"),
            PumpResearchAuthorityFileKindV1::RawCompletionReceipt,
        )
        .expect("digest fixture completion receipt");
        let suitability_executable_digest = operator_digest_bytes(b"fixture-go-e0-executable");
        let slot_findings: Vec<_> = expected_plan
            .slots
            .iter()
            .map(|(slot, roles)| {
                let raw_transactions = raw_by_slot.get(slot).map(Vec::as_slice).unwrap_or_default();
                let comparison =
                    compare_audit_transaction_multisets_v1(raw_transactions, raw_transactions);
                PumpResearchProviderSuitabilitySlotFindingV1 {
                    schema_version: 1,
                    slot: *slot,
                    selection_roles: roles.iter().cloned().collect(),
                    fetch_status: PumpResearchProviderSuitabilityFetchStatusV1::Block,
                    request_attempt_count: 1,
                    request_elapsed_ms: 1,
                    raw_identity_count: comparison.raw_identity_count,
                    audit_identity_count: comparison.audit_identity_count,
                    raw_failed_transaction_count: comparison.raw_failed_transaction_count,
                    audit_failed_transaction_count: comparison.audit_failed_transaction_count,
                    raw_invocation_class_counts: comparison.raw_invocation_class_counts,
                    audit_invocation_class_counts: comparison.audit_invocation_class_counts,
                    raw_only_identities: comparison.raw_only_identities,
                    audit_only_identities: comparison.audit_only_identities,
                    identity_multiset_matches: comparison.identity_multiset_matches,
                    invocation_class_counts_match: comparison.invocation_class_counts_match,
                    failed_status_multiset_matches: comparison.failed_status_multiset_matches,
                    audit_error: None,
                }
            })
            .collect();
        assert_eq!(slot_findings.len(), 2);
        assert!(expected_plan.missing_raw_representative_roles.is_empty());
        let suitability = PumpResearchProviderSuitabilityReceiptV1 {
            schema_version: 1,
            kind: "pump_research_provider_suitability_v1".to_owned(),
            created_wall_ms: 1,
            source_run_id: run_id.to_owned(),
            status: PumpResearchProviderSuitabilityStatusV1::ReadyForFullAudit,
            preparation_receipt_digest: operator_digest_bytes(b"fixture-preparation"),
            audit_config_digest: audit_config_digest.clone(),
            executable_digest: suitability_executable_digest.clone(),
            raw_binding_digest: raw_binding_digest.clone(),
            raw_start_manifest_digest: raw_start_manifest_digest.clone(),
            raw_completion_receipt_digest: raw_completion_receipt_digest.clone(),
            audit_provider_id: "fixture-independent-audit".to_owned(),
            audit_rpc_endpoint_blake3: blake3::hash(resolved_connection.endpoint.as_bytes())
                .to_hex()
                .to_string(),
            audit_auth_mode: qualification_audit_auth_mode_v1(&audit_config).to_owned(),
            provider_identity_independence_verified: false,
            qualification_stream_epoch: Some(1),
            qualification_start_slot: Some(101),
            qualification_end_slot: Some(102),
            qualification_blocker: None,
            bounded_concurrency: 1,
            bounded_retry_count: 2,
            request_timeout_ms: 30_000,
            burst_slot_target_count: 16,
            max_raw_representative_scan: 250_000,
            max_consecutive_unavailable: 3,
            max_provider_wall_ms: 420_000,
            raw_representative_transactions_examined: expected_plan
                .raw_representative_transactions_examined,
            missing_raw_representative_roles: expected_plan
                .missing_raw_representative_roles
                .clone(),
            sample_slot_count: slot_findings.len(),
            attempted_slot_count: slot_findings.len(),
            matched_slot_count: slot_findings.len(),
            unavailable_slot_count: 0,
            total_request_attempt_count: u64::try_from(slot_findings.len())
                .expect("fixture sample count fits u64"),
            provider_elapsed_ms: 1,
            slot_findings,
            provider_io_performed: true,
            raw_write_attempt_count: 0,
            exact_output_created: false,
            certify_started: false,
            export_started: false,
            strategy_started: false,
        };
        let mut duplicate_slot_receipt = suitability.clone();
        duplicate_slot_receipt.slot_findings[1] = duplicate_slot_receipt.slot_findings[0].clone();
        let duplicate_error = validate_provider_suitability_receipt_for_combined_audit_v1(
            &duplicate_slot_receipt,
            &raw,
            &canonicality,
            &audit_config,
            &resolved_connection.endpoint,
            &audit_config_digest,
            &raw_binding_digest,
            &raw_start_manifest_digest,
            &raw_completion_receipt_digest,
        )
        .expect_err("duplicate sample slot must fail combined authority validation");
        assert!(duplicate_error.to_string().contains("duplicate finding"));

        let mut role_drift_receipt = suitability.clone();
        role_drift_receipt.slot_findings[0]
            .selection_roles
            .push("unplanned-role".to_owned());
        let role_error = validate_provider_suitability_receipt_for_combined_audit_v1(
            &role_drift_receipt,
            &raw,
            &canonicality,
            &audit_config,
            &resolved_connection.endpoint,
            &audit_config_digest,
            &raw_binding_digest,
            &raw_start_manifest_digest,
            &raw_completion_receipt_digest,
        )
        .expect_err("sample role drift must fail combined authority validation");
        assert!(role_error.to_string().contains("deterministic raw plan"));

        let suitability_path = temporary
            .path()
            .join("provider_suitability_receipt_v1.json");
        let suitability_bytes =
            serde_json::to_vec_pretty(&suitability).expect("serialize suitability fixture");
        fs::write(&suitability_path, &suitability_bytes).expect("write suitability fixture");
        let suitability_digest = operator_digest_bytes(&suitability_bytes);
        let output_dir = temporary.path().join("exact");
        let canonical_output = canonical_create_new_output_path(&output_dir, "fixture exact")
            .expect("canonical fixture exact output");
        raw.seal_raw_segment_set_snapshot_v1(&output_dir)
            .expect("seal fixture raw segment-set authority");
        let running_executable_authority = capture_running_executable_authority_v1()
            .expect("capture running test executable authority");
        let attested_running_executable_digest = attested_running_executable_digest_override
            .clone()
            .unwrap_or_else(|| running_executable_authority.digest().clone());
        let attestation = PumpResearchProviderIndependenceAttestationV1 {
            attestation_version: "v1".to_owned(),
            source_run_id: run_id.to_owned(),
            go_e0_receipt_sha256: suitability_digest.sha256.clone(),
            primary_provider: PumpResearchProviderIndependenceIdentityV1 {
                provider_id: "epoch-corruption-fixture".to_owned(),
                service_type: "yellowstone_grpc_geyser".to_owned(),
                entity_name: "Fixture Primary LLC".to_owned(),
                infrastructure_type: "fixture_streaming_node".to_owned(),
                network_autonomous_system: "fixture-as-primary".to_owned(),
                primary_datacenter_location: "fixture-region-primary".to_owned(),
            },
            audit_provider: PumpResearchProviderIndependenceIdentityV1 {
                provider_id: "fixture-independent-audit".to_owned(),
                service_type: "json_rpc_read_only".to_owned(),
                entity_name: "Fixture Audit LLC".to_owned(),
                infrastructure_type: "fixture_rpc_node".to_owned(),
                network_autonomous_system: "fixture-as-audit".to_owned(),
                primary_datacenter_location: "fixture-region-audit".to_owned(),
            },
            independence_assertions: PumpResearchProviderIndependenceAssertionsV1 {
                distinct_legal_entities: true,
                distinct_infrastructure_operators: true,
                distinct_network_routing_paths: true,
                distinct_ingest_architecture: true,
                zero_shared_credential_domain: true,
                independent_retention_and_indexing: true,
            },
            attestation_status: "verified_independent".to_owned(),
            reviewer_signoff: PumpResearchProviderIndependenceReviewerSignoffV1 {
                reviewer_id: "fixture-reviewer".to_owned(),
                operator_assertion: "Synthetic providers have disjoint failure domains".to_owned(),
                evidence_references: vec!["fixture-evidence-digest".to_owned()],
                created_wall_ms: 1,
            },
            evidence_bindings: PumpResearchProviderIndependenceBindingsV1 {
                audit_rpc_endpoint_blake3: blake3::hash(resolved_connection.endpoint.as_bytes())
                    .to_hex()
                    .to_string(),
                audit_config_digest,
                provider_suitability_receipt_digest: suitability_digest.clone(),
                provider_suitability_executable_digest: suitability_executable_digest,
                combined_certifier_executable_digest: attested_running_executable_digest,
                raw_binding_digest,
                raw_start_manifest_digest,
                raw_completion_receipt_digest,
                qualification_stream_epoch: 1,
                qualification_start_slot: 101,
                qualification_end_slot: 102,
                planned_exact_output: canonical_output
                    .to_str()
                    .expect("UTF-8 exact fixture path")
                    .to_owned(),
            },
        };
        let attestation_path = temporary
            .path()
            .join("provider_independence_attestation_v1.json");
        let attestation_bytes =
            serde_json::to_vec_pretty(&attestation).expect("serialize attestation fixture");
        fs::write(&attestation_path, &attestation_bytes).expect("write attestation fixture");
        let expected_attestation_sha256 = operator_digest_bytes(&attestation_bytes).sha256;
        if attested_running_executable_digest_override.is_some() {
            let error = certify_pump_research_raw_run_with_qualification_audit_v1(
                &raw_dir,
                &output_dir,
                &audit_config_path,
                &suitability_path,
                &attestation_path,
                &expected_attestation_sha256,
            )
            .await
            .expect_err("pathname replacement digest must not attest the running executable");
            assert!(
                error
                    .to_string()
                    .contains("running executable binding is stale"),
                "unexpected running-executable mismatch: {error:#}"
            );
            assert!(matches!(
                listener.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ));
            assert!(!output_dir.exists());
            assert!(!temporary.path().join(".exact.partial").exists());
            assert!(!temporary
                .path()
                .read_dir()
                .expect("read pathname-replacement fixture root")
                .flatten()
                .any(|entry| entry.file_name().to_string_lossy().contains("raw-snapshot")));
            return;
        }
        let validated = validate_provider_independence_attestation_v1(
            &raw,
            &canonicality,
            &output_dir,
            &audit_config_path,
            &suitability_path,
            &attestation_path,
            &expected_attestation_sha256,
            running_executable_authority.clone(),
        )
        .expect("valid hash-pinned attestation must pass before provider I/O");
        assert!(!output_dir.exists());

        #[cfg(unix)]
        if verify_public_fifo_authorities {
            let authority_paths = [
                (
                    "raw start manifest",
                    raw_dir.join("run_start_manifest.json"),
                ),
                (
                    "raw completion receipt",
                    raw_dir.join("run_completion_receipt.json"),
                ),
                (
                    "raw provenance binding",
                    raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
                ),
                ("qualification audit config", audit_config_path.clone()),
                ("provider suitability receipt", suitability_path.clone()),
                (
                    "provider-independence attestation",
                    attestation_path.clone(),
                ),
            ];
            for (index, (label, path)) in authority_paths.iter().enumerate() {
                let backup = temporary
                    .path()
                    .join(format!("public-fifo-authority-{index}.backup"));
                replace_regular_file_with_fifo_v1(path, &backup);
                let writer_path = path.clone();
                let (cancel_tx, cancel_rx) = mpsc::channel();
                let delayed_writer = thread::spawn(move || {
                    if cancel_rx.recv_timeout(Duration::from_secs(8)).is_err() {
                        let mut options = OpenOptions::new();
                        options
                            .write(true)
                            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
                        let _ = options.open(writer_path);
                    }
                });
                let started = Instant::now();
                let outcome = certify_pump_research_raw_run_with_qualification_audit_v1(
                    &raw_dir,
                    &output_dir,
                    &audit_config_path,
                    &suitability_path,
                    &attestation_path,
                    &expected_attestation_sha256,
                )
                .await;
                let elapsed = started.elapsed();
                let _ = cancel_tx.send(());
                delayed_writer.join().expect("join delayed FIFO releaser");
                restore_regular_file_after_fifo_v1(path, &backup);
                let error = outcome.expect_err("FIFO authority must fail public combined locally");
                assert!(
                    elapsed < Duration::from_secs(6),
                    "public combined blocked on {label} FIFO for {elapsed:?}: {error:#}"
                );
                assert!(
                    error.to_string().contains("regular non-symlink file")
                        || error.to_string().contains("not a regular file"),
                    "unexpected {label} FIFO error: {error:#}"
                );
                assert!(matches!(
                    listener.accept(),
                    Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
                ));
                assert!(!output_dir.exists());
                assert!(!temporary.path().join(".exact.partial").exists());
                assert!(!temporary
                    .path()
                    .read_dir()
                    .expect("read public FIFO fixture root")
                    .flatten()
                    .any(|entry| entry.file_name().to_string_lossy().contains("raw-snapshot")));
            }
        }

        #[cfg(unix)]
        if verify_late_fifo_revalidation {
            let authority_paths = [
                (
                    "provider-independence attestation",
                    attestation_path.clone(),
                ),
                ("qualification audit config", audit_config_path.clone()),
                ("provider suitability receipt", suitability_path.clone()),
                (
                    "raw provenance binding",
                    raw_dir.join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
                ),
                (
                    "raw start manifest",
                    raw_dir.join("run_start_manifest.json"),
                ),
                (
                    "raw completion receipt",
                    raw_dir.join("run_completion_receipt.json"),
                ),
            ];
            for (index, (label, path)) in authority_paths.iter().enumerate() {
                let backup = temporary
                    .path()
                    .join(format!("late-fifo-authority-{index}.backup"));
                replace_regular_file_with_fifo_v1(path, &backup);
                let writer_path = path.clone();
                let (cancel_tx, cancel_rx) = mpsc::channel();
                let delayed_writer = thread::spawn(move || {
                    if cancel_rx.recv_timeout(Duration::from_secs(8)).is_err() {
                        let mut options = OpenOptions::new();
                        options
                            .write(true)
                            .custom_flags(libc::O_NONBLOCK | libc::O_CLOEXEC);
                        let _ = options.open(writer_path);
                    }
                });
                let started = Instant::now();
                let outcome = validated
                    .provider_independence
                    .revalidate_before_exact_output_v1(
                        &output_dir,
                        &validated.audit_rpc_endpoint_blake3,
                    );
                let elapsed = started.elapsed();
                let _ = cancel_tx.send(());
                delayed_writer.join().expect("join late FIFO releaser");
                restore_regular_file_after_fifo_v1(path, &backup);
                let error = outcome.expect_err("late FIFO authority must fail revalidation");
                assert!(
                    elapsed < Duration::from_secs(6),
                    "late revalidation blocked on {label} FIFO for {elapsed:?}: {error:#}"
                );
                assert!(
                    error.to_string().contains("regular non-symlink file")
                        || error.to_string().contains("not a regular file"),
                    "unexpected late {label} FIFO error: {error:#}"
                );
                assert!(!output_dir.exists());
            }
        }

        if verify_endpoint_drift {
            std::env::set_var(endpoint_path_env, "/mutated-authority-b");
            let re_resolved = audit_config
                .resolve_connection()
                .expect("prove process environment now resolves authority B");
            assert_ne!(
                re_resolved.endpoint, validated.resolved_connection.endpoint,
                "the regression must exercise a real A -> B environment drift"
            );
            let client = qualification_audit_rpc_client_with_connection_v1(
                &validated.audit_config,
                Duration::from_millis(validated.audit_config.request_timeout_ms),
                &validated.resolved_connection,
            )
            .expect("construct client from validated authority A");
            assert_eq!(client.url(), validated.resolved_connection.endpoint);
            let empty_canonicality = PumpResearchSlotCanonicalityIndexV1::default();
            let blocked_without_io =
                run_independent_source_completeness_audit_v1(&raw, &empty_canonicality, &validated)
                    .await
                    .expect("blocked range still emits a local audit report");
            assert_eq!(
                blocked_without_io.report.audit_rpc_endpoint_blake3,
                validated.audit_rpc_endpoint_blake3
            );
            assert_ne!(
                blocked_without_io.report.audit_rpc_endpoint_blake3,
                blake3::hash(re_resolved.endpoint.as_bytes())
                    .to_hex()
                    .to_string(),
                "full audit report must not adopt endpoint B after validation"
            );
        }

        let mut rejected = attestation.clone();
        rejected.attestation_status = "pending_review".to_owned();
        let rejected_bytes =
            serde_json::to_vec_pretty(&rejected).expect("serialize rejected attestation");
        fs::write(&attestation_path, &rejected_bytes).expect("replace temporary attestation");
        let rejected_sha256 = operator_digest_bytes(&rejected_bytes).sha256;
        std::env::set_var(endpoint_path_env, "/validated-authority-a");
        let error = match validate_provider_independence_attestation_v1(
            &raw,
            &canonicality,
            &output_dir,
            &audit_config_path,
            &suitability_path,
            &attestation_path,
            &rejected_sha256,
            running_executable_authority,
        ) {
            Ok(_) => panic!("non-verified attestation must fail before provider I/O"),
            Err(error) => error,
        };
        assert!(error.to_string().contains("approved physical-independence"));
        assert!(!output_dir.exists());

        if verify_public_pre_io_boundary {
            let public_error = tokio::time::timeout(
                Duration::from_secs(2),
                certify_pump_research_raw_run_with_qualification_audit_v1(
                    &raw_dir,
                    &output_dir,
                    &audit_config_path,
                    &suitability_path,
                    &attestation_path,
                    &rejected_sha256,
                ),
            )
            .await
            .expect("invalid public authority must fail without waiting on provider I/O")
            .expect_err("invalid public authority must fail before provider I/O");
            assert!(
                public_error
                    .to_string()
                    .contains("approved physical-independence"),
                "public failure must come from authority validation: {public_error:#}"
            );
            assert!(matches!(
                listener.accept(),
                Err(error) if error.kind() == std::io::ErrorKind::WouldBlock
            ));
            assert!(!output_dir.exists());
        }

        let error = validated
            .provider_independence
            .revalidate_before_exact_output_v1(&output_dir, &validated.audit_rpc_endpoint_blake3)
            .expect_err("attestation drift must fail before exact output creation");
        assert!(error
            .to_string()
            .contains("provider-independence attestation changed"));
        assert!(!output_dir.exists());
    }

    #[tokio::test]
    async fn validated_combined_endpoint_survives_process_environment_drift() {
        assert_combined_authority_fixture_v1(true, false, false, false, None).await;
    }

    #[tokio::test]
    async fn public_combined_invalid_authority_performs_zero_provider_requests() {
        assert_combined_authority_fixture_v1(false, true, false, false, None).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn public_combined_control_and_authority_fifos_fail_without_provider_io() {
        assert_combined_authority_fixture_v1(false, false, true, false, None).await;
    }

    #[cfg(unix)]
    #[tokio::test]
    async fn late_combined_authority_revalidation_rejects_fifos_without_blocking() {
        assert_combined_authority_fixture_v1(false, false, false, true, None).await;
    }

    #[cfg(target_os = "linux")]
    #[tokio::test]
    async fn public_combined_rejects_attested_path_replacement_for_running_executable() {
        const CHILD_ENV: &str = "PUMP_RESEARCH_G5_2_3_RUNNING_EXE_CHILD";
        const STARTED_ENV: &str = "PUMP_RESEARCH_G5_2_3_STARTED_PATH";
        const CONTINUE_ENV: &str = "PUMP_RESEARCH_G5_2_3_CONTINUE_PATH";
        const SHA256_ENV: &str = "PUMP_RESEARCH_G5_2_3_REPLACEMENT_SHA256";
        const BLAKE3_ENV: &str = "PUMP_RESEARCH_G5_2_3_REPLACEMENT_BLAKE3";
        const BYTES_ENV: &str = "PUMP_RESEARCH_G5_2_3_REPLACEMENT_BYTES";
        const TEST_NAME: &str = "research_tape_materializer::tests::public_combined_rejects_attested_path_replacement_for_running_executable";

        if std::env::var_os(CHILD_ENV).is_some() {
            let started = PathBuf::from(
                std::env::var_os(STARTED_ENV).expect("child started marker path is configured"),
            );
            let continue_path = PathBuf::from(
                std::env::var_os(CONTINUE_ENV).expect("child continue marker path is configured"),
            );
            fs::write(&started, b"running-image-a").expect("publish child-started marker");
            let deadline = Instant::now() + Duration::from_secs(10);
            while !continue_path.exists() {
                assert!(
                    Instant::now() < deadline,
                    "parent did not replace the executable pathname"
                );
                thread::sleep(Duration::from_millis(10));
            }
            let attested_path_replacement_digest = PumpResearchOperatorDigestV1 {
                sha256: std::env::var(SHA256_ENV).expect("replacement SHA-256 is configured"),
                blake3: std::env::var(BLAKE3_ENV).expect("replacement BLAKE3 is configured"),
                bytes: std::env::var(BYTES_ENV)
                    .expect("replacement byte count is configured")
                    .parse()
                    .expect("replacement byte count is valid u64"),
            };
            let running = capture_running_executable_authority_v1()
                .expect("capture child running image A after pathname replacement");
            assert_ne!(
                running.digest(),
                &attested_path_replacement_digest,
                "fixture must execute inode A while pathname contains B"
            );
            assert_combined_authority_fixture_v1(
                false,
                false,
                false,
                false,
                Some(attested_path_replacement_digest),
            )
            .await;
            return;
        }

        let temporary = tempdir().expect("temporary running-executable replacement fixture");
        let probe = temporary.path().join("combined-certifier-probe");
        let replacement = temporary.path().join("combined-certifier-replacement");
        let started = temporary.path().join("child-started");
        let continue_path = temporary.path().join("continue-after-replacement");
        fs::copy(
            std::env::current_exe().expect("resolve current test executable"),
            &probe,
        )
        .expect("copy test executable A to controlled pathname");
        fs::copy("/bin/true", &replacement).expect("copy distinct executable B fixture");
        let replacement_file =
            open_regular_nofollow_v1(&replacement, "attested pathname replacement B")
                .expect("open attested pathname replacement B");
        let replacement_digest = digest_open_regular_file_exact_v1(
            &replacement_file,
            COMBINED_CERTIFIER_EXECUTABLE_MAX_BYTES_V1,
            "attested pathname replacement B",
        )
        .expect("digest attested pathname replacement B");
        let mut child = Command::new(&probe)
            .arg("--exact")
            .arg(TEST_NAME)
            .arg("--nocapture")
            .arg("--test-threads=1")
            .env(CHILD_ENV, "1")
            .env(STARTED_ENV, &started)
            .env(CONTINUE_ENV, &continue_path)
            .env(SHA256_ENV, &replacement_digest.sha256)
            .env(BLAKE3_ENV, &replacement_digest.blake3)
            .env(BYTES_ENV, replacement_digest.bytes.to_string())
            .spawn()
            .expect("spawn executable A from controlled pathname");
        let start_deadline = Instant::now() + Duration::from_secs(20);
        while !started.exists() {
            assert!(
                Instant::now() < start_deadline,
                "child did not reach the pathname-replacement boundary"
            );
            thread::sleep(Duration::from_millis(10));
        }
        fs::rename(&replacement, &probe).expect("atomically replace pathname A with attested B");
        fs::write(&continue_path, b"continue").expect("release child after pathname replacement");
        let finish_deadline = Instant::now() + Duration::from_secs(30);
        let status = loop {
            if let Some(status) = child.try_wait().expect("poll replacement child") {
                break status;
            }
            if Instant::now() >= finish_deadline {
                child.kill().expect("kill stuck replacement child");
                let _ = child.wait();
                panic!("running-executable replacement subprocess exceeded local deadline");
            }
            thread::sleep(Duration::from_millis(10));
        };
        assert!(
            status.success(),
            "running-executable replacement subprocess failed with {status}"
        );
    }

    #[test]
    fn raw_index_rejects_segment_stream_epoch_regression() {
        let temporary = tempdir().expect("temporary epoch-regression raw run");
        let run_id = "segment-epoch-regression";
        write_epoch_raw_run_fixture(
            temporary.path(),
            run_id,
            vec![
                (
                    2,
                    vec![epoch_record_fixture(
                        EpochRecordFixtureKind::Transaction,
                        2,
                        0,
                    )],
                ),
                (
                    1,
                    vec![epoch_record_fixture(
                        EpochRecordFixtureKind::Transaction,
                        1,
                        1,
                    )],
                ),
            ],
        );

        let error = index_pump_research_raw_run_v1(temporary.path())
            .expect_err("a cryptographically valid 2 -> 1 segment epoch sequence must fail");
        let message = error.to_string();
        assert!(
            message.contains("stream epoch 1 regresses below previous epoch 2"),
            "unexpected index error: {message}"
        );
        assert!(!message.contains("digest mismatch"));
        assert!(!message.contains("footer does not match"));
    }

    #[test]
    fn raw_index_rejects_record_epoch_different_from_segment_header() {
        for kind in [
            EpochRecordFixtureKind::Transaction,
            EpochRecordFixtureKind::AccountUpdate,
            EpochRecordFixtureKind::SlotUpdate,
            EpochRecordFixtureKind::BlockMeta,
            EpochRecordFixtureKind::CoverageGap,
        ] {
            let control = tempdir().expect("temporary matching-epoch control raw run");
            let control_run_id = format!("record-epoch-control-{kind:?}");
            write_epoch_raw_run_fixture(
                control.path(),
                &control_run_id,
                vec![(
                    1,
                    vec![
                        epoch_record_fixture(EpochRecordFixtureKind::Transaction, 1, 0),
                        epoch_record_fixture(kind, 1, 1),
                    ],
                )],
            );
            index_pump_research_raw_run_v1(control.path())
                .unwrap_or_else(|error| panic!("matching {kind:?} fixture must index: {error:#}"));

            let corrupted = tempdir().expect("temporary mismatched-epoch raw run");
            let corrupted_run_id = format!("record-epoch-corrupted-{kind:?}");
            write_epoch_raw_run_fixture(
                corrupted.path(),
                &corrupted_run_id,
                vec![(
                    1,
                    vec![
                        epoch_record_fixture(EpochRecordFixtureKind::Transaction, 1, 0),
                        epoch_record_fixture(kind, 2, 1),
                    ],
                )],
            );
            let error = index_pump_research_raw_run_v1(corrupted.path()).unwrap_err();
            let message = error.to_string();
            assert!(
                message.contains("record whose stream epoch differs from header epoch 1"),
                "unexpected {kind:?} corruption error: {message}"
            );
            assert!(!message.contains("digest mismatch"));
            assert!(!message.contains("footer does not match"));
        }
    }

    #[test]
    fn qualification_range_rejects_mid_slot_stream_start() {
        let canonicality = rooted_canonicality(100..=103);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(1, Some(100), Some(103))],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(1, 101, 103),
            "the first BlockMeta closes only the observed tail of its slot"
        );
    }

    #[test]
    fn qualification_range_never_joins_reconnect_epochs() {
        let canonicality = rooted_canonicality(100..=106);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[
                    stream_epoch_boundary(1, Some(100), Some(102)),
                    stream_epoch_boundary(2, Some(102), Some(106)),
                ],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(2, 103, 106),
            "a reconnect boundary must produce separate epoch candidates"
        );
    }

    #[test]
    fn qualification_range_blocks_epoch_without_block_meta() {
        let canonicality = rooted_canonicality(100..=103);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[
                    stream_epoch_boundary(1, Some(99), Some(103)),
                    stream_epoch_boundary(2, None, None),
                ],
                &canonicality,
                |_epoch, _slot| false,
            ),
            PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            )
        );
    }

    #[test]
    fn qualification_range_blocks_empty_epoch_interval() {
        let canonicality = rooted_canonicality(100..=103);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(1, Some(100), Some(100))],
                &canonicality,
                |_epoch, _slot| false,
            ),
            PumpResearchQualificationRangeSelectionV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureStreamBoundaryUnproven,
            )
        );
    }

    #[test]
    fn qualification_range_excludes_unclosed_shutdown_tail() {
        let canonicality = rooted_canonicality(100..=105);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(1, Some(99), Some(102))],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(1, 100, 102),
            "rooted slots after the last preserved BlockMeta are not proven complete"
        );
    }

    #[test]
    fn qualification_range_keeps_earlier_candidate_across_missing_numeric_slots() {
        let canonicality = rooted_canonicality([100, 101, 102, 200, 201]);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(1, Some(99), Some(201))],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(1, 100, 102),
            "a numeric hole must close and retain the preceding candidate"
        );
    }

    #[test]
    fn qualification_range_uses_deterministic_longest_then_earliest_tie_break() {
        let canonicality = rooted_canonicality([100, 101, 102, 200, 201, 202, 300, 301, 302, 303]);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[
                    stream_epoch_boundary(7, Some(99), Some(102)),
                    stream_epoch_boundary(3, Some(199), Some(202)),
                    stream_epoch_boundary(9, Some(299), Some(303)),
                ],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(9, 300, 303),
            "the strictly longest candidate wins"
        );

        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[
                    stream_epoch_boundary(7, Some(99), Some(102)),
                    stream_epoch_boundary(3, Some(199), Some(202)),
                ],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(7, 100, 102),
            "equal-length candidates use the earliest start slot"
        );

        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[
                    stream_epoch_boundary(7, Some(99), Some(102)),
                    stream_epoch_boundary(3, Some(99), Some(102)),
                ],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(3, 100, 102),
            "equal-length candidates with the same start use the lower stream epoch"
        );
    }

    #[test]
    fn qualification_range_is_split_by_epoch_local_coverage_gap() {
        let canonicality = rooted_canonicality(100..=105);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(4, Some(99), Some(105))],
                &canonicality,
                |epoch, slot| epoch == 4 && slot == 102,
            ),
            ready_range(4, 103, 105),
            "the gap slot splits the epoch and the longer independent suffix wins"
        );
    }

    #[test]
    fn qualification_range_matches_go_d_first_complete_slot_boundary() {
        let canonicality = rooted_canonicality(439_703_807..=439_703_840);
        assert_eq!(
            qualification_range_selection_with_boundaries_v1(
                &[stream_epoch_boundary(
                    1,
                    Some(439_703_837),
                    Some(439_703_840),
                )],
                &canonicality,
                |_epoch, _slot| false,
            ),
            ready_range(1, 439_703_838, 439_703_840)
        );
    }

    #[tokio::test]
    async fn provider_hard_deadline_cuts_in_flight_request_and_prevents_retry() {
        let listener = TcpListener::bind("127.0.0.1:0").expect("bind stalled local RPC");
        let address = listener.local_addr().expect("local RPC address");
        let (release_server, server_released) = mpsc::channel();
        let server = thread::spawn(move || {
            let (connection, _) = listener.accept().expect("accept local RPC connection");
            let _connection = connection;
            let _ = server_released.recv_timeout(Duration::from_secs(2));
        });
        let client = crate::rpc_http_client::new_async_rpc_client_without_legacy_auth_with_timeout(
            format!("http://{address}"),
            Duration::from_secs(5),
        )
        .expect("construct local no-auth RPC client");
        let provider_started = Instant::now();
        let provider_deadline = provider_started
            .checked_add(Duration::from_millis(120))
            .expect("test deadline");

        let metrics = fetch_finalized_audit_slot_with_metrics_v1(
            Arc::new(client),
            123,
            8,
            Duration::from_secs(5),
            Some(provider_deadline),
        )
        .await;
        let elapsed = provider_started.elapsed();
        let _ = release_server.send(());
        server.join().expect("join stalled local RPC");

        assert_eq!(
            metrics.attempt_count, 1,
            "the exhausted budget forbids retry"
        );
        assert!(
            matches!(
                metrics.result,
                PumpResearchAuditSlotFetchV1::Unavailable(ref error)
                    if error.contains("hard provider wall deadline")
            ),
            "the deadline failure remains typed as unavailable without provider fallback"
        );
        assert!(
            elapsed < Duration::from_secs(1),
            "a 120 ms provider budget must not inherit the five-second request timeout: {elapsed:?}"
        );
    }

    #[test]
    fn reserve_transition_matches_existing_program_state_transition() {
        let state = PumpCurveStateV1 {
            virtual_quote_reserves: 1_000_000,
            virtual_token_reserves: 2_000_000,
            real_quote_reserves: 400_000,
            real_token_reserves: 1_500_000,
            complete: false,
        };
        let reserves = PumpReserveState {
            virtual_base_reserves: state.virtual_token_reserves,
            virtual_quote_reserves: state.virtual_quote_reserves,
            real_base_reserves: state.real_token_reserves,
            real_quote_reserves: state.real_quote_reserves,
        };
        let schedule = fixture_fee_schedule();

        let (buy_after, buy_curve_quote) = transition_buy(state, 100_000).expect("buy transition");
        let buy_quote = quote_exact_base_out(
            PumpRouteVariant::BuyV2,
            reserves,
            100_000,
            u64::MAX,
            &schedule,
        )
        .expect("existing quote transition");
        assert_eq!(
            buy_after.virtual_token_reserves,
            buy_quote.reserve_transition.base_after
        );
        assert_eq!(
            buy_after.virtual_quote_reserves,
            buy_quote.reserve_transition.quote_after
        );
        assert_eq!(
            buy_curve_quote,
            Some(buy_quote.reserve_transition.curve_quote_amount)
        );

        let (sell_after, sell_curve_quote) =
            transition_sell(state, 100_000).expect("sell transition");
        let sell_quote =
            quote_exact_base_in_sell(PumpRouteVariant::SellV2, reserves, 100_000, 0, &schedule)
                .expect("existing quote transition");
        assert_eq!(
            sell_after.virtual_token_reserves,
            sell_quote.reserve_transition.base_after
        );
        assert_eq!(
            sell_after.virtual_quote_reserves,
            sell_quote.reserve_transition.quote_after
        );
        assert_eq!(
            sell_curve_quote,
            Some(sell_quote.reserve_transition.curve_quote_amount)
        );
    }

    fn provenance_test_digest(label: &str) -> PumpResearchOperatorDigestV1 {
        PumpResearchOperatorDigestV1 {
            sha256: format!("sha256-{label}"),
            blake3: format!("blake3-{label}"),
            bytes: label.len() as u64,
        }
    }

    fn current_capture_provenance_binding_for_test(
        run_id: &str,
    ) -> PumpResearchCaptureProvenanceBindingV1 {
        let release_binary_digest = provenance_test_digest("sealed-binary");
        PumpResearchCaptureProvenanceBindingV1 {
            schema_version: 1,
            binding_kind: OPERATOR_PREFLIGHT_BINDING_KIND_V1.to_owned(),
            run_id: run_id.to_owned(),
            receipt_validated_wall_ms: 1,
            binding_written_wall_ms: 2,
            preflight_receipt_digest: provenance_test_digest("preflight"),
            artifact_provenance_fingerprint: provenance_test_digest("fingerprint"),
            repository_commit: "test-commit".to_owned(),
            repository_worktree_state: "dirty".to_owned(),
            release_binary_digest: release_binary_digest.clone(),
            config_bytes_digest: provenance_test_digest("config"),
            build_semantics: OPERATOR_PREFLIGHT_BUILD_SEMANTICS_V1.to_owned(),
            credential_scan_semantics: OPERATOR_PREFLIGHT_CREDENTIAL_SCAN_SEMANTICS_V1.to_owned(),
            qualification_provenance_eligible: true,
            sealed_release_binary_digest: Some(release_binary_digest),
        }
    }

    #[test]
    fn legacy_capture_binding_is_development_only_even_with_an_ideal_audit() {
        let temporary = tempdir().expect("temporary legacy raw run");
        let run_id = "legacy-run";
        let mut legacy = serde_json::to_value(current_capture_provenance_binding_for_test(run_id))
            .expect("serialize current binding before removing V4 fields");
        let object = legacy
            .as_object_mut()
            .expect("binding must serialize as an object");
        object.remove("build_semantics");
        object.remove("credential_scan_semantics");
        object.remove("qualification_provenance_eligible");
        object.remove("sealed_release_binary_digest");
        fs::write(
            temporary
                .path()
                .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            serde_json::to_vec(&legacy).expect("serialize legacy binding"),
        )
        .expect("write legacy binding");

        let (eligibility, digest) =
            assess_capture_provenance_eligibility_v1(temporary.path(), run_id);
        assert!(digest.is_some());
        assert!(matches!(
            eligibility,
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::LegacyOrUnsupportedBinding
            )
        ));
        assert_eq!(
            qualification_status_with_capture_provenance_v1(
                eligibility,
                false,
                Some(PumpResearchTapeQualificationStatusV1::Ready),
                false,
            ),
            PumpResearchTapeQualificationStatusV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureProvenanceUnqualified
            ),
            "an independent audit is forensic evidence for a legacy capture, never a promotion override"
        );
    }

    #[test]
    fn cargo_config_closure_v4_binding_cannot_be_promoted_after_v5_freeze() {
        let temporary = tempdir().expect("temporary V4 raw run");
        let run_id = "cargo-config-closure-v4-run";
        let mut binding = current_capture_provenance_binding_for_test(run_id);
        binding.build_semantics =
            "fresh_cargo_target_locked_offline_release_from_isolated_snapshot_staging_clean_toolchain_binary_child_env_and_cargo_config_closure_v4"
                .to_owned();
        fs::write(
            temporary
                .path()
                .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            serde_json::to_vec(&binding).expect("serialize V4 binding"),
        )
        .expect("write V4 binding");

        let (eligibility, digest) =
            assess_capture_provenance_eligibility_v1(temporary.path(), run_id);
        assert!(digest.is_some());
        assert!(matches!(
            eligibility,
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::LegacyOrUnsupportedBinding
            )
        ));
        assert_eq!(
            qualification_status_with_capture_provenance_v1(
                eligibility,
                false,
                Some(PumpResearchTapeQualificationStatusV1::Ready),
                false,
            ),
            PumpResearchTapeQualificationStatusV1::Blocked(
                PumpResearchQualificationBlockerV1::CaptureProvenanceUnqualified
            ),
            "the superseded V4 build contract is development-only even with an ideal audit"
        );
    }

    #[test]
    fn corrected_capture_binding_can_be_ready_only_after_ideal_audit() {
        let temporary = tempdir().expect("temporary corrected raw run");
        let run_id = "corrected-run";
        let binding = current_capture_provenance_binding_for_test(run_id);
        fs::write(
            temporary
                .path()
                .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            serde_json::to_vec(&binding).expect("serialize corrected binding"),
        )
        .expect("write corrected binding");

        let (eligibility, digest) =
            assess_capture_provenance_eligibility_v1(temporary.path(), run_id);
        assert!(digest.is_some());
        assert_eq!(
            eligibility,
            PumpResearchCaptureProvenanceEligibilityV1::Eligible
        );
        assert_eq!(
            qualification_status_with_capture_provenance_v1(
                eligibility,
                false,
                Some(PumpResearchTapeQualificationStatusV1::Ready),
                false,
            ),
            PumpResearchTapeQualificationStatusV1::Ready
        );
        assert_eq!(
            qualification_status_with_capture_provenance_v1(eligibility, false, None, false),
            PumpResearchTapeQualificationStatusV1::Unqualified,
            "plain development materialization stays Unqualified"
        );
        assert_eq!(
            qualification_status_with_capture_provenance_v1(eligibility, false, None, true),
            PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape,
            "a separately validated GO-D authority promotes without GO-E"
        );
    }

    #[test]
    fn export_authority_retires_go_e_ready_and_requires_verified_go_d() {
        let authority_sha256 = "ab".repeat(32);
        assert!(has_verified_go_d_export_authority_v1(
            &PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape,
            GO_D_SOURCE_AUTHORITY_VERIFIED_V1,
            true,
            &authority_sha256,
        ));
        assert!(
            !has_verified_go_d_export_authority_v1(
                &PumpResearchTapeQualificationStatusV1::Ready,
                GO_D_SOURCE_AUTHORITY_VERIFIED_V1,
                true,
                &authority_sha256,
            ),
            "the retired external-audit Ready status must never substitute for GO-D authority"
        );
        assert!(!has_verified_go_d_export_authority_v1(
            &PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape,
            "UNVERIFIED",
            true,
            &authority_sha256,
        ));
        assert!(!has_verified_go_d_export_authority_v1(
            &PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape,
            GO_D_SOURCE_AUTHORITY_VERIFIED_V1,
            false,
            &authority_sha256,
        ));
        assert!(!has_verified_go_d_export_authority_v1(
            &PumpResearchTapeQualificationStatusV1::VerifiedFrozenTape,
            GO_D_SOURCE_AUTHORITY_VERIFIED_V1,
            true,
            "not-a-sha256",
        ));
    }

    #[test]
    fn binding_with_mismatched_sealed_binary_is_not_qualification_eligible() {
        let temporary = tempdir().expect("temporary mismatched raw run");
        let run_id = "mismatched-binary-run";
        let mut binding = current_capture_provenance_binding_for_test(run_id);
        binding.sealed_release_binary_digest = Some(provenance_test_digest("other-binary"));
        fs::write(
            temporary
                .path()
                .join(OPERATOR_PREFLIGHT_CAPTURE_BINDING_FILE_V1),
            serde_json::to_vec(&binding).expect("serialize mismatched binding"),
        )
        .expect("write mismatched binding");

        assert!(matches!(
            assess_capture_provenance_eligibility_v1(temporary.path(), run_id).0,
            PumpResearchCaptureProvenanceEligibilityV1::Ineligible(
                PumpResearchCaptureProvenanceIneligibilityReasonV1::SealedBinaryMismatch
            )
        ));
    }

    #[test]
    fn exact_writer_never_publishes_partial_artifact_as_final_directory() {
        let temporary = tempdir().expect("temporary exact output root");
        let final_root = temporary.path().join("exact");
        let partial_root = temporary.path().join(".exact.partial");
        let writer = PumpResearchExactOutputWriterV1::create(&final_root)
            .expect("create partial exact output");
        assert!(partial_root.is_dir());
        assert!(!final_root.exists());
        drop(writer);
        assert!(partial_root.is_dir());
        assert!(!final_root.exists());
    }
}
