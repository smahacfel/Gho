//! Ghost Launcher - Integrated Standalone Application
//!
//! This launcher starts all Ghost components in a single process:
//! - Seer: Real-time pool detection
//! - Trigger: Transaction building and sending (using DirectBuyBuilder)
//! - GUI Backend: REST API and WebSocket server
//! - TuningService: Background weight optimization with bandit algorithms
//!
//! The BUY-flow uses DirectBuyBuilder for direct AMM interaction without
//! any on-chain program dependency.
//!
//! All components are configured via config.toml and logs are aggregated
//! to a single location.
//!
//! ## Event Bus
//!
//! Components communicate via a unified event bus using `tokio::sync::broadcast`.
//! This allows Seer to notify Trigger of new pools, and enables metrics collection.
//!
//! ## Weight Tuning
//!
//! The TuningService runs as a background task, periodically updating signal weights
//! using bandit algorithms (LinUCB/Thompson Sampling) based on trading outcomes.
//! Weights are updated every 3 minutes by default, with Bayesian optimization
//! running on 12-hour cycles for long-term parameter optimization.

use anyhow::{bail, Context, Result};
use ghost_brain::config::{GatekeeperV2Config, GhostBrainConfig};
use ghost_brain::oracle::SnapshotEngine;
use ghost_brain::tuning::{BanditAlgorithm, TuningMessage, TuningService, TuningServiceConfig};
use ghost_core::health::RuntimeHealth;
use ghost_core::shadow_ledger::ShadowLedger;
use ghost_core::Wal;
use ghost_launcher::{
    components::gatekeeper_commit_loop::GatekeeperCommitLoopConfig,
    components::live_position_registry::LivePositionRegistry,
    components::live_tx_sender::{
        probe_priority_fee_rpc, probe_sender_endpoint, resolve_live_sender_endpoint, LiveTxSender,
        LiveTxSenderConfig,
    },
    components::trigger::safety::{PositionLimitTracker, PositionSlotId},
    components::trigger::TriggerComponent,
    components::wallet_scanner::scan_wallet_positions,
    config::{
        redact_endpoint_for_logs, AppMode, ExecutionMode, LauncherConfig, ResolvedDurabilityConfig,
    },
    events::create_event_bus,
    events::{GhostEvent, PostBuySource},
    logging::{OracleDecisionFormatter, StandardFormatter},
    oracle_metrics, oracle_runtime, wal_recovery,
};
use seer::{
    configure_rpc_http_auth, new_async_rpc_client, new_async_rpc_client_with_timeout,
    rpc_http_auth_applies_to_url, DEFAULT_RPC_AUTH_HEADER, LEGACY_PROVIDER_AUTH_HEADER_ENV,
    LEGACY_PROVIDER_AUTH_TOKEN_ENV, RPC_HTTP_AUTH_HEADER_ENV, RPC_HTTP_AUTH_TOKEN_ENV,
};
use solana_sdk::pubkey::Pubkey;
use solana_sdk::signature::{read_keypair_file, Signer};
use std::env;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;
use tokio::signal;
use tokio::sync::{broadcast, mpsc, oneshot, watch};
use tracing::{debug, error, info, warn};
use tracing_subscriber::{
    filter::{FilterExt, FilterFn},
    layer::SubscriberExt,
    util::SubscriberInitExt,
    EnvFilter, Layer,
};
use yellowstone_grpc_client::GeyserGrpcClient;

const VERSION: &str = env!("CARGO_PKG_VERSION");
const CONFIG_FILE: &str = "config.toml";

/// Seconds to wait for gRPC subscribe-sent proof before exit.
const GRPC_SUBSCRIBE_TIMEOUT_SECS: u64 = 5;
/// Exit code when gRPC subscribe is not sent within the timeout.
const EXIT_GRPC_SUBSCRIBE_TIMEOUT: i32 = 5;
/// Exit code when the OracleRuntime task exits before shutdown is requested.
const EXIT_ORACLE_RUNTIME_STOPPED: i32 = 6;
const STARTUP_HYDRATION_TIMEOUT_SECS: u64 = 15;
const STARTUP_HYDRATION_IGNORE_MINTS_ENV: &str = "GHOST_STARTUP_HYDRATION_IGNORE_MINTS";

fn load_startup_hydration_ignore_mints(config_path: &Path) -> Result<Vec<Pubkey>> {
    let Some(raw) = LauncherConfig::lookup_secret_value_for_config_path(
        config_path,
        STARTUP_HYDRATION_IGNORE_MINTS_ENV,
    )?
    else {
        return Ok(Vec::new());
    };

    let mut parsed = std::collections::BTreeSet::new();
    for entry in raw.split(|ch: char| ch == ',' || ch.is_ascii_whitespace()) {
        let trimmed = entry.trim();
        if trimmed.is_empty() {
            continue;
        }
        let mint = trimmed.parse::<Pubkey>().with_context(|| {
            format!(
                "invalid {} entry '{}'",
                STARTUP_HYDRATION_IGNORE_MINTS_ENV, trimmed
            )
        })?;
        parsed.insert(mint);
    }

    Ok(parsed.into_iter().collect())
}

