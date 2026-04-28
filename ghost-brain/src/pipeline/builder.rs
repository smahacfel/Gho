//! Pipeline Builder - Initialization and Configuration
//!
//! This module handles:
//! - E2EPipeline construction
//! - Keypair loading from filesystem
//! - Leader Predictor initialization
//! - Jito Bundle Executor setup
//! - GUI state initialization
//! - Ghost Intelligence components initialization

use anyhow::{bail, Context, Result};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::sync::RwLock;
use tracing::{info, warn};

use crate::aem::{AemRuntime, JsonlAemLedger};
use crate::config::E2EConfig;
use crate::config::GhostBrainConfig;
use crate::events::{EventEmitter, EventWriterConfig};
use crate::execution::{ExecutionMode, Lane};
use crate::guardian::post_buy::{
    MonitoringEngine, PositionRuntimeRouter, PostBuyGuardianConfig, ShadowPositionBook,
    SignalRouter,
};
use crate::jito_bundle::JitoBundleExecutor;
use crate::leader_predictor::LeaderPredictor;
use crate::metrics::E2EMetrics;
use crate::oracle::{
    ClusterHunter, ClusterHunterConfig, DevProfiler, DevProfilerConfig, HyperOracle,
    SnapshotEngine, VisionCritic, VisionCriticConfig,
};

use gui_backend::AppState;
use solana_sdk::signer::keypair::Keypair;
use trigger::{PanicExecutor, Revolver};

use super::E2EPipeline;

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

