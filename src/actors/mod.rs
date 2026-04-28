//! Actor System Foundation Module
//!
//! This module provides an actor-based architecture for the bot components,
//! enabling better scalability, fault tolerance, and message-based communication.
//!
//! # Architecture
//!
//! The actor system consists of:
//! - **OracleActor**: Wraps PredictiveOracle for candidate scoring and Ghost Intelligence
//! - **StorageActor**: Wraps DecisionLedger for persistent storage
//! - **MonitorActor**: Wraps TransactionMonitor for transaction tracking
//! - **SupervisorActor**: Manages all actors with restart policies
//!
//! # Ghost Intelligence (Task 12)
//!
//! The OracleActor integrates Ghost Intelligence for post-buy analysis:
//! - DevProfiler: Behavioral analysis of token creators
//! - ClusterHunter: Cabal/Sniper cluster detection
//! - VisionCritic: AI-powered meme quality assessment
//!
//! On TokenBought events, OracleActor runs all three analyses in parallel
//! and aggregates results to make exit decisions (Panic Sell vs HODL).
//!
//! # Message Passing
//!
//! Actors communicate asynchronously via message passing. Each actor has
//! a mailbox that receives messages and processes them sequentially.
//!
//! # Fault Tolerance
//!
//! The SupervisorActor implements supervision strategies to automatically
//! restart crashed actors, ensuring system resilience.

pub mod messages;
pub mod monitor_actor;
pub mod oracle_actor;
pub mod storage_actor;
pub mod supervisor_actor;

// Re-export main types for convenience
pub use messages::*;
pub use monitor_actor::MonitorActor;
pub use oracle_actor::{GhostIntelligenceConfig, OracleActor};
pub use storage_actor::StorageActor;
pub use supervisor_actor::{SupervisionStrategy, SupervisorActor};