fn requires_live_sender(config: &LauncherConfig) -> bool {
    config.trigger.enabled
        && matches!(
            config.execution.execution_mode,
            ExecutionMode::Live | ExecutionMode::Dual
        )
        && matches!(
            config.trigger.entry_mode,
            ghost_launcher::config::TriggerEntryMode::Live
                | ghost_launcher::config::TriggerEntryMode::LiveAndShadow
        )
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum StartupCommand {
    Run,
    GenerateConfig,
    Preflight,
}

#[derive(Debug, Clone)]
struct CliOptions {
    command: StartupCommand,
    requested_config_path: PathBuf,
}

fn load_gatekeeper_v2_config(
    config_path: &str,
    ghost_brain_config: Option<&GhostBrainConfig>,
) -> Result<GatekeeperV2Config> {
    let config_path = Path::new(config_path);

    match GhostBrainConfig::gatekeeper_v2_from_toml_file(config_path) {
        Ok(Some(cfg)) => {
            info!(
                "🛡️ Gatekeeper V2 config loaded from {}: min_tx={} min_unique={} min_buy={} max_wait_ms={} min_sol_threshold={} min_phases={}",
                config_path.display(),
                cfg.min_tx_count,
                cfg.min_unique_signers,
                cfg.min_buy_count,
                cfg.max_wait_time_ms,
                cfg.min_sol_threshold,
                cfg.min_phases_to_pass,
            );
            Ok(cfg)
        }
        Ok(None) => {
            if let Some(cfg) = ghost_brain_config.and_then(|brain| brain.gatekeeper_v2.clone()) {
                warn!(
                    "⚠️ No [gatekeeper_v2] section found via direct TOML load at {} — falling back to the validated full Ghost Brain config",
                    config_path.display()
                );
                Ok(cfg)
            } else {
                bail!(
                    "missing required [gatekeeper_v2] section in {} — refusing to start Gatekeeper V2 with built-in defaults",
                    config_path.display()
                );
            }
        }
        Err(err) => {
            if let Some(cfg) = ghost_brain_config.and_then(|brain| brain.gatekeeper_v2.clone()) {
                warn!(
                    "⚠️ Failed to parse [gatekeeper_v2] directly from {}: {} — falling back to the validated full Ghost Brain config",
                    config_path.display(),
                    err
                );
                Ok(cfg)
            } else {
                bail!(
                    "failed to parse required [gatekeeper_v2] from {}: {} — refusing to start Gatekeeper V2 with built-in defaults",
                    config_path.display(),
                    err
                );
            }
        }
    }
}

fn sync_legacy_gatekeeper_aliases(
    config: &mut LauncherConfig,
    gatekeeper_v2_config: &GatekeeperV2Config,
) {
    let old_min_tx = config.gatekeeper.min_tx_to_pass;
    if old_min_tx != gatekeeper_v2_config.min_tx_count {
        info!(
            "🔄 Gatekeeper alias sync: min_tx_to_pass {} -> {}",
            old_min_tx, gatekeeper_v2_config.min_tx_count
        );
        config.gatekeeper.min_tx_to_pass = gatekeeper_v2_config.min_tx_count;
    }

    let old_window = config.gatekeeper.observation_window_ms;
    if old_window != gatekeeper_v2_config.max_wait_time_ms {
        info!(
            "🔄 Gatekeeper alias sync: observation_window_ms {} -> {}",
            old_window, gatekeeper_v2_config.max_wait_time_ms
        );
        config.gatekeeper.observation_window_ms = gatekeeper_v2_config.max_wait_time_ms;
    }
}

fn runtime_oracle_dry_run(config: &LauncherConfig) -> bool {
    config.oracle.dry_run
        || matches!(
            config.execution.execution_mode,
            ghost_launcher::config::ExecutionMode::Paper
                | ghost_launcher::config::ExecutionMode::Shadow
        )
}

fn print_usage() {
    println!("ghost-launcher [--config PATH] [--preflight | --generate-config]");
    println!("ghost-launcher [config.toml]");
}

fn parse_cli_args() -> Result<CliOptions> {
    let args: Vec<String> = env::args().skip(1).collect();
    let mut positional_config: Option<PathBuf> = None;
    let mut explicit_config: Option<PathBuf> = None;
    let mut command = StartupCommand::Run;
    let mut idx = 0usize;

    while idx < args.len() {
        match args[idx].as_str() {
            "--config" => {
                idx += 1;
                let Some(path) = args.get(idx) else {
                    bail!("--config requires a path");
                };
                explicit_config = Some(PathBuf::from(path));
            }
            "--preflight" => {
                if command == StartupCommand::GenerateConfig {
                    bail!("--preflight cannot be combined with --generate-config");
                }
                command = StartupCommand::Preflight;
            }
            "--generate-config" => {
                if command == StartupCommand::Preflight {
                    bail!("--generate-config cannot be combined with --preflight");
                }
                command = StartupCommand::GenerateConfig;
            }
            "--help" | "-h" => {
                print_usage();
                std::process::exit(0);
            }
            other if other.starts_with('-') => bail!("unknown argument: {other}"),
            other => {
                if positional_config.is_some() {
                    bail!("only one positional config path is supported");
                }
                positional_config = Some(PathBuf::from(other));
            }
        }
        idx += 1;
    }

    Ok(CliOptions {
        command,
        requested_config_path: explicit_config
            .or(positional_config)
            .unwrap_or_else(|| PathBuf::from(CONFIG_FILE)),
    })
}

fn ensure_directory_writable(path: &Path, label: &str) -> Result<()> {
    if path.exists() && !path.is_dir() {
        bail!(
            "{label} path exists but is not a directory: {}",
            path.display()
        );
    }

    std::fs::create_dir_all(path)
        .with_context(|| format!("failed to create {label} directory {}", path.display()))?;

    let probe_path = path.join(format!(
        ".ghost-preflight-probe-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    ));
    let probe_file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .open(&probe_path)
        .with_context(|| format!("{label} directory is not writable: {}", path.display()))?;
    drop(probe_file);
    std::fs::remove_file(&probe_path)
        .with_context(|| format!("failed to clean probe file {}", probe_path.display()))?;
    Ok(())
}

fn ensure_parent_directory_writable(path: &Path, label: &str) -> Result<()> {
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    ensure_directory_writable(parent, label)
}

fn derive_shadow_lifecycle_log_path(entry_log_path: &str) -> PathBuf {
    let path = Path::new(entry_log_path);
    let file_name = path.file_name().and_then(|name| name.to_str());
    let lifecycle_name = match file_name {
        Some("shadow_entries.jsonl") => "shadow_lifecycle.jsonl".to_string(),
        Some(name) if !name.is_empty() => format!("{name}.lifecycle.jsonl"),
        _ => "shadow_lifecycle.jsonl".to_string(),
    };
    path.parent()
        .map(|parent| parent.join(&lifecycle_name))
        .unwrap_or_else(|| PathBuf::from(lifecycle_name))
}

fn effective_shadow_lifecycle_log_path(config: &LauncherConfig) -> Option<PathBuf> {
    if config.execution.execution_mode != ExecutionMode::Shadow {
        return None;
    }

    Some(
        config
            .execution
            .shadow
            .lifecycle_log_path
            .as_deref()
            .map(PathBuf::from)
            .unwrap_or_else(|| {
                derive_shadow_lifecycle_log_path(&config.execution.shadow.entry_log_path)
            }),
    )
}

fn normalize_grpc_endpoint(raw: &str) -> String {
    if raw.starts_with("https://") || raw.starts_with("http://") {
        raw.to_string()
    } else {
        format!("https://{raw}")
    }
}

async fn probe_rpc_endpoint_app(rpc_url: &str) -> Result<String> {
    let redacted_rpc_url = redact_endpoint_for_logs(rpc_url);
    let client = new_async_rpc_client_with_timeout(rpc_url.to_string(), Duration::from_secs(5));
    let version = tokio::time::timeout(Duration::from_secs(6), client.get_version())
        .await
        .with_context(|| format!("rpc probe timed out for {redacted_rpc_url}"))?
        .with_context(|| format!("rpc getVersion failed for {redacted_rpc_url}"))?;
    Ok(version.solana_core)
}

fn lookup_runtime_secret(config_path: &Path, env_name: &str) -> Result<Option<String>> {
    LauncherConfig::lookup_secret_value_for_config_path(config_path, env_name)
}

fn configure_rpc_http_auth_from_secret_env(
    config: &LauncherConfig,
    config_path: &Path,
) -> Result<()> {
    let uses_header_auth_rpc = rpc_http_auth_applies_to_url(&config.seer.rpc_endpoint)
        || rpc_http_auth_applies_to_url(&config.trigger.rpc_url)
        || rpc_http_auth_applies_to_url(&config.trigger.shadow_run.shadow_rpc_url);
    if !uses_header_auth_rpc {
        return Ok(());
    }

    let mut token = lookup_runtime_secret(config_path, RPC_HTTP_AUTH_TOKEN_ENV)?;
    if token.is_none() {
        token = lookup_runtime_secret(config_path, LEGACY_PROVIDER_AUTH_TOKEN_ENV)?;
    }

    let mut header = lookup_runtime_secret(config_path, RPC_HTTP_AUTH_HEADER_ENV)?;
    if header.is_none() {
        header = lookup_runtime_secret(config_path, LEGACY_PROVIDER_AUTH_HEADER_ENV)?;
    }
    let header = header.unwrap_or_else(|| DEFAULT_RPC_AUTH_HEADER.to_string());

    match token {
        Some(token) => {
            configure_rpc_http_auth(header.clone(), token)
                .map_err(|err| anyhow::anyhow!("failed to configure RPC HTTP auth: {err}"))?;
            info!(
                header = %header,
                "RPC HTTP auth configured for header-auth RPC endpoints"
            );
        }
        None if uses_header_auth_rpc => {
            warn!(
                rpc_auth_token_env = RPC_HTTP_AUTH_TOKEN_ENV,
                legacy_fallback_env = LEGACY_PROVIDER_AUTH_TOKEN_ENV,
                "Header-auth RPC endpoint configured but no RPC auth token was found in process env or .env"
            );
        }
        None => {}
    }

    Ok(())
}

async fn probe_grpc_endpoint_app(config: &LauncherConfig) -> Result<String> {
    let token = config
        .effective_grpc_token()
        .context("grpc_x_token is required for gRPC application probe")?
        .to_string();
    let endpoint = normalize_grpc_endpoint(config.seer.grpc_endpoint.trim());
    let redacted_endpoint = redact_endpoint_for_logs(&endpoint);
    let mut client = GeyserGrpcClient::build_from_shared(endpoint.clone())?
        .x_token(Some(token))?
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(10))
        .connect()
        .await
        .with_context(|| format!("gRPC connect failed for {redacted_endpoint}"))?;

    match tokio::time::timeout(Duration::from_secs(6), client.get_version()).await {
        Ok(Ok(response)) => Ok(response.version),
        Ok(Err(version_err)) => {
            match tokio::time::timeout(Duration::from_secs(6), client.ping(1)).await {
                Ok(Ok(_)) => Ok("ping_ok".to_string()),
                Ok(Err(ping_err)) => Err(anyhow::anyhow!(
                    "gRPC app probe failed for {}: getVersion={} ; ping={}",
                    redacted_endpoint,
                    version_err,
                    ping_err
                )),
                Err(_) => Err(anyhow::anyhow!(
                    "gRPC ping probe timed out for {redacted_endpoint}"
                )),
            }
        }
        Err(_) => Err(anyhow::anyhow!(
            "gRPC getVersion probe timed out for {redacted_endpoint}"
        )),
    }
}

fn build_live_tx_sender(config: &LauncherConfig) -> Result<Option<Arc<LiveTxSender>>> {
    if !requires_live_sender(config) {
        return Ok(None);
    }

    let priority_fee_rpc_url = config
        .seer
        .helius_endpoint
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context(
            "live BUY/SELL execution requires [seer].helius_endpoint because Helius Priority Fee API is part of the Sender path",
        )?;
    let yellowstone_x_token = config
        .effective_grpc_token()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .context(
            "live BUY/SELL execution requires non-empty seer.grpc_x_token/grpc_auth_token because Yellowstone confirms Sender signatures",
        )?;
    let sender_endpoint = resolve_live_sender_endpoint();
    let live_tx_sender = Arc::new(LiveTxSender::new(LiveTxSenderConfig::new(
        sender_endpoint.clone(),
        priority_fee_rpc_url,
        config.seer.grpc_endpoint.clone(),
        yellowstone_x_token,
    ))?);

    info!(
        sender_endpoint = %redact_endpoint_for_logs(&sender_endpoint),
        priority_fee_rpc_url = %redact_endpoint_for_logs(priority_fee_rpc_url),
        yellowstone_grpc_endpoint = %redact_endpoint_for_logs(&config.seer.grpc_endpoint),
        "🚀 LiveTxSender: initialized Helius Sender + Yellowstone transport"
    );

    Ok(Some(live_tx_sender))
}

fn init_optional_wal(durability: &ResolvedDurabilityConfig) -> Result<Option<Arc<Wal>>> {
    let Some(wal_path) = durability.wal.as_ref() else {
        info!("🪵 Shared WAL disabled");
        return Ok(None);
    };

    ensure_directory_writable(&wal_path.path, "WAL")?;
    let wal = Arc::new(Wal::new(
        &wal_path.path,
        durability.wal_segment_ms,
        durability.wal_retention_ms,
    )?);

    info!(
        path = %wal_path.path.display(),
        source = %wal_path.source,
        segment_ms = durability.wal_segment_ms,
        retention_ms = durability.wal_retention_ms,
        "🪵 Shared WAL enabled for Seer ingest + OracleRuntime decisions"
    );

    Ok(Some(wal))
}

fn log_runtime_durability(durability: &ResolvedDurabilityConfig) {
    let wal_dir = durability
        .wal
        .as_ref()
        .map(|entry| entry.path.display().to_string())
        .unwrap_or_else(|| "-".to_string());
    let wal_source = durability
        .wal
        .as_ref()
        .map(|entry| entry.source.to_string())
        .unwrap_or_else(|| "-".to_string());
    let snapshot_dir = durability
        .snapshot
        .as_ref()
        .map(|entry| entry.path.display().to_string())
        .unwrap_or_else(|| "-".to_string());
    let snapshot_source = durability
        .snapshot
        .as_ref()
        .map(|entry| entry.source.to_string())
        .unwrap_or_else(|| "-".to_string());

    metrics::counter!(
        "runtime_durability_mode",
        1u64,
        "mode" => durability.mode().as_str()
    );
    info!(
        mode = durability.mode().as_str(),
        wal_dir = wal_dir,
        wal_source = wal_source,
        snapshot_dir = snapshot_dir,
        snapshot_source = snapshot_source,
        snapshot_interval_s = durability.snapshot_interval_s,
        wal_segment_ms = durability.wal_segment_ms,
        wal_retention_ms = durability.wal_retention_ms,
        "Runtime durability profile resolved"
    );
}

async fn run_preflight(config: &LauncherConfig, config_path: &Path) -> Result<()> {
    let mut failures = Vec::new();

    let execution_result = config
        .validate_execution_profile()
        .map_err(anyhow::Error::msg);
    match execution_result {
        Ok(()) => println!(
            "[ok] execution_profile: execution_mode={:?}, entry_mode={}",
            config.execution.execution_mode,
            config.trigger.entry_mode.as_str()
        ),
        Err(err) => {
            eprintln!("[fail] execution_profile: {err}");
            failures.push(format!("execution_profile: {err}"));
        }
    }

    let gatekeeper_contract_result =
        load_gatekeeper_v2_config(&config.ghost_brain_config_path, None).and_then(
            |gatekeeper_v2| {
                config
                    .validate_gatekeeper_runtime_contract(&gatekeeper_v2)
                    .map_err(anyhow::Error::msg)?;
                Ok(gatekeeper_v2)
            },
        );
    match gatekeeper_contract_result {
        Ok(gatekeeper_v2) => println!(
            "[ok] gatekeeper.contract: use_three_layer_decision={}",
            gatekeeper_v2.use_three_layer_decision
        ),
        Err(err) => {
            eprintln!("[fail] gatekeeper.contract: {err}");
            failures.push(format!("gatekeeper.contract: {err}"));
        }
    }

    let grpc_result = config.validate_grpc_config().map_err(anyhow::Error::msg);
    match grpc_result {
        Ok(()) => println!(
            "[ok] transport.grpc: source_mode={} endpoint={}",
            config.effective_source_mode(),
            redact_endpoint_for_logs(&config.seer.grpc_endpoint)
        ),
        Err(err) => {
            eprintln!("[fail] transport.grpc: {err}");
            failures.push(format!("transport.grpc: {err}"));
        }
    }

    let durability = match config.resolve_durability_config() {
        Ok(durability) => {
            println!(
                "[ok] durability.profile: mode={} wal={} snapshot={}",
                durability.mode().as_str(),
                durability
                    .wal
                    .as_ref()
                    .map(|entry| entry.path.display().to_string())
                    .unwrap_or_else(|| "-".to_string()),
                durability
                    .snapshot
                    .as_ref()
                    .map(|entry| entry.path.display().to_string())
                    .unwrap_or_else(|| "-".to_string())
            );
            durability
        }
        Err(err) => {
            eprintln!("[fail] durability.profile: {err}");
            failures.push(format!("durability.profile: {err}"));
            ResolvedDurabilityConfig {
                wal: None,
                wal_segment_ms: config.durability.wal_segment_ms,
                wal_retention_ms: config.durability.wal_retention_ms,
                snapshot: None,
                snapshot_interval_s: config.durability.snapshot_interval_s,
            }
        }
    };

    if let Some(wal) = durability.wal.as_ref() {
        match ensure_directory_writable(&wal.path, "WAL") {
            Ok(()) => println!(
                "[ok] durability.wal_dir: writable {} ({})",
                wal.path.display(),
                wal.source
            ),
            Err(err) => {
                eprintln!("[fail] durability.wal_dir: {err}");
                failures.push(format!("durability.wal_dir: {err}"));
            }
        }
    }
    if let Some(snapshot) = durability.snapshot.as_ref() {
        match ensure_directory_writable(&snapshot.path, "snapshot") {
            Ok(()) => println!(
                "[ok] durability.snapshot_dir: writable {} ({})",
                snapshot.path.display(),
                snapshot.source
            ),
            Err(err) => {
                eprintln!("[fail] durability.snapshot_dir: {err}");
                failures.push(format!("durability.snapshot_dir: {err}"));
            }
        }
    }

    match ensure_parent_directory_writable(Path::new(&config.logging.file_path), "system log") {
        Ok(()) => println!("[ok] logging.system_dir: {}", config.logging.file_path),
        Err(err) => {
            eprintln!("[fail] logging.system_dir: {err}");
            failures.push(format!("logging.system_dir: {err}"));
        }
    }
    match ensure_parent_directory_writable(Path::new(&config.logging.oracle_log_path), "oracle log")
    {
        Ok(()) => println!(
            "[ok] logging.oracle_dir: {}",
            config.logging.oracle_log_path
        ),
        Err(err) => {
            eprintln!("[fail] logging.oracle_dir: {err}");
            failures.push(format!("logging.oracle_dir: {err}"));
        }
    }
    match ensure_directory_writable(
        Path::new(&config.execution.events.output_dir),
        "events output",
    ) {
        Ok(()) => println!(
            "[ok] execution.events_dir: {}",
            config.execution.events.output_dir
        ),
        Err(err) => {
            eprintln!("[fail] execution.events_dir: {err}");
            failures.push(format!("execution.events_dir: {err}"));
        }
    }
    match ensure_directory_writable(
        Path::new(&config.oracle.decision_log_path),
        "decision log output",
    ) {
        Ok(()) => println!(
            "[ok] oracle.decision_log_dir: {}",
            config.oracle.decision_log_path
        ),
        Err(err) => {
            eprintln!("[fail] oracle.decision_log_dir: {err}");
            failures.push(format!("oracle.decision_log_dir: {err}"));
        }
    }
    match ensure_parent_directory_writable(
        Path::new(&config.execution.shadow.entry_log_path),
        "shadow entry log output",
    ) {
        Ok(()) => println!(
            "[ok] execution.shadow.entry_log_dir: {}",
            config.execution.shadow.entry_log_path
        ),
        Err(err) => {
            eprintln!("[fail] execution.shadow.entry_log_dir: {err}");
            failures.push(format!("execution.shadow.entry_log_dir: {err}"));
        }
    }
    if let Some(lifecycle_log_path) = effective_shadow_lifecycle_log_path(config) {
        match ensure_parent_directory_writable(&lifecycle_log_path, "shadow lifecycle log output") {
            Ok(()) => println!(
                "[ok] execution.shadow.lifecycle_log_dir: {}",
                lifecycle_log_path.display()
            ),
            Err(err) => {
                eprintln!("[fail] execution.shadow.lifecycle_log_dir: {err}");
                failures.push(format!("execution.shadow.lifecycle_log_dir: {err}"));
            }
        }
    }
    if config.trigger.shadow_run.enabled {
        match ensure_parent_directory_writable(
            Path::new(&config.trigger.shadow_run.output_path),
            "shadow report output",
        ) {
            Ok(()) => println!(
                "[ok] trigger.shadow_run_dir: {}",
                config.trigger.shadow_run.output_path
            ),
            Err(err) => {
                eprintln!("[fail] trigger.shadow_run_dir: {err}");
                failures.push(format!("trigger.shadow_run_dir: {err}"));
            }
        }
    }

    let payer = match config.trigger.keypair_path.as_deref() {
        Some(path) => match read_keypair_file(path) {
            Ok(keypair) => {
                println!("[ok] trigger.keypair: {} ({})", path, keypair.pubkey());
                Some(keypair.pubkey())
            }
            Err(err) => {
                let message = format!("failed to read keypair at {path}: {err}");
                eprintln!("[fail] trigger.keypair: {message}");
                failures.push(format!("trigger.keypair: {message}"));
                None
            }
        },
        None => {
            let message = "trigger.keypair_path is not configured".to_string();
            eprintln!("[fail] trigger.keypair: {message}");
            failures.push(format!("trigger.keypair: {message}"));
            None
        }
    };

    match probe_rpc_endpoint_app(&config.trigger.rpc_url).await {
        Ok(version) => println!(
            "[ok] trigger.rpc_url: jsonrpc getVersion={} via {}",
            version,
            redact_endpoint_for_logs(&config.trigger.rpc_url)
        ),
        Err(err) => {
            eprintln!("[fail] trigger.rpc_url: {err}");
            failures.push(format!("trigger.rpc_url: {err}"));
        }
    }

    let is_grpc = matches!(
        config.effective_source_mode().as_str(),
        "grpc" | "geyser_grpc" | "g"
    );
    if is_grpc {
        match probe_grpc_endpoint_app(config).await {
            Ok(version) => println!(
                "[ok] seer.grpc_endpoint: app_probe={} via {}",
                version,
                redact_endpoint_for_logs(&config.seer.grpc_endpoint)
            ),
            Err(err) => {
                eprintln!("[fail] seer.grpc_endpoint: {err}");
                failures.push(format!("seer.grpc_endpoint: {err}"));
            }
        }
    }

    if requires_live_sender(config) {
        let sender_endpoint = resolve_live_sender_endpoint();
        match probe_sender_endpoint(&sender_endpoint).await {
            Ok(()) => println!(
                "[ok] trigger.live_sender: ping returned HTTP 200 via {}",
                redact_endpoint_for_logs(&sender_endpoint)
            ),
            Err(err) => {
                eprintln!("[fail] trigger.live_sender: {err}");
                failures.push(format!("trigger.live_sender: {err}"));
            }
        }

        match config
            .seer
            .helius_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            Some(endpoint) => match probe_priority_fee_rpc(endpoint).await {
                Ok(version) => println!(
                    "[ok] seer.helius_endpoint: jsonrpc getVersion={} via {}",
                    version,
                    redact_endpoint_for_logs(endpoint)
                ),
                Err(err) => {
                    eprintln!("[fail] seer.helius_endpoint: {err}");
                    failures.push(format!("seer.helius_endpoint: {err}"));
                }
            },
            None => {
                let message =
                    "live Sender path requires non-empty [seer].helius_endpoint for priority fee lookups";
                eprintln!("[fail] seer.helius_endpoint: {message}");
                failures.push(format!("seer.helius_endpoint: {message}"));
            }
        }
    }

    if let Some(payer_pubkey) = payer {
        let rpc_client = new_async_rpc_client(config.trigger.rpc_url.clone());
        match rpc_client.get_balance(&payer_pubkey).await {
            Ok(balance_lamports) => {
                let balance_sol = balance_lamports as f64 / 1_000_000_000.0;
                let required_reserve_sol =
                    config.trigger.emergency_floor_sol + config.trigger.position_size_buffer_sol;
                let required_trade_budget_sol =
                    required_reserve_sol + config.trigger.max_position_size_sol;
                if balance_sol < required_trade_budget_sol {
                    let message = format!(
                        "wallet balance {:.9} SOL is below required reserve+trade budget {:.9} SOL (floor {:.9} + buffer {:.9} + size {:.9})",
                        balance_sol,
                        required_trade_budget_sol,
                        config.trigger.emergency_floor_sol,
                        config.trigger.position_size_buffer_sol,
                        config.trigger.max_position_size_sol
                    );
                    eprintln!("[fail] trigger.balance: {message}");
                    failures.push(format!("trigger.balance: {message}"));
                } else {
                    println!(
                        "[ok] trigger.balance: {:.9} SOL >= {:.9} SOL reserve+trade budget",
                        balance_sol, required_trade_budget_sol
                    );
                }
            }
            Err(err) => {
                let message = format!("failed to fetch balance over trigger.rpc_url: {err}");
                eprintln!("[fail] trigger.balance: {message}");
                failures.push(format!("trigger.balance: {message}"));
            }
        }
    }

    if config.metrics.enabled {
        let addr = format!("{}:{}", config.metrics.bind, config.metrics.port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => {
                drop(listener);
                println!("[ok] metrics.port: free {}", addr);
            }
            Err(err) => {
                let message = format!("metrics bind failed on {addr}: {err}");
                eprintln!("[fail] metrics.port: {message}");
                failures.push(format!("metrics.port: {message}"));
            }
        }
    }

    if failures.is_empty() {
        println!(
            "[ok] preflight: all runtime checks passed for {}",
            config_path.display()
        );
        Ok(())
    } else {
        bail!(
            "preflight failed for {}:\n- {}",
            config_path.display(),
            failures.join("\n- ")
        )
    }
}

fn build_live_sell_handle(
    config: &LauncherConfig,
    live_tx_sender: Option<Arc<LiveTxSender>>,
    shadow_ledger: Arc<ShadowLedger>,
    account_state_core: Arc<ghost_core::account_state_core::reducer::AccountStateReducer>,
) -> Result<Option<ghost_launcher::components::post_buy_runtime::LiveSellHandle>> {
    use ghost_launcher::components::post_buy_runtime::LiveSellHandle;

    if !requires_live_sender(config) {
        info!(
            execution_mode = ?config.execution.execution_mode,
            trigger_enabled = config.trigger.enabled,
            "ℹ️  LiveSellHandle: skipped (no live transport required at startup)"
        );
        return Ok(None);
    }

    let keypair_path = config.trigger.keypair_path.as_deref().context(
        "live BUY/SELL execution requires trigger.keypair_path so Sender signing stays fail-closed",
    )?;
    let payer = read_keypair_file(keypair_path).map_err(|err| {
        anyhow::anyhow!(
            "failed to read trigger keypair for live BUY/SELL execution from {}: {}",
            keypair_path,
            err
        )
    })?;
    let live_tx_sender = live_tx_sender.context(
        "live BUY/SELL execution requires initialized Helius Sender + Yellowstone transport",
    )?;
    let rpc_client = Arc::new(new_async_rpc_client(config.trigger.rpc_url.clone()));
    info!(
        payer = %payer.pubkey(),
        sender_endpoint = %redact_endpoint_for_logs(live_tx_sender.sender_endpoint()),
        "🔫 LiveSellHandle: initialized with Sender-only live transport"
    );

    Ok(Some(LiveSellHandle {
        rpc_client,
        live_tx_sender,
        payer: Arc::new(payer),
        account_state_core,
        shadow_ledger,
    }))
}

async fn hydrate_startup_live_positions(
    event_tx: &ghost_launcher::events::EventBusSender,
    position_limit_tracker: &PositionLimitTracker,
    live_sell_handle: &Option<ghost_launcher::components::post_buy_runtime::LiveSellHandle>,
    live_position_registry: &LivePositionRegistry,
    startup_hydration_ignore_mints: &[Pubkey],
) -> Result<()> {
    let Some(live_sell_handle) = live_sell_handle.as_ref() else {
        return Ok(());
    };

    let owner = live_sell_handle.payer.pubkey();
    let wallet_positions = scan_wallet_positions(&live_sell_handle.rpc_client, &owner)
        .await
        .context("failed to scan wallet positions during startup hydration")?;
    let tracked_positions = live_position_registry
        .load_open_positions()
        .await
        .context("failed to load live position recovery registry")?;

    let nonzero_positions: Vec<_> = wallet_positions
        .into_iter()
        .filter(|position| position.amount > 0)
        .collect();
    let ignored_mints: std::collections::HashSet<_> =
        startup_hydration_ignore_mints.iter().copied().collect();
    let baseline_active = position_limit_tracker.active_positions();
    let mut recovery_events = 0usize;

    for wallet_position in nonzero_positions {
        if ignored_mints.contains(&wallet_position.mint) {
            warn!(
                mint = %wallet_position.mint,
                amount = wallet_position.amount,
                env = STARTUP_HYDRATION_IGNORE_MINTS_ENV,
                "Startup hydration: skipping explicitly ignored wallet position"
            );
            continue;
        }

        let mint_key = wallet_position.mint.to_string();
        let tracked = tracked_positions.get(&mint_key).with_context(|| {
            format!(
                "startup hydration found non-zero wallet position for mint {} but no recovery registry entry exists",
                mint_key
            )
        })?;

        let slot_id = PositionSlotId::derive(&owner, &wallet_position.mint);
        if position_limit_tracker.contains(slot_id) {
            continue;
        }

        event_tx
            .send(GhostEvent::post_buy_submitted(
                tracked.pool_amm_id.clone(),
                tracked.base_mint.clone(),
                tracked.buy_signature.clone(),
                0.0,
                0,
                "live",
                0,
                Some(slot_id),
                PostBuySource::Recovery,
                Some(wallet_position.amount),
                Some(wallet_position.amount),
                tracked.buy_landed_slot,
                tracked.creator_pubkey.clone(),
            ))
            .map_err(|error| {
                anyhow::anyhow!("failed to emit recovery PostBuySubmitted: {error}")
            })?;
        recovery_events += 1;
    }

    if recovery_events == 0 {
        return Ok(());
    }

    let expected_active = baseline_active + recovery_events;
    let deadline = tokio::time::Instant::now()
        + tokio::time::Duration::from_secs(STARTUP_HYDRATION_TIMEOUT_SECS);
    loop {
        let active_positions = position_limit_tracker.active_positions();
        if active_positions >= expected_active {
            info!(
                recovered_positions = recovery_events,
                active_positions, "Startup hydration completed"
            );
            return Ok(());
        }

        if tokio::time::Instant::now() >= deadline {
            bail!(
                "startup hydration timed out: expected {} active positions after recovery, observed {}",
                expected_active,
                active_positions
            );
        }

        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = parse_cli_args()?;
    let config_path = LauncherConfig::resolve_config_path(&cli.requested_config_path)
        .unwrap_or_else(|| cli.requested_config_path.clone());

    // Load configuration
    let mut config = if config_path.exists() {
        info!("Loading configuration from: {:?}", config_path);
        LauncherConfig::from_file(&config_path)?
    } else {
        warn!(
            "Configuration file not found at {:?}, using defaults",
            config_path
        );
        warn!("To create a default config file, run with --generate-config");

        // Check if user wants to generate config
        if cli.command == StartupCommand::GenerateConfig {
            let default_config = LauncherConfig::default();
            default_config.save_to_file(&config_path)?;
            println!("Default configuration saved to: {:?}", config_path);
            println!("Edit this file and restart the launcher.");
            return Ok(());
        }

        if cli.command == StartupCommand::Preflight {
            bail!(
                "preflight requires an existing config file; not found at {}",
                config_path.display()
            );
        }

        LauncherConfig::default()
    };

    let legacy_warnings = config.legacy_config_warnings();

    // If the launcher is started under external GUI control (standalone `gui-backend`),
    // disable the embedded GUI component to avoid port conflicts and to keep the UI alive
    // when stopping/restarting the pipeline.
    if matches!(
        std::env::var("GHOST_GUI_BACKEND_DISABLED").as_deref(),
        Ok("1") | Ok("true") | Ok("TRUE") | Ok("yes") | Ok("YES")
    ) {
        config.gui_backend.enabled = false;
        info!("GUI Backend component disabled via GHOST_GUI_BACKEND_DISABLED");
    }

    // Initialize logging — guards must live until end of main to flush buffers on exit
    let _log_guards = init_logging(&config)?;

    configure_rpc_http_auth_from_secret_env(&config, &config_path)?;

    // ── CONFIG FINGERPRINT (always, single INFO line) ───────────────
    config.log_config_fingerprint();

    let startup_hydration_ignore_mints = load_startup_hydration_ignore_mints(&config_path)?;
    if !startup_hydration_ignore_mints.is_empty() {
        let ignored_mints = startup_hydration_ignore_mints
            .iter()
            .map(ToString::to_string)
            .collect::<Vec<_>>()
            .join(",");
        warn!(
            env = STARTUP_HYDRATION_IGNORE_MINTS_ENV,
            ignored_count = startup_hydration_ignore_mints.len(),
            ignored_mints = %ignored_mints,
            "Startup hydration will ignore explicit wallet positions"
        );
    }

    if cli.command == StartupCommand::Preflight {
        run_preflight(&config, &config_path).await?;
        return Ok(());
    }

    // ── FAIL-FAST: validate startup config before starting components ──
    if let Err(reason) = config.validate_grpc_config() {
        error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
        std::process::exit(1);
    }
    if let Err(reason) = config.validate_execution_profile() {
        error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
        std::process::exit(1);
    }
    if let Err(err) = ensure_parent_directory_writable(
        Path::new(&config.execution.shadow.entry_log_path),
        "shadow entry log output",
    ) {
        error!(
            "STARTUP ARTIFACT CHECK FAILED: {} — exiting with code 1",
            err
        );
        std::process::exit(1);
    }
    if let Some(lifecycle_log_path) = effective_shadow_lifecycle_log_path(&config) {
        if let Err(err) =
            ensure_parent_directory_writable(&lifecycle_log_path, "shadow lifecycle log output")
        {
            error!(
                "STARTUP ARTIFACT CHECK FAILED: {} — exiting with code 1",
                err
            );
            std::process::exit(1);
        }
    }
    if requires_live_sender(&config) {
        let sender_endpoint = resolve_live_sender_endpoint();
        if let Err(reason) = probe_sender_endpoint(&sender_endpoint).await {
            error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
            std::process::exit(1);
        }
        if let Some(priority_fee_rpc_url) = config
            .seer
            .helius_endpoint
            .as_deref()
            .map(str::trim)
            .filter(|value| !value.is_empty())
        {
            if let Err(reason) = probe_priority_fee_rpc(priority_fee_rpc_url).await {
                error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
                std::process::exit(1);
            }
        }
    }
    let durability = match config.resolve_durability_config() {
        Ok(durability) => durability,
        Err(err) => {
            error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", err);
            std::process::exit(1);
        }
    };
    if let Some(snapshot_dir) = durability.snapshot_dir() {
        if let Err(err) = ensure_directory_writable(snapshot_dir, "snapshot") {
            error!(
                "STARTUP DURABILITY CHECK FAILED: {} — exiting with code 1",
                err
            );
            std::process::exit(1);
        }
    }
    if let Some(wal_dir) = durability.wal_dir() {
        if let Err(err) = ensure_directory_writable(wal_dir, "WAL") {
            error!(
                "STARTUP DURABILITY CHECK FAILED: {} — exiting with code 1",
                err
            );
            std::process::exit(1);
        }
    }
    log_runtime_durability(&durability);

    // ── PROMETHEUS METRICS: jednorazowa rejestracja + serwer ────────
    if let Err(e) = oracle_metrics::register_oracle_metrics(prometheus::default_registry()) {
        warn!(
            "Oracle metrics registration failed (already registered?): {}",
            e
        );
    }
    if config.metrics.enabled {
        let metrics_bind = config.metrics.bind.clone();
        let metrics_port = config.metrics.port;
        tokio::spawn(async move {
            if let Err(e) = start_metrics_server(&metrics_bind, metrics_port).await {
                error!("Metrics server error: {}", e);
            }
        });
        info!(
            "Prometheus /metrics server started on {}:{}",
            config.metrics.bind, config.metrics.port
        );
    }

    // Propagate Ghost Brain config path for modules that load config directly
    std::env::set_var("GHOST_BRAIN_CONFIG_PATH", &config.ghost_brain_config_path);

    for warning in legacy_warnings {
        warn!("{warning}");
    }

    // ========================================
    // Load full Ghost Brain config for non-Gatekeeper analytical modules.
    // Gatekeeper V2 itself is loaded separately from [gatekeeper_v2] so edits
    // to that section remain effective even when other sections are invalid.
    // ========================================
    let brain_config_path = PathBuf::from(&config.ghost_brain_config_path);
    if brain_config_path.exists() {
        info!(
            "🧠 Loading Ghost Brain configuration from: {:?}",
            brain_config_path
        );
        match GhostBrainConfig::from_toml_file(&brain_config_path) {
            Ok(_) => info!("✅ Full Ghost Brain config loaded successfully"),
            Err(e) => {
                warn!(
                    "⚠️ Failed to load full Ghost Brain config from {:?}: {}. Gatekeeper V2 will still be loaded directly from [gatekeeper_v2]; other analytical modules fall back to defaults.",
                    brain_config_path,
                    e
                );
            }
        }
    } else {
        warn!(
            "⚠️ Ghost Brain config file not found at {:?}. Using internal defaults.",
            brain_config_path
        );
    }

    // Print banner
    print_banner(&config);

    // Create shutdown channel
    let (shutdown_tx, _) = broadcast::channel::<()>(1);

    // ========================================
    // EVENT BUS CREATION (Issue Criptocopenhaegen/ghost#156: Increased buffer to 10,240)
    // ========================================
    // Create unified event bus (Nervous System)
    // Buffer increased to 10,240 to handle Solana high-frequency data
    // and prevent RecvError::Lagged under load
    let (event_bus_tx, event_bus_rx) = create_event_bus();
    info!("🧠 Unified Memory Bus initialized (buffer: 10,240 events)");

    // ========================================
    // RUNTIME HEALTH (SSOT instance for watchdog + heartbeats)
    // ========================================
    let health = RuntimeHealth::new();
    info!("🩺 RuntimeHealth initialized (SSOT for watchdog heartbeats)");

    // ========================================
    // SYNCHRONIZATION CHANNEL (Issue Criptocopenhaegen/ghost#156: Race condition fix)
    // ========================================
    // Oracle Runtime will signal readiness via this channel
    // Main thread waits for signal before starting Seer to ensure no events are lost
    let (oracle_ready_tx, oracle_ready_rx) = oneshot::channel::<()>();
    info!("✅ Synchronization channel created for startup ordering");

    // Load GatekeeperV2 config early (ghost_brain_config not yet available — file-only load).
    // Will be re-synced after ghost_brain_config is loaded below.
    let mut gatekeeper_v2_config =
        load_gatekeeper_v2_config(&config.ghost_brain_config_path, None)?;
    sync_legacy_gatekeeper_aliases(&mut config, &gatekeeper_v2_config);
    if let Err(reason) = config.validate_gatekeeper_runtime_contract(&gatekeeper_v2_config) {
        error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
        std::process::exit(1);
    }

    // SnapshotEngine inactive buffer TTL must follow GatekeeperV2 hard window,
    // not legacy [gatekeeper] observation_window_ms.
    let snapshot_gatekeeper_window_ms = gatekeeper_v2_config.max_wait_time_ms;

    // ── Recovery: resolve durability startup profile from config/env ────────
    let snapshot_dir_opt = durability.snapshot_dir().map(PathBuf::from);

    // Initialize ShadowLedger — restored from disk snapshot if available.
    // MUST be created BEFORE SnapshotEngine to enable snapshot synchronization.
    let (shadow_ledger_init, snapshot_watermark_ms) = if let Some(ref dir) = snapshot_dir_opt {
        match ShadowLedger::restore_from_disk(dir) {
            Ok((ledger, stats)) => {
                info!(
                    dir = %dir.display(),
                    curves = stats.curves_loaded,
                    elapsed_ms = stats.elapsed_ms,
                    watermark_ms = stats.written_at_ms,
                    "ShadowLedger restored from disk snapshot"
                );
                metrics::histogram!("shadow_ledger_restore_duration_ms", stats.elapsed_ms as f64);
                (ledger, Some(stats.written_at_ms))
            }
            Err(e) => {
                warn!(error = %e, "ShadowLedger restore failed, starting fresh");
                (ShadowLedger::new(), None)
            }
        }
    } else {
        info!("Snapshot durability disabled — ShadowLedger starts fresh");
        (ShadowLedger::new(), None)
    };
    let shadow_ledger = Arc::new(shadow_ledger_init);

    // Create LivePipeline for post-commit live transaction processing (EPIC 4)
    let live_pipeline = Arc::new(ghost_core::shadow_ledger::LivePipeline::with_config(
        config.live_pipeline.to_core_config(),
    ));
    info!("💧 LivePipeline initialized (for post-commit snapshot appending)");

    let shared_wal = init_optional_wal(&durability)?;

    // Create SnapshotEngine (centralized market state management)
    // Capacity: 128 snapshots per pool, Interval: 200ms between snapshots
    let mut snapshot_engine_mut = SnapshotEngine::new(128, 200);
    let inactive_ttl_ms =
        snapshot_gatekeeper_window_ms.saturating_add(config.snapshot_inactive_tx_ttl_margin_ms);
    snapshot_engine_mut.set_inactive_tx_buffer_policy(
        config.snapshot_inactive_tx_buffer_capacity,
        inactive_ttl_ms,
    );
    info!(
        "📦 SnapshotEngine inactive buffer configured (cap={}, ttl_ms={}, window_ms={}, margin_ms={})",
        config.snapshot_inactive_tx_buffer_capacity,
        inactive_ttl_ms,
        snapshot_gatekeeper_window_ms,
        config.snapshot_inactive_tx_ttl_margin_ms
    );

    // ========== CRITICAL FIX: Connect ShadowLedger to SnapshotEngine ==========
    // This enables real-time snapshot synchronization from SnapshotEngine to ShadowLedger
    // so that PredictionEngine can access live, evolving market data across S1-S12 cycles.
    // Without this connection, scoring would be "frozen" on initial transfusion snapshots.
    snapshot_engine_mut.set_shadow_ledger(Arc::clone(&shadow_ledger));
    info!("🔗 ShadowLedger connected to SnapshotEngine for real-time synchronization");
    // ========== End CRITICAL FIX ==========

    let snapshot_engine = Arc::new(snapshot_engine_mut);
    info!("📸 SnapshotEngine initialized (capacity: 128, interval: 200ms)");

    // Initialize TuningService for background weight optimization
    // This service uses bandit algorithms to adapt signal weights based on trading outcomes
    let tuning_service_config = TuningServiceConfig {
        bandit_update_interval_secs: 180,    // 3 minutes
        bayesian_check_interval_secs: 43200, // 12 hours
        min_outcomes_for_update: 5,
        max_historical_outcomes: 10000,
        min_historical_for_bayesian: 100,
        bandit_algorithm: BanditAlgorithm::LinUCB,
        enable_bayesian: true,
        dry_run: false, // Set to true for initial testing
    };
    let (tuning_service, tuning_weights_rx) = TuningService::new(tuning_service_config);
    let (tuning_tx, tuning_rx) = mpsc::channel::<TuningMessage>(1000);

    // Store tuning channel sender for use by other components
    let tuning_tx_clone = tuning_tx.clone();

    // Spawn TuningService as background task
    let tuning_shutdown_rx = shutdown_tx.subscribe();
    let tuning_handle = tokio::spawn(async move {
        let mut shutdown_rx = tuning_shutdown_rx;
        tokio::select! {
            _ = tuning_service.run(tuning_rx) => {
                info!("TuningService completed normally");
            }
            _ = shutdown_rx.recv() => {
                info!("TuningService received shutdown signal");
            }
        }
    });
    info!(
        "⚖️  TuningService spawned: algorithm=LinUCB, bandit_interval=180s, bayesian_interval=43200s"
    );

    // TODO: The weights receiver can be used by Oracle or other components
    // that need real-time weight updates. Wire this up when integrating with
    // HyperPredictionOracle for dynamic weight application during scoring.
    let _tuning_weights_rx = tuning_weights_rx;

    let mut handles = Vec::new();
    handles.push(("TuningService", tuning_handle));

    // ShadowLedger was already initialized above (before SnapshotEngine)
    // Connect it to the event bus for pool detection and transaction updates
    let mut shadow_bus_rx = event_bus_tx.subscribe();
    let mut shadow_shutdown_rx = shutdown_tx.subscribe();

    let shadow_handle = tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = shadow_shutdown_rx.recv() => {
                    info!("ShadowLedger listener shutting down");
                    break;
                }
                event = shadow_bus_rx.recv() => {
                    match event {
                        Ok(GhostEvent::NewPoolDetected(pool)) => {
                            let bonding_curve_result = pool.bonding_curve.parse::<solana_sdk::pubkey::Pubkey>();
                            let base_mint_result = pool.base_mint.parse::<solana_sdk::pubkey::Pubkey>();

                            match (bonding_curve_result, base_mint_result) {
                                (Ok(bonding_curve_pubkey), Ok(base_mint_pubkey)) => debug!(
                                    "ShadowLedger listener observed NewPoolDetected without writing bootstrap; canonical bootstrap owned by Seer bonding_curve={} base_mint={}",
                                    bonding_curve_pubkey,
                                    base_mint_pubkey
                                ),
                                _ => {
                                    error!("❌ Invalid pubkeys - bonding_curve: {}, base_mint: {}",
                                        pool.bonding_curve, pool.base_mint);
                                }
                            }
                        }
                        Ok(GhostEvent::PoolTransaction(tx)) => {
                            // EPIC 2: Removed destructive set_snapshots with trim to 5.
                            //
                            // Previously this task would:
                            // 1. Clone the last snapshot and update tx_count/volume
                            // 2. Truncate history to 5 snapshots (destructive)
                            // 3. Overwrite with set_snapshots (resets all history)
                            //
                            // This violated single-writer architecture and destroyed
                            // canonical TX history needed for scoring across 12 cycles.
                            //
                            // Canonical snapshot flow is now:
                            // - Gatekeeper atomically commits sorted history to ShadowLedger
                            // - SnapshotEngine appends live snapshots after commit
                            //
                            // Orderflow-only updates without price/reserve context are
                            // insufficient for scoring and must not pollute canonical history.
                            if let Some(ref mint_str) = tx.token_mint {
                                debug!(
                                    "PoolTransaction received for {} (volume={:.4} SOL) - \
                                    snapshot update delegated to canonical writer (SnapshotEngine)",
                                    mint_str,
                                    tx.volume_sol
                                );
                            }
                        }
                        Ok(GhostEvent::GeyserTransaction { .. }) => {
                            // Skip cache updates without canonical base_mint context
                        }
                        _ => {}
                    }
                }
            }
        }
    });
    handles.push(("ShadowLedger", shadow_handle));

    // Load Ghost Brain configuration from TOML file
    // This config controls all analytical modules (SSMI, QASS, QEDD, MCI, etc.)
    let ghost_brain_config = {
        let config_path = std::path::Path::new(&config.ghost_brain_config_path);
        if config_path.exists() {
            match ghost_brain::config::GhostBrainConfig::from_toml_file(config_path) {
                Ok(cfg) => {
                    info!("🧠 Ghost Brain config loaded from: {:?}", config_path);
                    info!(
                        "   QEDD: lambda_base={}, abort_threshold={}",
                        cfg.qedd.lambda_base, cfg.qedd.lambda_abort_threshold
                    );
                    info!(
                        "   MCI: w_dc={}, w_sc={}, abort_threshold={}",
                        cfg.mci.weight_dc, cfg.mci.weight_sc, cfg.mci.coherence_abort_threshold
                    );
                    if let Some(initial) = &cfg.mci.initial_state {
                        info!(
                            "   MCI initial_state: base_sentiment={}, volatility_index={}, force_override={}",
                            initial.base_sentiment, initial.volatility_index, initial.force_override
                        );
                    }
                    info!(
                        "   Confidence: high={}, medium={}",
                        cfg.confidence.threshold_high, cfg.confidence.threshold_medium
                    );
                    Some(cfg)
                }
                Err(e) => {
                    warn!(
                        "⚠️  Failed to load full Ghost Brain config from {:?}: {}. Gatekeeper V2 will still use direct [gatekeeper_v2] loading; other modules use defaults.",
                        config_path,
                        e
                    );
                    None
                }
            }
        } else {
            warn!(
                "⚠️  Ghost Brain config not found at {:?}. Using defaults.",
                config_path
            );
            None
        }
    };

    // Re-load gatekeeper_v2_config now that ghost_brain_config is available (fallback path).
    gatekeeper_v2_config =
        load_gatekeeper_v2_config(&config.ghost_brain_config_path, ghost_brain_config.as_ref())?;
    sync_legacy_gatekeeper_aliases(&mut config, &gatekeeper_v2_config);
    if let Err(reason) = config.validate_gatekeeper_runtime_contract(&gatekeeper_v2_config) {
        error!("CONFIG VALIDATION FAILED: {} — exiting with code 1", reason);
        std::process::exit(1);
    }
    let gatekeeper_v3_config = ghost_brain_config
        .as_ref()
        .map(|cfg| cfg.gatekeeper_v3.clone())
        .unwrap_or_default();
    info!(
        "🧪 Gatekeeper V3 sidecar config: enabled={} shadow_emit={} policy_version={} materialization_version={} hash={}",
        gatekeeper_v3_config.enabled,
        gatekeeper_v3_config.shadow_emit_enabled,
        gatekeeper_v3_config.policy_version,
        gatekeeper_v3_config.materialization_version,
        gatekeeper_v3_config.v3_policy_config_hash(),
    );

    // Link Ghost Brain decision thresholds into Launcher pipeline config
    if let Some(ref cfg) = ghost_brain_config {
        let linked_threshold = cfg.confidence.high_threshold_points();
        config.oracle.pipeline.combined_score_threshold = linked_threshold;
        info!(
            "🔗 Config Linked: Launcher threshold set to {} from Ghost Brain config",
            linked_threshold
        );

        // Propagate engine cycle duration to runtime via env for PredictionSession
        env::set_var(
            "GHOST_ENGINE_CYCLE_MS",
            cfg.engine.cycle_duration_ms.to_string(),
        );
        info!(
            "🔁 Engine cycle duration set to {} ms from Ghost Brain config",
            cfg.engine.cycle_duration_ms
        );
    }

    // Create HyperPrediction Oracle (unified sub-2s token evaluation)
    use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;
    let hyper_prediction_oracle = Arc::new(if let Some(ref cfg) = ghost_brain_config {
        let threshold = cfg.confidence.high_threshold_points();
        // Use config-based threshold from confidence settings
        info!(
            "🔮 HyperPrediction Oracle initialized with config (threshold={})",
            threshold
        );
        let oracle = HyperPredictionOracle::new_with_config(threshold, cfg);

        // Log key threshold configurations for diagnostic purposes
        info!("📊 Followup Scoring Thresholds:");
        info!(
            "   - MCI drop threshold: {:.2}",
            oracle
                .hyper_prediction_config
                .followup_scoring
                .mci_drop_threshold
        );
        info!(
            "   - QEDD survival drop: {:.0}%",
            oracle
                .hyper_prediction_config
                .followup_scoring
                .qedd_survival_drop_pct
                * 100.0
        );
        info!(
            "   - Penalties enabled: {}",
            oracle
                .hyper_prediction_config
                .followup_scoring
                .enable_followup_penalties
        );

        info!("🎯 Survivor Score Thresholds:");
        info!(
            "   - Min survival: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .min_survival_threshold
        );
        info!(
            "   - Min quality: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .min_quality_threshold
        );
        info!(
            "   - Min LIGMA: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .min_ligma_threshold
        );
        info!(
            "   - Wash trading threshold: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .wash_trading_threshold
        );
        info!(
            "   - MESA wash severe: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .mesa_wash_severe
        );
        info!(
            "   - MESA wash elevated: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .mesa_wash_elevated
        );
        info!(
            "   - Wallet quality threshold: {:.2}",
            oracle
                .hyper_prediction_config
                .survivor_thresholds
                .wallet_quality_threshold
        );

        info!("⚠️ Risk Multipliers:");
        info!(
            "   - Exit signal weight: {:.2}",
            oracle
                .hyper_prediction_config
                .risk_multipliers
                .exit_signal_weight
        );
        info!(
            "   - Crash risk factor: {:.2}",
            oracle
                .hyper_prediction_config
                .risk_multipliers
                .crash_risk_factor
        );
        info!(
            "   - Anomaly penalty factor: {:.2}",
            oracle
                .hyper_prediction_config
                .risk_multipliers
                .anomaly_penalty_factor
        );
        info!(
            "   - Wallet quality multiplier: {:.2}",
            oracle
                .hyper_prediction_config
                .risk_multipliers
                .wallet_quality_multiplier
        );
        info!(
            "   - Wash penalty multiplier: {:.2}",
            oracle
                .hyper_prediction_config
                .risk_multipliers
                .wash_penalty_multiplier
        );

        info!("🔧 Orchestrator Thresholds:");
        info!(
            "   - Cabal risk threshold: {:.2}",
            oracle
                .hyper_prediction_config
                .orchestrator_thresholds
                .cabal_risk_threshold
        );
        info!(
            "   - MESA bot interpretation: {:.2}",
            oracle
                .hyper_prediction_config
                .orchestrator_thresholds
                .mesa_interpretation_bot_threshold
        );
        info!(
            "   - MESA organic interpretation: {:.2}",
            oracle
                .hyper_prediction_config
                .orchestrator_thresholds
                .mesa_interpretation_organic_threshold
        );

        oracle
    } else {
        info!("🔮 HyperPrediction Oracle initialized with defaults");
        info!("⚠️ Using default thresholds (see CONFIG_REFERENCE.md for details)");
        HyperPredictionOracle::default()
    });

    // Create Oracle Runtime (per-pool state management and scoring coordination)
    use oracle_runtime::OracleRuntime;
    let mut oracle_runtime_config =
        oracle_runtime::OracleRuntimeConfig::from_shadow_ledger_config(&config.shadow_ledger);
    oracle_runtime_config.session = config.session.clone();
    oracle_runtime_config.tx_intelligence = config.tx_intelligence.clone();
    oracle_runtime_config.p37_shadow_probe = config.p37_shadow_probe.clone();
    oracle_runtime_config.run_id = (!config.p37_shadow_probe.run_id.trim().is_empty())
        .then(|| config.p37_shadow_probe.run_id.clone());
    oracle_runtime_config.session_id = (!config.p37_shadow_probe.session_id.trim().is_empty())
        .then(|| config.p37_shadow_probe.session_id.clone());
    oracle_runtime_config.brain_config_path = Some(config.ghost_brain_config_path.clone());
    oracle_runtime_config.brain_config_hash = match std::fs::read(&config.ghost_brain_config_path) {
        Ok(bytes) => Some(blake3::hash(&bytes).to_hex().to_string()),
        Err(err) => {
            warn!(
                path = %config.ghost_brain_config_path,
                error = %err,
                "GHOST_BRAIN_CONFIG_FILE_HASH_UNAVAILABLE"
            );
            None
        }
    };
    let mut oracle_runtime_builder = OracleRuntime::new_with_config(
        hyper_prediction_oracle.clone(),
        config.seer.pump_program_id.clone(),
        config.seer.bonk_program_id.clone(),
        Arc::clone(&shadow_ledger),
        Some(Arc::new(new_async_rpc_client(
            config.seer.rpc_endpoint.clone(),
        ))),
        None, // paradox_rx (will be set later)
        Arc::clone(&live_pipeline),
        oracle_runtime_config,
    );
    if let Some(wal) = shared_wal.as_ref() {
        oracle_runtime_builder = oracle_runtime_builder.with_wal(Arc::clone(wal));
    }
    let oracle_runtime = Arc::new(oracle_runtime_builder);
    oracle_runtime.configure_orphan_adoption(
        config.oracle.orphan_grace_period_multiplier,
        config.oracle.max_orphans_adopted_on_register,
    );
    info!("⚡ Oracle Runtime initialized with LivePipeline");

    // ── Recovery: WAL replay from snapshot watermark ─────────────────────────
    let recovery_mode = match (snapshot_watermark_ms, shared_wal.as_ref()) {
        (Some(_), Some(wal)) => {
            let t0 = std::time::Instant::now();
            match wal_recovery::replay_shared_wal(wal, &oracle_runtime, snapshot_watermark_ms) {
                Ok(summary) => {
                    let elapsed_ms = t0.elapsed().as_millis() as u64;
                    info!(
                        total = summary.total_records,
                        skipped = summary.skipped_by_watermark,
                        committed = summary.committed_pools_restored,
                        staged = summary.staged_commits_restored,
                        live_trades = summary.live_trades_replayed,
                        curve_updates = summary.curve_updates_restored,
                        elapsed_ms,
                        "WAL replay complete (snapshot+WAL mode)"
                    );
                    metrics::histogram!("wal_replay_duration_ms", elapsed_ms as f64);
                    "snapshot_plus_wal"
                }
                Err(e) => {
                    warn!(error = %e, "WAL replay failed, continuing with snapshot state only");
                    "snapshot_only"
                }
            }
        }
        (None, Some(wal)) => {
            let t0 = std::time::Instant::now();
            match wal_recovery::replay_shared_wal(wal, &oracle_runtime, None) {
                Ok(summary) => {
                    let elapsed_ms = t0.elapsed().as_millis() as u64;
                    info!(
                        total = summary.total_records,
                        committed = summary.committed_pools_restored,
                        elapsed_ms,
                        "WAL replay complete (WAL-only mode, no snapshot)"
                    );
                    metrics::histogram!("wal_replay_duration_ms", elapsed_ms as f64);
                    "wal_only"
                }
                Err(e) => {
                    warn!(error = %e, "WAL replay failed");
                    "cold_start"
                }
            }
        }
        (Some(_), None) => "snapshot_only",
        (None, None) => "cold_start",
    };
    metrics::counter!("runtime_recovery_mode", 1u64, "mode" => recovery_mode);
    // Snapshot watermark: wall-clock ms at which the restored snapshot was written.
    // Plan calls this `runtime_recovery_watermark_slot`; the snapshot header stores
    // `written_at_ms` (not a Solana slot), so the gauge is emitted as
    // `runtime_recovery_watermark_ms` to avoid semantic confusion.
    if let Some(wm_ms) = snapshot_watermark_ms {
        metrics::gauge!("runtime_recovery_watermark_ms", wm_ms as f64);
    }
    info!(mode = recovery_mode, "Runtime recovery complete");

    // ── Periodic ShadowLedger snapshot task ──────────────────────────────────
    if let Some(ref dir) = snapshot_dir_opt {
        let snapshot_ledger = Arc::clone(&shadow_ledger);
        let snapshot_dir_clone = dir.clone();
        let interval_s = durability.snapshot_interval_s;
        let snapshot_keep_n = 3usize;
        let mut snapshot_shutdown_rx = shutdown_tx.subscribe();
        let snapshot_handle = tokio::spawn(async move {
            let mut ticker = tokio::time::interval(Duration::from_secs(interval_s));
            ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
            loop {
                tokio::select! {
                    _ = ticker.tick() => {
                        match snapshot_ledger.snapshot_to_disk(&snapshot_dir_clone) {
                            Ok(stats) => {
                                let _ = ShadowLedger::rotate_snapshots(&snapshot_dir_clone, snapshot_keep_n);
                                metrics::counter!("shadow_ledger_periodic_snapshot_total", 1u64, "result" => "ok");
                                debug!(elapsed_ms = stats.elapsed_ms, "Periodic ShadowLedger snapshot written");
                            }
                            Err(e) => {
                                warn!(error = %e, "Periodic snapshot failed");
                                metrics::counter!("shadow_ledger_periodic_snapshot_total", 1u64, "result" => "error");
                            }
                        }
                    }
                    _ = snapshot_shutdown_rx.recv() => break,
                }
            }
        });
        handles.push(("PeriodicSnapshotTask", snapshot_handle));
        info!(interval_s, dir = %dir.display(), "Periodic ShadowLedger snapshot task spawned");
    }

    // Wire canonical approved pool registry into SnapshotEngine and ShadowLedger
    let approved_pools = oracle_runtime.approved_pools();
    let pool_identities = oracle_runtime.pool_identity_registry();
    snapshot_engine.set_approved_pools(approved_pools.clone());
    shadow_ledger.set_approval_checker(Arc::new({
        let registry = approved_pools.clone();
        let identities = pool_identities.clone();
        move |base_mint: &Pubkey| {
            identities
                .get_by_base_mint(base_mint)
                .map(|identity| registry.is_approved(&identity.pool_id))
                .unwrap_or(false)
        }
    }));

    // Create channel for Paradox sensor state receiver
    let (paradox_tx, paradox_rx_oneshot) = tokio::sync::oneshot::channel();

    // Start all enabled components

    // ========================================================================
    // CRITICAL: Subscribe to event bus BEFORE starting any event producers!
    // ========================================================================
    // Oracle Runtime must subscribe BEFORE Seer starts emitting NewPoolDetected
    // events. tokio::sync::broadcast does NOT buffer events for future subscribers.
    // If we subscribe after Seer starts, we'll miss early pool detection events,
    // causing Gatekeeper to have no GatekeeperBuffer for those pools.
    info!("Subscribing Oracle Runtime to event bus BEFORE starting event producers...");
    let oracle_runtime_rx = event_bus_tx.subscribe();
    let position_limit_tracker =
        ghost_launcher::components::trigger::safety::PositionLimitTracker::new(
            config.trigger.max_concurrent_positions,
        );

    // ========================================================================
    // PostBuyRuntime: Subscribe BEFORE any producers start (prevents event loss).
    // Uses the original event_bus_rx receiver + fresh subscription for PostBuy.
    // ========================================================================
    let post_buy_rx = event_bus_tx.subscribe();
    let post_buy_shutdown_rx = shutdown_tx.subscribe();
    let (post_buy_direct_tx, post_buy_direct_rx) =
        ghost_launcher::components::post_buy_runtime::create_direct_post_buy_handoff_channel();

    let live_tx_sender = build_live_tx_sender(&config)
        .context("failed to initialize live BUY/SELL Sender transport")?;

    // Build LiveSellHandle for live-lane positions. Live BUY/SELL transport is fail-closed on
    // Helius Sender + Yellowstone, so startup must not silently downgrade to paper or disable the path.
    let live_sell_handle = build_live_sell_handle(
        &config,
        live_tx_sender.clone(),
        Arc::clone(&shadow_ledger),
        Arc::clone(oracle_runtime.account_state_core()),
    )
    .context("failed to initialize live SELL Sender transport")?;
    let live_position_registry = LivePositionRegistry::new(
        PathBuf::from(&config.execution.events.output_dir).join("live_positions.jsonl"),
    );
    let shadow_lifecycle_log_path = effective_shadow_lifecycle_log_path(&config);
    let probe_lifecycle_log_path = config
        .p37_shadow_probe
        .enabled
        .then(|| PathBuf::from(&config.p37_shadow_probe.lifecycle_log_path));

    let post_buy_config = ghost_launcher::components::post_buy_runtime::PostBuyRuntimeConfig {
        events_output_path: PathBuf::from(&config.execution.events.output_dir),
        paper_fill_delay_min_ms: config.execution.paper.fill_delay_ms_min,
        paper_fill_delay_max_ms: config.execution.paper.fill_delay_ms_max,
        tick_interval_ms: 500,
        max_ticks_before_exit: 240,
        execution_mode: format!("{:?}", config.execution.execution_mode).to_lowercase(),
        aem_t_s: 120,
        max_concurrent_positions: config.trigger.max_concurrent_positions,
        position_limit_tracker: Some(position_limit_tracker.clone()),
        live_sell: live_sell_handle,
        live_position_registry: Some(live_position_registry.clone()),
        slippage_tolerance: config.trigger.slippage_tolerance,
        live_exit_take_profit_pct: config.trigger.live_exit_take_profit_pct,
        live_exit_stop_loss_pct: config.trigger.live_exit_stop_loss_pct,
        shadow_ledger: Some(Arc::clone(&shadow_ledger)),
        account_state_core: Some(Arc::clone(oracle_runtime.account_state_core())),
        shadow_lifecycle_log_path,
        probe_lifecycle_log_path,
    };
    let hydration_live_sell_handle = post_buy_config.live_sell.clone();
    let post_buy_handle = tokio::spawn(async move {
        ghost_launcher::components::post_buy_runtime::run(
            post_buy_rx,
            post_buy_shutdown_rx,
            Some(post_buy_direct_rx),
            post_buy_config,
        )
        .await;
    });
    handles.push(("PostBuyRuntime", post_buy_handle));
    // Keep the original event_bus_rx alive to prevent channel closure
    let _event_bus_rx_guard = event_bus_rx;
    info!(
        "✅ PostBuyRuntime subscribed (receivers: {})",
        event_bus_tx.receiver_count()
    );

    hydrate_startup_live_positions(
        &event_bus_tx,
        &position_limit_tracker,
        &hydration_live_sell_handle,
        &live_position_registry,
        &startup_hydration_ignore_mints,
    )
    .await
    .context("startup hydration failed")?;

    // ========================================
    // ORACLE ACTOR: Start FIRST with readiness signal (Issue #156: Race condition fix)
    // ========================================
    info!("Starting Oracle Runtime Task (PRIORITY: First to guarantee subscription)...");
    let oracle_runtime_clone = Arc::clone(&oracle_runtime);
    let oracle_snapshot_engine = Arc::clone(&snapshot_engine);
    let oracle_event_tx = event_bus_tx.clone();
    let analysis_window_ms = 2000; // 2 seconds analysis window (configurable via config in future)

    // ========================================================================
    // GATEKEEPER V2 CONFIG was already loaded above from the canonical
    // [gatekeeper_v2] section. At this stage we only overlay the launcher-owned
    // Phase-5 curve-quality policy from top-level [shadow_ledger].
    // ========================================================================

    if gatekeeper_v2_config.curve_wait_ms != config.shadow_ledger.curve_wait_ms
        || gatekeeper_v2_config.curve_require_for_buy != config.shadow_ledger.curve_require_for_buy
        || gatekeeper_v2_config.stale_fallback != config.shadow_ledger.stale_fallback
    {
        info!(
            "🛡️ Phase-5 policy sync: Gatekeeper curve policy overridden by [shadow_ledger] SSOT (wait_ms {} -> {}, require {} -> {}, stale {:?} -> {:?})",
            gatekeeper_v2_config.curve_wait_ms,
            config.shadow_ledger.curve_wait_ms,
            gatekeeper_v2_config.curve_require_for_buy,
            config.shadow_ledger.curve_require_for_buy,
            gatekeeper_v2_config.stale_fallback,
            config.shadow_ledger.stale_fallback,
        );
    }
    gatekeeper_v2_config.curve_wait_ms = config.shadow_ledger.curve_wait_ms;
    gatekeeper_v2_config.curve_require_for_buy = config.shadow_ledger.curve_require_for_buy;
    gatekeeper_v2_config.stale_fallback = config.shadow_ledger.stale_fallback;
    // ========================================================================

    // ========================================================================
    // IWIM Veto Gate Config — loaded from [iwim_veto_gate] in ghost_brain_config.toml
    // Falls back to ghost_brain_config.iwim_veto_gate if TOML section missing.
    // ========================================================================
    let iwim_veto_config: ghost_brain::config::IwimVetoGateConfig = ghost_brain_config
        .as_ref()
        .map(|c| c.iwim_veto_gate.clone())
        .unwrap_or_default();
    gatekeeper_v2_config.iwim_veto_strong_margin = iwim_veto_config.strong_margin;
    gatekeeper_v2_config.iwim_veto_strong_max_manip_flags =
        iwim_veto_config.strong_max_manipulation_flags;
    info!(
        "🛡️ IWIM Veto Gate CONFIG: enabled={} mode={:?} max_wait_ms={} min_conf={:.2} min_tx_pp={} rug_thr={:.2} sybil_thr={:.2} organic_floor={:.2} strong_margin={} strong_max_manip_flags={}",
        iwim_veto_config.enabled,
        iwim_veto_config.mode,
        iwim_veto_config.max_wait_ms,
        iwim_veto_config.min_confidence,
        iwim_veto_config.min_tx_pp,
        iwim_veto_config.rug_threat_threshold,
        iwim_veto_config.sybil_threshold,
        iwim_veto_config.organic_floor,
        iwim_veto_config.strong_margin,
        iwim_veto_config.strong_max_manipulation_flags,
    );
    // ========================================================================

    // Create TriggerComponent for live fire execution
    let trigger_component = if config.trigger.enabled {
        let trigger =
            TriggerComponent::new_with_position_limit_tracker_and_runtime_state_and_sender(
                config.trigger.clone(),
                position_limit_tracker.clone(),
                Arc::clone(&shadow_ledger),
                Arc::clone(oracle_runtime.account_state_core()),
                live_tx_sender.clone(),
            );
        info!(
            "🔫 TriggerComponent initialized (execution_mode: {:?}, entry_mode: {:?})",
            config.execution.execution_mode,
            trigger.entry_mode(),
        );
        Some(Arc::new(trigger))
    } else {
        info!("🔫 TriggerComponent disabled by config");
        None
    };

    // Clone oracle config values before they're moved into the async block.
    // `oracle.dry_run` is a legacy flag; the runtime lane must also honor the
    // production execution profile so paper rollouts emit paper-lane events.
    let oracle_dry_run = runtime_oracle_dry_run(&config);
    // PR-3: authoritative FSC availability is fail-closed on startup and can be
    // promoted only by the dedicated full-chain funding-lane control plane.
    let (authoritative_funding_stream_tx, authoritative_funding_stream_rx) = if config.seer.enabled
    {
        let (tx, rx) = watch::channel(false);
        (Some(tx), Some(rx))
    } else {
        (None, None)
    };
    let initial_authoritative_funding_stream_available = authoritative_funding_stream_rx
        .as_ref()
        .map(|rx| *rx.borrow())
        .unwrap_or(false);
    let canonical_account_update_relay_enabled = if config.account_state_core.enable {
        if !config.oracle.canonical_account_update_relay_enabled {
            warn!(
                "oracle.canonical_account_update_relay_enabled=false is ignored when account_state_core.enable=true; production runtime requires canonical account updates"
            );
        }
        true
    } else {
        if config.oracle.canonical_account_update_relay_enabled {
            warn!(
                "oracle.canonical_account_update_relay_enabled=true has no effect when account_state_core.enable=false; degraded/test startup remains tx/bootstrap-only because canonical ingest is owned by AccountStateCore enablement"
            );
        }
        false
    };
    let oracle_decision_log_path = config.oracle.decision_log_path.clone();
    let oracle_events_output_dir = config.execution.events.output_dir.clone();
    let oracle_health = Arc::clone(&health);
    let oracle_authoritative_funding_stream_rx = authoritative_funding_stream_rx;
    let oracle_authoritative_funding_coverage_gate_enabled =
        config.seer.enabled && matches!(config.seer.funding_lane_mode.as_str(), "full_chain");

    let mut oracle_handle = tokio::spawn(async move {
        info!("📡 Oracle Runtime initializing...");

        // CRITICAL: Signal readiness BEFORE entering main event loop
        // This ensures Oracle is ready to receive events before Seer starts sending
        if let Err(e) = oracle_ready_tx.send(()) {
            error!("❌ Failed to signal Oracle Runtime readiness: {:?}", e);
            return;
        }

        info!("🟢 Oracle Runtime ready - subscribed to event bus, entering main loop");

        oracle_runtime::start_oracle_runtime_task_with_funding_availability(
            oracle_runtime_rx,
            oracle_runtime_clone,
            oracle_snapshot_engine,
            oracle_event_tx,
            Some(post_buy_direct_tx),
            analysis_window_ms,
            gatekeeper_v2_config,
            gatekeeper_v3_config,
            iwim_veto_config,
            config.execution.execution_mode,
            oracle_dry_run,
            oracle_decision_log_path,
            config.execution.shadow.entry_log_path.clone(),
            config.execution.shadow.lifecycle_log_path.clone(),
            trigger_component,
            oracle_events_output_dir,
            Some(oracle_health),
            canonical_account_update_relay_enabled,
            initial_authoritative_funding_stream_available,
            oracle_authoritative_funding_coverage_gate_enabled,
            oracle_authoritative_funding_stream_rx,
        )
        .await;
    });

    // ========================================
    // SYNCHRONIZATION BARRIER (Issue #156: Wait for Oracle readiness)
    // ========================================
    info!("⏳ Waiting for Oracle Runtime to initialize...");

    match tokio::time::timeout(Duration::from_secs(30), oracle_ready_rx).await {
        Ok(Ok(())) => {
            info!("✅ Oracle Runtime ready signal received");
        }
        Ok(Err(e)) => {
            error!("❌ Oracle Runtime failed to signal readiness: {:?}", e);
            return Err(anyhow::anyhow!("Oracle Runtime initialization failed"));
        }
        Err(_) => {
            error!("❌ Timeout waiting for Oracle Runtime readiness (30s)");
            return Err(anyhow::anyhow!("Oracle Runtime initialization timeout"));
        }
    }

    info!("🚀 Proceeding with event producer startup (Seer, Trigger, etc.)...");

    // Start Seer component with event bus and SnapshotEngine
    if config.seer.enabled {
        info!("Starting Seer component...");
        let seer_config = config.seer.clone();
        let shutdown_rx = shutdown_tx.subscribe();
        let seer_event_tx = event_bus_tx.clone();
        let seer_snapshot_engine = Arc::clone(&snapshot_engine);
        let oracle_runtime_for_paradox = Arc::clone(&oracle_runtime);
        let seer_shadow_ledger = Arc::clone(&shadow_ledger);
        let seer_wal = shared_wal.clone();
        let seer_health = Arc::clone(&health);
        let seer_authoritative_funding_stream_tx = authoritative_funding_stream_tx.clone();

        let handle = tokio::spawn(async move {
            if let Err(e) = ghost_launcher::components::seer::run(
                seer_config,
                shutdown_rx,
                Some(seer_event_tx),
                Some(seer_snapshot_engine),
                Some(seer_shadow_ledger),
                seer_wal,
                Some(paradox_tx),
                Some(seer_health),
                seer_authoritative_funding_stream_tx,
                canonical_account_update_relay_enabled,
            )
            .await
            {
                error!("Seer component error: {}", e);
            }
        });
        handles.push(("Seer", handle));

        // Spawn a task to wait for and connect the Paradox receiver
        tokio::spawn(async move {
            if let Ok(paradox_rx) = paradox_rx_oneshot.await {
                oracle_runtime_for_paradox.set_paradox_receiver(paradox_rx);
                info!("🔮 Paradox Sensor connected to Oracle Runtime");
            } else {
                warn!("🔮 Failed to receive Paradox Sensor state receiver");
            }
        });
    } else {
        info!("Seer component disabled");
    }

    // Start Trigger component with event bus receiver and Oracle pipeline
    if config.trigger.enabled {
        info!("Starting Trigger component...");
        let trigger_config = config.trigger.clone();
        let mut oracle_config = config.oracle.clone();
        if let Some(ref cfg) = ghost_brain_config {
            oracle_config.ghost_brain_config = Some(cfg.clone());
        }
        let shutdown_rx = shutdown_tx.subscribe();
        let trigger_event_rx = event_bus_tx.subscribe();
        let trigger_event_tx = event_bus_tx.clone();
        let trigger_shadow_ledger = Arc::clone(&shadow_ledger);

        let handle = tokio::spawn(async move {
            if let Err(e) = ghost_launcher::components::trigger::run_with_oracle(
                trigger_config,
                oracle_config,
                trigger_shadow_ledger,
                shutdown_rx,
                Some(trigger_event_rx),
                Some(trigger_event_tx),
            )
            .await
            {
                error!("Trigger component error: {}", e);
            }
        });
        handles.push(("Trigger", handle));
    } else {
        info!("Trigger component disabled");
    }

    // Start GUI Backend component
    if config.gui_backend.enabled {
        info!("Starting GUI Backend component...");
        let gui_config = config.gui_backend.clone();
        let shutdown_rx = shutdown_tx.subscribe();

        let handle = tokio::spawn(async move {
            if let Err(e) =
                ghost_launcher::components::gui_backend::run(gui_config, shutdown_rx).await
            {
                error!("GUI Backend component error: {}", e);
            }
        });
        handles.push(("GUI Backend", handle));
    } else {
        info!("GUI Backend component disabled");
    }

    // Start SnapshotListener component (always enabled when SnapshotEngine exists)
    info!("Starting SnapshotListener component...");
    let snapshot_listener_rx = event_bus_tx.subscribe();
    let snapshot_listener_engine = Arc::clone(&snapshot_engine);
    let snapshot_listener_approved = Arc::clone(&approved_pools);
    let snapshot_listener_identities = Arc::clone(&pool_identities);
    let snapshot_listener_shutdown = shutdown_tx.subscribe();
    let snapshot_forward_mode = config.snapshot_listener_forward_mode;
    let snapshot_max_pools = config.snapshot_listener_max_pools;
    let snapshot_staging_config =
        ghost_launcher::components::snapshot_listener::SnapshotStagingConfig::new(
            config.snapshot_inactive_tx_buffer_capacity,
            inactive_ttl_ms,
            snapshot_max_pools,
        );

    let handle = tokio::spawn(async move {
        if let Err(e) = ghost_launcher::components::snapshot_listener::run(
            snapshot_listener_engine,
            snapshot_listener_approved,
            snapshot_listener_identities,
            snapshot_listener_shutdown,
            snapshot_listener_rx,
            snapshot_forward_mode,
            snapshot_max_pools,
            snapshot_staging_config,
            None, // ack_tx: test-only channel, not used in production
        )
        .await
        {
            error!("SnapshotListener component error: {}", e);
        }
    });
    handles.push(("SnapshotListener", handle));

    info!("Starting GatekeeperCommitLoop...");
    let gatekeeper_commit_runtime = Arc::clone(&oracle_runtime);
    let gatekeeper_commit_pipeline = Arc::clone(&live_pipeline);
    let gatekeeper_commit_ledger = Arc::clone(&shadow_ledger);
    let gatekeeper_commit_events = event_bus_tx.clone();
    let gatekeeper_commit_shutdown = shutdown_tx.subscribe();
    let gatekeeper_commit_config = GatekeeperCommitLoopConfig {
        check_interval_ms: config.gatekeeper.check_interval_ms,
    };
    let gatekeeper_commit_handle = tokio::spawn(async move {
        ghost_launcher::components::gatekeeper_commit_loop::run(
            gatekeeper_commit_runtime,
            gatekeeper_commit_pipeline,
            gatekeeper_commit_ledger,
            Some(gatekeeper_commit_events),
            gatekeeper_commit_shutdown,
            gatekeeper_commit_config,
        )
        .await;
    });
    handles.push(("GatekeeperCommitLoop", gatekeeper_commit_handle));

    // ========================================
    // EPIC 3/4: Background Task Loops
    // ========================================

    // Start LivePipelineFlushLoop (periodic flushes)
    info!("Starting LivePipelineFlushLoop...");
    let live_pipeline_flush_config =
        ghost_launcher::components::live_pipeline_flush_loop::LivePipelineFlushLoopConfig {
            flush_interval_ms: config.live_pipeline.flush_interval_ms,
        };
    let live_pipeline_flush_shutdown = shutdown_tx.subscribe();
    let live_pipeline_flush_clone = Arc::clone(&live_pipeline);
    let shadow_ledger_flush_clone = Arc::clone(&shadow_ledger);

    let live_pipeline_flush_handle = tokio::spawn(async move {
        ghost_launcher::components::live_pipeline_flush_loop::run(
            live_pipeline_flush_clone,
            shadow_ledger_flush_clone,
            live_pipeline_flush_shutdown,
            live_pipeline_flush_config,
        )
        .await;
    });
    handles.push(("LivePipelineFlushLoop", live_pipeline_flush_handle));

    // ========================================
    // WATCHDOG TASK: Periodic health-status logging + controlled exit on stall
    // ========================================
    // NOTE: config.rs applies the same canonical aliases ("grpc", "geyser_grpc", "g")
    // in effective_source_mode / validate_grpc_config.  Keep in sync.
    let is_grpc_mode = config
        .seer
        .source_mode
        .as_ref()
        .map(|m| {
            let m = m.to_lowercase();
            m == "geyser_grpc" || m == "grpc" || m == "g"
        })
        .unwrap_or(false);
    let watchdog_health = Arc::clone(&health);
    let watchdog_handle = tokio::spawn(async move {
        ghost_launcher::components::watchdog::run(watchdog_health, is_grpc_mode).await;
    });
    handles.push(("Watchdog", watchdog_handle));

    // ── STARTUP GUARD: gRPC subscribe-proof within 5 s ──────────────
    if is_grpc_mode {
        let guard_health = Arc::clone(&health);
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_secs(GRPC_SUBSCRIBE_TIMEOUT_SECS)).await;
            let sent_ts = guard_health
                .subscribe_sent_ts_ms
                .load(std::sync::atomic::Ordering::Relaxed);
            if sent_ts == 0 {
                error!(
                    "STARTUP GUARD: gRPC subscribe was NOT sent within {} s — exiting with code {}",
                    GRPC_SUBSCRIBE_TIMEOUT_SECS, EXIT_GRPC_SUBSCRIBE_TIMEOUT,
                );
                std::process::exit(EXIT_GRPC_SUBSCRIBE_TIMEOUT);
            }
        });
    }

    info!("All components started successfully");
    info!("Press Ctrl+C to shutdown...");

    // Wait for shutdown signal or fail fast if OracleRuntime stops unexpectedly.
    tokio::select! {
        signal_result = signal::ctrl_c() => {
            match signal_result {
                Ok(()) => {
                    info!("Shutdown signal received, stopping all components...");
                }
                Err(err) => {
                    error!("Error listening for shutdown signal: {}", err);
                }
            }
        }
        oracle_result = &mut oracle_handle => {
            match oracle_result {
                Ok(()) => {
                    error!(
                        "Oracle Runtime task stopped before shutdown signal; exiting with code {} to avoid silent decision stall",
                        EXIT_ORACLE_RUNTIME_STOPPED
                    );
                }
                Err(err) => {
                    error!(
                        error = %err,
                        "Oracle Runtime task failed before shutdown signal; exiting with code {} to avoid silent decision stall",
                        EXIT_ORACLE_RUNTIME_STOPPED
                    );
                }
            }
            std::process::exit(EXIT_ORACLE_RUNTIME_STOPPED);
        }
    }

    // Send shutdown message to TuningService
    if let Err(e) = tuning_tx_clone.send(TuningMessage::Shutdown).await {
        warn!("Error sending TuningService shutdown message: {}", e);
    }

    // Send shutdown signal to all components
    if let Err(e) = shutdown_tx.send(()) {
        warn!("Error sending shutdown signal: {}", e);
    }

    info!("Waiting for Oracle Runtime to shut down...");
    if let Err(e) = oracle_handle.await {
        error!("Oracle Runtime shutdown error: {}", e);
    } else {
        info!("Oracle Runtime shut down successfully");
    }

    // Wait for all components to shut down
    for (name, handle) in handles {
        info!("Waiting for {} to shut down...", name);
        if let Err(e) = handle.await {
            error!("{} shutdown error: {}", name, e);
        } else {
            info!("{} shut down successfully", name);
        }
    }

    info!("Ghost Launcher shutdown complete");
    Ok(())
}

