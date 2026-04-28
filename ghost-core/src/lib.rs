//! # Ghost Core Types
//!
//! This library provides shared types and interfaces for the Ghost trading system,
//! bridging the gap between off-chain components (Oracle, Features) and on-chain
//! execution via DirectBuyBuilder (direct AMM interaction).
//!
//! ## Key Types
//!
//! - `SwapPlan`: Intermediate representation of a swap intent
//! - `ValidationError`: Errors that can occur during swap plan validation
//! - `TradingConstraints`: Trading constraints for validation
//! - `BondingCurve`: Zero-latency state replication for Pump.fun bonding curves
//! - `ShadowLedger`: Thread-safe in-memory cache with real-time market simulation
//! - `BuySimulationResult`: Pre-transaction buy simulation output
//! - `SellSimulationResult`: Pre-transaction sell simulation output
//! - `MarketSnapshot`: Geometric market state for derivative analysis

pub mod account_state_core;
pub mod checkpoint;
pub mod context_analysis;
pub mod coverage_audit;
pub mod enhanced_builder;
pub mod enhanced_candidate;
pub mod errors;
pub mod event_semantics;
pub mod event_time;
pub mod health;
pub mod init_pool_parser;
pub mod market_state;
pub mod pipeline_coverage;
pub mod pool_identity;
pub mod session;
pub mod shadow_ledger;
pub mod swap_plan;
pub mod trading_constraints;
pub mod transaction_parser;
pub mod tx_intelligence;
pub mod validation;
pub mod wal;

