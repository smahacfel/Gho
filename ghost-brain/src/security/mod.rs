//! Security Module - Static Analysis and Threat Detection
//!
//! This module provides tools for analyzing Solana program bytecode to detect
//! malicious patterns and known threats during the "2-Second Void" period.
//!
//! ## Components
//!
//! - **Gene Mapper**: Main bytecode analyzer for threat detection
//! - **Signatures**: Database of known malicious programs and dangerous opcodes
//!
//! ## Usage
//!
//! ```rust
//! use ghost_brain::security::{GeneMapper, GeneMapperConfig};
//!
//! let mapper = GeneMapper::new();
//! let bytecode = vec![0x0e, 0x06]; // Dangerous pattern
//! let result = mapper.analyze(&bytecode);
//!
//! if result.is_high_risk() {
//!     println!("ABORT: {}", result.threat_summary);
//! }
//! ```

pub mod cabal_detector;
pub mod gene_mapper;
pub mod signatures;

pub use cabal_detector::{
    CabalDetectorConfig, HolderProfile, RejectReason, SecurityEngine, TokenContext, Verdict,
};

// Re-export main types
pub use gene_mapper::{
    DetectedPattern, GeneAnalysisResult, GeneMapper, GeneMapperConfig, RiskLevel,
    DEFAULT_MAX_SCAN_DEPTH, HIGH_RISK_THRESHOLD, MEDIUM_RISK_THRESHOLD,
};

pub use signatures::{
    get_all_patterns, get_max_severity, is_malicious_program, scan_dangerous_opcodes, OpcodePattern,
};