/// Handle a single HTTP connection for the metrics server.
///
/// Routing:
/// - `GET /metrics`  → 200, Prometheus text format
/// - `GET /healthz`  → 200, "OK"
/// - anything else   → 404, "Not Found"
async fn handle_metrics_connection(mut stream: tokio::net::TcpStream) {
    use prometheus::{Encoder, TextEncoder};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};

    let mut buffer = vec![0u8; 4096];
    let n = match stream.read(&mut buffer).await {
        Ok(n) if n > 0 => n,
        _ => return,
    };

    let request = String::from_utf8_lossy(&buffer[..n]);
    let first_line = request.lines().next().unwrap_or("");

    let response: std::borrow::Cow<str> = if first_line.starts_with("GET /metrics") {
        let encoder = TextEncoder::new();
        let metric_families = prometheus::gather();
        let mut metrics_buf = Vec::new();
        if encoder.encode(&metric_families, &mut metrics_buf).is_err() {
            return;
        }
        let body = String::from_utf8_lossy(&metrics_buf).into_owned();
        format!(
            "HTTP/1.1 200 OK\r\nContent-Type: text/plain; version=0.0.4\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        )
        .into()
    } else if first_line.starts_with("GET /healthz") {
        "HTTP/1.1 200 OK\r\nContent-Type: text/plain\r\nContent-Length: 2\r\n\r\nOK".into()
    } else {
        "HTTP/1.1 404 Not Found\r\nContent-Type: text/plain\r\nContent-Length: 9\r\n\r\nNot Found"
            .into()
    };

    let _ = stream.write_all(response.as_bytes()).await;
    let _ = stream.flush().await;
}

