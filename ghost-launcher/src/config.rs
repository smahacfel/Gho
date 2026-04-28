//! Configuration module for Ghost Launcher
//!
//! This module defines the configuration schema for loading from config.toml

use anyhow::{anyhow, bail};
use ghost_brain::config::ExecutionShadowConfig;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::{env, fmt};
use uuid::Uuid;

const DEFAULT_SECRET_ENV_FILE: &str = ".env";
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SecretValueSource {
    ProcessEnv,
    DotEnv,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ResolvedSecretValue {
    value: String,
    source: SecretValueSource,
}

#[derive(Debug, Clone, Default)]
struct LoadedSecretEnv {
    values: HashMap<String, String>,
    base_dir: Option<PathBuf>,
}

/// Main launcher configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LauncherConfig {
    /// Application mode (test or production)
    #[serde(default = "default_mode")]
    pub mode: AppMode,

    /// Seer configuration
    pub seer: SeerComponentConfig,

    /// Trigger configuration
    pub trigger: TriggerComponentConfig,

    /// Execution SSOT configuration (mode + paper/quotes/events).
    #[serde(default)]
    pub execution: ExecutionConfig,

    /// GUI Backend configuration
    pub gui_backend: GuiBackendComponentConfig,

    /// Oracle pipeline configuration
    #[serde(default)]
    pub oracle: OracleConfig,

    /// Shadow Ledger runtime SSOT configuration.
    #[serde(default)]
    pub shadow_ledger: ShadowLedgerConfig,

    /// AccountStateCore canonical ingest configuration.
    #[serde(default)]
    pub account_state_core: AccountStateCoreConfig,

    /// Session/runtime observation configuration.
    #[serde(default)]
    pub session: SessionRuntimeConfig,

    /// Tx intelligence runtime defaults.
    #[serde(default)]
    pub tx_intelligence: TxIntelligenceRuntimeConfig,

    /// Logging configuration
    #[serde(default)]
    pub logging: LoggingConfig,

    /// EPIC 3: Gatekeeper runtime configuration (commit window + commit loop cadence)
    #[serde(default)]
    pub gatekeeper: GatekeeperRuntimeConfig,

    /// EPIC 4: LivePipeline runtime configuration (buffering + flush loop cadence)
    #[serde(default)]
    pub live_pipeline: LivePipelineRuntimeConfig,

    /// SnapshotListener forward mode: controls how TX events are forwarded to SnapshotEngine
    /// "none" = drop all TX, "approved_only" = forward only approved pools, "provisional" = forward all
    #[serde(default = "default_snapshot_listener_forward_mode")]
    pub snapshot_listener_forward_mode: SnapshotListenerForwardMode,

    /// Maximum active pool count in SnapshotEngine when using provisional mode (memory guard)
    #[serde(default = "default_snapshot_listener_max_pools")]
    pub snapshot_listener_max_pools: usize,

    /// Per-pool cap for SnapshotEngine inactive transaction ring buffer.
    #[serde(default = "default_snapshot_inactive_tx_buffer_capacity")]
    pub snapshot_inactive_tx_buffer_capacity: usize,

    /// TTL margin added on top of gatekeeper window for inactive tx buffering.
    #[serde(default = "default_snapshot_inactive_tx_ttl_margin_ms")]
    pub snapshot_inactive_tx_ttl_margin_ms: u64,

    /// Path to ghost_brain_config.toml (optional, defaults to "ghost-brain/ghost_brain_config.toml")
    /// This config controls all Ghost Brain analytical modules (SSMI, QASS, QEDD, MCI, etc.)
    #[serde(default = "default_ghost_brain_config_path")]
    pub ghost_brain_config_path: String,

    /// Prometheus metrics HTTP server configuration
    #[serde(default)]
    pub metrics: MetricsConfig,

    /// WAL + ShadowLedger snapshot durability configuration (Z1.1 / Z1.2).
    ///
    /// Setting `wal_dir` or `snapshot_dir` here is equivalent to setting the
    /// `GHOST_WAL_DIR` / `GHOST_SNAPSHOT_DIR` environment variables.
    /// Env vars take priority over this section when both are present.
    #[serde(default)]
    pub durability: DurabilityConfig,
}

/// Z1.1 / Z1.2 durability configuration.
///
/// Mirrors the `[durability]` table in `config.toml`.
/// All fields are optional; when omitted the feature is disabled unless the
/// corresponding environment variable is set.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DurabilityConfig {
    /// Explicit toggle for WAL durability.
    /// Equivalent to `GHOST_WAL_ENABLED` env var (env var takes precedence).
    #[serde(default = "default_true")]
    pub wal_enabled: bool,

    /// Directory for the shared Write-Ahead Log.
    /// Equivalent to `GHOST_WAL_DIR` env var (env var takes precedence).
    pub wal_dir: Option<PathBuf>,

    /// WAL segment duration in milliseconds.
    /// Equivalent to `GHOST_WAL_SEGMENT_MS` env var (env var takes precedence).
    #[serde(default = "default_wal_segment_ms")]
    pub wal_segment_ms: u64,

    /// WAL retention in milliseconds (how far back to keep segments).
    /// Equivalent to `GHOST_WAL_RETENTION_MS` env var (env var takes precedence).
    #[serde(default = "default_wal_retention_ms")]
    pub wal_retention_ms: u64,

    /// Directory for periodic ShadowLedger disk snapshots.
    /// Equivalent to `GHOST_SNAPSHOT_DIR` env var (env var takes precedence).
    pub snapshot_dir: Option<PathBuf>,

    /// Interval between disk snapshots (seconds, minimum 1).
    /// Equivalent to `GHOST_SNAPSHOT_INTERVAL_S` env var (env var takes precedence).
    #[serde(default = "default_snapshot_interval_s")]
    pub snapshot_interval_s: u64,
}