impl E2EPipeline {
    /// Create a new E2E pipeline
    pub fn new(config: E2EConfig) -> Result<Self> {
        // Load keypairs
        let authority = Self::load_keypair(&config.authority_keypair_path)
            .context("Failed to load authority keypair")?;
        let payer = Self::load_keypair(&config.payer_keypair_path)
            .context("Failed to load payer keypair")?;

        let metrics = Arc::new(E2EMetrics::new());

        // Shared execution event emitter(s).
        // In dual mode we create two lane-specific emitters sharing one writer/run_id.
        let event_lane = match config.execution.execution_mode {
            ExecutionMode::Live => Lane::Live,
            ExecutionMode::Paper => Lane::Paper,
            ExecutionMode::Shadow => Lane::Shadow,
            ExecutionMode::Dual => Lane::Live,
        };
        let run_id = format!("run-{}", uuid::Uuid::new_v4());
        let (event_emitter, paper_event_emitter) = {
            let writer_cfg = EventWriterConfig {
                output_dir: config.execution.events.output_dir.clone(),
                rotation_interval_ms: config.execution.events.rotation_interval_ms,
                flush_interval_ms: config.execution.events.flush_interval_ms,
                max_file_size_bytes: config.execution.events.max_file_size_bytes,
                enable_aem_ticks: config.execution.events.enable_aem_ticks,
                enable_optional_events: config.execution.events.enable_optional_events,
                ..Default::default()
            };
            match EventEmitter::new(writer_cfg, run_id.clone(), event_lane) {
                Ok(emitter) => {
                    let emitter = Arc::new(emitter);
                    let paper_emitter =
                        if matches!(config.execution.execution_mode, ExecutionMode::Dual) {
                            Some(Arc::new(EventEmitter::with_shared_writer(
                                emitter.shared_writer(),
                                run_id.clone(),
                                Lane::Paper,
                            )))
                        } else {
                            None
                        };
                    info!(
                        run_id = %run_id,
                        lane = %event_lane,
                        mode = %config.execution.execution_mode,
                        "Execution event emitter initialized"
                    );
                    (Some(emitter), paper_emitter)
                }
                Err(e) => {
                    warn!("Failed to initialize execution event emitter: {}. Continuing without instrumentation.", e);
                    (None, None)
                }
            }
        };

        // Create GUI state if enabled
        let gui_state = if config.gui_backend.enabled {
            Some(Arc::new(AppState::new()))
        } else {
            None
        };

        // Initialize Shadow Ledger
        info!("Initializing Shadow Ledger for zero-latency bonding curve state");
        let shadow_ledger = Arc::new(ghost_core::shadow_ledger::ShadowLedger::new());

        // Initialize Leader Predictor if enabled
        let leader_predictor = if config.leader_predictor.enabled {
            info!(
                "Initializing LeaderPredictor with {} designated leaders",
                config.leader_predictor.our_leaders.len()
            );
            Some(Arc::new(LeaderPredictor::new(
                config.leader_predictor.our_leaders.clone(),
                config.leader_predictor.grpc_endpoint.clone(),
                config.leader_predictor.verbose,
            )))
        } else {
            info!("LeaderPredictor is disabled");
            None
        };

        // Initialize Jito Bundle Executor if enabled
        let jito_executor = if config.trigger.enable_jito {
            let jito_endpoint = config
                .trigger
                .jito_block_engine_url
                .clone()
                .unwrap_or_else(|| "https://mainnet.block-engine.jito.wtf".to_string());

            info!(
                "Initializing JitoBundleExecutor with endpoint: {}",
                jito_endpoint
            );

            let payer_for_jito = Keypair::from_bytes(&payer.to_bytes()).unwrap();
            let payer_arc = Arc::new(payer_for_jito);

            let executor = if let Some(ref predictor) = leader_predictor {
                // Create with leader predictor for optimized tip calculation
                info!("JitoBundleExecutor will use LeaderPredictor for dynamic tip optimization");
                JitoBundleExecutor::new_with_leader_predictor(
                    jito_endpoint,
                    payer_arc,
                    Arc::clone(predictor),
                )
            } else {
                JitoBundleExecutor::new(jito_endpoint, payer_arc)
            };

            Some(Arc::new(executor))
        } else {
            info!("JitoBundleExecutor is disabled");
            None
        };

        // Initialize LeaderResolver for Leapfrog strategy (always enabled for TPU contact info)
        info!("Initializing LeaderResolver for TPU contact information");
        let rpc_client_for_resolver = Arc::new(solana_client::rpc_client::RpcClient::new(
            config.rpc_url.clone(),
        ));
        let leader_resolver = Arc::new(trigger::LeaderResolver::new(rpc_client_for_resolver));

        // Initialize TpuConnectionManager for QUIC connections to TPU leaders
        // This is initialized asynchronously, so we'll do it in a blocking context
        info!("Initializing TpuConnectionManager for Leapfrog strategy");
        let tpu_connection_manager = tokio::task::block_in_place(|| {
            tokio::runtime::Handle::current().block_on(async {
                match trigger::TpuConnectionManager::new(Arc::clone(&leader_resolver)).await {
                    Ok(manager) => {
                        info!("TpuConnectionManager initialized successfully");
                        Some(Arc::new(manager))
                    }
                    Err(e) => {
                        warn!("Failed to initialize TpuConnectionManager: {}. Leapfrog strategy will use fallback.", e);
                        None
                    }
                }
            })
        });

        // === Ghost Intelligence Initialization ===
        // Create RPC client for Ghost Intelligence (reuse existing pattern)
        let rpc_client_for_ghost = Arc::new(
            solana_client::nonblocking::rpc_client::RpcClient::new(config.rpc_url.clone()),
        );

        // Initialize Ghost Intelligence components using IntelligenceConfig
        info!("Initializing Ghost Intelligence components from config");

        // Build DevProfilerConfig from IntelligenceConfig
        let profiler_config = DevProfilerConfig {
            max_signatures: config.intelligence.max_signatures,
            rpc_timeout_secs: config.intelligence.rpc_timeout_secs,
            serial_minter_threshold: config.intelligence.serial_minter_threshold,
            serial_minter_window_hours: config.intelligence.serial_minter_window_hours,
            fresh_wallet_threshold_hours: 1, // Default
        };
        let profiler = Arc::new(DevProfiler::new(
            profiler_config,
            Arc::clone(&rpc_client_for_ghost),
        ));

        // Build ClusterHunterConfig from IntelligenceConfig
        let cluster_config = ClusterHunterConfig {
            top_holders_count: config.intelligence.max_cluster_size,
            min_cluster_size: config.intelligence.min_cluster_size,
            high_risk_threshold_pct: config.intelligence.high_risk_threshold_pct,
            rpc_timeout_secs: config.intelligence.rpc_timeout_secs,
            max_signatures_per_holder: 20, // Default
            enable_cache: true,            // Default
            cache_ttl_secs: 300,           // Default
        };
        let cluster_hunter = Arc::new(ClusterHunter::new(
            cluster_config,
            Arc::clone(&rpc_client_for_ghost),
        ));

        // Build VisionCriticConfig from IntelligenceConfig
        use crate::oracle::vision_critic::LlmProvider;
        let vision_provider = match config.intelligence.vision_provider.to_lowercase().as_str() {
            "anthropic" => LlmProvider::Anthropic,
            _ => LlmProvider::OpenAI,
        };
        let vision_config = VisionCriticConfig {
            enabled: config.intelligence.enable_vision,
            provider: vision_provider,
            api_key: config.intelligence.vision_api_key.clone(),
            api_timeout_secs: config.intelligence.vision_api_timeout_secs,
            max_retries: 2,                        // Default
            retry_delay_ms: 500,                   // Default
            metadata_timeout_secs: 10,             // Default
            max_image_size_bytes: 5 * 1024 * 1024, // Default 5MB
            openai_endpoint: "https://api.openai.com/v1/chat/completions".to_string(),
            anthropic_endpoint: "https://api.anthropic.com/v1/messages".to_string(),
            openai_model: config.intelligence.openai_model.clone(),
            anthropic_model: config.intelligence.anthropic_model.clone(),
        };
        let vision_critic = Arc::new(VisionCritic::new(vision_config, reqwest::Client::new()));

        info!(
            "Ghost Intelligence initialized: DevProfiler (max_signatures={}), ClusterHunter (max_cluster={}), VisionCritic (enabled={})",
            config.intelligence.max_signatures,
            config.intelligence.max_cluster_size,
            config.intelligence.enable_vision
        );

        // === HyperOracle Initialization ===
        info!("Initializing HyperOracle for advanced signal processing (T+2s window)");
        let hyper_oracle = Arc::new(HyperOracle::new());
        info!("HyperOracle initialized with SCR, ULVF, and POVC modules");

        // === SnapshotEngine Initialization ===
        info!("Initializing SnapshotEngine for real-time market snapshot collection");
        // Configuration: retain 128 snapshots per pool, emit every 200ms
        let snapshot_engine = Arc::new(SnapshotEngine::new(128, 200));
        info!("SnapshotEngine initialized (capacity=128, interval=200ms)");

        // === Panic Executor Initialization ===
        info!("Initializing Panic Executor (Isolated Emergency Path)");
        let panic_executor = if let Some(ref resolver) = Some(Arc::clone(&leader_resolver)) {
            let payer_for_panic = Arc::new(Keypair::from_bytes(&payer.to_bytes()).unwrap());

            match tokio::task::block_in_place(|| {
                tokio::runtime::Handle::current().block_on(async {
                    PanicExecutor::new(
                        config.rpc_url.clone(),
                        payer_for_panic,
                        Arc::clone(resolver),
                    )
                    .await
                })
            }) {
                Ok(executor) => {
                    info!("Panic Executor initialized successfully");
                    Some(Arc::new(executor))
                }
                Err(e) => {
                    warn!("Failed to initialize Panic Executor: {}. Emergency sells will be disabled.", e);
                    None
                }
            }
        } else {
            warn!("Panic Executor disabled: LeaderResolver not available");
            None
        };

        // === Panic Signal Channels (The Nervous System) ===
        info!("Initializing Panic Signal channels (LIGMA, QEDD, PARADOX, CLUSTER)");
        let panic_signals = crate::pipeline::PanicSignals::new();
        info!("Panic Bus initialized: Emergency signal routing active");

        // === Revolver Initialization ===
        let revolver = Arc::new(RwLock::new(Revolver::new()));

        // === PostBuy Guardian Initialization ===
        let post_buy_guardian = {
            // Load PostBuyGuardianConfig from GhostBrainConfig TOML (fallback to defaults)
            let guardian_config = GhostBrainConfig::from_toml_file("ghost_brain_config.toml")
                .map(|c| c.post_buy_guardian)
                .unwrap_or_else(|e| {
                    warn!("Failed to load ghost_brain_config.toml for PostBuy Guardian: {}. Using defaults.", e);
                    PostBuyGuardianConfig::default()
                });

            if guardian_config.enabled {
                if matches!(config.execution.execution_mode, ExecutionMode::Shadow) {
                    bail!(
                        "PostBuy Guardian shadow mode in ghost-brain E2E pipeline requires AccountStateCore feed; use ghost-launcher or disable post_buy_guardian"
                    );
                }
                let buffer_size = guardian_config.signal_channel_buffer;
                let (signal_tx, signal_rx) = tokio::sync::mpsc::channel(buffer_size);
                let aem_enabled = guardian_config.aem.enabled;

                let mut engine = MonitoringEngine::new(
                    guardian_config.clone(),
                    Arc::clone(&shadow_ledger),
                    signal_tx,
                );
                if matches!(config.execution.execution_mode, ExecutionMode::Shadow) {
                    let lifecycle_log_path = config
                        .execution
                        .shadow
                        .lifecycle_log_path
                        .clone()
                        .map(PathBuf::from)
                        .unwrap_or_else(|| {
                            derive_shadow_lifecycle_log_path(
                                &config.execution.shadow.entry_log_path,
                            )
                        });
                    engine.set_shadow_lifecycle_log_path(Some(lifecycle_log_path));
                }

                let position_router =
                    if matches!(config.execution.execution_mode, ExecutionMode::Shadow) {
                        Arc::new(PositionRuntimeRouter::with_shadow_book(Arc::new(
                            RwLock::new(ShadowPositionBook::new()),
                        )))
                    } else {
                        Arc::new(PositionRuntimeRouter::with_live_revolver(Arc::clone(
                            &revolver,
                        )))
                    };
                engine.set_position_router(Arc::clone(&position_router));

                // Attach shared event emitter for AEM/live execution instrumentation.
                if let Some(ref emitter) = event_emitter {
                    // AEM/Guardian stays on primary decision lane emitter.
                    // In dual mode this is intentionally paper for decision parity.
                    let guardian_emitter =
                        if matches!(config.execution.execution_mode, ExecutionMode::Dual) {
                            paper_event_emitter
                                .as_ref()
                                .cloned()
                                .unwrap_or_else(|| Arc::clone(emitter))
                        } else {
                            Arc::clone(emitter)
                        };
                    engine.set_event_emitter(guardian_emitter);

                    // In dual mode mirror AEM command application events to live lane.
                    if matches!(config.execution.execution_mode, ExecutionMode::Dual) {
                        engine.set_secondary_event_emitter(Arc::clone(emitter));
                    }
                }

                // Optional AEM runtime and ledger (deterministic replay-based controller)
                if aem_enabled {
                    match JsonlAemLedger::new(&guardian_config.aem.ledger_dir) {
                        Ok(ledger) => {
                            let mut runtime = AemRuntime::new(guardian_config.aem.clone());
                            let now_ms = std::time::SystemTime::now()
                                .duration_since(std::time::UNIX_EPOCH)
                                .unwrap_or_default()
                                .as_millis()
                                .min(u128::from(u64::MAX))
                                as u64;
                            if let Err(e) = runtime.bootstrap_from_ledger(&ledger, now_ms) {
                                warn!("AEM bootstrap replay degraded: {}", e);
                            }
                            engine.set_aem(
                                Arc::new(parking_lot::Mutex::new(runtime)),
                                Arc::new(ledger),
                            );
                            info!("AEM v1 initialized for PostBuy Guardian");
                        }
                        Err(e) => {
                            warn!("AEM ledger init failed, AEM disabled: {}", e);
                        }
                    }
                }

                let engine = Arc::new(engine);

                // Spawn SignalRouter only when AEM is disabled.
                if !aem_enabled {
                    let router = SignalRouter::new(signal_rx, position_router);
                    tokio::task::spawn(router.run());
                }

                if aem_enabled {
                    info!("PostBuy Guardian initialized: MonitoringEngine + AEM active");
                } else {
                    info!("PostBuy Guardian initialized: MonitoringEngine + SignalRouter active");
                }
                Some(engine)
            } else {
                info!("PostBuy Guardian is disabled via configuration");
                None
            }
        };

        Ok(Self {
            config,
            metrics,
            authority,
            payer,
            gui_state,
            revolver,
            leader_predictor,
            jito_executor,
            shadow_ledger,
            tpu_connection_manager,
            leader_resolver: Some(leader_resolver),
            profiler,
            cluster_hunter,
            vision_critic,
            hyper_oracle,
            snapshot_engine,
            panic_executor,
            panic_signals,
            post_buy_guardian,
            execution_event_emitter: event_emitter,
            execution_event_emitter_paper: paper_event_emitter,
        })
    }

    /// Load a keypair from file
    pub(super) fn load_keypair(path: &str) -> Result<Keypair> {
        let expanded_path = shellexpand::tilde(path);
        let keypair_bytes = std::fs::read(expanded_path.as_ref())
            .with_context(|| format!("Failed to read keypair file: {}", path))?;

        let keypair_data: Vec<u8> =
            serde_json::from_slice(&keypair_bytes).context("Failed to parse keypair JSON")?;

        Keypair::from_bytes(&keypair_data).context("Failed to create keypair from bytes")
    }
}