/// Accept loop for the metrics server — drives `handle_metrics_connection`.
///
/// Accepts the already-bound `TcpListener` so it can be created externally
/// (e.g. with port 0 in tests).
async fn run_metrics_accept_loop(listener: tokio::net::TcpListener) -> anyhow::Result<()> {
    loop {
        let (stream, _) = listener.accept().await?;
        tokio::spawn(handle_metrics_connection(stream));
    }
}

/// Minimal Prometheus HTTP server — serwuje `GET /metrics` w formacie tekstu.
///
/// Zbiera metryki z domyślnego rejestru (`prometheus::gather()`), który zawiera
/// statyki zarejestrowane przez `oracle_metrics::register_oracle_metrics`.
/// Adres bind i port konfigurowane przez `[metrics]` sekcję config.toml.
async fn start_metrics_server(bind: &str, port: u16) -> anyhow::Result<()> {
    use tokio::net::TcpListener;

    let addr = format!("{}:{}", bind, port);
    let listener = TcpListener::bind(&addr).await?;
    info!(
        "Prometheus metrics server listening on http://{}/metrics",
        addr
    );
    run_metrics_accept_loop(listener).await
}

#[cfg(test)]
mod metrics_server_tests {
    use super::*;
    use ghost_launcher::oracle_metrics;
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    /// Smoke: start server on ephemeral port, GET /metrics → 200 with known series.
    #[tokio::test]
    async fn smoke_get_metrics_contains_oracle_series() {
        // Register oracle metrics into the default registry (idempotent across tests).
        let _ = oracle_metrics::register_oracle_metrics(prometheus::default_registry());

        // Seed a CounterVec observation so it appears in gather() output
        // (CounterVec with zero observations is omitted by prometheus).
        oracle_metrics::POOL_IDENTITY_PROMOTION_TOTAL
            .with_label_values(&["success"])
            .inc_by(0);

        // Bind on port 0 — OS assigns a free ephemeral port.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();

        tokio::spawn(run_metrics_accept_loop(listener));

        // Brief yield so the spawned task enters accept().
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .expect("connect to metrics server");

        stream
            .write_all(b"GET /metrics HTTP/1.0\r\n\r\n")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();

        assert!(
            response.contains("200 OK"),
            "expected HTTP 200, got: {}",
            &response[..80.min(response.len())]
        );
        // IntCounter (no labels) — always present after registration
        assert!(
            response.contains("pool_identity_exhausted_total"),
            "missing pool_identity_exhausted_total"
        );
        // IntGauge (no labels) — always present after registration
        assert!(
            response.contains("shadow_ledger_committed_pools"),
            "missing shadow_ledger_committed_pools"
        );
        // IntCounterVec — present because seeded above
        assert!(
            response.contains("pool_identity_promotion_attempts_total"),
            "missing pool_identity_promotion_attempts_total"
        );
    }