impl Default for DurabilityConfig {
    fn default() -> Self {
        Self {
            wal_enabled: default_true(),
            wal_dir: None,
            wal_segment_ms: default_wal_segment_ms(),
            wal_retention_ms: default_wal_retention_ms(),
            snapshot_dir: None,
            snapshot_interval_s: default_snapshot_interval_s(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilitySettingSource {
    Config,
    Env,
}

impl fmt::Display for DurabilitySettingSource {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Config => write!(f, "config"),
            Self::Env => write!(f, "env"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DurabilityMode {
    Disabled,
    WalOnly,
    SnapshotOnly,
    SnapshotAndWal,
}

impl DurabilityMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Disabled => "disabled",
            Self::WalOnly => "wal_only",
            Self::SnapshotOnly => "snapshot_only",
            Self::SnapshotAndWal => "snapshot_and_wal",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDurabilityPath {
    pub path: PathBuf,
    pub source: DurabilitySettingSource,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedDurabilityConfig {
    pub wal: Option<ResolvedDurabilityPath>,
    pub wal_segment_ms: u64,
    pub wal_retention_ms: u64,
    pub snapshot: Option<ResolvedDurabilityPath>,
    pub snapshot_interval_s: u64,
}

impl ResolvedDurabilityConfig {
    pub fn mode(&self) -> DurabilityMode {
        match (self.wal.is_some(), self.snapshot.is_some()) {
            (false, false) => DurabilityMode::Disabled,
            (true, false) => DurabilityMode::WalOnly,
            (false, true) => DurabilityMode::SnapshotOnly,
            (true, true) => DurabilityMode::SnapshotAndWal,
        }
    }

    pub fn wal_dir(&self) -> Option<&Path> {
        self.wal.as_ref().map(|entry| entry.path.as_path())
    }

    pub fn snapshot_dir(&self) -> Option<&Path> {
        self.snapshot.as_ref().map(|entry| entry.path.as_path())
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AccountStateCoreConfig {
    #[serde(default = "default_true")]
    pub enable: bool,
}

impl Default for AccountStateCoreConfig {
    fn default() -> Self {
        Self { enable: true }
    }
}

fn default_session_max_sessions() -> usize {
    10_000
}

fn default_session_checkpoint_interval_ms() -> u64 {
    2_000
}

fn default_session_max_observation_window_ms() -> u64 {
    10_000
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionRuntimeConfig {
    #[serde(default = "default_session_max_sessions")]
    pub max_sessions: usize,
    #[serde(default = "default_session_checkpoint_interval_ms")]
    pub checkpoint_interval_ms: u64,
    #[serde(default = "default_session_max_observation_window_ms")]
    pub max_observation_window_ms: u64,
}

impl Default for SessionRuntimeConfig {
    fn default() -> Self {
        Self {
            max_sessions: default_session_max_sessions(),
            checkpoint_interval_ms: default_session_checkpoint_interval_ms(),
            max_observation_window_ms: default_session_max_observation_window_ms(),
        }
    }
}

fn default_tx_intelligence_dust_threshold_sol() -> f64 {
    0.001
}

fn default_tx_intelligence_burst_window_ms() -> u64 {
    500
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TxIntelligenceRuntimeConfig {
    #[serde(default = "default_tx_intelligence_dust_threshold_sol")]
    pub dust_threshold_sol: f64,
    #[serde(default = "default_tx_intelligence_burst_window_ms")]
    pub burst_window_ms: u64,
}

impl Default for TxIntelligenceRuntimeConfig {
    fn default() -> Self {
        Self {
            dust_threshold_sol: default_tx_intelligence_dust_threshold_sol(),
            burst_window_ms: default_tx_intelligence_burst_window_ms(),
        }
    }
}

fn default_wal_segment_ms() -> u64 {
    300_000 // 5 minutes
}

fn default_wal_retention_ms() -> u64 {
    24 * 60 * 60 * 1_000 // 24 hours
}

fn default_snapshot_interval_s() -> u64 {
    60
}

/// Prometheus metrics HTTP server configuration for the launcher process.
///
/// The server exposes `GET /metrics` in Prometheus text format, scraped by
/// Prometheus. All oracle metrics registered via `oracle_metrics::register_oracle_metrics`
/// are served from this endpoint (default registry).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable the Prometheus /metrics HTTP server
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Bind address for the /metrics server
    #[serde(default = "default_metrics_bind")]
    pub bind: String,
    /// Port for the /metrics HTTP server (used by Prometheus scraper)
    #[serde(default = "default_metrics_port")]
    pub port: u16,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            bind: default_metrics_bind(),
            port: default_metrics_port(),
        }
    }
}

fn default_metrics_bind() -> String {
    "0.0.0.0".to_string()
}

fn default_metrics_port() -> u16 {
    9090
}

/// SnapshotListener forward mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SnapshotListenerForwardMode {
    /// Drop all TX (SnapshotEngine receives nothing)
    None,
    /// Forward TX for tracked pools; unapproved pools are buffered as SoftTruth.
    TrackedBuffered,
    /// Forward only for pools approved by Gatekeeper
    ApprovedOnly,
    /// Forward all TX provisionally (soft-truth signals)
    Provisional,
}

fn default_snapshot_listener_forward_mode() -> SnapshotListenerForwardMode {
    SnapshotListenerForwardMode::TrackedBuffered
}

fn default_snapshot_listener_max_pools() -> usize {
    500
}

fn default_snapshot_inactive_tx_buffer_capacity() -> usize {
    8_192
}

fn default_snapshot_inactive_tx_ttl_margin_ms() -> u64 {
    2_000
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ExecutionMode {
    Live,
    Paper,
    Shadow,
    Dual,
}

impl Default for ExecutionMode {
    fn default() -> Self {
        Self::Live
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TriggerEntryMode {
    Live,
    DryRunMock,
    ShadowOnly,
    LiveAndShadow,
}

impl Default for TriggerEntryMode {
    fn default() -> Self {
        Self::Live
    }
}

impl TriggerEntryMode {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Live => "live",
            Self::DryRunMock => "dry_run_mock",
            Self::ShadowOnly => "shadow_only",
            Self::LiveAndShadow => "live_and_shadow",
        }
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum ShadowRunCommitment {
    Processed,
    Confirmed,
    Finalized,
}

impl Default for ShadowRunCommitment {
    fn default() -> Self {
        Self::Processed
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum SeerCommitment {
    #[serde(alias = "mempool")]
    Processed,
    Confirmed,
    Finalized,
}

impl Default for SeerCommitment {
    fn default() -> Self {
        Self::Processed
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    #[serde(default)]
    pub execution_mode: ExecutionMode,
    /// Deprecated alias for backwards compatibility.
    #[serde(default)]
    pub dry_run: Option<bool>,
    #[serde(default)]
    pub paper: ExecutionPaperConfig,
    #[serde(default)]
    pub quotes: ExecutionQuotesConfig,
    #[serde(default)]
    pub events: ExecutionEventsConfig,
    #[serde(default)]
    pub shadow: ExecutionShadowConfig,
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            execution_mode: ExecutionMode::Live,
            dry_run: None,
            paper: ExecutionPaperConfig::default(),
            quotes: ExecutionQuotesConfig::default(),
            events: ExecutionEventsConfig::default(),
            shadow: ExecutionShadowConfig::default(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPaperConfig {
    #[serde(default = "default_execution_fill_delay_min_ms")]
    pub fill_delay_ms_min: u64,
    #[serde(default = "default_execution_fill_delay_max_ms")]
    pub fill_delay_ms_max: u64,
    #[serde(default = "default_execution_jitter_ms")]
    pub jitter_ms: u64,
    #[serde(default = "default_execution_max_quote_age_ms")]
    pub max_quote_age_ms: u64,
}

impl Default for ExecutionPaperConfig {
    fn default() -> Self {
        Self {
            fill_delay_ms_min: default_execution_fill_delay_min_ms(),
            fill_delay_ms_max: default_execution_fill_delay_max_ms(),
            jitter_ms: default_execution_jitter_ms(),
            max_quote_age_ms: default_execution_max_quote_age_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionQuotesConfig {
    #[serde(default = "default_execution_max_quote_age_ms")]
    pub max_quote_age_ms: u64,
    #[serde(default = "default_execution_ring_buffer_size")]
    pub ring_buffer_size: usize,
    #[serde(default = "default_execution_quote_generation_interval_ms")]
    pub generation_interval_ms: u64,
    #[serde(default = "default_execution_stale_warning_threshold_ms")]
    pub stale_warning_threshold_ms: u64,
}

impl Default for ExecutionQuotesConfig {
    fn default() -> Self {
        Self {
            max_quote_age_ms: default_execution_max_quote_age_ms(),
            ring_buffer_size: default_execution_ring_buffer_size(),
            generation_interval_ms: default_execution_quote_generation_interval_ms(),
            stale_warning_threshold_ms: default_execution_stale_warning_threshold_ms(),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEventsConfig {
    #[serde(default = "default_execution_events_output_dir")]
    pub output_dir: String,
    #[serde(default = "default_execution_events_rotation_interval_ms")]
    pub rotation_interval_ms: u64,
    #[serde(default = "default_execution_events_flush_interval_ms")]
    pub flush_interval_ms: u64,
    #[serde(default = "default_execution_events_max_file_size_bytes")]
    pub max_file_size_bytes: u64,
    #[serde(default = "default_true")]
    pub enable_aem_ticks: bool,
    #[serde(default)]
    pub enable_optional_events: bool,
}

impl Default for ExecutionEventsConfig {
    fn default() -> Self {
        Self {
            output_dir: default_execution_events_output_dir(),
            rotation_interval_ms: default_execution_events_rotation_interval_ms(),
            flush_interval_ms: default_execution_events_flush_interval_ms(),
            max_file_size_bytes: default_execution_events_max_file_size_bytes(),
            enable_aem_ticks: true,
            enable_optional_events: false,
        }
    }
}

impl LauncherConfig {
    /// Resolve a config file path regardless of the current working directory.
    ///
    /// When `path` is relative (typically `config.toml`), search the current
    /// working directory and all of its ancestors first. If that fails, repeat
    /// the search from the launcher executable directory upward. This makes the
    /// launcher robust when started from nested subdirectories such as
    /// `logs/decisions.jsonl/`.
    pub fn resolve_config_path<P: AsRef<Path>>(path: P) -> Option<PathBuf> {
        let path = path.as_ref();

        if path.is_absolute() {
            return path.exists().then(|| path.to_path_buf());
        }

        if path.exists() {
            return Some(path.to_path_buf());
        }

        let mut roots = Vec::new();
        if let Ok(cwd) = std::env::current_dir() {
            roots.push(cwd);
        }
        if let Ok(exe) = std::env::current_exe() {
            if let Some(parent) = exe.parent() {
                roots.push(parent.to_path_buf());
            }
        }

        resolve_path_from_ancestors(path, roots)
    }

    pub fn lookup_secret_value_for_config_path<P: AsRef<Path>>(
        config_path: P,
        var_name: &str,
    ) -> anyhow::Result<Option<String>> {
        let requested_path = config_path.as_ref();
        let resolved_path = Self::resolve_config_path(requested_path)
            .unwrap_or_else(|| requested_path.to_path_buf());
        let config_dir = resolved_path.parent().unwrap_or_else(|| Path::new("."));
        let secret_env = load_secret_env(config_dir)?;
        Ok(lookup_secret_env(var_name, &secret_env).map(|value| value.value))
    }

    /// Returns warnings for config fields that are currently accepted but not applied
    /// by the production runtime.
    pub fn legacy_config_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();
        warnings.extend(self.gatekeeper.legacy_warnings());
        warnings.extend(self.live_pipeline.legacy_warnings());
        if self.execution.execution_mode == ExecutionMode::Paper
            && self.trigger.entry_mode == TriggerEntryMode::ShadowOnly
        {
            warnings.push(
                "[execution] execution_mode=paper + trigger.entry_mode=shadow_only remains a legacy compare-only compatibility profile; canonical shadow runtime should use [execution].execution_mode=shadow with [trigger].entry_mode=shadow_only"
                    .to_string(),
            );
        }
        if self.trigger.shadow_run.enabled && self.execution.execution_mode != ExecutionMode::Shadow
        {
            warnings.push(
                "[trigger.shadow_run] is a legacy compare-only surface outside canonical execution_mode=shadow; canonical shadow runtime artifacts live under [execution.shadow].entry_log_path / lifecycle_log_path"
                    .to_string(),
            );
        }
        if self.account_state_core.enable && !self.oracle.canonical_account_update_relay_enabled {
            warnings.push(
                "[oracle] canonical_account_update_relay_enabled=false is ignored when [account_state_core].enable=true; production runtime keeps canonical AccountUpdate ingest enabled"
                    .to_string(),
            );
        }
        if !self.account_state_core.enable && self.oracle.canonical_account_update_relay_enabled {
            warnings.push(
                "[oracle] canonical_account_update_relay_enabled=true has no effect when [account_state_core].enable=false; degraded/test startup remains tx/bootstrap-only because canonical AccountUpdate ingest is owned by AccountStateCore enablement"
                    .to_string(),
            );
        }
        warnings
    }

    pub fn validate_execution_profile(&self) -> Result<(), String> {
        validate_execution_pair(
            self.execution.execution_mode,
            self.trigger.entry_mode,
            false,
        )?;
        validate_shadow_transport(self)?;
        validate_live_sender_transport(self)?;
        validate_rollout_safety_profile(self)
    }

    pub fn validate_gatekeeper_runtime_contract(
        &self,
        gatekeeper_v2: &ghost_brain::config::GatekeeperV2Config,
    ) -> Result<(), String> {
        if self.mode == AppMode::Production && !gatekeeper_v2.use_three_layer_decision {
            return Err(
                "production config must set [gatekeeper_v2].use_three_layer_decision = true; legacy terminal feature mode is compat/test-only after Phase 2"
                    .to_string(),
            );
        }

        Ok(())
    }

    pub fn resolve_durability_config(&self) -> anyhow::Result<ResolvedDurabilityConfig> {
        let wal_enabled =
            parse_bool_env_override("GHOST_WAL_ENABLED", self.durability.wal_enabled)?;
        let wal = if wal_enabled {
            resolve_optional_durability_path("GHOST_WAL_DIR", self.durability.wal_dir.clone())?
        } else {
            None
        };
        let snapshot = resolve_optional_durability_path(
            "GHOST_SNAPSHOT_DIR",
            self.durability.snapshot_dir.clone(),
        )?;
        let wal_segment_ms =
            parse_u64_env_override("GHOST_WAL_SEGMENT_MS", self.durability.wal_segment_ms)?;
        let wal_retention_ms =
            parse_u64_env_override("GHOST_WAL_RETENTION_MS", self.durability.wal_retention_ms)?;
        let snapshot_interval_s = parse_u64_env_override(
            "GHOST_SNAPSHOT_INTERVAL_S",
            self.durability.snapshot_interval_s,
        )?;

        let resolved = ResolvedDurabilityConfig {
            wal,
            wal_segment_ms,
            wal_retention_ms,
            snapshot,
            snapshot_interval_s,
        };

        validate_resolved_durability(&resolved)?;
        Ok(resolved)
    }

    /// Resolve the effective source mode string (lowercased).
    pub fn effective_source_mode(&self) -> String {
        self.seer
            .source_mode
            .as_deref()
            .unwrap_or(&self.seer.connection_mode)
            .to_lowercase()
    }

    /// Resolve the effective gRPC x-token (prefers `grpc_x_token` over `grpc_auth_token`).
    pub fn effective_grpc_token(&self) -> Option<&str> {
        self.seer
            .grpc_x_token
            .as_deref()
            .or(self.seer.grpc_auth_token.as_deref())
            .filter(|t| !t.is_empty())
    }

    /// Validate gRPC-specific configuration.
    ///
    /// Returns `Ok(())` when `source_mode` is not gRPC, or when all gRPC
    /// requirements are met. Returns `Err(description)` when the config is
    /// invalid and the process should exit immediately.
    pub fn validate_grpc_config(&self) -> Result<(), String> {
        let mode = self.effective_source_mode();

        // Only validate when running in gRPC mode
        let is_grpc = mode == "grpc" || mode == "geyser_grpc" || mode == "g";
        if !is_grpc {
            return Ok(());
        }

        let ep = self.seer.grpc_endpoint.trim();

        // Endpoint must not be empty
        if ep.is_empty() {
            return Err("grpc_endpoint is empty".to_string());
        }

        // Reject localhost:10000 (default placeholder)
        let host = ep
            .strip_prefix("https://")
            .or_else(|| ep.strip_prefix("http://"))
            .unwrap_or(ep);
        if host == "localhost:10000" || host.starts_with("localhost:10000/") {
            return Err(format!(
                "grpc_endpoint is localhost:10000 (default placeholder): {}",
                ep
            ));
        }

        // Token is required for Chainstack/Yellowstone
        if self.effective_grpc_token().is_none() {
            return Err(
                "grpc_x_token is required in gRPC mode but is empty or missing".to_string(),
            );
        }

        Ok(())
    }

    /// Log a single-line config fingerprint (INFO level).
    pub fn log_config_fingerprint(&self) {
        let mode = self.effective_source_mode();
        let ep = redact_endpoint_for_logs(&self.seer.grpc_endpoint);
        let token_present = self.effective_grpc_token().is_some();
        let rpc = redact_endpoint_for_logs(&self.seer.rpc_endpoint);
        let events_dir = self.execution.events.output_dir.as_str();
        let shadow_entry_log = self.execution.shadow.entry_log_path.as_str();
        let shadow_timing_model = self.execution.shadow.timing_model.as_str();

        tracing::info!(
            "CONFIG | mode={} grpc_endpoint={} x_token={} rpc={} execution_mode={:?} entry_mode={} events_dir={} shadow_entry_log={} shadow_timing_model={}",
            mode,
            ep,
            token_present,
            rpc,
            self.execution.execution_mode,
            self.trigger.entry_mode.as_str(),
            events_dir,
            shadow_entry_log,
            shadow_timing_model,
        );
    }
}

fn validate_execution_pair(
    execution_mode: ExecutionMode,
    entry_mode: TriggerEntryMode,
    allow_legacy_dry_run_mock: bool,
) -> Result<(), String> {
    let valid = match (execution_mode, entry_mode) {
        (ExecutionMode::Paper, TriggerEntryMode::ShadowOnly)
        | (ExecutionMode::Shadow, TriggerEntryMode::ShadowOnly)
        | (ExecutionMode::Dual, TriggerEntryMode::LiveAndShadow)
        | (ExecutionMode::Live, TriggerEntryMode::Live) => true,
        (ExecutionMode::Paper, TriggerEntryMode::DryRunMock) if allow_legacy_dry_run_mock => true,
        _ => false,
    };

    if valid {
        return Ok(());
    }

    let expected_entry_mode = match execution_mode {
        ExecutionMode::Paper => "shadow_only",
        ExecutionMode::Shadow => "shadow_only",
        ExecutionMode::Dual => "live_and_shadow",
        ExecutionMode::Live => "live",
    };

    Err(format!(
        "invalid execution profile: [execution].execution_mode={:?} requires [trigger].entry_mode={} in production rollout configs (got {})",
        execution_mode,
        expected_entry_mode,
        entry_mode.as_str()
    ))
}

fn validate_rollout_safety_profile(config: &LauncherConfig) -> Result<(), String> {
    if !matches!(
        config.execution.execution_mode,
        ExecutionMode::Paper | ExecutionMode::Shadow | ExecutionMode::Dual
    ) {
        return Ok(());
    }

    if config.trigger.max_concurrent_positions == 0 {
        return Err(
            "rollout safety profile requires positive [trigger].max_concurrent_positions for paper/shadow/dual startup"
                .to_string(),
        );
    }
    if config.trigger.emergency_floor_sol <= 0.0 {
        return Err(
            "rollout safety profile requires non-zero [trigger].emergency_floor_sol for paper/shadow/dual startup"
                .to_string(),
        );
    }
    if config.trigger.position_size_buffer_sol <= 0.0 {
        return Err(
            "rollout safety profile requires non-zero [trigger].position_size_buffer_sol for paper/shadow/dual startup"
                .to_string(),
        );
    }
    if config.trigger.max_position_size_sol <= 0.0 {
        return Err(
            "rollout safety profile requires positive [trigger].max_position_size_sol for paper/shadow/dual startup"
                .to_string(),
        );
    }

    Ok(())
}

fn validate_live_sender_transport(config: &LauncherConfig) -> Result<(), String> {
    if !matches!(
        config.trigger.entry_mode,
        TriggerEntryMode::Live | TriggerEntryMode::LiveAndShadow
    ) {
        return Ok(());
    }

    let helius_endpoint = config
        .seer
        .helius_endpoint
        .as_deref()
        .map(str::trim)
        .unwrap_or_default();
    if helius_endpoint.is_empty() {
        return Err(
            "live execution requires non-empty [seer].helius_endpoint because BUY/SELL transport uses Helius Sender priority-fee RPC"
                .to_string(),
        );
    }
    if should_override_secret_value(Some(helius_endpoint)) {
        return Err(
            "live execution requires a real [seer].helius_endpoint (or GHOST_SEER_HELIUS_ENDPOINT); placeholder values are not allowed for the Sender priority-fee RPC"
                .to_string(),
        );
    }

    let mode = config.effective_source_mode();
    let is_grpc = mode == "grpc" || mode == "geyser_grpc" || mode == "g";
    if !is_grpc {
        return Err(
            "live execution requires [seer].source_mode=grpc because Yellowstone gRPC confirms Helius Sender signatures"
                .to_string(),
        );
    }

    config
        .validate_grpc_config()
        .map_err(|err| format!("live execution requires Yellowstone gRPC readiness: {err}"))
}

fn validate_shadow_transport(config: &LauncherConfig) -> Result<(), String> {
    if !matches!(
        config.trigger.entry_mode,
        TriggerEntryMode::ShadowOnly | TriggerEntryMode::LiveAndShadow
    ) {
        return Ok(());
    }

    if !config.trigger.shadow_run.enabled {
        return Err(
            "shadow-capable entry modes require [trigger.shadow_run].enabled = true because launcher shadow dispatch still uses the trigger.shadow_run transport adapter"
                .to_string(),
        );
    }

    let shadow_rpc_url = config.trigger.shadow_run.shadow_rpc_url.trim();
    if shadow_rpc_url.is_empty() {
        return Err(
            "shadow-capable entry modes require non-empty [trigger.shadow_run].shadow_rpc_url"
                .to_string(),
        );
    }

    if config.mode == AppMode::Production && should_override_secret_value(Some(shadow_rpc_url)) {
        return Err(
            "shadow-capable production profiles require a real [trigger.shadow_run].shadow_rpc_url (or GHOST_TRIGGER_SHADOW_RPC_URL); placeholder values are not allowed for launcher shadow transport"
                .to_string(),
        );
    }

    Ok(())
}

fn validate_loaded_execution_profile(
    config: &LauncherConfig,
    has_explicit_execution_mode: bool,
    has_explicit_trigger_entry_mode: bool,
    has_legacy_execution_dry_run: bool,
    has_legacy_oracle_dry_run: bool,
) -> anyhow::Result<()> {
    let uses_legacy_dry_run = has_legacy_execution_dry_run || has_legacy_oracle_dry_run;

    if config.mode == AppMode::Production {
        if !has_explicit_execution_mode {
            bail!(
                "production config must set [execution].execution_mode explicitly; implicit default/live fallback is not allowed"
            );
        }
        if !has_explicit_trigger_entry_mode {
            bail!(
                "production config must set [trigger].entry_mode explicitly; legacy trigger behavior mapping is not allowed"
            );
        }
        if uses_legacy_dry_run {
            bail!(
                "production config must not set legacy dry_run fields; remove [execution].dry_run / [oracle].dry_run and keep [execution].execution_mode + [trigger].entry_mode as the only rollout controls"
            );
        }
    } else if uses_legacy_dry_run {
        tracing::warn!(
            "legacy dry_run alias detected; keep [execution].execution_mode + [trigger].entry_mode explicit to avoid rollout ambiguity"
        );
    }

    validate_execution_pair(
        config.execution.execution_mode,
        config.trigger.entry_mode,
        config.mode != AppMode::Production,
    )
    .map_err(|err| anyhow!(err))?;

    if config.mode == AppMode::Production {
        validate_shadow_transport(config).map_err(|err| anyhow!(err))?;
    }
    validate_live_sender_transport(config).map_err(|err| anyhow!(err))?;

    if config.mode == AppMode::Production {
        validate_rollout_safety_profile(config).map_err(|err| anyhow!(err))?;
    }

    Ok(())
}

fn validate_resolved_durability(durability: &ResolvedDurabilityConfig) -> anyhow::Result<()> {
    if durability.wal.is_some() {
        if durability.wal_segment_ms == 0 {
            bail!("durability WAL requires wal_segment_ms > 0");
        }
        if durability.wal_retention_ms == 0 {
            bail!("durability WAL requires wal_retention_ms > 0");
        }
        if durability.wal_retention_ms < durability.wal_segment_ms {
            bail!(
                "durability WAL requires wal_retention_ms >= wal_segment_ms (got {} < {})",
                durability.wal_retention_ms,
                durability.wal_segment_ms
            );
        }
    }

    if durability.snapshot.is_some() && durability.snapshot_interval_s == 0 {
        bail!("durability snapshot requires snapshot_interval_s > 0");
    }

    Ok(())
}

fn parse_u64_env_override(var_name: &str, default: u64) -> anyhow::Result<u64> {
    match env::var(var_name) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                bail!("{var_name} is set but empty");
            }
            trimmed
                .parse::<u64>()
                .map_err(|err| anyhow!("invalid numeric env override {var_name}={trimmed}: {err}"))
        }
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("failed to read env var {var_name}: {err}")),
    }
}

fn parse_bool_env_override(var_name: &str, default: bool) -> anyhow::Result<bool> {
    match env::var(var_name) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                bail!("{var_name} is set but empty");
            }
            match trimmed.to_ascii_lowercase().as_str() {
                "1" | "true" | "yes" | "on" => Ok(true),
                "0" | "false" | "no" | "off" => Ok(false),
                _ => bail!("invalid boolean env override {var_name}={trimmed}"),
            }
        }
        Err(env::VarError::NotPresent) => Ok(default),
        Err(err) => Err(anyhow!("failed to read env var {var_name}: {err}")),
    }
}

fn resolve_optional_durability_path(
    env_var: &str,
    config_path: Option<PathBuf>,
) -> anyhow::Result<Option<ResolvedDurabilityPath>> {
    match env::var(env_var) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if trimmed.is_empty() {
                bail!("{env_var} is set but empty");
            }
            Ok(Some(ResolvedDurabilityPath {
                path: PathBuf::from(trimmed),
                source: DurabilitySettingSource::Env,
            }))
        }
        Err(env::VarError::NotPresent) => Ok(config_path.map(|path| ResolvedDurabilityPath {
            path,
            source: DurabilitySettingSource::Config,
        })),
        Err(err) => Err(anyhow!("failed to read env var {env_var}: {err}")),
    }
}

/// EPIC 3: Gatekeeper runtime configuration.
///
/// Mirrors the `[gatekeeper]` table in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatekeeperRuntimeConfig {
    /// Observation window before commit (milliseconds).
    #[serde(default = "default_gatekeeper_observation_window_ms")]
    pub observation_window_ms: u64,

    /// Minimum TX count required to pass and commit.
    #[serde(default = "default_gatekeeper_min_tx_to_pass")]
    pub min_tx_to_pass: usize,

    /// Maximum buffer age before cleanup (milliseconds).
    #[serde(default = "default_gatekeeper_drop_age_ms")]
    pub drop_age_ms: u64,

    /// Gatekeeper commit loop check interval (milliseconds).
    #[serde(default = "default_gatekeeper_check_interval_ms")]
    pub check_interval_ms: u64,

    /// K invariant tolerance (percent drift allowed due to fees).
    ///
    /// LEGACY/NO-OP (2026-01-22): Gatekeeper commit path no longer uses k-tolerance.
    /// Kept for backwards-compatible config parsing.
    #[serde(default)]
    pub k_tolerance_pct: Option<f64>,

    /// Maximum TX per mint buffer (to prevent memory exhaustion).
    ///
    /// LEGACY/NO-OP (2026-01-22): not enforced by launcher commit coordinator.
    #[serde(default)]
    pub max_tx_per_mint: Option<usize>,

    /// Maximum total buffers tracked (to prevent memory exhaustion).
    ///
    /// LEGACY/NO-OP (2026-01-22): not enforced by launcher commit coordinator.
    #[serde(default)]
    pub max_total_buffers: Option<usize>,

    /// Backoff base for commit failures (milliseconds).
    ///
    /// LEGACY/NO-OP (2026-01-22): commit retry/backoff policy is not implemented.
    #[serde(default)]
    pub commit_failure_backoff_base_ms: Option<u64>,

    /// Backoff max for commit failures (milliseconds).
    ///
    /// LEGACY/NO-OP (2026-01-22): commit retry/backoff policy is not implemented.
    #[serde(default)]
    pub commit_failure_backoff_max_ms: Option<u64>,

    /// Maximum retries for commit failures.
    ///
    /// LEGACY/NO-OP (2026-01-22): commit retry/backoff policy is not implemented.
    #[serde(default)]
    pub commit_failure_max_retries: Option<u32>,
}

impl Default for GatekeeperRuntimeConfig {
    fn default() -> Self {
        Self {
            observation_window_ms: default_gatekeeper_observation_window_ms(),
            min_tx_to_pass: default_gatekeeper_min_tx_to_pass(),
            drop_age_ms: default_gatekeeper_drop_age_ms(),
            check_interval_ms: default_gatekeeper_check_interval_ms(),
            k_tolerance_pct: None,
            max_tx_per_mint: None,
            max_total_buffers: None,
            commit_failure_backoff_base_ms: None,
            commit_failure_backoff_max_ms: None,
            commit_failure_max_retries: None,
        }
    }
}

impl GatekeeperRuntimeConfig {
    pub fn legacy_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if let Some(v) = self.k_tolerance_pct {
            warnings.push(format!(
                "[gatekeeper] k_tolerance_pct={} is currently not used by runtime (legacy/no-op)",
                v
            ));
        }

        if let Some(v) = self.max_tx_per_mint {
            warnings.push(format!(
                "[gatekeeper] max_tx_per_mint={} is currently not enforced by runtime (legacy/no-op)",
                v
            ));
        }

        if let Some(v) = self.max_total_buffers {
            warnings.push(format!(
                "[gatekeeper] max_total_buffers={} is currently not enforced by runtime (legacy/no-op)",
                v
            ));
        }

        if self.commit_failure_backoff_base_ms.is_some()
            || self.commit_failure_backoff_max_ms.is_some()
            || self.commit_failure_max_retries.is_some()
        {
            warnings.push(
                "[gatekeeper] commit_failure_* backoff/retry settings are currently not applied (legacy/no-op)"
                    .to_string(),
            );
        }

        warnings
    }
}

/// EPIC 4: LivePipeline runtime configuration.
///
/// Mirrors the `[live_pipeline]` table in `config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LivePipelineRuntimeConfig {
    /// Flush loop interval (milliseconds).
    #[serde(default = "default_live_pipeline_flush_interval_ms")]
    pub flush_interval_ms: u64,

    /// Flush delay in milliseconds for out-of-order buffering.
    #[serde(default = "default_live_pipeline_flush_delay_ms")]
    pub flush_delay_ms: u64,

    /// Maximum buffered TX per mint before forcing flush.
    #[serde(default = "default_live_pipeline_max_buffer_size")]
    pub max_buffer_size: usize,

    /// Maximum number of TxKeys retained in dedup cache per mint.
    #[serde(default = "default_live_pipeline_seen_keys_limit")]
    pub seen_keys_limit: usize,

    /// Maximum tracked mints (to prevent memory exhaustion).
    ///
    /// LEGACY/NO-OP (2026-01-22): not currently enforced by `ghost-core` LivePipeline.
    #[serde(default)]
    pub max_tracked_mints: Option<usize>,

    /// Backoff base for flush failures (milliseconds).
    ///
    /// LEGACY/NO-OP (2026-01-22): `flush_ready()` has no failure path.
    #[serde(default)]
    pub flush_failure_backoff_base_ms: Option<u64>,

    /// Backoff max for flush failures (milliseconds).
    ///
    /// LEGACY/NO-OP (2026-01-22): `flush_ready()` has no failure path.
    #[serde(default)]
    pub flush_failure_backoff_max_ms: Option<u64>,

    /// Maximum retries for flush failures.
    ///
    /// LEGACY/NO-OP (2026-01-22): `flush_ready()` has no failure path.
    #[serde(default)]
    pub flush_failure_max_retries: Option<u32>,
}

impl Default for LivePipelineRuntimeConfig {
    fn default() -> Self {
        Self {
            flush_interval_ms: default_live_pipeline_flush_interval_ms(),
            flush_delay_ms: default_live_pipeline_flush_delay_ms(),
            max_buffer_size: default_live_pipeline_max_buffer_size(),
            seen_keys_limit: default_live_pipeline_seen_keys_limit(),
            max_tracked_mints: None,
            flush_failure_backoff_base_ms: None,
            flush_failure_backoff_max_ms: None,
            flush_failure_max_retries: None,
        }
    }
}

impl LivePipelineRuntimeConfig {
    pub fn to_core_config(&self) -> ghost_core::shadow_ledger::LivePipelineConfig {
        ghost_core::shadow_ledger::LivePipelineConfig {
            flush_delay_ms: self.flush_delay_ms,
            max_buffer_size: self.max_buffer_size,
            seen_keys_limit: self.seen_keys_limit,
        }
    }

    pub fn legacy_warnings(&self) -> Vec<String> {
        let mut warnings = Vec::new();

        if let Some(v) = self.max_tracked_mints {
            warnings.push(format!(
                "[live_pipeline] max_tracked_mints={} is currently not enforced by runtime (legacy/no-op)",
                v
            ));
        }

        if self.flush_failure_backoff_base_ms.is_some()
            || self.flush_failure_backoff_max_ms.is_some()
            || self.flush_failure_max_retries.is_some()
        {
            warnings.push(
                "[live_pipeline] flush_failure_* backoff/retry settings are currently not applied (legacy/no-op)"
                    .to_string(),
            );
        }

        warnings
    }
}

/// Application mode
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum AppMode {
    Test,
    Production,
}

fn default_mode() -> AppMode {
    AppMode::Test
}

/// Seer component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeerComponentConfig {
    /// Enable Seer component
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Connection mode (websocket or grpc) - deprecated, use source_mode
    #[serde(default = "default_connection_mode")]
    pub connection_mode: String,

    /// Source mode (geyser_grpc, geyser_websocket, helius_websocket)
    pub source_mode: Option<String>,

    /// Geyser WebSocket endpoint
    #[serde(default = "default_geyser_endpoint")]
    pub geyser_endpoint: String,

    /// gRPC endpoint (for Yellowstone)
    #[serde(default = "default_grpc_endpoint")]
    pub grpc_endpoint: String,

    /// Helius WebSocket endpoint (optional, for helius_websocket mode)
    pub helius_endpoint: Option<String>,

    /// RPC endpoint
    #[serde(default = "default_rpc_endpoint")]
    pub rpc_endpoint: String,

    /// Enable Yellowstone slot-gap RPC backfill based on getTransaction.
    ///
    /// When disabled, Seer will not issue fallback RPC requests for slot gaps
    /// detected on the gRPC stream.
    #[serde(default = "default_true")]
    pub grpc_manual_backfill_enabled: bool,

    /// Enable legacy gRPC commitment fallback to the WebSocket transport.
    ///
    /// This fallback is disabled by default and should remain off in production.
    #[serde(default)]
    pub grpc_commitment_fallback_to_websocket: bool,

    /// Consecutive gRPC stalls required before a provider circuit opens.
    #[serde(default = "default_seer_grpc_max_stalls_before_open")]
    pub grpc_max_stalls_before_open: u32,

    /// Cooldown before an open provider circuit performs a half-open probe.
    #[serde(default = "default_seer_grpc_circuit_breaker_cooldown_ms")]
    pub grpc_circuit_breaker_cooldown_ms: u64,

    /// gRPC client ID (optional)
    pub grpc_client_id: Option<String>,

    /// gRPC authentication token (optional, legacy field - prefer grpc_x_token)
    pub grpc_auth_token: Option<String>,

    /// gRPC x-token for Chainstack/Yellowstone authentication (required for streaming)
    /// This token is sent with EVERY request including streaming messages via x-token header.
    /// Takes precedence over grpc_auth_token if both are provided.
    pub grpc_x_token: Option<String>,

    /// Enable Pump.fun detection
    #[serde(default = "default_true")]
    pub enable_pumpfun: bool,

    /// Enable Bonk.fun detection
    #[serde(default = "default_true")]
    pub enable_bonkfun: bool,

    /// Pump.fun program ID (configurable)
    #[serde(default = "default_pump_program_id")]
    pub pump_program_id: String,

    /// Bonk.fun program ID (configurable)
    #[serde(default = "default_bonk_program_id")]
    pub bonk_program_id: String,

    /// Metrics port
    #[serde(default = "default_seer_metrics_port")]
    pub metrics_port: u16,

    /// IPC buffer size
    #[serde(default = "default_ipc_buffer_size")]
    pub ipc_buffer_size: usize,

    /// IPC backpressure policy (block, drop_oldest, drop_new, drop_by_priority)
    #[serde(default = "default_backpressure_policy")]
    pub ipc_backpressure_policy: String,

    /// Stream architecture mode for gRPC ingestion (`single_global` | `pooled_filtered`).
    #[serde(default = "default_stream_mode")]
    pub stream_mode: String,

    /// Trade forwarding strategy (`per_pool` | `all`).
    #[serde(default = "default_tx_filter_strategy")]
    pub tx_filter_strategy: String,

    /// Dedicated funding-transfer lane mode (`disabled` | `pump_filtered` | `full_chain`).
    #[serde(default = "default_funding_lane_mode")]
    pub funding_lane_mode: String,

    /// TTL for watched pools in milliseconds (single_global mode).
    #[serde(default = "default_watched_pools_ttl_ms")]
    pub watched_pools_ttl_ms: u64,

    /// Maximum number of watched pools retained in memory.
    #[serde(default = "default_watched_pools_cap")]
    pub watched_pools_cap: usize,

    /// Debounce for repeated watch registrations (reserved for pooled_filtered mode).
    #[serde(default)]
    pub watch_debounce_ms: u64,

    /// Commitment level for Seer event ingestion (`processed` | `confirmed` | `finalized`).
    #[serde(default)]
    pub commitment: SeerCommitment,

    /// PumpPortal WebSocket configuration
    #[serde(default)]
    pub pumpportal: PumpPortalComponentConfig,
}

/// PumpPortal WebSocket configuration for launcher
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PumpPortalComponentConfig {
    /// PumpPortal WebSocket endpoint URL
    #[serde(default = "default_pumpportal_ws_url")]
    pub ws_url: String,

    /// Optional RPC endpoint override for PumpPortal mode
    /// If provided, it will be used as the Seer RPC endpoint
    #[serde(default)]
    pub rpc_endpoint: Option<String>,

    /// Maximum number of active mints to track simultaneously
    #[serde(default = "default_pumpportal_max_active_mints")]
    pub max_active_mints: usize,

    /// Batch size for subscription requests (to avoid rate limiting)
    #[serde(default = "default_pumpportal_subscription_batch_size")]
    pub subscription_batch_size: usize,

    /// Base delay between reconnection attempts (seconds)
    #[serde(default = "default_pumpportal_reconnect_base_delay")]
    pub reconnect_base_delay_secs: u64,

    /// Maximum delay between reconnection attempts (seconds)
    #[serde(default = "default_pumpportal_reconnect_max_delay")]
    pub reconnect_max_delay_secs: u64,

    /// Time window for tracking stats per mint (seconds)
    #[serde(default = "default_pumpportal_stats_window_secs")]
    pub stats_window_secs: u64,
}

impl Default for PumpPortalComponentConfig {
    fn default() -> Self {
        Self {
            ws_url: default_pumpportal_ws_url(),
            rpc_endpoint: None,
            max_active_mints: default_pumpportal_max_active_mints(),
            subscription_batch_size: default_pumpportal_subscription_batch_size(),
            reconnect_base_delay_secs: default_pumpportal_reconnect_base_delay(),
            reconnect_max_delay_secs: default_pumpportal_reconnect_max_delay(),
            stats_window_secs: default_pumpportal_stats_window_secs(),
        }
    }
}

/// Trigger component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerComponentConfig {
    /// Enable Trigger component
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Single Source of Truth for trigger entry behavior.
    #[serde(default)]
    pub entry_mode: TriggerEntryMode,

    /// RPC endpoint URL
    #[serde(default = "default_rpc_endpoint")]
    pub rpc_url: String,

    /// Path to the keypair JSON file (solana-keygen format)
    /// REQUIRED: Bot will not start without a valid keypair file
    pub keypair_path: Option<String>,

    /// BUY tip guard configuration used when Sender is unavailable and the local
    /// guard still needs to clamp a requested tip to a safe value.
    #[serde(default)]
    pub tip_guard: TriggerTipGuardConfig,

    /// Metrics port for trigger runtime.
    #[serde(default = "default_trigger_metrics_port")]
    pub metrics_port: u16,

    /// Maximum number of concurrent positions allowed.
    #[serde(default = "default_max_concurrent_positions")]
    pub max_concurrent_positions: usize,

    /// Maximum position size in SOL.
    #[serde(default = "default_max_position_size_sol")]
    pub max_position_size_sol: f64,

    /// Emergency floor in SOL used by rollout safety profile.
    #[serde(default = "default_emergency_floor_sol")]
    pub emergency_floor_sol: f64,

    /// Position size safety buffer in SOL.
    #[serde(default = "default_position_size_buffer_sol")]
    pub position_size_buffer_sol: f64,

    /// Slippage tolerance percentage.
    #[serde(default = "default_trigger_slippage_tolerance")]
    pub slippage_tolerance: f64,

    /// Maximum acceptable age of AccountStateCore state for live BUY preflight.
    #[serde(default = "default_live_preflight_max_state_age_slots")]
    pub live_preflight_max_state_age_slots: u64,

    /// Profit threshold used by the live SELL monitor (0.02 = +2%).
    #[serde(default = "default_live_exit_take_profit_pct")]
    pub live_exit_take_profit_pct: f64,

    /// Stop-loss threshold used by the live SELL monitor (0.02 = -2%).
    #[serde(default = "default_live_exit_stop_loss_pct")]
    pub live_exit_stop_loss_pct: f64,

    /// Shadow-run execution/reporting configuration.
    #[serde(default)]
    pub shadow_run: TriggerShadowRunConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerShadowRunConfig {
    /// Enable shadow-run reporting path.
    #[serde(default)]
    pub enabled: bool,

    /// RPC endpoint used for shadow simulation / reporting.
    #[serde(default = "default_shadow_run_rpc_url")]
    pub shadow_rpc_url: String,

    /// Commitment used for shadow simulation.
    #[serde(default)]
    pub commitment: ShadowRunCommitment,

    /// Whether signature verification is enabled during shadow simulation.
    #[serde(default)]
    pub sig_verify: bool,

    /// Whether recent blockhash should be replaced during shadow simulation.
    #[serde(default = "default_true")]
    pub replace_recent_blockhash: bool,

    /// Timeout for shadow-run simulation tasks.
    #[serde(default = "default_shadow_run_timeout_ms")]
    pub timeout_ms: u64,

    #[serde(default = "default_shadow_run_max_retries")]
    pub max_retries: usize,

    /// Maximum concurrent shadow simulation tasks.
    #[serde(default = "default_shadow_run_max_concurrent")]
    pub max_concurrent: usize,

    /// Output path for structured shadow-run reports.
    #[serde(default = "default_shadow_run_output_path")]
    pub output_path: String,

    /// Whether shadow-run results should also be emitted on the event bus.
    #[serde(default = "default_true")]
    pub emit_event_bus: bool,
}

impl Default for TriggerShadowRunConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            shadow_rpc_url: default_shadow_run_rpc_url(),
            commitment: ShadowRunCommitment::default(),
            sig_verify: false,
            replace_recent_blockhash: true,
            timeout_ms: default_shadow_run_timeout_ms(),
            max_retries: default_shadow_run_max_retries(),
            max_concurrent: default_shadow_run_max_concurrent(),
            output_path: default_shadow_run_output_path(),
            emit_event_bus: true,
        }
    }
}

/// Sender-era tip guard configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerTipGuardConfig {
    /// Absolute maximum tip in SOL enforced by the local tip guard.
    #[serde(default = "default_max_tip_absolute_sol")]
    pub max_tip_absolute_sol: f64,

    /// Fallback tip in SOL when the local tip guard rejects the requested amount.
    #[serde(default = "default_fallback_tip_sol")]
    pub fallback_tip_sol: f64,
}

impl Default for TriggerTipGuardConfig {
    fn default() -> Self {
        Self {
            max_tip_absolute_sol: default_max_tip_absolute_sol(),
            fallback_tip_sol: default_fallback_tip_sol(),
        }
    }
}

/// GUI Backend component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiBackendComponentConfig {
    /// Enable GUI Backend component
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Bind address
    #[serde(default = "default_gui_bind_address")]
    pub bind_address: String,

    /// Port
    #[serde(default = "default_gui_port")]
    pub port: u16,
}

/// Logging configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LoggingConfig {
    /// Log level (trace, debug, info, warn, error)
    #[serde(default = "default_log_level")]
    pub level: String,

    /// Enable file logging
    #[serde(default = "default_true")]
    pub file_enabled: bool,

    /// Log file path (for general/system logs)
    #[serde(default = "default_log_file")]
    pub file_path: String,

    /// Enable console logging
    #[serde(default = "default_true")]
    pub console_enabled: bool,

    /// Use JSON format for logs
    #[serde(default)]
    pub json_format: bool,

    /// Enable separate Oracle decision log file
    #[serde(default = "default_true")]
    pub oracle_log_enabled: bool,

    /// Oracle decision log file path
    #[serde(default = "default_oracle_log_file")]
    pub oracle_log_path: String,

    /// Use JSON format for Oracle logs
    #[serde(default)]
    pub oracle_json_format: bool,
}

impl Default for LoggingConfig {
    fn default() -> Self {
        Self {
            level: default_log_level(),
            file_enabled: true,
            file_path: default_log_file(),
            console_enabled: true,
            json_format: false,
            oracle_log_enabled: true,
            oracle_log_path: default_oracle_log_file(),
            oracle_json_format: false,
        }
    }
}

/// Oracle pipeline configuration
///
/// Controls the async Oracle scoring layer that filters and scores candidates
/// before they reach the Trigger component.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Enable Oracle scoring pipeline
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// SimpleOracle configuration
    #[serde(default)]
    pub simple_oracle: SimpleOracleConfig,

