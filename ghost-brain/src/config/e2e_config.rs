//! Configuration module for E2E pipeline
//!
//! Loads configuration from environment variables and provides
//! typed configuration structs for all components.

use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

use crate::execution::backend::{EntryStalePolicy, ExecutionMode};
use crate::execution::paper::{
    PaperBrokerConfig, SlippageModel, StressInjectionMode, StressRulesConfig,
};
use crate::quotes::provider::QuoteProviderConfig;

/// Complete E2E pipeline configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct E2EConfig {
    /// Devnet RPC URL
    pub rpc_url: String,

    /// Devnet WebSocket URL
    pub websocket_url: String,

    /// Authority keypair path
    pub authority_keypair_path: String,

    /// Payer keypair path (for transaction fees)
    pub payer_keypair_path: String,

    /// Seer configuration
    pub seer: SeerConfig,

    /// Oracle configuration
    pub oracle: OracleConfig,

    /// Features configuration
    pub features: FeaturesConfig,

    /// Trigger configuration
    pub trigger: TriggerConfig,

    /// Metrics configuration
    pub metrics: MetricsConfig,

    /// GUI Backend configuration
    pub gui_backend: GuiBackendConfig,

    /// Leader Predictor configuration
    pub leader_predictor: LeaderPredictorConfig,

    /// Ghost Intelligence configuration (DevProfiler, ClusterHunter, VisionCritic)
    pub intelligence: IntelligenceConfig,

    /// Execution mode configuration (SSOT)
    pub execution: ExecutionConfig,
}

/// Seer component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SeerConfig {
    /// Enable Pump.fun detection
    pub enable_pumpfun: bool,

    /// Enable Bonk.fun detection
    pub enable_bonkfun: bool,

    /// Minimum liquidity in SOL
    pub min_liquidity_sol: Option<f64>,

    /// Maximum reconnect attempts
    pub max_reconnect_attempts: u32,

    /// Reconnect delay in seconds
    pub reconnect_delay_secs: u64,

    /// Verbose logging
    pub verbose: bool,
}

/// Oracle component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OracleConfig {
    /// Minimum oracle score (0-100) to proceed
    pub min_score_threshold: u8,

    /// Enable anomaly detection
    pub enable_anomaly_detection: bool,

    /// RPC endpoints for data fetching
    pub rpc_endpoints: Vec<String>,
}

/// Features/Strategy configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    /// Default strategy to use
    pub default_strategy: String,

    /// Maximum position size in lamports
    pub max_position_size_lamports: u64,

    /// Maximum slippage tolerance (0.0 - 1.0)
    pub max_slippage: f64,

    /// Intent timeout in seconds
    pub intent_timeout_secs: u64,
}

/// Trigger component configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TriggerConfig {
    /// N+X redundancy factor (N+1 for tests, N+3 for production, N+5 for special mode)
    pub redundancy_factor: u32,

    /// Max span slots for leader schedule
    pub max_span_slots: u64,

    /// Enable Jito bundle submission
    pub enable_jito: bool,

    /// Jito block engine URL (if enabled)
    pub jito_block_engine_url: Option<String>,

    /// Dry-run mode: log transactions instead of sending them
    pub dry_run: bool,

    /// Maximum concurrent positions (default: 3)
    pub max_concurrent_positions: Option<usize>,

    /// Enable Leapfrog TPU strategy (default: false)
    pub enable_leapfrog: bool,

    /// Leapfrog redundancy (N+X leaders, default: 2 for N+2 = 3 leaders total)
    pub leapfrog_redundancy: u32,

    /// Use QUIC for Leapfrog (default: false = UDP)
    pub leapfrog_use_quic: bool,
}

/// Metrics collection configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MetricsConfig {
    /// Enable Prometheus metrics
    pub enable_prometheus: bool,

    /// Prometheus port
    pub prometheus_port: u16,

    /// Target Land Rate (percentage)
    pub target_land_rate: f64,

    /// Target Inclusion Rate (percentage)
    pub target_inclusion_rate: f64,
}