    /// Non-metrics path must return 404.
    #[tokio::test]
    async fn get_unknown_path_returns_404() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(run_metrics_accept_loop(listener));
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /foobar HTTP/1.0\r\n\r\n")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(
            response.contains("404 Not Found"),
            "expected 404, got: {}",
            &response[..80.min(response.len())]
        );
    }

    /// /healthz must return 200 OK.
    #[tokio::test]
    async fn get_healthz_returns_200() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let port = listener.local_addr().unwrap().port();
        tokio::spawn(run_metrics_accept_loop(listener));
        tokio::time::sleep(std::time::Duration::from_millis(5)).await;

        let mut stream = tokio::net::TcpStream::connect(format!("127.0.0.1:{}", port))
            .await
            .unwrap();
        stream
            .write_all(b"GET /healthz HTTP/1.0\r\n\r\n")
            .await
            .unwrap();
        stream.shutdown().await.unwrap();

        let mut response = String::new();
        stream.read_to_string(&mut response).await.unwrap();
        assert!(response.contains("200 OK"));
        assert!(response.ends_with("OK"));
    }

    #[test]
    fn runtime_oracle_dry_run_enables_paper_lane_for_paper_execution_mode() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ghost_launcher::config::ExecutionMode::Paper;
        config.oracle.dry_run = false;

        assert!(runtime_oracle_dry_run(&config));
    }

    #[test]
    fn runtime_oracle_dry_run_enables_shadow_lane_for_shadow_execution_mode() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ghost_launcher::config::ExecutionMode::Shadow;
        config.oracle.dry_run = false;

        assert!(runtime_oracle_dry_run(&config));
    }

    #[test]
    fn runtime_oracle_dry_run_stays_false_for_live_execution_without_legacy_flag() {
        let mut config = LauncherConfig::default();
        config.execution.execution_mode = ghost_launcher::config::ExecutionMode::Live;
        config.oracle.dry_run = false;

        assert!(!runtime_oracle_dry_run(&config));
    }
}