    /// QASS (Quantum-Style Amplitude Superposition Scoring) configuration
    #[serde(default)]
    pub qass: QassConfig,

    /// HyperOracle configuration (SCR/ULVF/POVC)
    #[serde(default)]
    pub hyper_oracle: HyperOracleConfig,

    /// Legacy alias for `[shadow_ledger]`.
    ///
    /// New configs should use the top-level section. The loader mirrors the
    /// effective top-level value back here so older internal call sites keep
    /// seeing a consistent value.
    #[serde(default, skip_serializing)]
    pub shadow_ledger: ShadowLedgerConfig,

    /// VisionCritic configuration (AI-powered meme quality assessment)
    #[serde(default)]
    pub vision_critic: VisionCriticWorkerConfig,

    /// ClusterHunter configuration (Cabal detection)
    #[serde(default)]
    pub cluster_hunter: ClusterHunterWorkerConfig,

    /// DevProfiler configuration (Creator behavioral analysis)
    #[serde(default)]
    pub dev_profiler: DevProfilerWorkerConfig,

    /// Global pipeline configuration
    #[serde(default)]
    pub pipeline: PipelineConfig,

    /// Reconciliation runtime hardening / alert thresholds.
    #[serde(default)]
    pub reconciliation: OracleReconciliationConfig,