// Re-export main types
pub use context_analysis::{
    calculate_vanity_score, compute_metadata_len_score, liquidity_precision_penalty,
    TransactionContext,
};
pub use coverage_audit::{
    coverage_audit, CoverageAuditClosedWindow, CoverageAuditGlobalSignatureState,
    CoverageAuditInvariantSummary, CoverageAuditMissingSignature, CoverageAuditPoolSignatureState,
    CoverageAuditReason, CoverageAuditRecord, CoverageAuditRecorder,
    CoverageAuditTruthSignatureState, CoverageAuditWatchRegistration,
    CoverageAuditWindowDiagnostics, CoverageAuditWindowState,
};
pub use enhanced_builder::{
    analyze_bundle, analyze_transaction, EnhancedAnalysis, InstructionData, TransactionData,
};
pub use enhanced_candidate::EnhancedCandidate;
pub use errors::{GhostError, GhostResult};
pub use event_semantics::{
    normalize_account_update_semantics, normalize_transaction_semantics,
    record_event_semantic_metric, source_kind_from_label, EventCompleteness, EventSemanticEnvelope,
    EventTruthKind, SlotQuality, SourceKind, TimestampQuality,
};
pub use event_time::EventTimeMetadata;
pub use init_pool_parser::{
    compute_initialize_pool_discriminator,
    extract_accounts,
    extract_trade_accounts,
    is_initialize_pool,
    is_trade_instruction,
    log_unknown_instruction,
    parse_initialize_pool,
    parse_trade_instruction,
    AmmType,
    InitializePoolAccounts,
    InitializePoolData,
    TradeAccounts,
    // Trade parsing exports
    TradeData,
    PUMPFUN_BUY_DISCRIMINATOR,
    PUMPFUN_CREATE_DISCRIMINATOR,
    PUMPFUN_SELL_DISCRIMINATOR,
    PUMPSWAP_BUY_EXACT_QUOTE_IN_DISCRIMINATOR,
    PUMPSWAP_EVENT_BUY_DISCRIMINATOR,
    PUMPSWAP_EVENT_SELL_DISCRIMINATOR,
    PUMPSWAP_OUTER_WRAPPER_DISCRIMINATOR,
};
pub use market_state::{
    BondingCurve, ShadowBondingCurve, ShadowLedgerStateConfidence, ShadowLedgerWriteReason,
    ShadowLedgerWriteSource, ShadowLedgerWriteStrength,
};
pub use pipeline_coverage::{
    pipeline_coverage, PipelineCoverage, PipelineCoverageSnapshot, PipelineCoverageStage,
};
pub use pool_identity::{BaseMint, BondingCurveKey, PoolId, PoolIdentity, PoolIdentityRegistry};
pub use shadow_ledger::{
    build_market_snapshots_from_trades,
    calculate_price_impact,
    calculate_sell_price_impact,
    compute_all_derivatives,
    // Simulation functions (pure mathematical functions)
    simulate_buy_pure,
    simulate_buy_with_slippage_pure,
    simulate_sell_pure,
    simulate_sell_with_slippage_pure,
    AggressiveEvictionConfig,
    BuySimulationResult,
    CurveAwareStorage,
    CurveFinality,
    CurveFreshnessInfo,
    CurveFreshnessState,
    CurveInfo,
    // Curve storage types
    CurveStorage,
    CurveWriteMetadata,
    CurveWriteOutcome,
    DashMapCurveStorage,
    DashMapSnapshotStorage,
    DiagnosticStats,
    // Lazy loading types
    DirtyCurveMarker,
    DirtyCurveRegistry,
    // Eviction types
    EvictionConfig,
    EvictionManager,
    EvictionMetrics,
    EvictionResult,
    FlushResult,
    LivePipeline,
    LivePipelineConfig,
    LivePipelineError,
    LivePipelineStats,
    // Live pipeline types (EPIC 4)
    LiveTxEvent,
    LruConfig,
    MarketSnapshot,
    MintLiveState,
    SellSimulationResult,
    ShadowLedger,
    ShadowLedgerStaleFallback,
    ShadowLedgerWriteResult,
    ShardedCurveStorage,
    SnapshotBuffer,
    SnapshotDiagnostic,
    SnapshotSource,
    // Storage types
    SnapshotStorage,
    TradeSide,
    TradeSnapshot,
    TradeSnapshotError,
    TradeSource,
    TxKey,
    TxKeyError,
    BPS_DENOMINATOR,
    DEFAULT_FLUSH_DELAY_MS,
    DEFAULT_MAX_BUFFER_SIZE,
    DEFAULT_SEEN_KEYS_LIMIT,
    DEFAULT_SHARD_COUNT,
    DEFAULT_SLIPPAGE_BPS,
    DEFAULT_SNAPSHOT_MAX_AGE_MS,
    FEE_BPS,
    LAMPORTS_PER_SOL,
    PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY,
};
pub use swap_plan::{SwapPlan, SwapPlanBuilder};
pub use trading_constraints::TradingConstraints;
pub use transaction_parser::{
    error_code_from_status, error_code_from_transaction_error, extract_signers, is_set_authority,
    parse_create_metadata, parse_set_authority, parse_swap_instruction, ParsedMetadata, ProgramIds,
    SwapInfo, TransactionMetadata,
};
pub use validation::{validate_swap_plan, ValidationError, ValidationResult, Validator};
pub use wal::{
    CommitPersistedRecord, CommitStagedRecord, GatekeeperDecision, ParsedEventKind,
    ShadowLedgerCurveUpdateRecord, TradeForwardRecord, Wal, WalRecord, WalRecordClock,
    WalReplayEntry, WalStorageVersion, WalSyncMode,
};

/// Prelude module for convenient imports
pub mod prelude {
    pub use crate::errors::{GhostError, GhostResult};
    pub use crate::market_state::{BondingCurve, ShadowBondingCurve};
    pub use crate::shadow_ledger::{
        build_market_snapshots_from_trades,
        calculate_price_impact,
        compute_all_derivatives,
        // Simulation functions
        simulate_buy_pure,
        simulate_sell_pure,
        AggressiveEvictionConfig,
        BuySimulationResult,
        DiagnosticStats,
        // Eviction types
        EvictionConfig,
        EvictionManager,
        EvictionMetrics,
        EvictionResult,
        FlushResult,
        LivePipeline,
        LivePipelineConfig,
        LivePipelineStats,
        // Live pipeline types (EPIC 4)
        LiveTxEvent,
        LruConfig,
        MarketSnapshot,
        SellSimulationResult,
        ShadowLedger,
        SnapshotBuffer,
        TradeSide,
        TradeSnapshot,
        TradeSource,
        TxKey,
    };
    pub use crate::swap_plan::{SwapPlan, SwapPlanBuilder};
    pub use crate::trading_constraints::TradingConstraints;
    pub use crate::validation::{validate_swap_plan, ValidationError, ValidationResult, Validator};
}