/// Initialize logging based on configuration.
/// Returns `WorkerGuard`s that MUST be held for the entire program lifetime.
/// Dropping them early flushes the background writer thread and stops log output.
fn init_logging(
    config: &LauncherConfig,
) -> Result<Vec<tracing_appender::non_blocking::WorkerGuard>> {
    let log_level = config.logging.level.as_str();

    // Base environment filter for all logs
    let base_filter = EnvFilter::try_from_default_env()
        .or_else(|_| EnvFilter::try_new(log_level))
        .unwrap_or_else(|_| EnvFilter::new("info"));

    // Create filters for different targets
    // Oracle filter: only ghost_brain::oracle and ghost_launcher::oracle_runtime targets
    let oracle_filter = EnvFilter::try_from_default_env()
        .or_else(|_| {
            EnvFilter::try_new(format!(
                "ghost_brain={},ghost_launcher::oracle_runtime={}",
                log_level, log_level
            ))
        })
        .unwrap_or_else(|_| EnvFilter::new("ghost_brain=info,ghost_launcher::oracle_runtime=info"));

    // System filter: Use a FilterFn to exclude Oracle targets
    // This is a proper exclusion filter that checks the target name
    let system_filter_fn = FilterFn::new(|metadata| {
        let target = metadata.target();
        !target.starts_with("ghost_brain") && !target.starts_with("ghost_launcher::oracle_runtime")
    });

    let mut layers: Vec<Box<dyn Layer<_> + Send + Sync>> = Vec::new();
    let mut guards: Vec<tracing_appender::non_blocking::WorkerGuard> = Vec::new();

    // 1. Console layer (if enabled) - shows all logs with ANSI colors
    if config.logging.console_enabled {
        let console_layer = if config.logging.json_format {
            tracing_subscriber::fmt::layer()
                .json()
                .with_filter(base_filter.clone())
                .boxed()
        } else {
            tracing_subscriber::fmt::layer()
                .with_target(true)
                .with_filter(base_filter.clone())
                .boxed()
        };
        layers.push(console_layer);
    }

    // 2. Oracle decision log file (if enabled)
    if config.logging.oracle_log_enabled {
        let oracle_log_path = PathBuf::from(&config.logging.oracle_log_path);
        if let Some(parent) = oracle_log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let oracle_file_appender = tracing_appender::rolling::daily(
            oracle_log_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
            oracle_log_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("oracle_decision.log"),
        );

        let (oracle_non_blocking, oracle_guard) =
            tracing_appender::non_blocking(oracle_file_appender);

        guards.push(oracle_guard);

        let oracle_layer = if config.logging.oracle_json_format {
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(oracle_non_blocking)
                .with_filter(oracle_filter)
                .boxed()
        } else {
            tracing_subscriber::fmt::layer()
                .event_format(OracleDecisionFormatter::new(false))
                .with_writer(oracle_non_blocking)
                .with_ansi(false)
                .with_filter(oracle_filter)
                .boxed()
        };

        layers.push(oracle_layer);
    }

    // 3. System log file (if enabled) - EXCLUDES oracle logs via FilterFn
    if config.logging.file_enabled {
        let log_path = PathBuf::from(&config.logging.file_path);
        if let Some(parent) = log_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let file_appender = tracing_appender::rolling::daily(
            log_path
                .parent()
                .unwrap_or_else(|| std::path::Path::new(".")),
            log_path
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("system.log"),
        );

        let (non_blocking, guard) = tracing_appender::non_blocking(file_appender);

        guards.push(guard);

        // Combine base filter with exclusion filter
        let combined_system_filter = base_filter.clone().and(system_filter_fn);

        // System layer with filter to exclude oracle targets
        let system_layer = if config.logging.json_format {
            tracing_subscriber::fmt::layer()
                .json()
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(combined_system_filter)
                .boxed()
        } else {
            tracing_subscriber::fmt::layer()
                .event_format(StandardFormatter::new(false))
                .with_writer(non_blocking)
                .with_ansi(false)
                .with_filter(combined_system_filter)
                .boxed()
        };

        layers.push(system_layer);
    }

    tracing_subscriber::registry().with(layers).init();

    Ok(guards)
}