    /// Sampling Loop Configuration (Hybrid Heartbeat)
    #[serde(default)]
    pub sampling_loop: SamplingLoopConfig,

    /// Orphan adoption grace multiplier (applied to ORPHAN_TTL_MS during adoption)
    #[serde(default = "default_orphan_grace_period_multiplier")]
    pub orphan_grace_period_multiplier: u64,

    /// Maximum number of orphan transactions adopted during pool registration
    #[serde(default = "default_max_orphans_adopted_on_register")]
    pub max_orphans_adopted_on_register: usize,

    /// Compatibility flag for explicit degraded/test AccountUpdate relay startup.
    ///
    /// The production launcher does **not** use this field as the primary
    /// runtime selector. Effective canonical ingest is derived from
    /// `[account_state_core].enable`.
    ///
    /// This field is retained only so explicit degraded/test startup can remain
    /// representable without reopening tx/bootstrap-only fallback as a normal
    /// production mode.
    #[serde(default = "default_oracle_canonical_account_update_relay_enabled")]
    pub canonical_account_update_relay_enabled: bool,

    /// Dry-run mode: simulate trading without sending real transactions
    /// When enabled, all decisions are logged but no actual buy/sell orders are executed
    #[serde(default)]
    pub dry_run: bool,

    /// Decision log path for telemetry (default: "logs/decisions.jsonl")
    /// Path where cyclic engine decisions are logged in JSONL format
    #[serde(default = "default_decision_log_path")]
    pub decision_log_path: String,

    /// In-memory Ghost Brain configuration (loaded separately from ghost_brain_config.toml)
    /// This is injected at runtime after loading the brain config and is not deserialized from launcher config.
    #[serde(skip)]
    pub ghost_brain_config: Option<ghost_brain::config::GhostBrainConfig>,
}

impl Default for OracleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            simple_oracle: SimpleOracleConfig::default(),
            qass: QassConfig::default(),
            hyper_oracle: HyperOracleConfig::default(),
            shadow_ledger: ShadowLedgerConfig::default(),
            vision_critic: VisionCriticWorkerConfig::default(),
            cluster_hunter: ClusterHunterWorkerConfig::default(),
            dev_profiler: DevProfilerWorkerConfig::default(),
            pipeline: PipelineConfig::default(),
            reconciliation: OracleReconciliationConfig::default(),
            sampling_loop: SamplingLoopConfig::default(),
            orphan_grace_period_multiplier: default_orphan_grace_period_multiplier(),
            max_orphans_adopted_on_register: default_max_orphans_adopted_on_register(),
            canonical_account_update_relay_enabled: true,
            dry_run: false,
            decision_log_path: default_decision_log_path(),
            ghost_brain_config: None,
        }
    }
}

fn default_oracle_canonical_account_update_relay_enabled() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleReconciliationConfig {
    /// Drift threshold that emits WARN + critical drift counter.
    #[serde(default = "default_reconciliation_drift_alert_threshold_lamports")]
    pub drift_alert_threshold_lamports: u64,
}

impl Default for OracleReconciliationConfig {
    fn default() -> Self {
        Self {
            drift_alert_threshold_lamports: default_reconciliation_drift_alert_threshold_lamports(),
        }
    }
}

/// SimpleOracle worker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SimpleOracleConfig {
    /// Enable SimpleOracle scoring
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Minimum score threshold to pass (0-100)
    #[serde(default = "default_min_score_threshold")]
    pub min_score_threshold: u8,

    /// Worker timeout in milliseconds
    #[serde(default = "default_worker_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for SimpleOracleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            min_score_threshold: default_min_score_threshold(),
            timeout_ms: default_worker_timeout_ms(),
        }
    }
}

/// QASS worker configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QassConfig {
    /// Enable QASS scoring
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Minimum confidence threshold for valid result (0.0-1.0)
    #[serde(default = "default_qass_collapse_threshold")]
    pub collapse_threshold: f32,

    /// Minimum number of active waves required
    #[serde(default = "default_qass_min_waves")]
    pub min_active_waves: usize,

    /// Worker timeout in milliseconds
    #[serde(default = "default_worker_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for QassConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            collapse_threshold: default_qass_collapse_threshold(),
            min_active_waves: default_qass_min_waves(),
            timeout_ms: default_worker_timeout_ms(),
        }
    }
}

/// HyperOracle worker configuration (SCR/ULVF/POVC)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HyperOracleConfig {
    /// Enable HyperOracle scoring
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// SCR (Slot-Coherence Resonance) threshold for bot detection (0.0-1.0)
    #[serde(default = "default_scr_threshold")]
    pub scr_threshold: f32,

    /// ULVF (Ultra-Early Liquidity Vector Field) divergence threshold
    #[serde(default = "default_ulvf_divergence_threshold")]
    pub ulvf_divergence_threshold: f32,

    /// ULVF curl threshold for wash trading detection
    #[serde(default = "default_ulvf_curl_threshold")]
    pub ulvf_curl_threshold: f32,

    /// Worker timeout in milliseconds
    #[serde(default = "default_worker_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for HyperOracleConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            scr_threshold: default_scr_threshold(),
            ulvf_divergence_threshold: default_ulvf_divergence_threshold(),
            ulvf_curl_threshold: default_ulvf_curl_threshold(),
            timeout_ms: default_worker_timeout_ms(),
        }
    }
}

/// Shadow Ledger configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ShadowLedgerConfig {
    /// Enable Shadow Ledger integration
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Maximum acceptable wall-clock age for launcher-side curve enrichment.
    #[serde(default = "default_shadow_ledger_enrichment_freshness_ms")]
    pub enrichment_freshness_ms: u64,

    /// How stale AccountUpdate-derived curves should be handled on the hot path.
    #[serde(default)]
    pub stale_fallback: ghost_core::shadow_ledger::ShadowLedgerStaleFallback,

    /// Maximum event-time wait budget (ms) for `PendingCurve` before timeout.
    #[serde(default = "default_curve_wait_ms")]
    pub curve_wait_ms: u64,

    /// Phase-5 unknown-curve policy selector.
    ///
    /// `true`  => wait in `PendingCurve` up to `curve_wait_ms`
    /// `false` => reject immediately when BUY would require an unknown curve
    #[serde(default = "default_curve_require_for_buy")]
    pub curve_require_for_buy: bool,

    /// Migration risk threshold (bonding progress percentage, e.g., 98 = 98%)
    #[serde(default = "default_migration_risk_threshold")]
    pub migration_risk_threshold: u64,

    /// Entry price penalty threshold (percentage above initial price)
    #[serde(default = "default_entry_price_penalty_threshold")]
    pub entry_price_penalty_threshold: f64,

    /// Worker timeout in milliseconds
    #[serde(default = "default_worker_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for ShadowLedgerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            enrichment_freshness_ms: default_shadow_ledger_enrichment_freshness_ms(),
            stale_fallback: ghost_core::shadow_ledger::ShadowLedgerStaleFallback::default(),
            curve_wait_ms: default_curve_wait_ms(),
            curve_require_for_buy: default_curve_require_for_buy(),
            migration_risk_threshold: default_migration_risk_threshold(),
            entry_price_penalty_threshold: default_entry_price_penalty_threshold(),
            timeout_ms: default_worker_timeout_ms(),
        }
    }
}

/// VisionCritic worker configuration (AI-powered meme quality assessment)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VisionCriticWorkerConfig {
    /// Enable VisionCritic scoring (disabled by default to prevent API costs)
    #[serde(default)]
    pub enabled: bool,

    /// Minimum viral score threshold (0-10)
    #[serde(default = "default_vision_min_score")]
    pub min_viral_score: u8,

    /// Worker timeout in milliseconds
    #[serde(default = "default_vision_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for VisionCriticWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: false, // Disabled by default to prevent API costs
            min_viral_score: default_vision_min_score(),
            timeout_ms: default_vision_timeout_ms(),
        }
    }
}

/// ClusterHunter worker configuration (Cabal/cluster detection)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClusterHunterWorkerConfig {
    /// Enable ClusterHunter analysis
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// High risk threshold for cluster control percentage (0-100)
    #[serde(default = "default_cluster_high_risk_threshold")]
    pub high_risk_threshold_pct: f32,

    /// Minimum cluster size to flag as suspicious
    #[serde(default = "default_cluster_min_size")]
    pub min_cluster_size: usize,

    /// Worker timeout in milliseconds
    #[serde(default = "default_cluster_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for ClusterHunterWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            high_risk_threshold_pct: default_cluster_high_risk_threshold(),
            min_cluster_size: default_cluster_min_size(),
            timeout_ms: default_cluster_timeout_ms(),
        }
    }
}

/// DevProfiler worker configuration (Creator behavioral analysis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevProfilerWorkerConfig {
    /// Enable DevProfiler analysis
    #[serde(default = "default_true")]
    pub enabled: bool,

    /// Risk score threshold for high risk flag (0.0-1.0)
    #[serde(default = "default_profiler_risk_threshold")]
    pub risk_threshold: f32,

    /// Serial minter threshold (tokens created in 24h)
    #[serde(default = "default_serial_minter_threshold")]
    pub serial_minter_threshold: usize,

    /// Worker timeout in milliseconds
    #[serde(default = "default_profiler_timeout_ms")]
    pub timeout_ms: u64,
}

impl Default for DevProfilerWorkerConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            risk_threshold: default_profiler_risk_threshold(),
            serial_minter_threshold: default_serial_minter_threshold(),
            timeout_ms: default_profiler_timeout_ms(),
        }
    }
}

