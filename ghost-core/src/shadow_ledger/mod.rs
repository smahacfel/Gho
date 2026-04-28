//! Shadow Ledger - Zero-Latency State Replication & Real-Time Market Simulator
//!
//! This module provides thread-safe, in-memory replication of Pump.fun bonding curve state,
//! eliminating RPC latency in the critical trading path. It also serves as a **real-time
//! market simulator** for pre-transaction analysis.
//!
//! ## Module Structure
//!
//! - `types` - Pure data types (MarketSnapshot, SnapshotBuffer, BuySimulationResult, SellSimulationResult)
//! - `trade_types` - Canonical TX-level trade snapshot types and projections
//! - `simulation` - Pure mathematical simulation functions (stateless, no storage access)
//! - `storage` - Thread-safe persistence layer for curves and snapshots (DashMap abstraction, sharding)
//! - `eviction` - Age-based snapshot cleanup and diagnostic tools
//! - `bootstrap` - G0/G1/G2 snapshot generation and initialization logic
//! - `commit_types` - Neutral commit/config result types
//! - `history_types` - Neutral buffered-history and reconstruction helpers
//! - `ledger` - Main ShadowLedger implementation with simulation and snapshot management
//!
//! ## Storage Architecture
//!
//! The `storage` module provides multiple storage implementations:
//! - `DashMapCurveStorage` - Standard lock-free curve storage
//! - `ShardedCurveStorage` - Horizontally-scaled storage with Pubkey prefix sharding
//! - `DashMapSnapshotStorage` - Snapshot buffer storage
//! - `DirtyCurveRegistry` - Foundation for lazy loading / disk eviction
//!
//! ## Overview
//!
//! The Shadow Ledger maintains a high-performance cache of bonding curve states indexed by
//! mint address (Pubkey). It uses `DashMap` to provide:
//! - **Lock-free concurrent access** via fine-grained sharding
//! - **Shared ownership** via Arc for multi-threaded environments
//! - **Zero-copy reads** for hot-path price calculations
//! - **Sub-50ns simulation** for pre-transaction price/slippage/impact analysis
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────────────┐
//! │                      Shadow Ledger                              │
//! │  DashMap<Pubkey, ShadowBondingCurve>  (Lock-Free)              │
//! └─────────────────────────────────────────────────────────────────┘
//!                          │
//!           ┌──────────────┼──────────────┐
//!           ▼              ▼              ▼
//!     [Thread 1]      [Thread 2]     [Thread N]
//!     (Simulator)     (Updater)      (Simulator)
//!        │                │               │
//!        ▼                ▼               ▼
//!   SimulationResult  insert()      SimulationResult
//! ```
//!
//! ## Eviction & Cleanup
//!
//! The `eviction` module provides age-based garbage collection for snapshot buffers:
//!
//! ```ignore
//! use ghost_core::shadow_ledger::eviction::{EvictionManager, EvictionConfig};
//!
//! // Create eviction manager
//! let eviction = EvictionManager::new(storage);
//!
//! // Periodic cleanup (recommended: every 30-60 seconds on mainnet)
//! let result = eviction.evict_stale_default();
//! println!("Evicted {} stale snapshots", result.evicted_count);
//!
//! // Force cleanup on shutdown
//! eviction.force_cleanup();
//! ```
//!
//! ## Usage Example
//!
//! ```rust
//! use ghost_core::shadow_ledger::ShadowLedger;
//! use ghost_core::market_state::BondingCurve;
//! use solana_sdk::pubkey::Pubkey;
//!
//! // Create a new shadow ledger
//! let ledger = ShadowLedger::new();
//!
//! // Insert a bonding curve state
//! let mint = Pubkey::new_unique();
//! let curve = BondingCurve {
//!     discriminator: 0,
//!     virtual_token_reserves: 1_000_000_000,
//!     virtual_sol_reserves: 30_000_000,
//!     real_token_reserves: 800_000_000,
//!     real_sol_reserves: 20_000_000,
//!     token_total_supply: 1_000_000_000,
//!     complete: 0,
//!     _padding: [0; 7],
//! };
//! ledger.insert(mint, curve);
//!
//! // Simulate a buy BEFORE sending transaction
//! if let Ok(sim) = ledger.simulate_buy(&mint, 1_000_000_000, 1000) {
//!     println!("Expected tokens: {}", sim.tokens_out);
//!     println!("Price impact: {}%", sim.price_impact_percent);
//!     println!("Min tokens out (with slippage): {}", sim.min_tokens_out);
//! }
//! ```