/// Print startup banner
fn print_banner(config: &LauncherConfig) {
    println!("\n╔════════════════════════════════════════════════════════════╗");
    println!(
        "║           Ghost Launcher v{}                          ║",
        VERSION
    );
    println!("║  Integrated Standalone Application for Solana Trading     ║");
    println!("╚════════════════════════════════════════════════════════════╝\n");

    println!("Configuration:");
    println!("  Mode: {:?}", config.mode);
    println!("  Seer: {}", if config.seer.enabled { "✓" } else { "✗" });
    println!(
        "  Trigger: {}",
        if config.trigger.enabled { "✓" } else { "✗" }
    );
    println!(
        "  GUI Backend: {}",
        if config.gui_backend.enabled {
            "✓"
        } else {
            "✗"
        }
    );

    if config.mode == AppMode::Production {
        println!("\n⚠️  PRODUCTION MODE - Trading with real funds!");
    } else {
        println!("\n🧪 TEST MODE - Using devnet/testnet");
    }

    if config.gui_backend.enabled {
        println!(
            "\n🌐 GUI Backend available at: http://{}:{}",
            config.gui_backend.bind_address, config.gui_backend.port
        );
    }

    if config.logging.file_enabled {
        println!("📝 Logs: {}", config.logging.file_path);
    }

    println!();
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::pin::Pin;
    use std::sync::{Mutex, OnceLock};
    use tempfile::{tempdir, NamedTempFile};
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio_stream::wrappers::{ReceiverStream, TcpListenerStream};
    use yellowstone_grpc_proto::prelude::{
        geyser_server::{Geyser, GeyserServer},
        GetBlockHeightRequest, GetBlockHeightResponse, GetLatestBlockhashRequest,
        GetLatestBlockhashResponse, GetSlotRequest, GetSlotResponse, GetVersionRequest,
        GetVersionResponse, IsBlockhashValidRequest, IsBlockhashValidResponse, PingRequest,
        PongResponse, SubscribeRequest, SubscribeUpdate,
    };
    use yellowstone_grpc_proto::tonic::{transport::Server, Request, Response, Status, Streaming};

    static ENV_LOCK: OnceLock<Mutex<()>> = OnceLock::new();

    struct EnvVarGuard {
        key: &'static str,
        previous: Option<String>,
    }

    impl EnvVarGuard {
        fn set(key: &'static str, value: &str) -> Self {
            let previous = std::env::var(key).ok();
            std::env::set_var(key, value);
            Self { key, previous }
        }
    }

    impl Drop for EnvVarGuard {
        fn drop(&mut self) {
            match &self.previous {
                Some(value) => std::env::set_var(self.key, value),
                None => std::env::remove_var(self.key),
            }
        }
    }

    #[test]
    fn test_load_gatekeeper_v2_config_prefers_gatekeeper_v2() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_launcher_gk2_window_{ts}.toml"));
        let toml = r#"
version = 10

[gatekeeper_v2]
max_wait_time_ms = 3333
"#;
        std::fs::write(&path, toml).expect("write temp gatekeeper_v2 toml");

        let mut cfg = LauncherConfig::default();
        cfg.gatekeeper.observation_window_ms = 777;
        cfg.ghost_brain_config_path = path.to_string_lossy().to_string();

        let gatekeeper_v2 = load_gatekeeper_v2_config(&cfg.ghost_brain_config_path, None)
            .expect("load gatekeeper_v2");
        assert_eq!(gatekeeper_v2.max_wait_time_ms, 3333);
        assert!(
            gatekeeper_v2.use_three_layer_decision,
            "missing field should inherit the Phase 2 feature-driven default"
        );
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_load_gatekeeper_v2_config_rejects_invalid_gatekeeper_section() {
        let ts = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos();
        let path = std::env::temp_dir().join(format!("ghost_launcher_invalid_gk2_{ts}.toml"));
        let toml = r#"
version = 10

[gatekeeper_v2]
min_consecutive_buys = 1.0
"#;
        std::fs::write(&path, toml).expect("write invalid gatekeeper_v2 toml");

        let err = load_gatekeeper_v2_config(&path.to_string_lossy(), None)
            .expect_err("invalid gatekeeper_v2 must fail closed");
        assert!(err
            .to_string()
            .contains("refusing to start Gatekeeper V2 with built-in defaults"));
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn test_sync_legacy_gatekeeper_aliases_uses_gatekeeper_v2_values() {
        let mut cfg = LauncherConfig::default();
        cfg.gatekeeper.min_tx_to_pass = 5;
        cfg.gatekeeper.observation_window_ms = 777;

        let gatekeeper_v2 = GatekeeperV2Config {
            min_tx_count: 26,
            max_wait_time_ms: 8_000,
            ..GatekeeperV2Config::default()
        };

        sync_legacy_gatekeeper_aliases(&mut cfg, &gatekeeper_v2);

        assert_eq!(cfg.gatekeeper.min_tx_to_pass, 26);
        assert_eq!(cfg.gatekeeper.observation_window_ms, 8_000);
    }

    #[test]
    fn test_pr8_startup_derives_canonical_account_update_relay_from_account_state_core() {
        let source = include_str!("main.rs");
        let startup_start = source
            .find(
                "let canonical_account_update_relay_enabled = if config.account_state_core.enable {",
            )
            .expect("canonical_account_update_relay_enabled startup block must exist");
        let startup_end = source[startup_start..]
            .find("let oracle_decision_log_path =")
            .map(|offset| startup_start + offset)
            .expect("oracle_decision_log_path must follow account update startup block");
        let startup_src = &source[startup_start..startup_end];

        assert!(
            startup_src.contains(
                "let canonical_account_update_relay_enabled = if config.account_state_core.enable {"
            ),
            "startup must derive canonical_account_update_relay_enabled from AccountStateCore enablement"
        );
        assert!(
            startup_src.contains(
                "oracle.canonical_account_update_relay_enabled=false is ignored when account_state_core.enable=true"
            ),
            "startup must warn when the legacy disable switch is ignored"
        );
        assert!(
            !startup_src.contains(
                "config.account_state_core.enable && config.oracle.canonical_account_update_relay_enabled"
            ),
            "startup must not let the compatibility relay flag disable canonical production ingest"
        );
    }

    #[test]
    fn test_phase2_shipped_configs_keep_three_layer_gatekeeper_enabled() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        for relative in [
            "../configs/rollout/shadow-burnin.toml",
            "../configs/rollout/paper-burnin.toml",
            "../configs/rollout/dual-micro-live.toml",
            "../configs/rollout/future-live.toml",
        ] {
            let path = manifest_dir.join(relative);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|read_err| panic!("failed to read {}: {read_err}", path.display()));
            let is_shadow_burnin = relative == "../configs/rollout/shadow-burnin.toml";
            let is_paper_burnin = relative == "../configs/rollout/paper-burnin.toml";
            let is_shadow_only_profile = is_shadow_burnin || is_paper_burnin;
            let requires_helius_override = matches!(
                relative,
                "../configs/rollout/dual-micro-live.toml" | "../configs/rollout/future-live.toml"
            );
            if !is_shadow_only_profile {
                assert!(
                    source.contains("helius_endpoint"),
                    "{} must declare [seer].helius_endpoint explicitly",
                    path.display()
                );
                assert!(
                    source.contains("helius_endpoint = \"replace-me\""),
                    "{} must require env/.env override for seer.helius_endpoint",
                    path.display()
                );
            }
            let _env_lock = ENV_LOCK
                .get_or_init(|| Mutex::new(()))
                .lock()
                .expect("env lock poisoned");
            let empty_env = NamedTempFile::new().expect("empty env file");
            let _env_file = EnvVarGuard::set(
                "GHOST_ENV_FILE",
                empty_env.path().to_string_lossy().as_ref(),
            );
            let _unset_helius = EnvVarGuard::set("GHOST_SEER_HELIUS_ENDPOINT", "");
            let _unset_shadow_rpc = EnvVarGuard::set("GHOST_TRIGGER_SHADOW_RPC_URL", "");
            if requires_helius_override {
                let err = LauncherConfig::from_file(&path).expect_err(
                    "live rollout config should fail closed without Sender env overrides",
                );
                let err_text = err.to_string();
                assert!(
                    err_text.contains("helius_endpoint"),
                    "{} should fail because Helius placeholder was not overridden, got: {}",
                    path.display(),
                    err_text
                );
            } else if is_shadow_only_profile {
                let err = LauncherConfig::from_file(&path).expect_err(
                    "shadow-only rollout config should fail closed without shadow RPC override",
                );
                let err_text = err.to_string();
                assert!(
                    err_text.contains("shadow_rpc_url"),
                    "{} should fail because shadow RPC placeholder was not overridden, got: {}",
                    path.display(),
                    err_text
                );
            }
            let _shadow_rpc = if is_shadow_only_profile {
                Some(EnvVarGuard::set(
                    "GHOST_TRIGGER_SHADOW_RPC_URL",
                    "https://mainnet.helius-rpc.com/?api-key=test-shadow",
                ))
            } else {
                None
            };
            let _helius = if requires_helius_override {
                Some(EnvVarGuard::set(
                    "GHOST_SEER_HELIUS_ENDPOINT",
                    "https://mainnet.helius-rpc.com/?api-key=test",
                ))
            } else {
                None
            };
            let _grpc_token = if requires_helius_override {
                Some(EnvVarGuard::set(
                    "GHOST_SEER_GRPC_X_TOKEN",
                    "test-yellowstone-token",
                ))
            } else {
                None
            };
            let config = LauncherConfig::from_file(&path).unwrap_or_else(|err| {
                panic!("failed to load {} with env override: {err}", path.display())
            });
            let gatekeeper_v2 = load_gatekeeper_v2_config(&config.ghost_brain_config_path, None)
                .unwrap_or_else(|err| {
                    panic!(
                        "failed to load gatekeeper_v2 for {} ({}): {err}",
                        path.display(),
                        config.ghost_brain_config_path
                    )
                });

            assert!(
                gatekeeper_v2.use_three_layer_decision,
                "{} must keep the feature-driven Gatekeeper path enabled",
                path.display()
            );
            assert!(
                config
                    .validate_gatekeeper_runtime_contract(&gatekeeper_v2)
                    .is_ok(),
                "{} must satisfy the Phase 2 production Gatekeeper contract",
                path.display()
            );
            let expected_funding_lane_mode = if is_shadow_burnin {
                "disabled"
            } else if is_paper_burnin {
                "full_chain"
            } else {
                "disabled"
            };
            assert_eq!(
                config.seer.funding_lane_mode,
                expected_funding_lane_mode,
                "{} must keep the expected funding lane mode for this rollout profile",
                path.display()
            );
            assert_eq!(
                gatekeeper_v2.soft_penalty_high_fsc,
                0,
                "{} must keep FSC penalties inactive during PR-4 bake",
                path.display()
            );
            assert_eq!(
                gatekeeper_v2.soft_penalty_high_fsc_high_cpv_combo,
                0,
                "{} must keep FSC combo penalties inactive during PR-4 bake",
                path.display()
            );
            assert!(
                !gatekeeper_v2.enable_sybil_combo_veto,
                "{} must keep sybil combo veto disabled during PR-4 bake",
                path.display()
            );
        }
    }

    #[test]
    fn test_tracked_default_config_keeps_authoritative_funding_lane_disabled() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir.join("../config.toml");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|read_err| panic!("failed to read {}: {read_err}", path.display()));
        assert!(
            source.contains("funding_lane_mode = \"disabled\""),
            "{} must keep the authoritative funding lane disabled in the tracked default template",
            path.display()
        );
    }

    #[test]
    fn test_tracked_shadow_burnin_config_uses_primary_only_funding_mode() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir.join("../configs/rollout/shadow-burnin.toml");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|read_err| panic!("failed to read {}: {read_err}", path.display()));
        assert!(
            source.contains("funding_lane_mode = \"disabled\""),
            "{} must keep the canonical shadow burn-in command on the primary stream under single-stream provider constraints",
            path.display()
        );
        assert!(
            source.contains("payer_strategy = \"configured\""),
            "{} must use a funded, chain-visible configured payer for shadow simulation",
            path.display()
        );
    }

    #[test]
    fn test_tracked_paper_burnin_config_remains_legacy_shadow_compat_profile() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let path = manifest_dir.join("../configs/rollout/paper-burnin.toml");
        let source = std::fs::read_to_string(&path)
            .unwrap_or_else(|read_err| panic!("failed to read {}: {read_err}", path.display()));
        assert!(
            source.contains("execution_mode = \"paper\""),
            "{} must keep the legacy paper runtime for compatibility coverage",
            path.display()
        );
        assert!(
            source.contains("entry_mode = \"shadow_only\""),
            "{} must keep shadow-only entry semantics for legacy paper compatibility",
            path.display()
        );
    }

    #[test]
    fn test_load_startup_hydration_ignore_mints_parses_unique_pubkeys() {
        let _env_lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        let _ignore_mints = EnvVarGuard::set(
            STARTUP_HYDRATION_IGNORE_MINTS_ENV,
            "11111111111111111111111111111111, So11111111111111111111111111111111111111112, 11111111111111111111111111111111",
        );

        let parsed = load_startup_hydration_ignore_mints(Path::new("/tmp/ghost-launcher.toml"))
            .expect("ignore mint env var should parse");
        assert_eq!(parsed.len(), 2);
        assert!(parsed.contains(&"11111111111111111111111111111111".parse().unwrap()));
        assert!(parsed.contains(
            &"So11111111111111111111111111111111111111112"
                .parse()
                .unwrap()
        ));
    }

    #[test]
    fn test_load_startup_hydration_ignore_mints_rejects_invalid_pubkeys() {
        let _env_lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        let _ignore_mints = EnvVarGuard::set(STARTUP_HYDRATION_IGNORE_MINTS_ENV, "not-a-pubkey");

        let err = load_startup_hydration_ignore_mints(Path::new("/tmp/ghost-launcher.toml"))
            .expect_err("invalid ignore mint env var must fail closed");
        assert!(
            err.to_string().contains(STARTUP_HYDRATION_IGNORE_MINTS_ENV),
            "error should reference env var name, got: {err:#}"
        );
    }

    #[test]
    fn test_load_startup_hydration_ignore_mints_reads_dotenv_override() {
        let _env_lock = ENV_LOCK
            .get_or_init(|| Mutex::new(()))
            .lock()
            .expect("env lock poisoned");
        let temp_dir = tempdir().expect("temp dir");
        let config_path = temp_dir.path().join("config.toml");
        std::fs::write(&config_path, "mode = \"production\"\n").expect("write config");
        std::fs::write(
            temp_dir.path().join(".env"),
            format!(
                "{}=11111111111111111111111111111111,So11111111111111111111111111111111111111112\n",
                STARTUP_HYDRATION_IGNORE_MINTS_ENV
            ),
        )
        .expect("write dotenv");

        let parsed = load_startup_hydration_ignore_mints(&config_path)
            .expect("dotenv-backed ignore mint env var should parse");
        assert_eq!(parsed.len(), 2);
        assert!(parsed.contains(&"11111111111111111111111111111111".parse().unwrap()));
        assert!(parsed.contains(
            &"So11111111111111111111111111111111111111112"
                .parse()
                .unwrap()
        ));
    }

    #[test]
    fn test_phase6_shipped_configs_omit_legacy_account_update_switch_name() {
        let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
        let legacy_switch_name = ["account", "updates", "enabled"].join("_");
        for relative in [
            "../config.toml",
            "../configs/rollout/shadow-burnin.toml",
            "../configs/rollout/paper-burnin.toml",
            "../configs/rollout/dual-micro-live.toml",
            "../configs/rollout/future-live.toml",
        ] {
            let path = manifest_dir.join(relative);
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|err| panic!("failed to read {}: {err}", path.display()));
            assert!(
                !source.contains(&legacy_switch_name),
                "{} must not ship the legacy account update switch name",
                path.display()
            );
        }
    }

    #[test]
    fn test_ensure_directory_writable_rejects_file_path() {
        let file = NamedTempFile::new().expect("temp file");
        let err = ensure_directory_writable(file.path(), "wal").unwrap_err();
        assert!(err.to_string().contains("not a directory"));
    }

    fn write_test_keypair(path: &Path) {
        let keypair = solana_sdk::signature::Keypair::new();
        let bytes = keypair.to_bytes().to_vec();
        std::fs::write(path, serde_json::to_vec(&bytes).unwrap()).expect("write keypair");
    }

    async fn spawn_mock_rpc_server(balance_lamports: u64) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let addr = listener.local_addr().expect("rpc addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 8192];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request.contains("\"getVersion\"") {
                    r#"{"jsonrpc":"2.0","result":{"solana-core":"1.18.26","feature-set":1},"id":1}"#
                        .to_string()
                } else if request.contains("\"getBalance\"") {
                    format!(
                        "{{\"jsonrpc\":\"2.0\",\"result\":{{\"context\":{{\"slot\":1}},\"value\":{}}},\"id\":1}}",
                        balance_lamports
                    )
                } else {
                    r#"{"jsonrpc":"2.0","result":"ok","id":1}"#.to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}", addr)
    }

    async fn spawn_mock_jito_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind jito");
        let addr = listener.local_addr().expect("jito addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 8192];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let body = if request.contains("\"getTipAccounts\"") {
                    r#"{"jsonrpc":"2.0","result":["Tip111111111111111111111111111111111111111","Tip222222222222222222222222222222222222222"],"id":1}"#
                        .to_string()
                } else {
                    r#"{"jsonrpc":"2.0","result":"ok","id":1}"#.to_string()
                };
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}", addr)
    }

    async fn spawn_mock_jito_rate_limited_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rate-limited jito");
        let addr = listener.local_addr().expect("rate-limited jito addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 8192];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let (status_line, body) = if request.starts_with("POST /api/v1/bundles ")
                    && request.contains("\"getTipAccounts\"")
                {
                    (
                        "HTTP/1.1 429 Too Many Requests",
                        r#"{"jsonrpc":"2.0","error":{"code":-32097,"message":"Network congested. Endpoint is globally rate limited.","data":null},"id":null}"#
                            .to_string(),
                    )
                } else if request.starts_with("POST /api/v1/getInflightBundleStatuses?uuid=") {
                    (
                        "HTTP/1.1 200 OK",
                        r#"{"bundle_id":"test-bundle","status":"Pending","landed_slot":null,"reason":null}"#
                            .to_string(),
                    )
                } else {
                    (
                        "HTTP/1.1 404 Not Found",
                        r#"{"jsonrpc":"2.0","error":{"code":404,"message":"wrong path"},"id":1}"#
                            .to_string(),
                    )
                };
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}", addr)
    }

    async fn spawn_mock_sender_server() -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind sender");
        let addr = listener.local_addr().expect("sender addr");
        tokio::spawn(async move {
            loop {
                let Ok((mut stream, _)) = listener.accept().await else {
                    break;
                };
                let mut buffer = vec![0u8; 8192];
                let n = match stream.read(&mut buffer).await {
                    Ok(n) if n > 0 => n,
                    _ => continue,
                };
                let request = String::from_utf8_lossy(&buffer[..n]);
                let (status_line, body) = if request.starts_with("GET /ping ") {
                    ("HTTP/1.1 200 OK", "pong".to_string())
                } else if request.starts_with("POST /fast ")
                    && request.contains("\"sendTransaction\"")
                {
                    (
                        "HTTP/1.1 200 OK",
                        r#"{"jsonrpc":"2.0","result":"5f2GXvZ9szR67pG7SLzFea9mLFg6mqH8R1hR7yC7SXgMLvYzE8ApX2HFqRSbVSSMzdg3NofM8JrjYNewc19hXtod","id":"ghost-live-sender"}"#
                            .to_string(),
                    )
                } else {
                    (
                        "HTTP/1.1 404 Not Found",
                        r#"{"jsonrpc":"2.0","error":{"code":404,"message":"wrong path"},"id":1}"#
                            .to_string(),
                    )
                };
                let response = format!(
                    "{status_line}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{}",
                    body.len(),
                    body
                );
                let _ = stream.write_all(response.as_bytes()).await;
                let _ = stream.shutdown().await;
            }
        });
        format!("http://{}", addr)
    }

    #[derive(Clone)]
    struct MockGeyserService {
        expected_x_token: String,
        version: String,
    }

    impl MockGeyserService {
        fn assert_x_token<T>(&self, request: &Request<T>) -> std::result::Result<(), Status> {
            let actual = request
                .metadata()
                .get("x-token")
                .and_then(|value| value.to_str().ok());
            if actual == Some(self.expected_x_token.as_str()) {
                Ok(())
            } else {
                Err(Status::unauthenticated("missing or invalid x-token"))
            }
        }
    }

    #[yellowstone_grpc_proto::tonic::async_trait]
    impl Geyser for MockGeyserService {
        type SubscribeStream = Pin<
            Box<
                dyn tokio_stream::Stream<Item = std::result::Result<SubscribeUpdate, Status>>
                    + Send,
            >,
        >;

        async fn subscribe(
            &self,
            request: Request<Streaming<SubscribeRequest>>,
        ) -> std::result::Result<Response<Self::SubscribeStream>, Status> {
            self.assert_x_token(&request)?;
            let (_tx, rx) = tokio::sync::mpsc::channel(1);
            Ok(Response::new(Box::pin(ReceiverStream::new(rx))))
        }

        async fn ping(
            &self,
            request: Request<PingRequest>,
        ) -> std::result::Result<Response<PongResponse>, Status> {
            self.assert_x_token(&request)?;
            Ok(Response::new(PongResponse {
                count: request.into_inner().count,
            }))
        }

        async fn get_latest_blockhash(
            &self,
            request: Request<GetLatestBlockhashRequest>,
        ) -> std::result::Result<Response<GetLatestBlockhashResponse>, Status> {
            self.assert_x_token(&request)?;
            Err(Status::unimplemented("not needed in preflight test"))
        }

        async fn get_block_height(
            &self,
            request: Request<GetBlockHeightRequest>,
        ) -> std::result::Result<Response<GetBlockHeightResponse>, Status> {
            self.assert_x_token(&request)?;
            Err(Status::unimplemented("not needed in preflight test"))
        }

        async fn get_slot(
            &self,
            request: Request<GetSlotRequest>,
        ) -> std::result::Result<Response<GetSlotResponse>, Status> {
            self.assert_x_token(&request)?;
            Err(Status::unimplemented("not needed in preflight test"))
        }

        async fn is_blockhash_valid(
            &self,
            request: Request<IsBlockhashValidRequest>,
        ) -> std::result::Result<Response<IsBlockhashValidResponse>, Status> {
            self.assert_x_token(&request)?;
            Err(Status::unimplemented("not needed in preflight test"))
        }

        async fn get_version(
            &self,
            request: Request<GetVersionRequest>,
        ) -> std::result::Result<Response<GetVersionResponse>, Status> {
            self.assert_x_token(&request)?;
            Ok(Response::new(GetVersionResponse {
                version: self.version.clone(),
            }))
        }
    }

    async fn spawn_mock_grpc_server(expected_x_token: &str) -> String {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind grpc");
        let addr = listener.local_addr().expect("grpc addr");
        let service = MockGeyserService {
            expected_x_token: expected_x_token.to_string(),
            version: "mock-yellowstone-1.0".to_string(),
        };
        tokio::spawn(async move {
            Server::builder()
                .add_service(GeyserServer::new(service))
                .serve_with_incoming(TcpListenerStream::new(listener))
                .await
                .expect("serve grpc");
        });
        format!("http://{}", addr)
    }

    fn base_preflight_config(rpc_url: String, base_dir: &Path) -> LauncherConfig {
        let mut config = LauncherConfig::default();
        config.mode = AppMode::Test;
        config.ghost_brain_config_path = Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../ghost-brain/ghost_brain_config.toml")
            .to_string_lossy()
            .into_owned();
        config.seer.source_mode = Some("pump_portal_ws".to_string());
        config.trigger.rpc_url = rpc_url;
        config.trigger.shadow_run.enabled = true;
        config.metrics.enabled = false;
        config.logging.file_path = base_dir
            .join("logs/system.log")
            .to_string_lossy()
            .into_owned();
        config.logging.oracle_log_path = base_dir
            .join("logs/oracle.log")
            .to_string_lossy()
            .into_owned();
        config.execution.events.output_dir = base_dir
            .join("datasets/events")
            .to_string_lossy()
            .into_owned();
        config.execution.shadow.entry_log_path = base_dir
            .join("logs/shadow_run/shadow_entries.jsonl")
            .to_string_lossy()
            .into_owned();
        config.oracle.decision_log_path = base_dir
            .join("logs/decisions")
            .to_string_lossy()
            .into_owned();
        config.trigger.shadow_run.output_path = base_dir
            .join("logs/shadow_run/buys.jsonl")
            .to_string_lossy()
            .into_owned();
        config
    }

    async fn configure_live_sender_preflight(config: &mut LauncherConfig) -> EnvVarGuard {
        let grpc_endpoint = spawn_mock_grpc_server("token").await;
        let sender_endpoint = spawn_mock_sender_server().await;
        config.seer.source_mode = Some("grpc".to_string());
        config.seer.grpc_endpoint = grpc_endpoint;
        config.seer.grpc_x_token = Some("token".to_string());
        config.seer.helius_endpoint = Some(config.trigger.rpc_url.clone());
        EnvVarGuard::set(
            "GHOST_HELIUS_SENDER_ENDPOINT",
            &format!("{sender_endpoint}/fast"),
        )
    }

    #[tokio::test]
    async fn test_run_preflight_reports_missing_keypair() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let base_dir = tempdir().expect("base dir");
        let config = base_preflight_config(rpc_url, base_dir.path());

        let err = run_preflight(&config, Path::new("test-config.toml"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("trigger.keypair"));
    }

    #[tokio::test]
    async fn test_run_preflight_reports_missing_helius_endpoint() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        config.seer.helius_endpoint = None;
        let keypair_dir = tempdir().expect("keypair dir");
        let keypair_path = keypair_dir.path().join("id.json");
        write_test_keypair(&keypair_path);
        config.trigger.keypair_path = Some(keypair_path.to_string_lossy().into_owned());

        let err = run_preflight(&config, Path::new("test-config.toml"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("seer.helius_endpoint"));
    }

    #[tokio::test]
    async fn test_run_preflight_rejects_missing_grpc_token() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        config.seer.grpc_x_token = None;
        let keypair_dir = tempdir().expect("keypair dir");
        let keypair_path = keypair_dir.path().join("id.json");
        write_test_keypair(&keypair_path);
        config.trigger.keypair_path = Some(keypair_path.to_string_lossy().into_owned());

        let err = run_preflight(&config, Path::new("test-config.toml"))
            .await
            .expect_err("missing Yellowstone x-token must fail preflight");
        assert!(err.to_string().contains("grpc_x_token"));
    }

    #[tokio::test]
    async fn test_run_preflight_reports_unwritable_wal_path() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        let file = NamedTempFile::new().expect("wal file");
        let snapshot_dir = tempdir().expect("snapshot dir");
        config.durability.wal_dir = Some(file.path().to_path_buf());
        config.durability.snapshot_dir = Some(snapshot_dir.path().to_path_buf());

        let err = run_preflight(&config, Path::new("test-config.toml"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("durability.wal_dir"));
    }

    #[tokio::test]
    async fn test_run_preflight_reports_unwritable_artifact_paths() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind rpc");
        let rpc_url = format!("http://{}", listener.local_addr().unwrap());
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        let decision_log_file = NamedTempFile::new().expect("decision log file");
        let shadow_entry_parent_file = NamedTempFile::new().expect("shadow entry parent file");
        let shadow_parent_file = NamedTempFile::new().expect("shadow parent file");
        config.oracle.decision_log_path = decision_log_file.path().to_string_lossy().into_owned();
        config.execution.shadow.entry_log_path = shadow_entry_parent_file
            .path()
            .join("shadow_entries.jsonl")
            .to_string_lossy()
            .into_owned();
        config.trigger.shadow_run.output_path = shadow_parent_file
            .path()
            .join("shadow.jsonl")
            .to_string_lossy()
            .into_owned();

        let err = run_preflight(&config, Path::new("artifact-config.toml"))
            .await
            .unwrap_err();
        let message = err.to_string();
        assert!(message.contains("oracle.decision_log_dir"));
        assert!(message.contains("execution.shadow.entry_log_dir"));
        assert!(message.contains("trigger.shadow_run_dir"));
    }

    #[tokio::test]
    async fn test_run_preflight_happy_path() {
        let rpc_url = spawn_mock_rpc_server(1_000_000_000).await;
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        let keypair_path = base_dir.path().join("id.json");
        write_test_keypair(&keypair_path);
        config.trigger.keypair_path = Some(keypair_path.to_string_lossy().into_owned());
        config.trigger.max_position_size_sol = 0.00001;
        config.trigger.emergency_floor_sol = 0.05;
        config.trigger.position_size_buffer_sol = 0.02;

        run_preflight(&config, Path::new("happy-config.toml"))
            .await
            .expect("happy-path preflight should pass");
    }

    #[tokio::test]
    async fn test_run_preflight_accepts_reachable_sender_endpoint() {
        let rpc_url = spawn_mock_rpc_server(1_000_000_000).await;
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let _sender_env = configure_live_sender_preflight(&mut config).await;
        let keypair_path = base_dir.path().join("id.json");
        write_test_keypair(&keypair_path);
        config.trigger.keypair_path = Some(keypair_path.to_string_lossy().into_owned());
        config.trigger.max_position_size_sol = 0.00001;
        config.trigger.emergency_floor_sol = 0.05;
        config.trigger.position_size_buffer_sol = 0.02;

        run_preflight(&config, Path::new("sender-ok.toml"))
            .await
            .expect("reachable Sender endpoint should pass preflight");
    }

    #[tokio::test]
    async fn test_run_preflight_reports_inconsistent_execution_profile() {
        let rpc_url = spawn_mock_rpc_server(1_000_000_000).await;
        let base_dir = tempdir().expect("base dir");
        let mut config = base_preflight_config(rpc_url, base_dir.path());
        let keypair_path = base_dir.path().join("id.json");
        write_test_keypair(&keypair_path);
        config.trigger.keypair_path = Some(keypair_path.to_string_lossy().into_owned());
        config.execution.execution_mode = ghost_launcher::config::ExecutionMode::Paper;
        config.trigger.entry_mode = ghost_launcher::config::TriggerEntryMode::Live;

        let err = run_preflight(&config, Path::new("invalid-config.toml"))
            .await
            .unwrap_err();
        assert!(err.to_string().contains("execution_profile"));
    }
}