/// GUI Backend configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GuiBackendConfig {
    /// Enable GUI backend server
    pub enabled: bool,

    /// GUI backend port
    pub port: u16,

    /// Bind address (default: localhost)
    pub bind_address: String,
}

/// Leader Predictor configuration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LeaderPredictorConfig {
    /// Enable leader prediction
    pub enabled: bool,

    /// Yellowstone gRPC endpoint for leader schedule monitoring
    pub grpc_endpoint: String,

    /// Our designated leader validator pubkeys (comma-separated)
    pub our_leaders: Vec<Pubkey>,

    /// Enable verbose logging
    pub verbose: bool,
}

/// Ghost Intelligence configuration (DevProfiler, ClusterHunter, VisionCritic)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntelligenceConfig {
    // === VisionCritic Configuration ===
    /// Enable VisionCritic AI analysis (default: false to prevent API costs)
    pub enable_vision: bool,
    /// Vision LLM provider: "openai" or "anthropic"
    pub vision_provider: String,
    /// API key for Vision LLM provider
    pub vision_api_key: Option<String>,
    /// OpenAI model for vision analysis
    pub openai_model: String,
    /// Anthropic model for vision analysis
    pub anthropic_model: String,

    // === ClusterHunter Configuration ===
    /// Number of top holders to analyze (default: 20)
    pub max_cluster_size: usize,
    /// Minimum cluster size to flag (default: 3)
    pub min_cluster_size: usize,
    /// Supply percentage threshold for high risk flag (default: 30%)
    pub high_risk_threshold_pct: f32,

    // === DevProfiler Configuration ===
    /// Maximum signatures to fetch for creator analysis (default: 10)
    pub max_signatures: usize,
    /// Threshold for serial minter detection (default: 5 tokens in window)
    pub serial_minter_threshold: usize,
    /// Time window for serial minter detection in hours (default: 24)
    pub serial_minter_window_hours: u64,

    // === General Configuration ===
    /// RPC timeout for blockchain queries in seconds (default: 10)
    pub rpc_timeout_secs: u64,
    /// API timeout for LLM provider requests in seconds (default: 30)
    pub vision_api_timeout_secs: u64,
}

/// Execution mode configuration (SSOT)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionConfig {
    /// Execution mode: "live", "paper", "shadow", or "dual"
    pub execution_mode: ExecutionMode,

    /// Paper broker configuration (used when mode = Paper or Dual)
    pub paper: PaperBrokerConfig,

    /// Shadow execution/reporting configuration (used when mode = Shadow).
    pub shadow: ExecutionShadowConfig,

    /// Quote provider configuration
    pub quotes: QuoteProviderConfig,

    /// Event logging configuration (maps to [execution.events]).
    pub events: ExecutionEventsConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionEventsConfig {
    pub output_dir: String,
    pub rotation_interval_ms: u64,
    pub flush_interval_ms: u64,
    pub max_file_size_bytes: u64,
    pub enable_aem_ticks: bool,
    pub enable_optional_events: bool,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionShadowTimingModel {
    LegacyCompareOnly,
    PreparedEntryMirror,
}

impl ExecutionShadowTimingModel {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::LegacyCompareOnly => "legacy_compare_only",
            Self::PreparedEntryMirror => "prepared_entry_mirror",
        }
    }
}