pub mod bootstrap;
pub mod canonical_tx; // C2: Canonical transaction event type
pub mod commit_types;
pub mod disk_snapshot;
pub mod drift_observability;
pub mod eviction;
pub mod forward_simulation;
pub mod genesis;
pub mod history_types;
mod ledger;
pub mod live_pipeline;
pub mod pipeline_metrics;
pub mod reconciliation;
pub mod reconciliation_runtime;
pub mod simulation;
pub mod storage;
pub mod trade_types;
pub mod types;

// Re-export types from types module
pub use crate::market_state::{
    ShadowLedgerStateConfidence, ShadowLedgerWriteReason, ShadowLedgerWriteSource,
    ShadowLedgerWriteStrength,
};
pub use commit_types::{CommitResult, GatekeeperStats};
pub use history_types::{
    build_trade_snapshots_observed, BufferedTx, CurveFinality, HistoryError, ReconciliationDiff,
    ReconciliationPoint, ReconstructedState,
};
pub use ledger::{CurveWriteMetadata, CurveWriteOutcome, ShadowLedgerWriteResult};
pub use trade_types::{
    build_market_snapshots_from_trades, TradeSide, TradeSnapshot, TradeSnapshotError, TradeSource,
    TxKey, TxKeyError,
};
pub use types::{
    current_time_ms, BuySimulationResult, BvaArchive, BvaClassification, BvaMetrics,
    MarketSnapshot, SellSimulationResult, SnapshotBuffer, DEFAULT_SNAPSHOT_MAX_AGE_MS,
    DERIVATIVE_EPSILON, LAMPORTS_PER_SOL,
};

// Re-export simulation functions (pure mathematical functions)
pub use simulation::{
    apply_slippage_bps,
    calculate_d_price_d_liquidity,
    calculate_d_price_d_slippage,
    // Derivative calculations
    calculate_d_price_d_volume,
    // Price impact calculations
    calculate_price_impact,
    calculate_price_impact_bps,
    calculate_sell_price_impact,
    calculate_sol_out_fixed,
    // Fixed-point arithmetic
    calculate_tokens_out_fixed,
    compute_all_derivatives,
    // Micro-simulation for bootstrap
    micro_simulate_buy,
    // Core buy/sell simulation
    simulate_buy_pure,
    simulate_buy_with_slippage_pure,
    simulate_sell_pure,
    simulate_sell_with_slippage_pure,
    // Constants
    BPS_DENOMINATOR,
    DEFAULT_SLIPPAGE_BPS,
    FEE_BPS,
    FIXED_POINT_PRECISION,
};

// Re-export storage types
pub use storage::{
    CurveAwareStorage,
    // Curve info for eviction
    CurveInfo,
    // Curve storage
    CurveStorage,
    DashMapCurveStorage,
    DashMapSnapshotStorage,
    // Lazy loading / dirty curves
    DirtyCurveMarker,
    DirtyCurveRegistry,
    ShardedCurveStorage,
    // Snapshot storage
    SnapshotStorage,
    DEFAULT_SHARD_COUNT,
    ESTIMATED_SERIALIZED_CURVE_SIZE,
};

// Re-export eviction types and functions
pub use eviction::{
    evict_stale,
    evict_stale_default,
    force_cleanup,
    // New eviction config types
    AggressiveEvictionConfig,
    DiagnosticStats,
    EvictionConfig,
    EvictionManager,
    EvictionMetrics,
    EvictionResult,
    LruConfig,
    SnapshotDiagnostic,
};

