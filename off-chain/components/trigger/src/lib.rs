//! # Trigger - Ghost Transaction Builder and Sender
//!
//! The Trigger module is responsible for building minimal Ghost Transactions (~180B)
//! using Address Lookup Tables (LUT), sending them with N+3 redundancy for high
//! inclusion rate, and integrating with Jito Bundle for MEV extraction.
//!
//! ## Features
//!
//! - **Transaction Building**: Constructs minimal transactions using LUT compression
//! - **N+3 Redundancy**: Sends transactions multiple times to maximize inclusion rate (target: ≥92%)
//! - **Jito Integration**: Stub for bundle submission to Jito MEV
//! - **Metrics**: Prometheus metrics for monitoring performance
//!
//! ## Architecture
//!
//! ```text
//! SwapPlan (from Oracle/Features)
//!     ↓
//! GhostTransactionBuilder
//!     ↓
//! TpuClient (N+3 redundancy) or JitoClient (bundles)
//!     ↓
//! Solana Network (TPU leaders)
//! ```
//!
//! ## Usage Example
//!
//! ```ignore
//! use trigger::{GhostTransactionBuilder, TpuClient, AmmAccounts, AmmType};
//! use ghost_core::SwapPlan;
//!
//! // Build transaction
//! let builder = GhostTransactionBuilder::new(swap_plan, AmmType::PumpFun, amm_accounts);
//! let tx = builder.build_initialize_intent_tx(&payer, recent_blockhash)?;
//!
//! // Send with N+3 redundancy
//! let tpu_client = TpuClient::new(rpc_url, Some(3))?;
//! let signature = tpu_client.send_transaction_with_redundancy(&tx).await?;
//! ```

pub mod bundle_builder;
pub mod config;
pub mod control_command;
pub mod direct_buy_builder;
pub mod direct_sell_builder;
pub mod entry_price_extractor;
pub mod errors;
pub mod execution_guard;
pub mod ipc_integration;
pub mod jito_client;
pub(crate) mod jito_protos;
pub mod leader_resolver;
pub mod leader_tracker;
pub mod metrics;
pub mod panic_executor;
pub mod revolver;
pub mod revolver_integration;
pub mod revolver_price_feed;
pub mod revolver_price_logic;
pub mod revolver_sell_builder;
pub mod revolver_shoot;
pub mod revolver_worker;
pub mod rpc_provider;
pub mod safety;
pub mod tpu_connection_manager;
pub mod tpu_sender;
pub mod transaction_builder;
pub mod transaction_monitor;
pub mod udp_client;
pub mod validation;
pub mod wallet_loader;

// Re-export commonly used types
pub use bundle_builder::BundleBuilder;
pub use config::{
    load_keypair, AmmType, BundleConfig, LeapfrogConfig, LutConfig, RedundancyPolicy, TipConfig,
};
pub use control_command::{
    AemCommandApplyResult, AemCommandDirective, AemCommandPriority, ExecutionStressSnapshot,
};
pub use direct_buy_builder::{
    BreakingFeeRecipientStrategy, DirectBuyBuilder, LegacyBondingCurveTailResolver,
    PumpfunBuyVariant, BREAKING_FEE_RECIPIENTS, PUMPFUN_BUYBACK_REMAINING_ACCOUNT_COUNT,
    PUMPFUN_BUY_FIXED_ACCOUNT_COUNT,
};
pub use direct_sell_builder::{DirectSellBuilder, DEFAULT_SELL_SLIPPAGE_BPS};
pub use entry_price_extractor::{
    EntryPriceError, EntryPriceExtractor, EntryPriceInfo, PriceTruthError, PriceTruthEvidence,
    PriceTruthResolver, PriceTruthSource, PriceTruthStatus, ShadowExitPriceSample, ShadowExitTruth,
};
pub use errors::{Result, TriggerError};
pub use execution_guard::{ExecutionGuardError, SafeBuyBuilder};
pub use ipc_integration::{
    CandidatePool as IpcCandidatePool, DetectedPoolEvent, EventPriority as IpcEventPriority,
    IpcEventProcessor, ProcessingError, ProcessingResult, ProcessorConfig,
};
pub use jito_client::{
    normalize_jito_endpoint, probe_jito_endpoint, BalanceDeltaDirection, BundleDiagnostics,
    JitoBundle, JitoBundleSubmission, JitoClient, JitoClientBuilder, JitoConfirmedBundle,
    JitoEndpointProbeOutcome, JITO_BUNDLES_JSONRPC_PATH,
};
pub use leader_resolver::{LeaderResolver, TpuContactInfo};
pub use leader_tracker::{
    ClusterNodeInfo, LeaderTpuInfo, LeaderTracker, LeaderTrackerConfig, TpuProtocol,
};
pub use metrics::TriggerMetrics;
pub use panic_executor::{KillReason, PanicExecutor};
pub use revolver::{Bullet, Revolver, StrategyMode, TokenRevolver};
pub use revolver_integration::{
    create_magazine_after_buy, create_standard_magazine, create_virtual_magazine, MagazineConfig,
    PriceTarget,
};
pub use revolver_price_feed::{
    GuiBackendPriceOracle, JitoBulletExecutor, PriceFeedIntegration, PriceFeedWithTpuClient,
    PriceFeedWorkerHandle, PriceOracleProvider,
};
pub use revolver_price_logic::{PositionPriceTargets, TargetLevel, TpPanicConfig};
pub use revolver_sell_builder::{AmmProtocol, SellTxBuilder, SellTxConfig};
pub use revolver_shoot::{
    register_shot_context, set_shot_event_sink, shoot_all_targets, shoot_at_price,
    shoot_bullets_by_index, unregister_shot_context, MockPriceOracle, PriceOracle, ShotContext,
    ShotEvent, ShotEventSink, ShotEventStage, ShotResult,
};
pub use revolver_worker::{
    build_direct_sell_instruction, check_shadow_ledger_staleness, get_staleness_info,
    load_magazine_from_direct_buy, RevolverWorker, SellStrategyConfig, StalenessResult,
    WorkerConfig, WorkerHandle, DEFAULT_MAX_STALE_SLOTS,
};
pub use rpc_provider::BlockhashProvider;
pub use safety::{
    calculate_safe_tip, calculate_safe_trade_amount, check_emergency_floor, get_fallback_tip,
    validate_tip, validate_trade, SafetyConfig, SafetyViolation, TipGuardConfig,
    DEFAULT_MAX_TIP_ABSOLUTE_SOL, DEFAULT_MAX_TIP_RATIO_PERCENT, EMERGENCY_FLOOR_SOL,
    FALLBACK_TIP_SOL, POSITION_SIZE_BUFFER_SOL,
};
pub use tpu_connection_manager::{PrewarmConfig, TpuConnectionManager};
pub use tpu_sender::{SendResult, TpuSender, TpuSenderConfig};
pub use transaction_builder::{AmmAccounts, GhostTransactionBuilder};
pub use transaction_monitor::{
    extract_entry_price_after_buy, extract_exit_price_after_sell, BuyTransactionMetadata,
    SellTransactionMetadata, TransactionMonitor,
};
pub use udp_client::{LeaderInfo, LeaderSchedule, RedundancyConfig, TpuClient};
pub use validation::{
    derive_bonding_curve_pda, is_whitelisted_amm_program, verify_buy_discriminator,
    verify_sell_discriminator, PoolValidationResult, PoolValidator, BONK_PROGRAM_ID,
    DEFAULT_CACHE_TTL_SECS, PUMP_PROGRAM_ID,
};
pub use wallet_loader::load_payer_keypair;