/// Pipeline-level configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PipelineConfig {
    /// Maximum number of concurrent scoring tasks
    #[serde(default = "default_max_concurrent_scoring")]
    pub max_concurrent_scoring: usize,

    /// Global timeout for entire scoring pipeline (milliseconds)
    #[serde(default = "default_pipeline_timeout_ms")]
    pub pipeline_timeout_ms: u64,

    /// Skip candidate on timeout (true) or fail entire pipeline (false)
    #[serde(default = "default_true")]
    pub skip_on_timeout: bool,

    /// Enable telemetry/metrics collection for Oracle workers
    #[serde(default = "default_true")]
    pub telemetry_enabled: bool,

    /// Minimum combined score to pass to Trigger (0-100)
    #[serde(default = "default_combined_score_threshold")]
    pub combined_score_threshold: u8,

    /// Weight boost for enhanced scoring results (percentage, e.g., 1.2 = 20% boost)
    /// Enhanced scoring has fresher Shadow Ledger data, so it's weighted higher
    #[serde(default = "default_enhanced_score_weight")]
    pub enhanced_score_weight: f64,
}

impl Default for PipelineConfig {
    fn default() -> Self {
        Self {
            max_concurrent_scoring: default_max_concurrent_scoring(),
            pipeline_timeout_ms: default_pipeline_timeout_ms(),
            skip_on_timeout: true,
            telemetry_enabled: true,
            combined_score_threshold: default_combined_score_threshold(),
            enhanced_score_weight: default_enhanced_score_weight(),
        }
    }
}

/// DEPRECATED: Sampling loop has been replaced by Gatekeeper v2.
/// This struct is retained for backward compatibility with existing config files.
/// All fields are ignored at runtime.
#[deprecated(
    since = "3.0.0",
    note = "Replaced by GatekeeperV2Config. Sampling loop removed."
)]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SamplingLoopConfig {
    /// Total observation duration in milliseconds (default: 8000ms)
    #[serde(default = "default_sampling_duration_ms")]
    pub sampling_duration_ms: u64,

    /// Initial cycle delay (first breath, in milliseconds) (default: 2833ms)
    #[serde(default = "default_initial_cycle_delay_ms")]
    pub initial_cycle_delay_ms: u64,

    /// Fast cycle interval (rapid fire, in milliseconds) (default: 566ms)
    #[serde(default = "default_fast_cycle_interval_ms")]
    pub fast_cycle_interval_ms: u64,

    /// Sniper threshold - score that triggers immediate exit (default: 90)
    #[serde(default = "default_sniper_threshold")]
    pub sniper_threshold: u8,

    /// Minimum transaction count required before scoring (Hard Gatekeeper) (default: 12)
    #[serde(default = "default_min_tx_count_for_scoring")]
    pub min_tx_count_for_scoring: usize,

    /// Maximum number of cycles before forced exit (default: 50)
    #[serde(default = "default_max_cycles")]
    pub max_cycles: u64,

    /// Maximum observation cycles for score history (default: 50)
    #[serde(default = "default_max_observation_cycles")]
    pub max_observation_cycles: usize,

    /// Final decision time as percentage of sampling duration (default: 95% of sampling_duration_ms)
    /// Note: This is calculated from sampling_duration_ms, not directly configurable
    #[serde(skip)]
    pub final_decision_time_ms: u64,
}

impl Default for SamplingLoopConfig {
    fn default() -> Self {
        let sampling_duration_ms = default_sampling_duration_ms();
        Self {
            sampling_duration_ms,
            initial_cycle_delay_ms: default_initial_cycle_delay_ms(),
            fast_cycle_interval_ms: default_fast_cycle_interval_ms(),
            sniper_threshold: default_sniper_threshold(),
            min_tx_count_for_scoring: default_min_tx_count_for_scoring(),
            max_cycles: default_max_cycles(),
            max_observation_cycles: default_max_observation_cycles(),
            final_decision_time_ms: (sampling_duration_ms * 95) / 100,
        }
    }
}

impl SamplingLoopConfig {
    /// Update final_decision_time_ms based on current sampling_duration_ms
    pub fn update_final_decision_time(&mut self) {
        self.final_decision_time_ms = (self.sampling_duration_ms * 95) / 100;
    }
}

// Default value functions
fn default_true() -> bool {
    true
}

fn default_connection_mode() -> String {
    "grpc".to_string()
}

fn default_geyser_endpoint() -> String {
    "ws://localhost:8900".to_string()
}

fn default_grpc_endpoint() -> String {
    "http://localhost:10000".to_string()
}

fn default_rpc_endpoint() -> String {
    "https://api.devnet.solana.com".to_string()
}

fn default_seer_metrics_port() -> u16 {
    9090
}

fn default_trigger_metrics_port() -> u16 {
    9091
}

fn default_ipc_buffer_size() -> usize {
    10000
}

fn default_backpressure_policy() -> String {
    "block".to_string()
}

fn default_stream_mode() -> String {
    "single_global".to_string()
}

fn default_tx_filter_strategy() -> String {
    "per_pool".to_string()
}

fn default_funding_lane_mode() -> String {
    "disabled".to_string()
}

fn default_watched_pools_ttl_ms() -> u64 {
    120_000
}

fn default_seer_grpc_max_stalls_before_open() -> u32 {
    3
}

fn default_seer_grpc_circuit_breaker_cooldown_ms() -> u64 {
    15_000
}

fn default_watched_pools_cap() -> usize {
    32_768
}

fn default_pump_program_id() -> String {
    "6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P".to_string()
}

fn default_bonk_program_id() -> String {
    "LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj".to_string()
}

/// Default maximum concurrent positions
/// This value of 3 provides a good balance between:
/// - Diversification (not all capital in one position)
/// - Risk management (limited exposure)
/// - Capital efficiency (enough positions to capture opportunities)
/// Note: This matches the default in ProcessorConfig (trigger component)
fn default_max_concurrent_positions() -> usize {
    3
}

/// Default absolute maximum tip in SOL enforced by the local tip guard.
fn default_max_tip_absolute_sol() -> f64 {
    0.04
}

/// Default fallback tip when the local tip guard rejects the requested amount.
fn default_fallback_tip_sol() -> f64 {
    0.001
}

fn default_live_preflight_max_state_age_slots() -> u64 {
    10
}

fn default_live_exit_take_profit_pct() -> f64 {
    0.02
}

fn default_live_exit_stop_loss_pct() -> f64 {
    0.02
}

/// Default maximum position size in SOL (The Bulkhead)
fn default_max_position_size_sol() -> f64 {
    0.1
}

/// Default emergency floor balance (The Bulkhead)
fn default_emergency_floor_sol() -> f64 {
    0.05
}

/// Default position size buffer (The Bulkhead)
fn default_position_size_buffer_sol() -> f64 {
    0.02
}

fn default_trigger_slippage_tolerance() -> f64 {
    0.20
}

fn default_gui_bind_address() -> String {
    "127.0.0.1".to_string()
}

fn default_gui_port() -> u16 {
    8800
}

fn default_log_level() -> String {
    "info".to_string()
}

fn default_log_file() -> String {
    "logs/system.log".to_string()
}

fn default_oracle_log_file() -> String {
    "logs/oracle_decision.log".to_string()
}

/// Default path to Ghost Brain configuration file
fn default_ghost_brain_config_path() -> String {
    "ghost-brain/ghost_brain_config.toml".to_string()
}

/// Default decision log path for cyclic engine telemetry
fn default_decision_log_path() -> String {
    "logs/decisions.jsonl".to_string()
}

// Oracle configuration default functions

/// Default minimum score threshold for SimpleOracle (70 out of 100)
fn default_min_score_threshold() -> u8 {
    70
}

/// Default worker timeout: 100ms - aggressive but allows for CPU-heavy scoring
fn default_worker_timeout_ms() -> u64 {
    100
}

/// Default QASS collapse threshold: 0.5 (minimum 50% confidence required)
fn default_qass_collapse_threshold() -> f32 {
    0.5
}

/// Default minimum active waves for QASS: 3 signals required
fn default_qass_min_waves() -> usize {
    3
}

/// Default SCR threshold: 0.7 (above indicates likely bot activity)
fn default_scr_threshold() -> f32 {
    0.7
}

/// Default ULVF divergence threshold: 0.3 (below indicates stagnation)
fn default_ulvf_divergence_threshold() -> f32 {
    0.3
}

/// Default ULVF curl threshold: 15.0 (above indicates wash trading)
fn default_ulvf_curl_threshold() -> f32 {
    15.0
}

/// Default migration risk threshold: 98% bonding progress
fn default_migration_risk_threshold() -> u64 {
    98
}

/// Default freshness SLA for launcher-side ShadowLedger enrichment.
fn default_shadow_ledger_enrichment_freshness_ms() -> u64 {
    200
}

fn default_curve_wait_ms() -> u64 {
    800
}

fn default_curve_require_for_buy() -> bool {
    true
}

/// Default entry price penalty threshold: 50% above initial price
fn default_entry_price_penalty_threshold() -> f64 {
    50.0
}

fn default_reconciliation_drift_alert_threshold_lamports() -> u64 {
    50_000_000
}

/// Default max concurrent scoring tasks: 4 (parallelizes well on most systems)
fn default_max_concurrent_scoring() -> usize {
    4
}

/// Default pipeline timeout: 500ms total
fn default_pipeline_timeout_ms() -> u64 {
    500
}

/// Default combined score threshold: 60 out of 100
fn default_combined_score_threshold() -> u8 {
    60
}

/// Default enhanced score weight: 1.2 (20% boost for fresher Shadow Ledger data)
fn default_enhanced_score_weight() -> f64 {
    1.2
}

// VisionCritic defaults

/// Default minimum viral score for VisionCritic: 5 (neutral threshold)
fn default_vision_min_score() -> u8 {
    5
}

/// Default VisionCritic timeout: 5000ms (API calls take longer)
fn default_vision_timeout_ms() -> u64 {
    5000
}

// ClusterHunter defaults

/// Default high risk threshold: 30% supply controlled by cluster
fn default_cluster_high_risk_threshold() -> f32 {
    30.0
}

/// Default minimum cluster size: 3 holders from same funder
fn default_cluster_min_size() -> usize {
    3
}

/// Default ClusterHunter timeout: 2000ms (RPC calls)
fn default_cluster_timeout_ms() -> u64 {
    2000
}

// DevProfiler defaults

/// Default risk threshold for DevProfiler: 0.7 (high risk)
fn default_profiler_risk_threshold() -> f32 {
    0.7
}

/// Default serial minter threshold: 5 tokens in 24h
fn default_serial_minter_threshold() -> usize {
    5
}

/// Default DevProfiler timeout: 2000ms (RPC calls)
fn default_profiler_timeout_ms() -> u64 {
    2000
}

// Sampling Loop Configuration defaults

/// Default sampling duration: 7200ms (Ghost Predator Loop + IWIM Wait)
fn default_sampling_duration_ms() -> u64 {
    7200
}

/// Default initial cycle delay: 1780ms (Gatekeeper Window - Optimized)
fn default_initial_cycle_delay_ms() -> u64 {
    1780
}

/// Default fast cycle interval: 400ms (Scoring Loop Interval - Optimized)
fn default_fast_cycle_interval_ms() -> u64 {
    400
}

/// Default sniper threshold: 90 (immediate exit on high score)
/// Note: Gunshot thresholds override this in Predator strategy
fn default_sniper_threshold() -> u8 {
    90
}

/// Default minimum transaction count for scoring: 15 (Hard Gatekeeper for Predator)
fn default_min_tx_count_for_scoring() -> usize {
    15
}

/// Default maximum cycles: 12 (Ghost Predator S1-S12)
fn default_max_cycles() -> u64 {
    12
}

/// Default maximum observation cycles: 12 (matches max_cycles)
fn default_max_observation_cycles() -> usize {
    12
}

fn default_orphan_grace_period_multiplier() -> u64 {
    2
}

fn default_max_orphans_adopted_on_register() -> usize {
    50
}

// PumpPortal configuration default functions

fn default_pumpportal_ws_url() -> String {
    "wss://pumpportal.fun/api/data".to_string()
}

fn default_pumpportal_max_active_mints() -> usize {
    1_000
}

fn default_pumpportal_subscription_batch_size() -> usize {
    10
}

fn default_pumpportal_reconnect_base_delay() -> u64 {
    5
}

fn default_pumpportal_reconnect_max_delay() -> u64 {
    300
}

fn default_pumpportal_stats_window_secs() -> u64 {
    900 // 15 minutes
}

fn default_gatekeeper_observation_window_ms() -> u64 {
    1_780
}

fn default_gatekeeper_min_tx_to_pass() -> usize {
    17
}

fn default_gatekeeper_drop_age_ms() -> u64 {
    10_000
}

fn default_gatekeeper_check_interval_ms() -> u64 {
    crate::components::gatekeeper_commit_loop::GatekeeperCommitLoopConfig::default()
        .check_interval_ms
}

fn default_live_pipeline_flush_interval_ms() -> u64 {
    crate::components::live_pipeline_flush_loop::LivePipelineFlushLoopConfig::default()
        .flush_interval_ms
}

fn default_live_pipeline_flush_delay_ms() -> u64 {
    ghost_core::shadow_ledger::LivePipelineConfig::default().flush_delay_ms
}

fn default_live_pipeline_max_buffer_size() -> usize {
    ghost_core::shadow_ledger::LivePipelineConfig::default().max_buffer_size
}

fn default_live_pipeline_seen_keys_limit() -> usize {
    ghost_core::shadow_ledger::LivePipelineConfig::default().seen_keys_limit
}

fn default_execution_fill_delay_min_ms() -> u64 {
    200
}

fn default_execution_fill_delay_max_ms() -> u64 {
    400
}

fn default_execution_jitter_ms() -> u64 {
    50
}

fn default_execution_max_quote_age_ms() -> u64 {
    1500
}

fn default_execution_ring_buffer_size() -> usize {
    256
}

fn default_execution_quote_generation_interval_ms() -> u64 {
    500
}

fn default_execution_stale_warning_threshold_ms() -> u64 {
    1000
}

fn default_execution_events_output_dir() -> String {
    "datasets/events".to_string()
}

fn default_execution_events_rotation_interval_ms() -> u64 {
    300_000
}

fn default_execution_events_flush_interval_ms() -> u64 {
    1_000
}

fn default_execution_events_max_file_size_bytes() -> u64 {
    50_000_000
}

fn default_shadow_run_rpc_url() -> String {
    "http://127.0.0.1:8899".to_string()
}

fn default_shadow_run_timeout_ms() -> u64 {
    100
}

fn default_shadow_run_max_retries() -> usize {
    1
}

fn default_shadow_run_max_concurrent() -> usize {
    8
}

fn default_shadow_run_output_path() -> String {
    "logs/shadow_run/buys.jsonl".to_string()
}

impl LauncherConfig {
    /// Load configuration from a TOML file
    pub fn from_file<P: AsRef<Path>>(path: P) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let path = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let content = std::fs::read_to_string(&path)?;
        let mut config: LauncherConfig = toml::from_str(&content)?;
        let has_explicit_execution_mode =
            toml_has_explicit_path(&content, &["execution", "execution_mode"]);
        let has_explicit_trigger_entry_mode =
            toml_has_explicit_path(&content, &["trigger", "entry_mode"]);
        let has_legacy_execution_dry_run =
            toml_has_explicit_path(&content, &["execution", "dry_run"]);
        let has_legacy_oracle_dry_run = toml_has_explicit_path(&content, &["oracle", "dry_run"]);
        let has_explicit_shadow_ledger_ssot = toml_has_explicit_path(&content, &["shadow_ledger"]);
        let has_legacy_oracle_shadow_ledger =
            toml_has_explicit_path(&content, &["oracle", "shadow_ledger"]);
        let config_dir = path.parent().unwrap_or_else(|| Path::new("."));
        let secret_env = load_secret_env(config_dir)?;

        if has_explicit_shadow_ledger_ssot {
            if has_legacy_oracle_shadow_ledger {
                tracing::warn!(
                    "Both [shadow_ledger] and legacy [oracle.shadow_ledger] are set; using top-level [shadow_ledger] as SSOT"
                );
            }
        } else if has_legacy_oracle_shadow_ledger {
            tracing::warn!(
                "Legacy [oracle.shadow_ledger] is deprecated; promote it to top-level [shadow_ledger]"
            );
            config.shadow_ledger = config.oracle.shadow_ledger.clone();
        }
        config.oracle.shadow_ledger = config.shadow_ledger.clone();