// Re-export bootstrap types and functions
pub use bootstrap::{
    // Core functions
    bootstrap_snapshots,
    bootstrap_with_result,
    check_payload_entropy,
    // Individual snapshot generators
    generate_g0,
    generate_g1,
    generate_g2,
    generate_quick_seed,
    // Configuration
    BootstrapConfig,
    // Metrics
    BootstrapMetrics,
    // Result tracking
    BootstrapResult,
    // Synthetic seed generation
    SyntheticTransaction,
};
pub use genesis::{
    protocol_genesis_curve, PROTOCOL_GENESIS_REAL_SOL_RESERVES,
    PROTOCOL_GENESIS_REAL_TOKEN_RESERVES, PROTOCOL_GENESIS_TOKEN_TOTAL_SUPPLY,
    PROTOCOL_GENESIS_VIRTUAL_SOL_RESERVES, PROTOCOL_GENESIS_VIRTUAL_TOKEN_RESERVES,
};

pub use ledger::{
    CommitHistoryResult, CommitHistoryStatus, CurveFreshnessInfo, CurveFreshnessState,
    ShadowLedgerStaleFallback,
};

// Re-export live pipeline types and functions (EPIC 4)
pub use live_pipeline::{
    // Result types
    FlushResult,
    // Pipeline
    LivePipeline,
    LivePipelineConfig,
    // Error types
    LivePipelineError,
    LivePipelineStats,
    // Event types
    LiveTxEvent,
    // State types
    MintLiveState,
    // Constants
    DEFAULT_FLUSH_DELAY_MS,
    DEFAULT_MAX_BUFFER_SIZE,
    DEFAULT_SEEN_KEYS_LIMIT,
};

// Re-export canonical transaction types (EPIC C2)
pub use canonical_tx::{CanonicalTxEvent, CanonicalTxEventError};

// Re-export drift observability, hot-pool tx-loss, and replay validation
pub use drift_observability::{
    // Convenience helper
    record_reconciliation_outcome,
    // Drift report
    DriftObservabilityReport,
    HotPoolTxLossTracker,
    // Per-pool drift stats
    PoolDriftStats,
    // Hot-pool tx-loss
    PoolTxLossSummary,
    // Replay validation
    ReplayTx,
    ReplayValidationResult,
    ReplayValidator,
    // Constant
    HOT_POOL_TX_THRESHOLD,
};

// Re-export reconciliation / healing layer types
pub use reconciliation::{
    // Policy
    DriftPolicy,
    // Classification
    DriftSeverity,
    // Action / outcome
    ReconciliationAction,
    ReconciliationOutcome,
    // Main entrypoint
    ShadowLedgerReconciler,
    // Thresholds
    MEANINGFUL_THRESHOLD_LAMPORTS,
    NOISE_THRESHOLD_LAMPORTS,
    SEVERE_THRESHOLD_LAMPORTS,
};

// Re-export main ShadowLedger from ledger module
pub use ledger::{ShadowLedger, SnapshotSource};

// Re-export disk snapshot types
pub use disk_snapshot::{
    DiskSnapshot, SnapshotError, SnapshotRestoreStats, SnapshotWriteStats, SNAPSHOT_FORMAT_VERSION,
};

// Re-export forward simulation / execution guardrail layer
pub use forward_simulation::{
    // Core simulation functions
    apply_hypothetical_buy,
    apply_hypothetical_sell,
    // Bundle assessment
    assess_bundle,
    // Single-trade assessment
    assess_buy,
    assess_sell,
    // Guardrail evaluation
    evaluate_guardrails,
    simulate_forward_bundle,
    simulate_forward_buy,
    simulate_forward_sell,
    BundleAssessment,
    BundleStepAssessment,
    ForwardBundleResult,
    // Input types
    ForwardSimAction,
    // Result types
    ForwardSimResult,
    // Guardrail config
    GuardrailConfig,
    // Rejection reasons
    RejectionReason,
    // Assessment types
    TradeAssessment,
};

// Re-export reconciliation runtime — explicit production loop
pub use reconciliation_runtime::{
    ReconciliationRuntime, ReconciliationRuntimeConfig, ReconciliationRuntimeStatus,
};
