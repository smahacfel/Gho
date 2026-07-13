use anyhow::{Context, Result};
use ghost_core::metric_contracts::{
    CanonicalHashV1, MetricContractDecisionSummaryV1, MetricContractEvidenceTransportV1,
    MetricContractPairedRecordV1, MetricEvidenceRecordIdentityV1,
    ResolvedMetricContractEffectiveConfigV1,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use tokio::fs::{create_dir_all, rename, File, OpenOptions};
use tokio::io::AsyncWriteExt;

pub const METRIC_CONTRACT_SUMMARY_V34_FILE: &str = "metric_contract_decisions_v34.jsonl";
pub const METRIC_CONTRACT_EVIDENCE_V1_FILE: &str = "metric_contract_evidence_v1.jsonl";
pub const METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE: &str =
    "metric_contract_rotation_manifest_v1.json";
pub const DEFAULT_METRIC_CONTRACT_ROTATION_MAX_BYTES: u64 = 64 * 1024 * 1024;
pub const METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1: [u32; 12] =
    [1, 2, 4, 8, 16, 32, 64, 128, 256, 512, 1_000, 2_000];

#[derive(Debug, Clone)]
pub struct MetricContractPairedWriterConfigV1 {
    pub directory: PathBuf,
    pub rotation_max_bytes: u64,
    pub build_commit: String,
    pub queue_capacity: usize,
    /// Deterministic failure hook used only by durability regression fixtures.
    /// Production construction always leaves it at `None`.
    pub fault_injection: Option<MetricContractWriterFaultInjectionV1>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MetricContractWriterFaultInjectionV1 {
    SummaryEnospc,
    EvidenceEnospcAfterSummary,
}

impl MetricContractPairedWriterConfigV1 {
    #[must_use]
    pub fn new(directory: PathBuf, build_commit: impl Into<String>) -> Self {
        Self {
            directory,
            rotation_max_bytes: DEFAULT_METRIC_CONTRACT_ROTATION_MAX_BYTES,
            build_commit: build_commit.into(),
            queue_capacity: 1_000,
            fault_injection: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MetricContractLatencyHistogramSnapshotV1 {
    pub bucket_upper_bounds_us: [u32; 12],
    /// One count per upper bound plus a final overflow bucket.
    pub bucket_counts: [u64; 13],
    pub sample_count: u64,
    pub max_us: u64,
}

impl Default for MetricContractLatencyHistogramSnapshotV1 {
    fn default() -> Self {
        Self {
            bucket_upper_bounds_us: METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1,
            bucket_counts: [0; 13],
            sample_count: 0,
            max_us: 0,
        }
    }
}

impl MetricContractLatencyHistogramSnapshotV1 {
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
    bucket_counts: [AtomicU64; 13],
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
        let bucket = METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1
            .iter()
            .position(|upper| value_us <= u64::from(*upper))
            .unwrap_or(METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1.len());
        self.bucket_counts[bucket].fetch_add(1, Ordering::Relaxed);
        self.sample_count.fetch_add(1, Ordering::Relaxed);
        self.max_us.fetch_max(value_us, Ordering::Relaxed);
    }

    fn snapshot(&self) -> MetricContractLatencyHistogramSnapshotV1 {
        MetricContractLatencyHistogramSnapshotV1 {
            bucket_upper_bounds_us: METRIC_CONTRACT_LATENCY_BUCKET_UPPER_BOUNDS_US_V1,
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

    fn record_pair_resources(&self, pair: &MetricContractPairedRecordV1) {
        self.metric_contract_build_and_serialize_us
            .record(u64::from(pair.metric_contract_build_and_serialize_us));
        self.projection_build_and_validate_us
            .record(u64::from(pair.projection_build_and_validate_us));
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
    pub gatekeeper_config_hash: String,
    pub brain_config_hash: Option<String>,
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
    brain_config_hash: Option<String>,
    profile_id: String,
    profile_hash: CanonicalHashV1,
    metric_contract_effective_config_hash: CanonicalHashV1,
    effective_config: ResolvedMetricContractEffectiveConfigV1,
}

impl MetricContractPartProvenanceV1 {
    fn from_pair(pair: &MetricContractPairedRecordV1) -> Self {
        Self {
            run_id: pair.record_identity().run_id.clone(),
            gatekeeper_config_hash: pair.gatekeeper_config_hash.clone(),
            brain_config_hash: pair.brain_config_hash.clone(),
            profile_id: pair.decision_v34.profile_id.as_str().to_string(),
            profile_hash: pair.decision_v34.profile_hash.clone(),
            metric_contract_effective_config_hash: pair
                .decision_v34
                .metric_contract_effective_config_hash
                .clone(),
            effective_config: pair.effective_config.clone(),
        }
    }
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

    async fn write_json_line<T: Serialize>(
        &mut self,
        value: &T,
        identity: &MetricEvidenceRecordIdentityV1,
    ) -> Result<()> {
        let mut bytes = serde_json::to_vec(value).context("serialize metric-contract row")?;
        bytes.push(b'\n');

        // Account for every byte accepted by the filesystem, including a
        // prefix written before a later error. This keeps the rotation
        // manifest truthful for truncated/partial-write audit instead of
        // pretending that the part remained unchanged.
        let mut written = 0usize;
        while written < bytes.len() {
            let count = self.file.write(&bytes[written..]).await?;
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
        Ok(())
    }

    fn manifest(
        &self,
        provenance: &MetricContractPartProvenanceV1,
        build_commit: &str,
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
            build_commit: build_commit.to_string(),
            gatekeeper_config_hash: provenance.gatekeeper_config_hash.clone(),
            brain_config_hash: provenance.brain_config_hash.clone(),
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

    pub async fn write_pair(&mut self, pair: MetricContractPairedRecordV1) -> Result<()> {
        pair.validate_pair()
            .context("validate paired metric-contract record")?;
        let candidate_provenance = MetricContractPartProvenanceV1::from_pair(&pair);
        if let Some(frozen) = self.frozen_provenance.as_ref() {
            anyhow::ensure!(
                frozen == &candidate_provenance,
                "paired writer run/build/profile/effective-config provenance changed within one run"
            );
        } else {
            self.frozen_provenance = Some(candidate_provenance);
        }
        self.rotate_if_needed().await?;
        self.stats.record_pair_resources(&pair);
        self.stats
            .paired_commands_total
            .fetch_add(1, Ordering::Relaxed);
        let identity = pair.record_identity().clone();
        let transport = MetricContractEvidenceTransportV1::try_new(
            pair.evidence.payload.clone(),
            pair.evidence.writer_timestamp_ms,
            self.evidence.part_index,
        )?;

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
            .write_json_line::<MetricContractDecisionSummaryV1>(&pair.decision_v34, &identity)
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

        if let Err(error) = self.evidence.write_json_line(&transport, &identity).await {
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
        if let Err(manifest_error) = self.update_manifest().await {
            return Err(error.context(format!(
                "paired write failed and failure-state manifest persistence also failed: {manifest_error:#}"
            )));
        }
        Err(error)
    }

    async fn update_manifest(&mut self) -> Result<()> {
        let provenance = self
            .frozen_provenance
            .as_ref()
            .context("cannot persist metric-contract manifest without frozen provenance")?;
        upsert_part(
            &mut self.manifest.summary_parts,
            self.summary.manifest(provenance, &self.config.build_commit),
        );
        self.manifest.writer_stats = self.stats.snapshot();
        upsert_part(
            &mut self.manifest.evidence_parts,
            self.evidence
                .manifest(provenance, &self.config.build_commit),
        );
        self.persist_manifest().await
    }

    async fn persist_manifest(&mut self) -> Result<()> {
        self.manifest.writer_stats = self.stats.snapshot();
        let bytes = serde_json::to_vec_pretty(&self.manifest)?;
        let path = self
            .config
            .directory
            .join(METRIC_CONTRACT_ROTATION_MANIFEST_V1_FILE);
        let temp_path = path.with_extension("json.tmp");
        let mut file = File::create(&temp_path).await?;
        file.write_all(&bytes).await?;
        file.flush().await?;
        drop(file);
        rename(&temp_path, &path).await?;
        Ok(())
    }

    pub async fn finalize(&mut self) -> Result<()> {
        self.manifest.writer_finalized = true;
        if self.frozen_provenance.is_some() {
            self.update_manifest().await
        } else {
            self.persist_manifest().await
        }
    }
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