        apply_secret_env_overrides(&mut config, config_dir, &secret_env);

        config.rebase_relative_paths(config_dir);

        // Support legacy PumpPortal RPC override under [seer.pumpportal]
        if let Some(ref pumpportal_rpc) = config.seer.pumpportal.rpc_endpoint {
            if !pumpportal_rpc.is_empty() && config.seer.rpc_endpoint == default_rpc_endpoint() {
                config.seer.rpc_endpoint = pumpportal_rpc.clone();
            }
        }

        if !has_explicit_execution_mode {
            let alias_dry_run = config.execution.dry_run.unwrap_or(config.oracle.dry_run);
            config.execution.execution_mode = if alias_dry_run {
                ExecutionMode::Paper
            } else {
                ExecutionMode::Live
            };
        }

        if !has_explicit_trigger_entry_mode && config.oracle.dry_run {
            tracing::warn!(
                "oracle.dry_run=true is using legacy trigger behavior mapping; \
                 setting trigger.entry_mode=dry_run_mock. \
                 Configure [trigger].entry_mode explicitly to remove this warning."
            );
            config.trigger.entry_mode = TriggerEntryMode::DryRunMock;
        }

        validate_loaded_execution_profile(
            &config,
            has_explicit_execution_mode,
            has_explicit_trigger_entry_mode,
            has_legacy_execution_dry_run,
            has_legacy_oracle_dry_run,
        )?;
        config.resolve_durability_config()?;

        Ok(config)
    }

    fn rebase_relative_paths(&mut self, config_dir: &Path) {
        self.ghost_brain_config_path =
            resolve_runtime_path(config_dir, &self.ghost_brain_config_path);
        self.logging.file_path = resolve_runtime_path(config_dir, &self.logging.file_path);
        self.logging.oracle_log_path =
            resolve_runtime_path(config_dir, &self.logging.oracle_log_path);
        self.execution.events.output_dir =
            resolve_runtime_path(config_dir, &self.execution.events.output_dir);
        self.execution.shadow.entry_log_path =
            resolve_runtime_path(config_dir, &self.execution.shadow.entry_log_path);
        if let Some(lifecycle_log_path) = self.execution.shadow.lifecycle_log_path.as_mut() {
            *lifecycle_log_path = resolve_runtime_path(config_dir, lifecycle_log_path);
        }
        self.oracle.decision_log_path =
            resolve_runtime_path(config_dir, &self.oracle.decision_log_path);
        self.trigger.shadow_run.output_path =
            resolve_runtime_path(config_dir, &self.trigger.shadow_run.output_path);
        if let Some(wal_dir) = self.durability.wal_dir.as_mut() {
            *wal_dir = PathBuf::from(resolve_runtime_path(config_dir, &wal_dir.to_string_lossy()));
        }
        if let Some(snapshot_dir) = self.durability.snapshot_dir.as_mut() {
            *snapshot_dir = PathBuf::from(resolve_runtime_path(
                config_dir,
                &snapshot_dir.to_string_lossy(),
            ));
        }

        if let Some(keypair_path) = self.trigger.keypair_path.as_mut() {
            *keypair_path = resolve_runtime_path(config_dir, keypair_path);
        }
    }

    /// Load configuration from a TOML file or use defaults
    #[allow(dead_code)]
    pub fn from_file_or_default<P: AsRef<Path>>(path: P) -> Self {
        Self::from_file(path).unwrap_or_else(|_| Self::default())
    }

    /// Get default configuration
    pub fn default() -> Self {
        Self {
            mode: AppMode::Test,
            seer: SeerComponentConfig {
                enabled: true,
                connection_mode: default_connection_mode(),
                source_mode: None,
                geyser_endpoint: default_geyser_endpoint(),
                grpc_endpoint: default_grpc_endpoint(),
                helius_endpoint: None,
                rpc_endpoint: default_rpc_endpoint(),
                grpc_manual_backfill_enabled: true,
                grpc_commitment_fallback_to_websocket: false,
                grpc_max_stalls_before_open: default_seer_grpc_max_stalls_before_open(),
                grpc_circuit_breaker_cooldown_ms: default_seer_grpc_circuit_breaker_cooldown_ms(),
                grpc_client_id: None,
                grpc_auth_token: None,
                grpc_x_token: None,
                enable_pumpfun: true,
                enable_bonkfun: true,
                pump_program_id: default_pump_program_id(),
                bonk_program_id: default_bonk_program_id(),
                metrics_port: default_seer_metrics_port(),
                ipc_buffer_size: default_ipc_buffer_size(),
                ipc_backpressure_policy: default_backpressure_policy(),
                stream_mode: default_stream_mode(),
                tx_filter_strategy: default_tx_filter_strategy(),
                funding_lane_mode: default_funding_lane_mode(),
                watched_pools_ttl_ms: default_watched_pools_ttl_ms(),
                watched_pools_cap: default_watched_pools_cap(),
                watch_debounce_ms: 0,
                commitment: SeerCommitment::default(),
                pumpportal: PumpPortalComponentConfig::default(),
            },
            trigger: TriggerComponentConfig {
                enabled: true,
                entry_mode: TriggerEntryMode::default(),
                rpc_url: default_rpc_endpoint(),
                keypair_path: None,
                tip_guard: TriggerTipGuardConfig::default(),
                metrics_port: default_trigger_metrics_port(),
                max_concurrent_positions: default_max_concurrent_positions(),
                max_position_size_sol: default_max_position_size_sol(),
                emergency_floor_sol: default_emergency_floor_sol(),
                position_size_buffer_sol: default_position_size_buffer_sol(),
                slippage_tolerance: default_trigger_slippage_tolerance(),
                live_preflight_max_state_age_slots: default_live_preflight_max_state_age_slots(),
                live_exit_take_profit_pct: default_live_exit_take_profit_pct(),
                live_exit_stop_loss_pct: default_live_exit_stop_loss_pct(),
                shadow_run: TriggerShadowRunConfig::default(),
            },
            execution: ExecutionConfig::default(),
            gui_backend: GuiBackendComponentConfig {
                enabled: true,
                bind_address: default_gui_bind_address(),
                port: default_gui_port(),
            },
            oracle: OracleConfig::default(),
            shadow_ledger: ShadowLedgerConfig::default(),
            account_state_core: AccountStateCoreConfig::default(),
            session: SessionRuntimeConfig::default(),
            tx_intelligence: TxIntelligenceRuntimeConfig::default(),
            logging: LoggingConfig::default(),
            gatekeeper: GatekeeperRuntimeConfig::default(),
            live_pipeline: LivePipelineRuntimeConfig::default(),
            snapshot_listener_forward_mode: default_snapshot_listener_forward_mode(),
            snapshot_listener_max_pools: default_snapshot_listener_max_pools(),
            snapshot_inactive_tx_buffer_capacity: default_snapshot_inactive_tx_buffer_capacity(),
            snapshot_inactive_tx_ttl_margin_ms: default_snapshot_inactive_tx_ttl_margin_ms(),
            ghost_brain_config_path: default_ghost_brain_config_path(),
            metrics: MetricsConfig::default(),
            durability: DurabilityConfig::default(),
        }
    }
    /// Save configuration to a TOML file
    pub fn save_to_file<P: AsRef<Path>>(&self, path: P) -> anyhow::Result<()> {
        let content = toml::to_string_pretty(self)?;
        std::fs::write(path, content)?;
        Ok(())
    }
}

fn toml_has_explicit_path(content: &str, path: &[&str]) -> bool {
    let Ok(value) = toml::from_str::<toml::Value>(content) else {
        return false;
    };

    let mut current = &value;
    for segment in path {
        let Some(next) = current.get(*segment) else {
            return false;
        };
        current = next;
    }

    true
}

fn resolve_path_from_ancestors<I>(path: &Path, roots: I) -> Option<PathBuf>
where
    I: IntoIterator<Item = PathBuf>,
{
    for mut root in roots {
        loop {
            let candidate = root.join(path);
            if candidate.exists() {
                return Some(candidate);
            }
            if !root.pop() {
                break;
            }
        }
    }

    None
}

fn resolve_runtime_path(config_dir: &Path, raw: &str) -> String {
    if raw.is_empty() {
        return raw.to_string();
    }

    if raw.starts_with('~') {
        return raw.to_string();
    }

    let path = Path::new(raw);
    if path.is_absolute() {
        raw.to_string()
    } else {
        config_dir.join(path).to_string_lossy().into_owned()
    }
}

pub fn redact_endpoint_for_logs(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return "<empty>".to_string();
    }

    if let Some((scheme, rest)) = trimmed.split_once("://") {
        let authority_end = rest.find(['/', '?', '#']).unwrap_or(rest.len());
        let authority = &rest[..authority_end];
        return if authority_end < rest.len() {
            format!("{scheme}://{authority}/<redacted>")
        } else {
            format!("{scheme}://{authority}")
        };
    }

    if let Some((host, _query)) = trimmed.split_once('?') {
        return format!("{host}?<redacted>");
    }

    trimmed.to_string()
}

fn load_secret_env(config_dir: &Path) -> anyhow::Result<LoadedSecretEnv> {
    match env::var("GHOST_ENV_FILE") {
        Ok(raw_path) => {
            let trimmed = raw_path.trim();
            if trimmed.is_empty() {
                bail!("GHOST_ENV_FILE is set but empty");
            }

            let env_path = if Path::new(trimmed).is_absolute() {
                PathBuf::from(trimmed)
            } else {
                config_dir.join(trimmed)
            };

            return parse_secret_env_file(&env_path);
        }
        Err(env::VarError::NotPresent) => {}
        Err(err) => return Err(anyhow!("failed to read env var GHOST_ENV_FILE: {err}")),
    }

    let mut cursor = config_dir.to_path_buf();
    loop {
        let candidate = cursor.join(DEFAULT_SECRET_ENV_FILE);
        if candidate.exists() {
            return parse_secret_env_file(&candidate);
        }

        if !cursor.pop() {
            break;
        }
    }

    Ok(LoadedSecretEnv::default())
}

fn parse_secret_env_file(path: &Path) -> anyhow::Result<LoadedSecretEnv> {
    let content = std::fs::read_to_string(path)
        .map_err(|err| anyhow!("failed to read secret env file {}: {err}", path.display()))?;
    let mut values = HashMap::new();

    for (line_no, line) in content.lines().enumerate() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        let entry = trimmed.strip_prefix("export ").unwrap_or(trimmed);
        let Some((raw_key, raw_value)) = entry.split_once('=') else {
            bail!(
                "invalid secret env file {}:{}: expected KEY=VALUE",
                path.display(),
                line_no + 1
            );
        };

        let key = raw_key.trim();
        if key.is_empty() {
            bail!(
                "invalid secret env file {}:{}: empty KEY",
                path.display(),
                line_no + 1
            );
        }

        values.insert(key.to_string(), parse_secret_env_value(raw_value.trim()));
    }

    Ok(LoadedSecretEnv {
        values,
        base_dir: path.parent().map(Path::to_path_buf),
    })
}

fn parse_secret_env_value(raw: &str) -> String {
    let trimmed = raw.trim();
    if trimmed.len() >= 2 {
        let first = trimmed.as_bytes()[0] as char;
        let last = trimmed.as_bytes()[trimmed.len() - 1] as char;
        if (first == '"' && last == '"') || (first == '\'' && last == '\'') {
            return trimmed[1..trimmed.len() - 1].to_string();
        }
    }

    trimmed.to_string()
}

fn lookup_secret_env(var_name: &str, secret_env: &LoadedSecretEnv) -> Option<ResolvedSecretValue> {
    match env::var(var_name) {
        Ok(raw) => {
            let trimmed = raw.trim();
            if !trimmed.is_empty() {
                return Some(ResolvedSecretValue {
                    value: trimmed.to_string(),
                    source: SecretValueSource::ProcessEnv,
                });
            }
        }
        Err(env::VarError::NotPresent) => {}
        Err(_) => return None,
    }

    secret_env.values.get(var_name).and_then(|value| {
        let trimmed = value.trim();
        if trimmed.is_empty() {
            None
        } else {
            Some(ResolvedSecretValue {
                value: trimmed.to_string(),
                source: SecretValueSource::DotEnv,
            })
        }
    })
}

enum SecretEnvDirective {
    Clear,
    Set(ResolvedSecretValue),
}

fn is_explicit_none_secret_value(raw: &str) -> bool {
    let trimmed = raw.trim();
    trimmed.is_empty() || trimmed.eq_ignore_ascii_case("null")
}

fn lookup_secret_env_directive(
    var_name: &str,
    secret_env: &LoadedSecretEnv,
) -> Option<SecretEnvDirective> {
    match env::var(var_name) {
        Ok(raw) => {
            let trimmed = raw.trim();
            return if is_explicit_none_secret_value(trimmed) {
                Some(SecretEnvDirective::Clear)
            } else {
                Some(SecretEnvDirective::Set(ResolvedSecretValue {
                    value: trimmed.to_string(),
                    source: SecretValueSource::ProcessEnv,
                }))
            };
        }
        Err(env::VarError::NotPresent) => {}
        Err(_) => return None,
    }

    secret_env.values.get(var_name).map(|value| {
        let trimmed = value.trim();
        if is_explicit_none_secret_value(trimmed) {
            SecretEnvDirective::Clear
        } else {
            SecretEnvDirective::Set(ResolvedSecretValue {
                value: trimmed.to_string(),
                source: SecretValueSource::DotEnv,
            })
        }
    })
}

fn resolve_secret_path_value(
    resolved: &ResolvedSecretValue,
    config_dir: &Path,
    secret_env: &LoadedSecretEnv,
) -> String {
    let base_dir = match resolved.source {
        SecretValueSource::ProcessEnv => config_dir,
        SecretValueSource::DotEnv => secret_env.base_dir.as_deref().unwrap_or(config_dir),
    };
    resolve_runtime_path(base_dir, &resolved.value)
}

fn should_override_secret_value(current: Option<&str>) -> bool {
    let Some(current) = current else {
        return true;
    };

    let trimmed = current.trim();
    if trimmed.is_empty() {
        return true;
    }

    let lowered = trimmed.to_ascii_lowercase();
    lowered.contains("example.invalid")
        || lowered.contains("placeholder")
        || lowered.contains("replace-me")
        || lowered == "http://localhost:10000"
        || lowered == "localhost:10000"
        || lowered == "http://127.0.0.1:8899"
        || lowered == "127.0.0.1:8899"
        || lowered == "http://localhost:8899"
        || lowered == "localhost:8899"
}

fn validate_uuid_v4(raw: &str, field_name: &str) -> Result<(), String> {
    let parsed = Uuid::parse_str(raw)
        .map_err(|err| format!("{field_name} must be a valid UUID v4: {err}"))?;
    if parsed.get_version_num() != 4 {
        return Err(format!(
            "{field_name} must be a UUID v4 (got version {})",
            parsed.get_version_num()
        ));
    }
    Ok(())
}