impl Default for ExecutionShadowTimingModel {
    fn default() -> Self {
        Self::PreparedEntryMirror
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct ExecutionShadowConfig {
    pub tx_build_compensation_ms: u64,
    pub max_quote_age_ms: u64,
    pub entry_log_path: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub lifecycle_log_path: Option<String>,
    #[serde(default)]
    pub timing_model: ExecutionShadowTimingModel,
    #[serde(default = "default_shadow_stale_policy")]
    pub stale_policy: EntryStalePolicy,
}

impl Default for ExecutionShadowConfig {
    fn default() -> Self {
        Self {
            tx_build_compensation_ms: default_shadow_tx_build_compensation_ms(),
            max_quote_age_ms: default_execution_max_quote_age_ms(),
            entry_log_path: default_shadow_entry_log_path(),
            lifecycle_log_path: None,
            timing_model: ExecutionShadowTimingModel::default(),
            stale_policy: default_shadow_stale_policy(),
        }
    }
}

impl Default for ExecutionEventsConfig {
    fn default() -> Self {
        Self {
            output_dir: "datasets/events".to_string(),
            rotation_interval_ms: 300_000,
            flush_interval_ms: 1_000,
            max_file_size_bytes: 50_000_000,
            enable_aem_ticks: true,
            enable_optional_events: false,
        }
    }
}

impl Default for ExecutionConfig {
    fn default() -> Self {
        Self {
            execution_mode: ExecutionMode::Paper,
            paper: PaperBrokerConfig::default(),
            shadow: ExecutionShadowConfig::default(),
            quotes: QuoteProviderConfig::default(),
            events: ExecutionEventsConfig::default(),
        }
    }
}

impl E2EConfig {
    /// Load configuration from environment variables
    ///
    /// Expects a `.env.devnet` file or environment variables to be set.
    pub fn from_env() -> Result<Self> {
        // Load .env.devnet if it exists
        dotenv::from_filename(".env.devnet").ok();

        let config = Self {
            rpc_url: std::env::var("RPC_URL_DEVNET").context("RPC_URL_DEVNET not set")?,
            websocket_url: std::env::var("WEBSOCKET_URL_DEVNET")
                .context("WEBSOCKET_URL_DEVNET not set")?,
            authority_keypair_path: std::env::var("AUTHORITY_KEYPAIR_PATH")
                .unwrap_or_else(|_| "~/.config/solana/id.json".to_string()),
            payer_keypair_path: std::env::var("PAYER_KEYPAIR_PATH")
                .unwrap_or_else(|_| "~/.config/solana/id.json".to_string()),
            seer: SeerConfig {
                enable_pumpfun: std::env::var("SEER_ENABLE_PUMPFUN")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                enable_bonkfun: std::env::var("SEER_ENABLE_BONKFUN")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                min_liquidity_sol: std::env::var("SEER_MIN_LIQUIDITY_SOL")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                max_reconnect_attempts: std::env::var("SEER_MAX_RECONNECT_ATTEMPTS")
                    .unwrap_or_else(|_| "5".to_string())
                    .parse()
                    .unwrap_or(5),
                reconnect_delay_secs: std::env::var("SEER_RECONNECT_DELAY_SECS")
                    .unwrap_or_else(|_| "5".to_string())
                    .parse()
                    .unwrap_or(5),
                verbose: std::env::var("SEER_VERBOSE")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
            oracle: OracleConfig {
                min_score_threshold: std::env::var("ORACLE_MIN_SCORE_THRESHOLD")
                    .unwrap_or_else(|_| "70".to_string())
                    .parse()
                    .unwrap_or(70),
                enable_anomaly_detection: std::env::var("ORACLE_ENABLE_ANOMALY_DETECTION")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                rpc_endpoints: std::env::var("ORACLE_RPC_ENDPOINTS")
                    .unwrap_or_else(|_| std::env::var("RPC_URL_DEVNET").unwrap_or_default())
                    .split(',')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect(),
            },
            features: FeaturesConfig {
                default_strategy: std::env::var("FEATURES_DEFAULT_STRATEGY")
                    .unwrap_or_else(|_| "snipe_new_pool".to_string()),
                max_position_size_lamports: std::env::var("FEATURES_MAX_POSITION_SIZE_LAMPORTS")
                    .unwrap_or_else(|_| "10000000".to_string()) // 0.01 SOL default
                    .parse()
                    .unwrap_or(10_000_000),
                max_slippage: std::env::var("FEATURES_MAX_SLIPPAGE")
                    .unwrap_or_else(|_| "0.05".to_string()) // 5% default
                    .parse()
                    .unwrap_or(0.05),
                intent_timeout_secs: std::env::var("FEATURES_INTENT_TIMEOUT_SECS")
                    .unwrap_or_else(|_| "3600".to_string()) // 1 hour default
                    .parse()
                    .unwrap_or(3600),
            },
            trigger: TriggerConfig {
                redundancy_factor: std::env::var("TRIGGER_REDUNDANCY_FACTOR")
                    .unwrap_or_else(|_| "3".to_string())
                    .parse()
                    .unwrap_or(3),
                max_span_slots: std::env::var("TRIGGER_MAX_SPAN_SLOTS")
                    .unwrap_or_else(|_| "4".to_string())
                    .parse()
                    .unwrap_or(4),
                enable_jito: std::env::var("TRIGGER_ENABLE_JITO")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                jito_block_engine_url: std::env::var("TRIGGER_JITO_BLOCK_ENGINE_URL").ok(),
                dry_run: std::env::var("TRIGGER_DRY_RUN")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                max_concurrent_positions: std::env::var("TRIGGER_MAX_CONCURRENT_POSITIONS")
                    .ok()
                    .and_then(|s| s.parse().ok()),
                enable_leapfrog: std::env::var("TRIGGER_ENABLE_LEAPFROG")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                leapfrog_redundancy: std::env::var("TRIGGER_LEAPFROG_REDUNDANCY")
                    .unwrap_or_else(|_| "2".to_string())
                    .parse()
                    .unwrap_or(2),
                leapfrog_use_quic: std::env::var("TRIGGER_LEAPFROG_USE_QUIC")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
            metrics: MetricsConfig {
                enable_prometheus: std::env::var("METRICS_ENABLE_PROMETHEUS")
                    .unwrap_or_else(|_| "true".to_string())
                    .parse()
                    .unwrap_or(true),
                prometheus_port: std::env::var("METRICS_PROMETHEUS_PORT")
                    .unwrap_or_else(|_| "9090".to_string())
                    .parse()
                    .unwrap_or(9090),
                target_land_rate: std::env::var("METRICS_TARGET_LAND_RATE")
                    .unwrap_or_else(|_| "95.0".to_string())
                    .parse()
                    .unwrap_or(95.0),
                target_inclusion_rate: std::env::var("METRICS_TARGET_INCLUSION_RATE")
                    .unwrap_or_else(|_| "92.0".to_string())
                    .parse()
                    .unwrap_or(92.0),
            },
            gui_backend: GuiBackendConfig {
                enabled: std::env::var("GUI_BACKEND_ENABLED")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                port: std::env::var("GUI_BACKEND_PORT")
                    .unwrap_or_else(|_| "8800".to_string())
                    .parse()
                    .unwrap_or(8800),
                bind_address: std::env::var("GUI_BACKEND_BIND_ADDRESS")
                    .unwrap_or_else(|_| "127.0.0.1".to_string()),
            },
            leader_predictor: LeaderPredictorConfig {
                enabled: std::env::var("LEADER_PREDICTOR_ENABLED")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                grpc_endpoint: std::env::var("LEADER_PREDICTOR_GRPC_ENDPOINT")
                    .unwrap_or_else(|_| "http://localhost:10000".to_string()),
                our_leaders: std::env::var("LEADER_PREDICTOR_OUR_LEADERS")
                    .unwrap_or_default()
                    .split(',')
                    .map(|s| s.trim())
                    .filter(|s| !s.is_empty())
                    .filter_map(|s| Pubkey::from_str(s).ok())
                    .collect(),
                verbose: std::env::var("LEADER_PREDICTOR_VERBOSE")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
            },
            intelligence: IntelligenceConfig {
                // VisionCritic config
                enable_vision: std::env::var("INTELLIGENCE_ENABLE_VISION")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse()
                    .unwrap_or(false),
                vision_provider: std::env::var("INTELLIGENCE_VISION_PROVIDER")
                    .unwrap_or_else(|_| "openai".to_string()),
                vision_api_key: std::env::var("INTELLIGENCE_VISION_API_KEY").ok(),
                openai_model: std::env::var("INTELLIGENCE_OPENAI_MODEL")
                    .unwrap_or_else(|_| "gpt-4o-mini".to_string()),
                anthropic_model: std::env::var("INTELLIGENCE_ANTHROPIC_MODEL")
                    .unwrap_or_else(|_| "claude-3-haiku-20240307".to_string()),
                // ClusterHunter config
                max_cluster_size: std::env::var("INTELLIGENCE_MAX_CLUSTER_SIZE")
                    .unwrap_or_else(|_| "20".to_string())
                    .parse()
                    .unwrap_or(20),
                min_cluster_size: std::env::var("INTELLIGENCE_MIN_CLUSTER_SIZE")
                    .unwrap_or_else(|_| "3".to_string())
                    .parse()
                    .unwrap_or(3),
                high_risk_threshold_pct: std::env::var("INTELLIGENCE_HIGH_RISK_THRESHOLD_PCT")
                    .unwrap_or_else(|_| "30.0".to_string())
                    .parse()
                    .unwrap_or(30.0),
                // DevProfiler config
                max_signatures: std::env::var("INTELLIGENCE_MAX_SIGNATURES")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
                serial_minter_threshold: std::env::var("INTELLIGENCE_SERIAL_MINTER_THRESHOLD")
                    .unwrap_or_else(|_| "5".to_string())
                    .parse()
                    .unwrap_or(5),
                serial_minter_window_hours: std::env::var(
                    "INTELLIGENCE_SERIAL_MINTER_WINDOW_HOURS",
                )
                .unwrap_or_else(|_| "24".to_string())
                .parse()
                .unwrap_or(24),
                // General config
                rpc_timeout_secs: std::env::var("INTELLIGENCE_RPC_TIMEOUT_SECS")
                    .unwrap_or_else(|_| "10".to_string())
                    .parse()
                    .unwrap_or(10),
                vision_api_timeout_secs: std::env::var("INTELLIGENCE_VISION_API_TIMEOUT_SECS")
                    .unwrap_or_else(|_| "30".to_string())
                    .parse()
                    .unwrap_or(30),
            },
            execution: {
                // Backward compat: dry_run = true → Paper mode
                let explicit_mode = std::env::var("EXECUTION_MODE").ok();
                let dry_run_val = std::env::var("TRIGGER_DRY_RUN")
                    .unwrap_or_else(|_| "false".to_string())
                    .parse::<bool>()
                    .unwrap_or(false);

                let execution_mode = match explicit_mode.as_deref() {
                    Some("live") => ExecutionMode::Live,
                    Some("paper") => ExecutionMode::Paper,
                    Some("shadow") => ExecutionMode::Shadow,
                    Some("dual") => ExecutionMode::Dual,
                    _ => {
                        if dry_run_val {
                            ExecutionMode::Paper
                        } else {
                            ExecutionMode::Live
                        }
                    }
                };

                ExecutionConfig {
                    execution_mode,
                    paper: PaperBrokerConfig {
                        fill_delay_ms_min: std::env::var("PAPER_FILL_DELAY_MS_MIN")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(200),
                        fill_delay_ms_max: std::env::var("PAPER_FILL_DELAY_MS_MAX")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(400),
                        jitter_ms: std::env::var("PAPER_JITTER_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(50),
                        max_quote_age_ms: std::env::var("PAPER_MAX_QUOTE_AGE_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1500),
                        slippage_model: match std::env::var("PAPER_SLIPPAGE_MODEL").as_deref() {
                            Ok("fixed_bps") => SlippageModel::FixedBps,
                            Ok("impact_from_quote") => SlippageModel::ImpactFromQuote,
                            _ => SlippageModel::Off,
                        },
                        slippage_bps_fixed: std::env::var("PAPER_SLIPPAGE_BPS_FIXED")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        fail_prob: std::env::var("PAPER_FAIL_PROB")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0.0),
                        stress_injection: match std::env::var("PAPER_STRESS_INJECTION").as_deref() {
                            Ok("rules") => StressInjectionMode::Rules,
                            Ok("random") => StressInjectionMode::Random,
                            _ => StressInjectionMode::Off,
                        },
                        max_open_positions_paper: std::env::var("PAPER_MAX_OPEN_POSITIONS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(10),
                        candidate_sampling: std::env::var("PAPER_CANDIDATE_SAMPLING")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1.0),
                        rng_seed: std::env::var("PAPER_RNG_SEED")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(0),
                        stress_rules: StressRulesConfig::default(),
                    },
                    shadow: ExecutionShadowConfig {
                        tx_build_compensation_ms: std::env::var("SHADOW_TX_BUILD_COMPENSATION_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(default_shadow_tx_build_compensation_ms()),
                        max_quote_age_ms: std::env::var("SHADOW_MAX_QUOTE_AGE_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(default_execution_max_quote_age_ms()),
                        entry_log_path: std::env::var("SHADOW_ENTRY_LOG_PATH")
                            .unwrap_or_else(|_| default_shadow_entry_log_path()),
                        lifecycle_log_path: std::env::var("SHADOW_LIFECYCLE_LOG_PATH")
                            .ok()
                            .filter(|value| !value.trim().is_empty()),
                        timing_model: match std::env::var("SHADOW_TIMING_MODEL").as_deref() {
                            Ok("prepared_entry_mirror") => {
                                ExecutionShadowTimingModel::PreparedEntryMirror
                            }
                            _ => ExecutionShadowTimingModel::LegacyCompareOnly,
                        },
                        stale_policy: match std::env::var("SHADOW_STALE_POLICY").as_deref() {
                            Ok("reject") => EntryStalePolicy::Reject,
                            _ => EntryStalePolicy::EmitWarning,
                        },
                    },
                    quotes: QuoteProviderConfig {
                        max_quote_age_ms: std::env::var("QUOTE_MAX_AGE_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1500),
                        ring_buffer_size: std::env::var("QUOTE_RING_BUFFER_SIZE")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(256),
                        generation_interval_ms: std::env::var("QUOTE_GENERATION_INTERVAL_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(500),
                        stale_warning_threshold_ms: std::env::var("QUOTE_STALE_WARNING_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1000),
                    },
                    events: ExecutionEventsConfig {
                        output_dir: std::env::var("EXECUTION_EVENTS_OUTPUT_DIR")
                            .unwrap_or_else(|_| "datasets/events".to_string()),
                        rotation_interval_ms: std::env::var(
                            "EXECUTION_EVENTS_ROTATION_INTERVAL_MS",
                        )
                        .ok()
                        .and_then(|s| s.parse().ok())
                        .unwrap_or(300_000),
                        flush_interval_ms: std::env::var("EXECUTION_EVENTS_FLUSH_INTERVAL_MS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(1_000),
                        max_file_size_bytes: std::env::var("EXECUTION_EVENTS_MAX_FILE_SIZE_BYTES")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(50_000_000),
                        enable_aem_ticks: std::env::var("EXECUTION_ENABLE_AEM_TICKS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(true),
                        enable_optional_events: std::env::var("EXECUTION_ENABLE_OPTIONAL_EVENTS")
                            .ok()
                            .and_then(|s| s.parse().ok())
                            .unwrap_or(false),
                    },
                }
            },
        };

        Ok(config)
    }

    /// Validate configuration
    pub fn validate(&self) -> Result<()> {
        // Validate RPC URLs
        if self.rpc_url.is_empty() {
            anyhow::bail!("RPC URL cannot be empty");
        }
        if self.websocket_url.is_empty() {
            anyhow::bail!("WebSocket URL cannot be empty");
        }

        // Validate keypair paths exist
        if !std::path::Path::new(&self.authority_keypair_path).exists() {
            anyhow::bail!(
                "Authority keypair file not found: {}",
                self.authority_keypair_path
            );
        }
        if !std::path::Path::new(&self.payer_keypair_path).exists() {
            anyhow::bail!("Payer keypair file not found: {}", self.payer_keypair_path);
        }

        // Validate Oracle score threshold
        if self.oracle.min_score_threshold > 100 {
            anyhow::bail!("Oracle min score threshold must be 0-100");
        }

        // Validate Features slippage
        if self.features.max_slippage < 0.0 || self.features.max_slippage > 1.0 {
            anyhow::bail!("Max slippage must be between 0.0 and 1.0");
        }

        // Validate Trigger redundancy
        if self.trigger.redundancy_factor == 0 {
            anyhow::bail!("Trigger redundancy factor must be at least 1");
        }

        // Validate metrics targets
        if self.metrics.target_land_rate < 0.0 || self.metrics.target_land_rate > 100.0 {
            anyhow::bail!("Target land rate must be between 0.0 and 100.0");
        }
        if self.metrics.target_inclusion_rate < 0.0 || self.metrics.target_inclusion_rate > 100.0 {
            anyhow::bail!("Target inclusion rate must be between 0.0 and 100.0");
        }

        // Validate leader predictor configuration
        if self.leader_predictor.enabled {
            if self.leader_predictor.grpc_endpoint.is_empty() {
                anyhow::bail!("Leader predictor gRPC endpoint cannot be empty");
            }
            if self.leader_predictor.our_leaders.is_empty() {
                anyhow::bail!("Leader predictor requires at least one designated leader validator");
            }
        }

        // Validate Intelligence configuration
        if self.intelligence.enable_vision && self.intelligence.vision_api_key.is_none() {
            anyhow::bail!(
                "VisionCritic enabled but no API key provided (INTELLIGENCE_VISION_API_KEY)"
            );
        }
        if self.intelligence.high_risk_threshold_pct < 0.0
            || self.intelligence.high_risk_threshold_pct > 100.0
        {
            anyhow::bail!("Intelligence high_risk_threshold_pct must be between 0.0 and 100.0");
        }
        let valid_providers = ["openai", "anthropic"];
        if !valid_providers.contains(&self.intelligence.vision_provider.as_str()) {
            anyhow::bail!(
                "Invalid vision provider: {}. Must be 'openai' or 'anthropic'",
                self.intelligence.vision_provider
            );
        }

        Ok(())
    }
}

fn default_shadow_tx_build_compensation_ms() -> u64 {
    250
}

fn default_execution_max_quote_age_ms() -> u64 {
    1_500
}

fn default_shadow_entry_log_path() -> String {
    "logs/shadow_run/shadow_entries.jsonl".to_string()
}

fn default_shadow_stale_policy() -> EntryStalePolicy {
    EntryStalePolicy::EmitWarning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn execution_shadow_config_defaults_are_stable() {
        let cfg = ExecutionShadowConfig::default();
        assert_eq!(cfg.tx_build_compensation_ms, 250);
        assert_eq!(cfg.max_quote_age_ms, 1_500);
        assert_eq!(cfg.entry_log_path, "logs/shadow_run/shadow_entries.jsonl");
        assert_eq!(cfg.lifecycle_log_path, None);
        assert_eq!(
            cfg.timing_model,
            ExecutionShadowTimingModel::PreparedEntryMirror
        );
        assert_eq!(cfg.stale_policy, EntryStalePolicy::EmitWarning);
    }

    #[test]
    fn execution_shadow_config_allows_partial_toml_override() {
        let cfg: ExecutionShadowConfig = toml::from_str(
            r#"
entry_log_path = "custom/shadow_entries.jsonl"
"#,
        )
        .expect("deserialize partial shadow config");

        assert_eq!(cfg.entry_log_path, "custom/shadow_entries.jsonl");
        assert_eq!(cfg.tx_build_compensation_ms, 250);
        assert_eq!(cfg.max_quote_age_ms, 1_500);
        assert_eq!(
            cfg.timing_model,
            ExecutionShadowTimingModel::PreparedEntryMirror
        );
    }
}
