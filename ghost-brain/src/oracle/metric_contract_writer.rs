use anyhow::{Context, Result};
use ghost_core::metric_contracts::{
    CanonicalHashV1, MetricContractDecisionSummaryV1, MetricContractPairedRecordV1,
    MetricContractRolloutMode, MetricEvidenceRecordIdentityV1,
    ResolvedMetricContractEffectiveConfigV1, BURN_IN_CONTRACT_V3_CANONICAL_HASH,
    BURN_IN_CONTRACT_VERSION_V3, METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1,
    METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
    METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34, METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1,
    METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3,
};
pub use ghost_core::metric_contracts::{
    METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2,
    METRIC_CONTRACT_LATENCY_HISTOGRAM_CODEBOOK_VERSION_V2,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::fs::{create_dir_all, rename, File, OpenOptions};
use tokio::io::AsyncWriteExt;

pub const METRIC_CONTRACT_SUMMARY_V34_FILE: &str = "metric_contract_decisions_v34.jsonl";
pub const METRIC_CONTRACT_EVIDENCE_V1_FILE: &str = "metric_contract_evidence_v1.jsonl";
pub const METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE: &str =
    "metric_contract_rotation_manifest_v1.json";
pub const DEFAULT_METRIC_CONTRACT_ROTATION_MAX_BYTES: u64 = 64 * 1024 * 1024;
#[derive(Debug, Clone)]
pub struct MetricContractPairedWriterConfigV1 {
    pub directory: PathBuf,
    pub rotation_max_bytes: u64,
    pub build_commit: String,
    pub build_worktree_clean: bool,
    pub queue_capacity: usize,
    /// Deterministic failure hook used only by durability regression fixtures.
    /// Production construction always leaves it at `None`.
    pub fault_injection: Option<MetricContractWriterFaultInjectionV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricContractWriterFaultInjectionV1 {
    SummaryEnospc,
    EvidenceEnospcAfterSummary,
    /// Persist a complete evidence row first, then fail the matching summary.
    /// This exercises and proves the otherwise rare orphan-evidence branch.
    SummaryEnospcAfterEvidence,
    SummaryShortWriteAfterBytes(usize),
    EvidenceShortWriteAfterSummaryBytes(usize),
    FinalManifestEnospc,
}

impl MetricContractPairedWriterConfigV1 {
    #[must_use]
    pub fn new(directory: PathBuf, build_commit: impl Into<String>) -> Self {
        Self {
            directory,
            rotation_max_bytes: DEFAULT_METRIC_CONTRACT_ROTATION_MAX_BYTES,
            build_commit: build_commit.into(),
            // A public caller has not supplied a trustworthy clean-tree
            // attestation. Production DecisionLogger replaces this with its
            // compile-time Git provenance; all other callers fail closed.
            build_worktree_clean: false,
            queue_capacity: 1_000,
            fault_injection: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractLatencyHistogramSnapshotV1 {
    pub bucket_upper_bounds_us: [u32; 18],
    /// One count per upper bound plus a final overflow bucket.
    pub bucket_counts: [u64; 19],
    pub sample_count: u64,
    pub max_us: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum MetricContractLatencyHistogramErrorV1 {
    #[error("latency histogram uses an unknown bucket codebook")]
    BucketCodebookMismatch,
    #[error("latency histogram bucket sum overflow")]
    BucketSumOverflow,
    #[error("latency histogram bucket sum does not equal sample_count")]
    BucketSumMismatch,
    #[error("latency histogram sample_count does not equal paired command count")]
    SampleCountMismatch,
    #[error("empty latency histogram has non-zero max or buckets")]
    EmptyHistogramMismatch,
    #[error("latency histogram max is inconsistent with populated buckets")]
    MaxBucketMismatch,
    #[error("latency histogram overflow bucket is inconsistent with max")]
    OverflowBucketMismatch,
}

impl Default for MetricContractLatencyHistogramSnapshotV1 {
    fn default() -> Self {
        Self {
            bucket_upper_bounds_us: METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2,
            bucket_counts: [0; 19],
            sample_count: 0,
            max_us: 0,
        }
    }
}

impl MetricContractLatencyHistogramSnapshotV1 {
    pub fn validate(
        &self,
        expected_sample_count: u64,
    ) -> std::result::Result<(), MetricContractLatencyHistogramErrorV1> {
        if self.bucket_upper_bounds_us != METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2 {
            return Err(MetricContractLatencyHistogramErrorV1::BucketCodebookMismatch);
        }
        let bucket_sum = self.bucket_counts.iter().try_fold(0u64, |sum, count| {
            sum.checked_add(*count)
                .ok_or(MetricContractLatencyHistogramErrorV1::BucketSumOverflow)
        })?;
        if bucket_sum != self.sample_count {
            return Err(MetricContractLatencyHistogramErrorV1::BucketSumMismatch);
        }
        if self.sample_count != expected_sample_count {
            return Err(MetricContractLatencyHistogramErrorV1::SampleCountMismatch);
        }
        if self.sample_count == 0 {
            return if self.max_us == 0 && self.bucket_counts.iter().all(|count| *count == 0) {
                Ok(())
            } else {
                Err(MetricContractLatencyHistogramErrorV1::EmptyHistogramMismatch)
            };
        }

        let overflow_index = self.bucket_upper_bounds_us.len();
        let max_bucket = self
            .bucket_upper_bounds_us
            .iter()
            .position(|upper| self.max_us <= u64::from(*upper))
            .unwrap_or(overflow_index);
        if self.bucket_counts[max_bucket] == 0
            || self.bucket_counts[(max_bucket + 1)..]
                .iter()
                .any(|count| *count != 0)
        {
            return Err(MetricContractLatencyHistogramErrorV1::MaxBucketMismatch);
        }
        let overflow_count = self.bucket_counts[overflow_index];
        let last_bound = u64::from(
            *self
                .bucket_upper_bounds_us
                .last()
                .expect("frozen histogram codebook is non-empty"),
        );
        if (overflow_count > 0) != (self.max_us > last_bound) {
            return Err(MetricContractLatencyHistogramErrorV1::OverflowBucketMismatch);
        }
        Ok(())
    }

    #[must_use]
    pub fn percentile_upper_bound_us(&self, percentile: u32) -> Option<u64> {
        if self.sample_count == 0 || !(1..=100).contains(&percentile) {
            return None;
        }
        let target = self
            .sample_count
            .saturating_mul(u64::from(percentile))
            .saturating_add(99)
            / 100;
        let mut cumulative = 0u64;
        for (index, count) in self.bucket_counts.iter().enumerate() {
            cumulative = cumulative.saturating_add(*count);
            if cumulative >= target {
                return self
                    .bucket_upper_bounds_us
                    .get(index)
                    .copied()
                    .map(u64::from)
                    .or(Some(self.max_us));
            }
        }
        Some(self.max_us)
    }
}

#[derive(Debug)]
struct MetricContractLatencyHistogramV1 {
    bucket_counts: [AtomicU64; 19],
    sample_count: AtomicU64,
    max_us: AtomicU64,
}

impl Default for MetricContractLatencyHistogramV1 {
    fn default() -> Self {
        Self {
            bucket_counts: std::array::from_fn(|_| AtomicU64::new(0)),
            sample_count: AtomicU64::new(0),
            max_us: AtomicU64::new(0),
        }
    }
}

impl MetricContractLatencyHistogramV1 {
    fn record(&self, value_us: u64) {
        let bucket = METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2
            .iter()
            .position(|upper| value_us <= u64::from(*upper))
            .unwrap_or(METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2.len());
        self.bucket_counts[bucket].fetch_add(1, Ordering::Relaxed);
        self.sample_count.fetch_add(1, Ordering::Relaxed);
        self.max_us.fetch_max(value_us, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricContractLatencyHistogramSnapshotV1 {
        MetricContractLatencyHistogramSnapshotV1 {
            bucket_upper_bounds_us: METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V2,
            bucket_counts: std::array::from_fn(|index| {
                self.bucket_counts[index].load(Ordering::Relaxed)
            }),
            sample_count: self.sample_count.load(Ordering::Relaxed),
            max_us: self.max_us.load(Ordering::Relaxed),
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractPairedWriterStatsSnapshotV1 {
    pub paired_commands_total: u64,
    pub summary_rows_written_total: u64,
    pub evidence_rows_written_total: u64,
    pub summary_write_failures_total: u64,
    pub evidence_write_failures_total: u64,
    pub writer_disabled_total: u64,
    pub queue_send_failures_total: u64,
    pub queue_dropped_rows_total: u64,
    pub orphan_summary_total: u64,
    pub orphan_evidence_total: u64,
    pub missing_pair_total: u64,
    pub manifest_write_failures_total: u64,
    pub finalization_failures_total: u64,
    pub writer_queue_high_water: u64,
    pub logger_enqueue_wait_us: MetricContractLatencyHistogramSnapshotV1,
    pub metric_contract_build_and_serialize_us: MetricContractLatencyHistogramSnapshotV1,
    pub projection_build_and_validate_us: MetricContractLatencyHistogramSnapshotV1,
}

#[derive(Debug, Default)]
pub struct MetricContractPairedWriterStatsV1 {
    paired_commands_total: AtomicU64,
    summary_rows_written_total: AtomicU64,
    evidence_rows_written_total: AtomicU64,
    summary_write_failures_total: AtomicU64,
    evidence_write_failures_total: AtomicU64,
    writer_disabled_total: AtomicU64,
    queue_send_failures_total: AtomicU64,
    queue_dropped_rows_total: AtomicU64,
    orphan_summary_total: AtomicU64,
    orphan_evidence_total: AtomicU64,
    missing_pair_total: AtomicU64,
    manifest_write_failures_total: AtomicU64,
    finalization_failures_total: AtomicU64,
    writer_queue_high_water: AtomicU64,
    logger_enqueue_wait_us: MetricContractLatencyHistogramV1,
    metric_contract_build_and_serialize_us: MetricContractLatencyHistogramV1,
    projection_build_and_validate_us: MetricContractLatencyHistogramV1,
}

impl MetricContractPairedWriterStatsV1 {
    #[must_use]
    pub fn snapshot(&self) -> MetricContractPairedWriterStatsSnapshotV1 {
        let load = |value: &AtomicU64| value.load(Ordering::Relaxed);
        MetricContractPairedWriterStatsSnapshotV1 {
            paired_commands_total: load(&self.paired_commands_total),
            summary_rows_written_total: load(&self.summary_rows_written_total),
            evidence_rows_written_total: load(&self.evidence_rows_written_total),
            summary_write_failures_total: load(&self.summary_write_failures_total),
            evidence_write_failures_total: load(&self.evidence_write_failures_total),
            writer_disabled_total: load(&self.writer_disabled_total),
            queue_send_failures_total: load(&self.queue_send_failures_total),
            queue_dropped_rows_total: load(&self.queue_dropped_rows_total),
            orphan_summary_total: load(&self.orphan_summary_total),
            orphan_evidence_total: load(&self.orphan_evidence_total),
            missing_pair_total: load(&self.missing_pair_total),
            manifest_write_failures_total: load(&self.manifest_write_failures_total),
            finalization_failures_total: load(&self.finalization_failures_total),
            writer_queue_high_water: load(&self.writer_queue_high_water),
            logger_enqueue_wait_us: self.logger_enqueue_wait_us.snapshot(),
            metric_contract_build_and_serialize_us: self
                .metric_contract_build_and_serialize_us
                .snapshot(),
            projection_build_and_validate_us: self.projection_build_and_validate_us.snapshot(),
        }
    }

    pub(crate) fn record_queue_depth(&self, depth: usize) {
        self.writer_queue_high_water
            .fetch_max(depth as u64, Ordering::Relaxed);
    }

    pub(crate) fn record_send_failure(&self) {
        self.queue_send_failures_total
            .fetch_add(1, Ordering::Relaxed);
        self.queue_dropped_rows_total
            .fetch_add(1, Ordering::Relaxed);
        self.missing_pair_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_disabled(&self) {
        self.writer_disabled_total.fetch_add(1, Ordering::Relaxed);
        self.queue_dropped_rows_total
            .fetch_add(1, Ordering::Relaxed);
        self.missing_pair_total.fetch_add(1, Ordering::Relaxed);
    }

    pub(crate) fn record_writer_disabled_event(&self) {
        self.writer_disabled_total.fetch_add(1, Ordering::Relaxed);
    }

    pub fn record_enqueue_wait_us(&self, value_us: u64) {
        self.logger_enqueue_wait_us.record(value_us);
    }

    fn record_pair_resources(&self, pair: &MetricContractPairedRecordV1) -> Result<()> {
        let full_path_us =
            u64::try_from(pair.metric_contract_full_path_started.elapsed().as_micros())
                .context("metric-contract full build+serialize duration does not fit u64")?;
        self.metric_contract_build_and_serialize_us
            .record(full_path_us);
        self.projection_build_and_validate_us
            .record(u64::from(pair.projection_build_and_validate_us));
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractRotatedPartManifestV1 {
    pub stream: String,
    pub file_path: String,
    pub schema: String,
    pub part_index: u32,
    pub row_count: u64,
    pub byte_count: u64,
    pub first_record_identity: Option<MetricEvidenceRecordIdentityV1>,
    pub last_record_identity: Option<MetricEvidenceRecordIdentityV1>,
    pub part_sha256: CanonicalHashV1,
    pub run_id: String,
    pub build_commit: String,
    pub build_worktree_clean: bool,
    pub gatekeeper_config_hash: String,
    pub brain_config_hash: String,
    pub rollout_mode: MetricContractRolloutMode,
    pub metric_contract_schema_version: u16,
    pub projection_wire_version: u16,
    pub evidence_schema_version: u16,
    pub decision_schema_version: u32,
    pub wire_schema_manifest_blake3: String,
    pub burn_in_contract_version: u16,
    pub burn_in_contract_canonical_hash: CanonicalHashV1,
    pub profile_id: String,
    pub profile_hash: CanonicalHashV1,
    pub metric_contract_effective_config_hash: CanonicalHashV1,
    pub effective_config: ResolvedMetricContractEffectiveConfigV1,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractRotationManifestV1 {
    pub manifest_schema_version: u16,
    /// Set only after the writer has stopped accepting commands and flushed
    /// the final part metadata. Audits reject mutable/in-progress runs.
    pub writer_finalized: bool,
    pub summary_parts: Vec<MetricContractRotatedPartManifestV1>,
    pub evidence_parts: Vec<MetricContractRotatedPartManifestV1>,
    pub writer_stats: MetricContractPairedWriterStatsSnapshotV1,
    pub writer_queue_capacity: usize,
}

#[derive(Debug, Clone, PartialEq)]
struct MetricContractPartProvenanceV1 {
    run_id: String,
    gatekeeper_config_hash: String,
    brain_config_hash: String,
    rollout_mode: MetricContractRolloutMode,
    metric_contract_schema_version: u16,
    evidence_schema_version: u16,
    profile_id: String,
    profile_hash: CanonicalHashV1,
    metric_contract_effective_config_hash: CanonicalHashV1,
    effective_config: ResolvedMetricContractEffectiveConfigV1,
}

impl MetricContractPartProvenanceV1 {
    fn try_from_pair(pair: &MetricContractPairedRecordV1) -> Result<Self> {
        let brain_config_hash = pair
            .brain_config_hash
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .context("paired writer requires a non-empty brain config hash")?;
        if !is_exact_lower_hex(&pair.gatekeeper_config_hash, 64)
            || !is_exact_lower_hex(brain_config_hash, 64)
            || pair.decision_v34.metric_contract_schema_version
                != METRIC_CONTRACT_DECISION_PROJECTION_SCHEMA_VERSION_V1
            || pair.decision_v34.evidence_schema_version
                != METRIC_CONTRACT_EVIDENCE_SCHEMA_VERSION_V1
        {
            anyhow::bail!("paired writer requires exact config hashes and schema provenance");
        }
        Ok(Self {
            run_id: pair.record_identity().run_id.clone(),
            gatekeeper_config_hash: pair.gatekeeper_config_hash.clone(),
            brain_config_hash: brain_config_hash.to_string(),
            rollout_mode: pair.decision_v34.rollout_mode,
            metric_contract_schema_version: pair.decision_v34.metric_contract_schema_version,
            evidence_schema_version: pair.decision_v34.evidence_schema_version,
            profile_id: pair.decision_v34.profile_id.as_str().to_string(),
            profile_hash: pair.decision_v34.profile_hash.clone(),
            metric_contract_effective_config_hash: pair
                .decision_v34
                .metric_contract_effective_config_hash
                .clone(),
            effective_config: pair.effective_config.clone(),
        })
    }
}

fn is_exact_lower_hex(value: &str, length: usize) -> bool {
    value.len() == length
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[derive(Debug)]
struct OpenPart {
    stream: &'static str,
    schema: &'static str,
    part_index: u32,
    file_path: PathBuf,
    file: File,
    row_count: u64,
    byte_count: u64,
    sha256: Sha256,
    first_record_identity: Option<MetricEvidenceRecordIdentityV1>,
    last_record_identity: Option<MetricEvidenceRecordIdentityV1>,
}

impl OpenPart {
    async fn open(
        directory: &Path,
        stream: &'static str,
        schema: &'static str,
        base_name: &'static str,
        part_index: u32,
    ) -> Result<Self> {
        let file_name = if part_index == 0 {
            base_name.to_string()
        } else {
            let stem = base_name.strip_suffix(".jsonl").unwrap_or(base_name);
            format!("{stem}.part-{part_index:05}.jsonl")
        };
        let file_path = directory.join(file_name);
        let file = OpenOptions::new()
            .create_new(true)
            .write(true)
            .open(&file_path)
            .await
            .with_context(|| format!("create metric-contract part {}", file_path.display()))?;
        Ok(Self {
            stream,
            schema,
            part_index,
            file_path,
            file,
            row_count: 0,
            byte_count: 0,
            sha256: Sha256::new(),
            first_record_identity: None,
            last_record_identity: None,
        })
    }

    async fn write_encoded_json_line(
        &mut self,
        mut bytes: Vec<u8>,
        identity: &MetricEvidenceRecordIdentityV1,
        fail_after_bytes: Option<usize>,
    ) -> Result<()> {
        bytes.push(b'\n');

        // Account for every byte accepted by the filesystem, including a
        // prefix written before a later error. This keeps the rotation
        // manifest truthful for truncated/partial-write audit instead of
        // pretending that the part remained unchanged.
        let mut written = 0usize;
        while written < bytes.len() {
            if fail_after_bytes.is_some_and(|limit| written >= limit) {
                return Err(std::io::Error::from_raw_os_error(28).into());
            }
            let write_end = fail_after_bytes
                .map(|limit| limit.min(bytes.len()))
                .unwrap_or(bytes.len());
            let count = self.file.write(&bytes[written..write_end]).await?;
            if count == 0 {
                return Err(std::io::Error::new(
                    ErrorKind::WriteZero,
                    "metric-contract JSONL write returned zero bytes",
                )
                .into());
            }
            let end = written
                .checked_add(count)
                .context("metric-contract JSONL write offset overflow")?;
            self.sha256.update(&bytes[written..end]);
            self.byte_count = self
                .byte_count
                .checked_add(u64::try_from(count).context("write size does not fit u64")?)
                .context("metric-contract part byte count overflow")?;
            written = end;
        }
        self.row_count = self
            .row_count
            .checked_add(1)
            .context("metric-contract part row count overflow")?;
        self.first_record_identity
            .get_or_insert_with(|| identity.clone());
        self.last_record_identity = Some(identity.clone());
        self.file.flush().await?;
        self.file.sync_data().await?;
        Ok(())
    }

    async fn sync_data(&mut self) -> Result<()> {
        self.file.flush().await?;
        self.file.sync_data().await?;
        Ok(())
    }

    fn manifest(
        &self,
        provenance: &MetricContractPartProvenanceV1,
        config: &MetricContractPairedWriterConfigV1,
    ) -> MetricContractRotatedPartManifestV1 {
        MetricContractRotatedPartManifestV1 {
            stream: self.stream.to_string(),
            file_path: self
                .file_path
                .file_name()
                .expect("opened metric-contract part always has a file name")
                .to_string_lossy()
                .into_owned(),
            schema: self.schema.to_string(),
            part_index: self.part_index,
            row_count: self.row_count,
            byte_count: self.byte_count,
            first_record_identity: self.first_record_identity.clone(),
            last_record_identity: self.last_record_identity.clone(),
            part_sha256: CanonicalHashV1::parse(format!("{:x}", self.sha256.clone().finalize()))
                .expect("SHA-256 always formats as 64 lowercase hexadecimal characters"),
            run_id: provenance.run_id.clone(),
            build_commit: config.build_commit.clone(),
            build_worktree_clean: config.build_worktree_clean,
            gatekeeper_config_hash: provenance.gatekeeper_config_hash.clone(),
            brain_config_hash: provenance.brain_config_hash.clone(),
            rollout_mode: provenance.rollout_mode,
            metric_contract_schema_version: provenance.metric_contract_schema_version,
            projection_wire_version: METRIC_CONTRACT_DECISION_PROJECTION_WIRE_VERSION_V1,
            evidence_schema_version: provenance.evidence_schema_version,
            decision_schema_version: METRIC_CONTRACT_DECISION_SCHEMA_VERSION_V34,
            wire_schema_manifest_blake3: METRIC_CONTRACT_PROJECTION_WIRE_V1_SCHEMA_MANIFEST_BLAKE3
                .to_string(),
            burn_in_contract_version: BURN_IN_CONTRACT_VERSION_V3,
            burn_in_contract_canonical_hash: CanonicalHashV1::parse(
                BURN_IN_CONTRACT_V3_CANONICAL_HASH,
            )
            .expect("compiled BURN_IN_CONTRACT_V3 hash is valid SHA-256"),
            profile_id: provenance.profile_id.clone(),
            profile_hash: provenance.profile_hash.clone(),
            metric_contract_effective_config_hash: provenance
                .metric_contract_effective_config_hash
                .clone(),
            effective_config: provenance.effective_config.clone(),
        }
    }
}

pub struct MetricContractPairedWriterV1 {
    config: MetricContractPairedWriterConfigV1,
    stats: Arc<MetricContractPairedWriterStatsV1>,
    summary: OpenPart,
    evidence: OpenPart,
    manifest: MetricContractRotationManifestV1,
    frozen_provenance: Option<MetricContractPartProvenanceV1>,
}

impl MetricContractPairedWriterV1 {
    pub async fn open(
        config: MetricContractPairedWriterConfigV1,
        stats: Arc<MetricContractPairedWriterStatsV1>,
    ) -> Result<Self> {
        create_dir_all(&config.directory).await?;
        let summary = OpenPart::open(
            &config.directory,
            "decision_v34",
            "metric_contract_decision_v34",
            METRIC_CONTRACT_SUMMARY_V34_FILE,
            0,
        )
        .await?;
        let evidence = OpenPart::open(
            &config.directory,
            "full_evidence_v1",
            "metric_contract_evidence_v1",
            METRIC_CONTRACT_EVIDENCE_V1_FILE,
            0,
        )
        .await?;
        let writer_queue_capacity = config.queue_capacity;
        Ok(Self {
            config,
            stats,
            summary,
            evidence,
            manifest: MetricContractRotationManifestV1 {
                manifest_schema_version: 1,
                writer_finalized: false,
                summary_parts: Vec::new(),
                evidence_parts: Vec::new(),
                writer_stats: MetricContractPairedWriterStatsSnapshotV1::default(),
                writer_queue_capacity,
            },
            frozen_provenance: None,
        })
    }

    async fn rotate_if_needed(&mut self) -> Result<()> {
        if self.summary.byte_count < self.config.rotation_max_bytes
            && self.evidence.byte_count < self.config.rotation_max_bytes
        {
            return Ok(());
        }
        let next = self
            .summary
            .part_index
            .checked_add(1)
            .context("part index overflow")?;
        self.summary = OpenPart::open(
            &self.config.directory,
            "decision_v34",
            "metric_contract_decision_v34",
            METRIC_CONTRACT_SUMMARY_V34_FILE,
            next,
        )
        .await?;
        self.evidence = OpenPart::open(
            &self.config.directory,
            "full_evidence_v1",
            "metric_contract_evidence_v1",
            METRIC_CONTRACT_EVIDENCE_V1_FILE,
            next,
        )
        .await?;
        Ok(())
    }

    pub async fn write_pair(&mut self, mut pair: MetricContractPairedRecordV1) -> Result<()> {
        // Count every command received by the writer boundary, including
        // commands that fail structural/provenance validation before I/O.
        self.stats
            .paired_commands_total
            .fetch_add(1, Ordering::Relaxed);
        pair.validate_pair()
            .context("validate paired metric-contract record")?;
        let candidate_provenance = MetricContractPartProvenanceV1::try_from_pair(&pair)?;
        if let Some(frozen) = self.frozen_provenance.as_ref() {
            anyhow::ensure!(
                frozen == &candidate_provenance,
                "paired writer run/build/profile/effective-config provenance changed within one run"
            );
        } else {
            self.frozen_provenance = Some(candidate_provenance);
        }
        self.rotate_if_needed().await?;
        // One monotonic timer was captured before the first canonical
        // producer call and travelled with the frozen snapshot and pair. The
        // writer samples that exact clock only after timestamp/part binding,
        // semantic transport hashing, both serde passes and the fixed-width
        // final v34 telemetry substitution. This includes all gaps between
        // producer, comparator and writer boundaries, bounded-queue admission,
        // scheduler delay and the preceding v33 command's filesystem I/O. The
        // current pair's own write happens after its exact final bytes exist.
        let final_bytes_started = std::time::Instant::now();
        let identity = pair.record_identity().clone();
        let writer_timestamp_ms = u64::try_from(
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .context("system clock precedes Unix epoch")?
                .as_millis(),
        )
        .context("writer timestamp does not fit u64")?;
        // Both fields are transport-only and deliberately excluded from the
        // semantic evidence hash. The terminal pair builder has already
        // validated and hashed this exact payload, so rebuilding the
        // transport with `try_new` here would run all ten family validators
        // and JCS/SHA-256 a second time. Durable deserialization and replay
        // still independently repeat those checks at their trust boundary.
        pair.evidence.writer_timestamp_ms = writer_timestamp_ms;
        pair.evidence.rotation_part_index = self.evidence.part_index;

        // Evidence is serialized once with the writer-owned timestamp and
        // rotation index. The v34 summary uses a bounded fixed-point pass so
        // its embedded `metric_contract_serialize_us` describes the exact
        // final summary+evidence representation that is handed to write().
        let evidence_serialize_started = std::time::Instant::now();
        let evidence_bytes = serde_json::to_vec(&pair.evidence)
            .context("serialize final metric-contract evidence")?;
        let evidence_serialize_us = u32::try_from(evidence_serialize_started.elapsed().as_micros())
            .context("evidence serialization duration does not fit u32")?;
        let (summary_bytes, final_serialization_us) =
            serialize_final_summary_bytes(&mut pair.decision_v34, evidence_serialize_us)?;
        let final_byte_materialization_us =
            u32::try_from(final_bytes_started.elapsed().as_micros())
                .context("final metric-contract byte materialization duration does not fit u32")?;
        self.stats.record_pair_resources(&pair)?;
        debug_assert!(final_byte_materialization_us >= final_serialization_us);

        if self.config.fault_injection
            == Some(MetricContractWriterFaultInjectionV1::SummaryEnospcAfterEvidence)
        {
            self.evidence
                .write_encoded_json_line(evidence_bytes, &identity, None)
                .await
                .context("write metric-contract evidence before injected summary failure")?;
            self.stats
                .evidence_rows_written_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .summary_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .orphan_evidence_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .missing_pair_total
                .fetch_add(1, Ordering::Relaxed);
            return self
                .return_error_after_manifest(std::io::Error::from_raw_os_error(28).into())
                .await;
        }

        if self.config.fault_injection == Some(MetricContractWriterFaultInjectionV1::SummaryEnospc)
        {
            self.stats
                .summary_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .missing_pair_total
                .fetch_add(1, Ordering::Relaxed);
            return self
                .return_error_after_manifest(std::io::Error::from_raw_os_error(28).into())
                .await;
        }

        if let Err(error) = self
            .summary
            .write_encoded_json_line(
                summary_bytes,
                &identity,
                match self.config.fault_injection {
                    Some(MetricContractWriterFaultInjectionV1::SummaryShortWriteAfterBytes(
                        bytes,
                    )) => Some(bytes),
                    _ => None,
                },
            )
            .await
        {
            self.stats
                .summary_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .missing_pair_total
                .fetch_add(1, Ordering::Relaxed);
            return self
                .return_error_after_manifest(error.context("write metric-contract v34 summary"))
                .await;
        }
        self.stats
            .summary_rows_written_total
            .fetch_add(1, Ordering::Relaxed);

        if self.config.fault_injection
            == Some(MetricContractWriterFaultInjectionV1::EvidenceEnospcAfterSummary)
        {
            self.stats
                .evidence_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .orphan_summary_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .missing_pair_total
                .fetch_add(1, Ordering::Relaxed);
            return self
                .return_error_after_manifest(std::io::Error::from_raw_os_error(28).into())
                .await;
        }

        if let Err(error) = self
            .evidence
            .write_encoded_json_line(
                evidence_bytes,
                &identity,
                match self.config.fault_injection {
                    Some(
                        MetricContractWriterFaultInjectionV1::EvidenceShortWriteAfterSummaryBytes(
                            bytes,
                        ),
                    ) => Some(bytes),
                    _ => None,
                },
            )
            .await
        {
            self.stats
                .evidence_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .orphan_summary_total
                .fetch_add(1, Ordering::Relaxed);
            self.stats
                .missing_pair_total
                .fetch_add(1, Ordering::Relaxed);
            return self
                .return_error_after_manifest(
                    error.context("write metric-contract evidence sidecar"),
                )
                .await;
        }
        self.stats
            .evidence_rows_written_total
            .fetch_add(1, Ordering::Relaxed);

        self.update_manifest().await?;
        Ok(())
    }

    async fn return_error_after_manifest(&mut self, error: anyhow::Error) -> Result<()> {
        // Persist any accepted prefix before recording the failure manifest;
        // the manifest must never claim bytes that were not made durable.
        let _ = self.summary.sync_data().await;
        let _ = self.evidence.sync_data().await;
        if let Err(manifest_error) = self.update_manifest().await {
            return Err(error.context(format!(
                "paired write failed and failure-state manifest persistence also failed: {manifest_error:#}"
            )));
        }
        Err(error)
    }

    async fn update_manifest(&mut self) -> Result<()> {
        self.summary.sync_data().await?;
        self.evidence.sync_data().await?;
        let provenance = self
            .frozen_provenance
            .as_ref()
            .context("cannot persist metric-contract manifest without frozen provenance")?;
        upsert_part(
            &mut self.manifest.summary_parts,
            self.summary.manifest(provenance, &self.config),
        );
        self.manifest.writer_stats = self.stats.snapshot();
        upsert_part(
            &mut self.manifest.evidence_parts,
            self.evidence.manifest(provenance, &self.config),
        );
        self.persist_manifest().await
    }

    async fn persist_manifest(&mut self) -> Result<()> {
        let result = self.persist_manifest_inner().await;
        if result.is_err() {
            self.stats
                .manifest_write_failures_total
                .fetch_add(1, Ordering::Relaxed);
        }
        result
    }

    async fn persist_manifest_inner(&mut self) -> Result<()> {
        self.manifest.writer_stats = self.stats.snapshot();
        if self.manifest.writer_finalized
            && self.config.fault_injection
                == Some(MetricContractWriterFaultInjectionV1::FinalManifestEnospc)
        {
            return Err(std::io::Error::from_raw_os_error(28).into());
        }
        let bytes = serde_json::to_vec_pretty(&self.manifest)?;
        let path = self
            .config
            .directory
            .join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
        let temp_path = path.with_extension("json.tmp");
        let mut file = File::create(&temp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        file.sync_all().await?;
        drop(file);
        rename(&temp_path, &path).await?;
        let directory = File::open(&self.config.directory).await?;
        directory.sync_all().await?;
        Ok(())
    }

    pub async fn finalize(&mut self) -> Result<()> {
        if let Err(error) = self.summary.sync_data().await {
            self.stats
                .finalization_failures_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        if let Err(error) = self.evidence.sync_data().await {
            self.stats
                .finalization_failures_total
                .fetch_add(1, Ordering::Relaxed);
            return Err(error);
        }
        self.manifest.writer_finalized = true;
        let result = if self.frozen_provenance.is_some() {
            self.update_manifest().await
        } else {
            self.persist_manifest().await
        };
        if let Err(error) = result {
            self.stats
                .finalization_failures_total
                .fetch_add(1, Ordering::Relaxed);
            // Never leave an older manifest claiming immutable completion.
            // A best-effort second write with `writer_finalized=false` makes a
            // transient finalization failure durable and audit-rejectable.
            self.manifest.writer_finalized = false;
            let _ = self.persist_manifest().await;
            return Err(error);
        }
        Ok(())
    }
}

fn serialize_final_summary_bytes(
    summary: &mut MetricContractDecisionSummaryV1,
    evidence_serialize_us: u32,
) -> Result<(Vec<u8>, u32)> {
    // A self-referential duration cannot be obtained by repeatedly timing a
    // variable-width JSON integer: scheduler jitter can make the value
    // oscillate forever. Serialize exactly once with a fixed-width numeric
    // telemetry slot, then replace only that slot with JSON whitespace plus
    // the measured number. JSON whitespace before a number is lossless, the
    // final bytes deserialize to the exact in-memory summary, and the timed
    // serde pass has the same byte width as the persisted record.
    const SENTINEL: u32 = u32::MAX;
    const SENTINEL_BYTES: &[u8; 10] = b"4294967295";
    const TELEMETRY_FIELD_WITH_SENTINEL: &[u8] = b"\"metric_contract_serialize_us\":4294967295";
    summary.metric_contract_serialize_us = SENTINEL;
    let started = std::time::Instant::now();
    let mut bytes =
        serde_json::to_vec(summary).context("serialize final metric-contract summary")?;
    let summary_serialize_us = u32::try_from(started.elapsed().as_micros())
        .context("summary serialization duration does not fit u32")?;
    let combined = evidence_serialize_us
        .checked_add(summary_serialize_us)
        .context("combined summary/evidence serialization duration overflow")?;
    let value = combined.to_string();
    anyhow::ensure!(
        value.len() <= SENTINEL_BYTES.len(),
        "metric-contract serialization duration exceeds fixed JSON telemetry slot"
    );
    let position = bytes
        .windows(TELEMETRY_FIELD_WITH_SENTINEL.len())
        .position(|window| window == TELEMETRY_FIELD_WITH_SENTINEL)
        .map(|field_start| field_start + TELEMETRY_FIELD_WITH_SENTINEL.len() - SENTINEL_BYTES.len())
        .context("final v34 serialization is missing its exact telemetry sentinel field")?;
    let slot = &mut bytes[position..position + SENTINEL_BYTES.len()];
    slot.fill(b' ');
    let value_start = slot.len() - value.len();
    slot[value_start..].copy_from_slice(value.as_bytes());
    summary.metric_contract_serialize_us = combined;
    debug_assert_eq!(
        serde_json::from_slice::<MetricContractDecisionSummaryV1>(&bytes).ok(),
        Some(summary.clone())
    );
    Ok((bytes, combined))
}

fn upsert_part(
    parts: &mut Vec<MetricContractRotatedPartManifestV1>,
    part: MetricContractRotatedPartManifestV1,
) {
    if let Some(existing) = parts
        .iter_mut()
        .find(|existing| existing.part_index == part.part_index)
    {
        *existing = part;
    } else {
        parts.push(part);
        parts.sort_by_key(|entry| entry.part_index);
    }
}