fn apply_secret_env_overrides(
    config: &mut LauncherConfig,
    config_dir: &Path,
    secret_env: &LoadedSecretEnv,
) {
    if should_override_secret_value(Some(&config.seer.grpc_endpoint)) {
        if let Some(value) = lookup_secret_env("GHOST_SEER_GRPC_ENDPOINT", secret_env) {
            config.seer.grpc_endpoint = value.value;
        }
    }
    if should_override_secret_value(config.seer.grpc_x_token.as_deref()) {
        if let Some(value) = lookup_secret_env("GHOST_SEER_GRPC_X_TOKEN", secret_env) {
            config.seer.grpc_x_token = Some(value.value);
        }
    }
    if should_override_secret_value(config.seer.grpc_auth_token.as_deref()) {
        if let Some(value) = lookup_secret_env("GHOST_SEER_GRPC_AUTH_TOKEN", secret_env) {
            config.seer.grpc_auth_token = Some(value.value);
        }
    }
    if should_override_secret_value(Some(&config.seer.rpc_endpoint)) {
        if let Some(value) = lookup_secret_env("GHOST_SEER_RPC_ENDPOINT", secret_env) {
            config.seer.rpc_endpoint = value.value;
        }
    }
    if should_override_secret_value(config.seer.helius_endpoint.as_deref()) {
        if let Some(value) = lookup_secret_env("GHOST_SEER_HELIUS_ENDPOINT", secret_env) {
            config.seer.helius_endpoint = Some(value.value);
        }
    }
    if should_override_secret_value(Some(&config.trigger.rpc_url)) {
        if let Some(value) = lookup_secret_env("GHOST_TRIGGER_RPC_URL", secret_env) {
            config.trigger.rpc_url = value.value;
        }
    }
    if should_override_secret_value(config.trigger.keypair_path.as_deref()) {
        if let Some(value) = lookup_secret_env("GHOST_TRIGGER_KEYPAIR_PATH", secret_env) {
            config.trigger.keypair_path =
                Some(resolve_secret_path_value(&value, config_dir, secret_env));
        }
    }
    if should_override_secret_value(Some(&config.trigger.shadow_run.shadow_rpc_url)) {
        if let Some(value) = lookup_secret_env("GHOST_TRIGGER_SHADOW_RPC_URL", secret_env) {
            config.trigger.shadow_run.shadow_rpc_url = value.value;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::trigger::TriggerComponent;
    use std::fs;

    fn unique_temp_dir(label: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!(
            "ghost_launcher_config_test_{}_{}",
            label,
            std::process::id()
        ));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn test_redact_endpoint_for_logs_hides_sensitive_suffix() {
        assert_eq!(
            redact_endpoint_for_logs("https://solana-mainnet.example.com/secret-key"),
            "https://solana-mainnet.example.com/<redacted>"
        );
        assert_eq!(
            redact_endpoint_for_logs("https://rpc.example.com?api-key=secret"),
            "https://rpc.example.com/<redacted>"
        );
        assert_eq!(
            redact_endpoint_for_logs("yellowstone.example.com:443"),
            "yellowstone.example.com:443"
        );
    }

    #[test]
    fn test_from_file_applies_secret_env_overrides_from_ancestor_dotenv() {
        let base = unique_temp_dir("secret_env_override");
        let config_dir = base.join("configs/rollout");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(base.join("secrets")).unwrap();
        let config_path = config_dir.join("paper-burnin.toml");

        fs::write(
            base.join(".env"),
            "GHOST_SEER_GRPC_ENDPOINT=yellowstone.example.com:443\n\
GHOST_SEER_GRPC_X_TOKEN=env-token\n\
GHOST_SEER_RPC_ENDPOINT=https://rpc.example.com/api-key\n\
GHOST_TRIGGER_RPC_URL=https://trigger.example.com/api-key\n\
GHOST_TRIGGER_KEYPAIR_PATH=secrets/rollout-wallet.json\n\
GHOST_TRIGGER_SHADOW_RPC_URL=https://shadow.example.com/api-key\n",
        )
        .unwrap();

        fs::write(
            &config_path,
            r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "http://localhost:10000"
grpc_x_token = ""
rpc_endpoint = "https://placeholder.invalid/rpc"

[trigger]
enabled = true
rpc_url = "https://placeholder.invalid/trigger"
keypair_path = "keys/placeholder.json"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true

[execution]
execution_mode = "paper"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#,
        )
        .unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();

        assert_eq!(config.seer.grpc_endpoint, "yellowstone.example.com:443");
        assert_eq!(config.effective_grpc_token(), Some("env-token"));
        assert_eq!(config.seer.rpc_endpoint, "https://rpc.example.com/api-key");
        assert_eq!(
            config.trigger.rpc_url,
            "https://trigger.example.com/api-key"
        );
        assert_eq!(
            config.trigger.keypair_path.as_deref(),
            Some(
                base.join("secrets/rollout-wallet.json")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            config.trigger.shadow_run.shadow_rpc_url,
            "https://shadow.example.com/api-key"
        );
    }

    #[test]
    fn test_from_file_loads_sender_tip_guard_and_live_thresholds() {
        let base = unique_temp_dir("sender_tip_guard_and_live_thresholds");
        let config_dir = base.join("configs/rollout");
        fs::create_dir_all(&config_dir).unwrap();
        let config_path = config_dir.join("dual-micro-live.toml");

        fs::write(
            &config_path,
            r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "live_and_shadow"
max_concurrent_positions = 1
max_position_size_sol = 0.0001
emergency_floor_sol = 0.06
position_size_buffer_sol = 0.0001
keypair_path = "/root/.config/solana/id.json"
live_preflight_max_state_age_slots = 9
live_exit_take_profit_pct = 0.30
live_exit_stop_loss_pct = 0.30

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[trigger.tip_guard]
max_tip_absolute_sol = 0.0007
fallback_tip_sol = 0.0007

[execution]
execution_mode = "dual"

[durability]
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#,
        )
        .unwrap();

        let config = LauncherConfig::from_file(&config_path)
            .expect("sender-only trigger config should load tip guard and live thresholds");

        assert_eq!(config.trigger.live_preflight_max_state_age_slots, 9);
        assert_eq!(config.trigger.live_exit_take_profit_pct, 0.30);
        assert_eq!(config.trigger.live_exit_stop_loss_pct, 0.30);
        assert_eq!(config.trigger.tip_guard.max_tip_absolute_sol, 0.0007);
        assert_eq!(config.trigger.tip_guard.fallback_tip_sol, 0.0007);
    }

    #[test]
    fn test_from_file_prefers_explicit_config_secret_values_over_dotenv() {
        let base = unique_temp_dir("explicit_config_secrets_win");
        let config_dir = base.join("configs/rollout");
        fs::create_dir_all(&config_dir).unwrap();
        fs::create_dir_all(base.join("keys")).unwrap();
        let config_path = config_dir.join("paper-burnin.toml");

        fs::write(
            base.join(".env"),
            "GHOST_SEER_GRPC_ENDPOINT=dotenv.example.invalid:443\n\
GHOST_SEER_GRPC_X_TOKEN=dotenv-token\n\
GHOST_SEER_RPC_ENDPOINT=https://dotenv.example.invalid/rpc\n\
GHOST_TRIGGER_RPC_URL=https://dotenv.example.invalid/trigger\n\
GHOST_TRIGGER_KEYPAIR_PATH=secrets/dotenv-wallet.json\n",
        )
        .unwrap();

        fs::write(
            &config_path,
            r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "config-token"
rpc_endpoint = "https://mainnet.helius-rpc.com/?api-key=config"

[trigger]
enabled = true
rpc_url = "https://mainnet.helius-rpc.com/?api-key=trigger-config"
keypair_path = "keys/config-wallet.json"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://mainnet.helius-rpc.com/?api-key=shadow-config"

[execution]
execution_mode = "paper"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#,
        )
        .unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();

        assert_eq!(
            config.seer.grpc_endpoint,
            "yellowstone-solana-mainnet.core.chainstack.com:443"
        );
        assert_eq!(config.effective_grpc_token(), Some("config-token"));
        assert_eq!(
            config.seer.rpc_endpoint,
            "https://mainnet.helius-rpc.com/?api-key=config"
        );
        assert_eq!(
            config.trigger.rpc_url,
            "https://mainnet.helius-rpc.com/?api-key=trigger-config"
        );
        assert_eq!(
            config.trigger.keypair_path.as_deref(),
            Some(
                config_dir
                    .join("keys/config-wallet.json")
                    .to_string_lossy()
                    .as_ref()
            )
        );
        assert_eq!(
            config.trigger.shadow_run.shadow_rpc_url,
            "https://mainnet.helius-rpc.com/?api-key=shadow-config"
        );
    }

    #[test]
    fn test_default_config() {
        let config = LauncherConfig::default();
        assert_eq!(config.mode, AppMode::Test);
        assert!(config.seer.enabled);
        assert!(config.trigger.enabled);
        assert!(config.gui_backend.enabled);
        assert!(config.oracle.enabled);
        assert_eq!(config.execution.execution_mode, ExecutionMode::Live);
        assert_eq!(config.trigger.entry_mode, TriggerEntryMode::Live);
        assert_eq!(config.snapshot_inactive_tx_buffer_capacity, 8_192);
        assert_eq!(config.snapshot_inactive_tx_ttl_margin_ms, 2_000);
    }

    #[test]
    fn test_config_serialization() {
        let config = LauncherConfig::default();
        let toml_str = toml::to_string(&config).unwrap();
        assert!(toml_str.contains("mode"));
        assert!(toml_str.contains("seer"));
        assert!(toml_str.contains("trigger"));
        assert!(toml_str.contains("oracle"));
        assert!(toml_str.contains("[shadow_ledger]"));
        assert!(!toml_str.contains("[oracle.shadow_ledger]"));
    }

    #[test]
    fn test_seer_commitment_defaults_to_processed() {
        let config = LauncherConfig::default();
        assert_eq!(config.seer.commitment, SeerCommitment::Processed);
    }

    #[test]
    fn test_seer_commitment_alias_mempool_deserializes_to_processed() {
        #[derive(Deserialize)]
        struct SeerCommitmentDoc {
            commitment: SeerCommitment,
        }

        let parsed: SeerCommitmentDoc = toml::from_str("commitment = \"mempool\"").unwrap();
        assert_eq!(parsed.commitment, SeerCommitment::Processed);
    }

    #[test]
    fn test_oracle_config_defaults() {
        let config = OracleConfig::default();
        assert!(config.enabled);
        assert!(config.simple_oracle.enabled);
        assert!(config.qass.enabled);
        assert!(config.hyper_oracle.enabled);
        assert!(config.shadow_ledger.enabled);
        assert_eq!(config.shadow_ledger.enrichment_freshness_ms, 200);
        assert_eq!(
            config.shadow_ledger.stale_fallback,
            ghost_core::shadow_ledger::ShadowLedgerStaleFallback::PendingCurve
        );
        assert_eq!(config.shadow_ledger.curve_wait_ms, 800);
        assert!(config.shadow_ledger.curve_require_for_buy);
        assert_eq!(
            config.reconciliation.drift_alert_threshold_lamports,
            50_000_000
        );
        // New workers
        assert!(!config.vision_critic.enabled); // Disabled by default to prevent API costs
        assert!(config.cluster_hunter.enabled);
        assert!(config.dev_profiler.enabled);
        assert_eq!(config.simple_oracle.min_score_threshold, 70);
        assert_eq!(config.pipeline.combined_score_threshold, 60);
        assert_eq!(config.pipeline.pipeline_timeout_ms, 500);
        // Sampling loop
        assert_eq!(config.sampling_loop.sampling_duration_ms, 7200);
        assert_eq!(config.sampling_loop.min_tx_count_for_scoring, 15);
        assert_eq!(config.sampling_loop.sniper_threshold, 90);
    }

    #[test]
    fn test_legacy_oracle_shadow_ledger_is_promoted_to_top_level_ssot() {
        let base = unique_temp_dir("legacy_shadow_ledger");
        let config_path = base.join("config.toml");

        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Paper;
        config.trigger.entry_mode = TriggerEntryMode::DryRunMock;
        config.shadow_ledger.enrichment_freshness_ms = 777;
        config.shadow_ledger.stale_fallback =
            ghost_core::shadow_ledger::ShadowLedgerStaleFallback::Reject;
        config.shadow_ledger.curve_wait_ms = 1234;
        config.shadow_ledger.curve_require_for_buy = false;
        config.save_to_file(&config_path).unwrap();

        let legacy_content = fs::read_to_string(&config_path)
            .unwrap()
            .replace("[shadow_ledger]", "[oracle.shadow_ledger]");
        fs::write(&config_path, legacy_content).unwrap();

        let loaded = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(loaded.shadow_ledger.enrichment_freshness_ms, 777);
        assert_eq!(
            loaded.shadow_ledger.stale_fallback,
            ghost_core::shadow_ledger::ShadowLedgerStaleFallback::Reject
        );
        assert_eq!(loaded.shadow_ledger.curve_wait_ms, 1234);
        assert!(!loaded.shadow_ledger.curve_require_for_buy);
        assert_eq!(
            loaded.oracle.shadow_ledger.enrichment_freshness_ms,
            loaded.shadow_ledger.enrichment_freshness_ms
        );
        assert_eq!(
            loaded.oracle.shadow_ledger.curve_wait_ms,
            loaded.shadow_ledger.curve_wait_ms
        );
        assert_eq!(
            loaded.oracle.shadow_ledger.curve_require_for_buy,
            loaded.shadow_ledger.curve_require_for_buy
        );
    }

    #[test]
    fn test_top_level_shadow_ledger_wins_over_legacy_nested_alias() {
        let base = unique_temp_dir("shadow_ledger_precedence");
        let config_path = base.join("config.toml");

        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Paper;
        config.trigger.entry_mode = TriggerEntryMode::DryRunMock;
        config.shadow_ledger.enrichment_freshness_ms = 250;
        config.shadow_ledger.stale_fallback =
            ghost_core::shadow_ledger::ShadowLedgerStaleFallback::PendingCurve;
        config.shadow_ledger.curve_wait_ms = 700;
        config.shadow_ledger.curve_require_for_buy = true;
        config.save_to_file(&config_path).unwrap();

        let mut content = fs::read_to_string(&config_path).unwrap();
        content.push_str(
            "\n[oracle.shadow_ledger]\n\
             enrichment_freshness_ms = 999\n\
               stale_fallback = \"reject\"\n\
               curve_wait_ms = 9999\n\
               curve_require_for_buy = false\n",
        );
        fs::write(&config_path, content).unwrap();

        let loaded = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(loaded.shadow_ledger.enrichment_freshness_ms, 250);
        assert_eq!(
            loaded.shadow_ledger.stale_fallback,
            ghost_core::shadow_ledger::ShadowLedgerStaleFallback::PendingCurve
        );
        assert_eq!(loaded.shadow_ledger.curve_wait_ms, 700);
        assert!(loaded.shadow_ledger.curve_require_for_buy);
    }

    #[test]
    fn test_validate_grpc_config_non_grpc_mode_ok() {
        let mut config = LauncherConfig::default();
        config.seer.source_mode = Some("helius_websocket".to_string());
        assert!(config.validate_grpc_config().is_ok());
    }

    #[test]
    fn test_validate_grpc_config_localhost_rejected() {
        let mut config = LauncherConfig::default();
        config.seer.source_mode = Some("geyser_grpc".to_string());
        config.seer.grpc_endpoint = "localhost:10000".to_string();
        config.seer.grpc_x_token = Some("test-token".to_string());
        let result = config.validate_grpc_config();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("localhost:10000"));

        // Also reject with http:// prefix (the actual default)
        config.seer.grpc_endpoint = "http://localhost:10000".to_string();
        let result2 = config.validate_grpc_config();
        assert!(result2.is_err());
    }

    #[test]
    fn test_validate_grpc_config_token_missing_rejected() {
        let mut config = LauncherConfig::default();
        config.seer.source_mode = Some("geyser_grpc".to_string());
        config.seer.grpc_endpoint = "https://my-node.chainstack.com:443".to_string();
        config.seer.grpc_x_token = None;
        config.seer.grpc_auth_token = None;
        let result = config.validate_grpc_config();
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("grpc_x_token"));
    }

    #[test]
    fn test_validate_grpc_config_empty_endpoint_rejected() {
        let mut config = LauncherConfig::default();
        config.seer.source_mode = Some("geyser_grpc".to_string());
        config.seer.grpc_endpoint = "".to_string();
        config.seer.grpc_x_token = Some("test-token".to_string());
        assert!(config.validate_grpc_config().is_err());
    }

    #[test]
    fn test_validate_grpc_config_valid() {
        let mut config = LauncherConfig::default();
        config.seer.source_mode = Some("geyser_grpc".to_string());
        config.seer.grpc_endpoint = "https://my-node.chainstack.com:443".to_string();
        config.seer.grpc_x_token = Some("valid-token".to_string());
        assert!(config.validate_grpc_config().is_ok());
    }

    #[test]
    fn test_validate_execution_profile_rejects_live_transport_without_helius_endpoint() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Live;
        config.trigger.entry_mode = TriggerEntryMode::Live;
        config.seer.source_mode = Some("grpc".to_string());
        config.seer.grpc_endpoint = "https://yellowstone.example.test:443".to_string();
        config.seer.grpc_x_token = Some("yellowstone-token".to_string());
        config.seer.helius_endpoint = None;

        let err = config
            .validate_execution_profile()
            .expect_err("live transport without Helius endpoint must fail closed");

        assert!(err.contains("[seer].helius_endpoint"));
    }

    #[test]
    fn test_validate_execution_profile_rejects_live_transport_without_grpc_mode() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Live;
        config.trigger.entry_mode = TriggerEntryMode::Live;
        config.seer.source_mode = Some("helius_websocket".to_string());
        config.seer.grpc_endpoint = "https://yellowstone.example.test:443".to_string();
        config.seer.grpc_x_token = Some("yellowstone-token".to_string());
        config.seer.helius_endpoint =
            Some("https://mainnet.helius-rpc.com/?api-key=test".to_string());

        let err = config
            .validate_execution_profile()
            .expect_err("live transport without grpc mode must fail closed");

        assert!(err.contains("[seer].source_mode=grpc"));
    }

    #[test]
    fn test_validate_execution_profile_rejects_live_transport_without_grpc_token() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Live;
        config.trigger.entry_mode = TriggerEntryMode::Live;
        config.seer.source_mode = Some("grpc".to_string());
        config.seer.grpc_endpoint = "https://yellowstone.example.test:443".to_string();
        config.seer.grpc_x_token = None;
        config.seer.grpc_auth_token = None;
        config.seer.helius_endpoint =
            Some("https://mainnet.helius-rpc.com/?api-key=test".to_string());

        let err = config
            .validate_execution_profile()
            .expect_err("missing grpc token must fail closed");
        assert!(err.contains("grpc_x_token"));
    }

    #[test]
    fn test_validate_execution_profile_rejects_placeholder_helius_endpoint() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Live;
        config.trigger.entry_mode = TriggerEntryMode::Live;
        config.seer.source_mode = Some("grpc".to_string());
        config.seer.grpc_endpoint = "https://yellowstone.example.test:443".to_string();
        config.seer.grpc_x_token = Some("yellowstone-token".to_string());
        config.seer.helius_endpoint = Some("replace-me".to_string());

        let err = config
            .validate_execution_profile()
            .expect_err("placeholder Helius endpoint must fail closed");

        assert!(err.contains("placeholder values"));
    }

    #[test]
    fn test_validate_execution_profile_accepts_live_transport_with_sender() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Live;
        config.trigger.entry_mode = TriggerEntryMode::Live;
        config.seer.source_mode = Some("grpc".to_string());
        config.seer.grpc_endpoint = "https://yellowstone.example.test:443".to_string();
        config.seer.grpc_x_token = Some("yellowstone-token".to_string());
        config.seer.helius_endpoint =
            Some("https://mainnet.helius-rpc.com/?api-key=test".to_string());

        assert!(config.validate_execution_profile().is_ok());
    }

    #[test]
    fn test_validate_execution_profile_accepts_shadow_mode_without_live_sender() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Shadow;
        config.trigger.entry_mode = TriggerEntryMode::ShadowOnly;
        config.trigger.max_concurrent_positions = 7;
        config.trigger.max_position_size_sol = 0.25;
        config.trigger.shadow_run.enabled = true;
        config.trigger.shadow_run.shadow_rpc_url = "https://shadow.example.com/api-key".to_string();
        config.seer.source_mode = Some("pump_portal_ws".to_string());
        config.seer.helius_endpoint = None;

        assert!(config.validate_execution_profile().is_ok());
    }

    #[test]
    fn test_production_accepts_operator_defined_rollout_limits() {
        let base = unique_temp_dir("production_accepts_operator_defined_rollout_limits");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 8
max_position_size_sol = 0.1
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "shadow"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path)
            .expect("custom operator-defined rollout limits should be accepted");
        assert_eq!(config.trigger.max_concurrent_positions, 8);
        assert_eq!(config.trigger.max_position_size_sol, 0.1);
    }

    #[test]
    fn test_validate_execution_profile_rejects_shadow_mode_without_shadow_transport() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Shadow;
        config.trigger.entry_mode = TriggerEntryMode::ShadowOnly;
        config.trigger.max_concurrent_positions = 1;
        config.trigger.shadow_run.enabled = false;

        let err = config
            .validate_execution_profile()
            .expect_err("shadow-capable profile must fail closed without shadow transport");

        assert!(err.contains("trigger.shadow_run"));
    }

    #[test]
    fn test_validate_execution_profile_rejects_placeholder_shadow_rpc_in_production() {
        let mut config = LauncherConfig::default();
        config.mode = AppMode::Production;
        config.execution.execution_mode = ExecutionMode::Shadow;
        config.trigger.entry_mode = TriggerEntryMode::ShadowOnly;
        config.trigger.max_concurrent_positions = 1;
        config.trigger.shadow_run.enabled = true;
        config.trigger.shadow_run.shadow_rpc_url = "replace-me".to_string();

        let err = config
            .validate_execution_profile()
            .expect_err("production shadow profile must fail closed on placeholder shadow rpc");

        assert!(err.contains("shadow_rpc_url"));
        assert!(err.contains("placeholder values"));
    }

    #[test]
    fn test_legacy_config_warnings_include_shadow_compat_and_legacy_shadow_run() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ExecutionMode::Paper;
        config.trigger.entry_mode = TriggerEntryMode::ShadowOnly;
        config.trigger.shadow_run.enabled = true;

        let warnings = config.legacy_config_warnings();
        assert!(warnings.iter().any(|warning| {
            warning.contains("execution_mode=paper + trigger.entry_mode=shadow_only")
        }));
        assert!(warnings.iter().any(
            |warning| warning.contains("[trigger.shadow_run] is a legacy compare-only surface")
        ));
    }

    #[test]
    fn test_validate_gatekeeper_runtime_contract_rejects_legacy_mode_in_production() {
        let mut config = LauncherConfig::default();
        config.mode = AppMode::Production;
        let mut gatekeeper = ghost_brain::config::GatekeeperV2Config::default();
        gatekeeper.use_three_layer_decision = false;

        let err = config
            .validate_gatekeeper_runtime_contract(&gatekeeper)
            .expect_err("production must fail closed on legacy terminal Gatekeeper mode");

        assert!(err.contains("[gatekeeper_v2].use_three_layer_decision = true"));
    }

    #[test]
    fn test_validate_gatekeeper_runtime_contract_allows_explicit_legacy_mode_in_test() {
        let config = LauncherConfig::default();
        let mut gatekeeper = ghost_brain::config::GatekeeperV2Config::default();
        gatekeeper.use_three_layer_decision = false;

        assert!(config
            .validate_gatekeeper_runtime_contract(&gatekeeper)
            .is_ok());
    }

    #[test]
    fn test_resolve_config_path_from_parent_directory() {
        let base = unique_temp_dir("resolve");
        let nested = base.join("a/b/c");
        fs::create_dir_all(&nested).unwrap();
        let config_path = base.join("config.toml");
        fs::write(&config_path, "mode = \"test\"\n[seer]\nenabled = true\n").unwrap();

        let resolved = resolve_path_from_ancestors(Path::new("config.toml"), vec![nested.clone()])
            .expect("config should be found in parent dirs");

        assert_eq!(resolved, config_path);
    }

    #[test]
    fn test_from_file_rebases_relative_runtime_paths() {
        let base = unique_temp_dir("rebase");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
keypair_path = "keys/id.json"
entry_mode = "live"

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"
output_path = "logs/shadow_run/buys.jsonl"

[execution]
execution_mode = "live"

[execution.shadow]
entry_log_path = "logs/shadow_run/shadow_entries.jsonl"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true

[logging]
file_path = "logs/system.log"
oracle_log_path = "logs/oracle.log"

[execution.events]
output_dir = "datasets/events"

[oracle]
decision_log_path = "logs/decisions.jsonl"
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();

        assert_eq!(
            config.ghost_brain_config_path,
            base.join("ghost-brain/ghost_brain_config.toml")
                .to_string_lossy()
        );
        assert_eq!(
            config.logging.file_path,
            base.join("logs/system.log").to_string_lossy()
        );
        assert_eq!(
            config.logging.oracle_log_path,
            base.join("logs/oracle.log").to_string_lossy()
        );
        assert_eq!(
            config.execution.events.output_dir,
            base.join("datasets/events").to_string_lossy()
        );
        assert_eq!(
            config.oracle.decision_log_path,
            base.join("logs/decisions.jsonl").to_string_lossy()
        );
        assert_eq!(
            config.trigger.shadow_run.output_path,
            base.join("logs/shadow_run/buys.jsonl").to_string_lossy()
        );
        assert_eq!(
            config.execution.shadow.entry_log_path,
            base.join("logs/shadow_run/shadow_entries.jsonl")
                .to_string_lossy()
        );
        assert_eq!(
            config.trigger.keypair_path.as_deref(),
            Some(base.join("keys/id.json").to_string_lossy().as_ref())
        );
    }

    #[test]
    fn test_from_file_rebases_relative_durability_paths() {
        let base = unique_temp_dir("rebase_durability");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        let durability = config.resolve_durability_config().unwrap();

        assert_eq!(durability.wal_dir(), Some(base.join("data/wal").as_path()));
        assert_eq!(
            durability.snapshot_dir(),
            Some(base.join("data/snapshots").as_path())
        );
    }

    #[test]
    fn test_from_file_maps_legacy_oracle_dry_run_to_trigger_entry_mode() {
        let base = unique_temp_dir("legacy_trigger_mode");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "test"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true

[oracle]
dry_run = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(config.execution.execution_mode, ExecutionMode::Paper);
        assert_eq!(config.trigger.entry_mode, TriggerEntryMode::ShadowOnly);
    }

    #[test]
    fn test_durability_wal_enabled_false_disables_wal_even_with_path() {
        let base = unique_temp_dir("wal_enabled_false");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "test"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true

[durability]
wal_enabled = false
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        let durability = config.resolve_durability_config().unwrap();

        assert_eq!(durability.mode(), DurabilityMode::SnapshotOnly);
        assert_eq!(durability.wal_dir(), None);
        assert_eq!(
            durability.snapshot_dir(),
            Some(base.join("data/snapshots").as_path())
        );
    }

    #[test]
    fn test_production_allows_wal_enabled_false() {
        let base = unique_temp_dir("production_wal_disabled");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true

[durability]
wal_enabled = false
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        let durability = config.resolve_durability_config().unwrap();

        assert_eq!(durability.mode(), DurabilityMode::SnapshotOnly);
        assert_eq!(durability.wal_dir(), None);
        assert_eq!(
            durability.snapshot_dir(),
            Some(base.join("data/snapshots").as_path())
        );
    }

    #[test]
    fn test_from_file_explicit_trigger_entry_mode_wins_over_legacy_alias() {
        let base = unique_temp_dir("explicit_trigger_mode");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "test"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true

[oracle]
dry_run = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(config.trigger.entry_mode, TriggerEntryMode::ShadowOnly);
    }

    #[test]
    fn test_from_file_preserves_explicit_trigger_entry_mode_live() {
        let base = unique_temp_dir("explicit_trigger_live");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "test"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "live"

[execution]
execution_mode = "live"

[gui_backend]
enabled = true

[oracle]
dry_run = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(config.trigger.entry_mode, TriggerEntryMode::Live);
    }

    #[test]
    fn test_startup_smoke_loaded_trigger_mode_reaches_component() {
        let base = unique_temp_dir("startup_smoke_trigger_mode");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
helius_endpoint = "https://mainnet.helius-rpc.com/?api-key=test"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "live_and_shadow"
max_concurrent_positions = 1
max_position_size_sol = 0.005
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "dual"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        let trigger = TriggerComponent::new(config.trigger.clone());

        assert_eq!(trigger.entry_mode(), TriggerEntryMode::LiveAndShadow);
        assert_eq!(config.trigger.max_position_size_sol, 0.005);
    }

    #[test]
    fn test_production_rejects_unsafe_rollout_safety_profile() {
        let base = unique_temp_dir("production_rejects_unsafe_rollout_safety_profile");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 3
max_position_size_sol = 0.1
emergency_floor_sol = 0
position_size_buffer_sol = 0

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let err = LauncherConfig::from_file(&config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("rollout safety profile"));
    }

    #[test]
    fn test_production_requires_full_durability_profile() {
        let base = unique_temp_dir("production_requires_full_durability_profile");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.00001
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[durability]
wal_dir = "data/wal"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        let durability = config.resolve_durability_config().unwrap();
        assert_eq!(durability.mode(), DurabilityMode::WalOnly);
    }

    #[test]
    fn test_production_requires_explicit_execution_mode() {
        let base = unique_temp_dir("production_requires_execution_mode");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let err = LauncherConfig::from_file(&config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[execution].execution_mode"));
    }

    #[test]
    fn test_production_requires_explicit_trigger_entry_mode() {
        let base = unique_temp_dir("production_requires_trigger_entry_mode");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[execution]
execution_mode = "paper"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let err = LauncherConfig::from_file(&config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("[trigger].entry_mode"));
    }

    #[test]
    fn test_production_rejects_invalid_execution_entry_pair() {
        let base = unique_temp_dir("production_invalid_execution_pair");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"

[execution]
execution_mode = "live"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let err = LauncherConfig::from_file(&config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("invalid execution profile"));
    }

    #[test]
    fn test_production_rejects_legacy_dry_run_aliases() {
        let base = unique_temp_dir("production_rejects_legacy_dry_run");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"

[execution]
execution_mode = "paper"
dry_run = true

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let err = LauncherConfig::from_file(&config_path)
            .unwrap_err()
            .to_string();
        assert!(err.contains("legacy dry_run"));
    }

    #[test]
    fn test_production_paper_profile_does_not_require_legacy_oracle_dry_run() {
        let base = unique_temp_dir("production_paper_without_legacy_oracle_dry_run");
        let config_path = base.join("config.toml");
        let config_body = r#"
mode = "production"

[seer]
enabled = true
source_mode = "grpc"
grpc_endpoint = "yellowstone-solana-mainnet.core.chainstack.com:443"
grpc_x_token = "token"
rpc_endpoint = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"

[trigger]
enabled = true
rpc_url = "https://solana-mainnet.core.chainstack.com/f2993325e24cc4aaa8f2d5fdd2b4c6fa"
entry_mode = "shadow_only"
max_concurrent_positions = 1
max_position_size_sol = 0.00001
emergency_floor_sol = 0.05
position_size_buffer_sol = 0.02

[trigger.shadow_run]
enabled = true
shadow_rpc_url = "https://shadow.example.com/api-key"

[execution]
execution_mode = "paper"

[durability]
wal_dir = "data/wal"
snapshot_dir = "data/snapshots"

[gui_backend]
enabled = true
"#;
        fs::write(&config_path, config_body).unwrap();

        let config = LauncherConfig::from_file(&config_path).unwrap();
        assert_eq!(config.execution.execution_mode, ExecutionMode::Paper);
        assert_eq!(config.trigger.entry_mode, TriggerEntryMode::ShadowOnly);
        assert!(!config.oracle.dry_run);
    }

    #[test]
    fn test_toml_has_explicit_path_is_section_aware() {
        let content = r#"
[execution]
execution_mode = "paper"

[trigger]
entry_mode = "live"

[other]
entry_mode = "shadow_only"
"#;

        assert!(toml_has_explicit_path(
            content,
            &["execution", "execution_mode"]
        ));
        assert!(toml_has_explicit_path(content, &["trigger", "entry_mode"]));
        assert!(toml_has_explicit_path(content, &["other", "entry_mode"]));
        assert!(!toml_has_explicit_path(
            content,
            &["trigger", "missing_key"]
        ));
        assert!(!toml_has_explicit_path(
            content,
            &["missing_table", "entry_mode"]
        ));
    }
}
