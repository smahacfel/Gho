//! Gene Mapper - Static Bytecode Analysis for Malicious Program Detection
//!
//! This module implements a high-performance bytecode analyzer that scans Solana
//! program accounts for malicious patterns during the "2-Second Void" period.
//!
//! ## Core Concept
//!
//! The Gene Mapper performs two-stage analysis:
//! 1. **Hash Lookup**: Calculate program hash and check against known malicious database
//! 2. **Opcode Scanning**: If not in database, scan for dangerous instruction patterns
//!
//! ## Algorithm
//!
//! ```text
//! 1. Fetch account.data (program bytecode)
//! 2. Calculate SHA256 hash using blake3
//! 3. Lookup hash in malicious program database
//!    ├─ HIT → Return HIGH_RISK (known malicious)
//!    └─ MISS → Scan for dangerous opcodes
//!        ├─ Detect FreezeAccount, SetAuthority, etc.
//!        ├─ Calculate aggregate risk score
//!        └─ Return risk assessment
//! ```
//!
//! ## Performance
//!
//! - **Hash Calculation**: ~50μs for 10KB program (blake3)
//! - **Opcode Scan**: ~100μs for 10KB program (linear scan)
//! - **Total Latency**: < 200μs typical, < 500μs worst-case
//!
//! ## Usage
//!
//! ```rust
//! use ghost_brain::security::GeneMapper;
//!
//! let mapper = GeneMapper::new();
//! let bytecode = vec![0x06, 0x03]; // SetAuthority + Transfer
//! let result = mapper.analyze(&bytecode);
//!
//! if result.is_high_risk() {
//!     println!("HIGH RISK: {} (score: {})", result.threat_summary, result.risk_score);
//! }
//! ```

use crate::security::signatures::{
    get_max_severity, is_malicious_program, scan_dangerous_opcodes, OpcodePattern,
};
use blake3::Hasher;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Default maximum scan depth (bytes)
/// Limits scanning to first N bytes for performance
pub const DEFAULT_MAX_SCAN_DEPTH: usize = 10_000;

/// Maximum bytecode size for hash calculation (bytes)
/// Prevents DoS from excessively large inputs
/// Solana programs are typically < 1MB, this provides headroom
const MAX_HASH_SIZE: usize = 2_000_000; // 2MB

/// Risk threshold for high-risk classification
pub const HIGH_RISK_THRESHOLD: f64 = 0.75;

/// Risk threshold for medium-risk classification
pub const MEDIUM_RISK_THRESHOLD: f64 = 0.50;

/// Time window (ms) to consider for sybil cluster detection
const SYBIL_CLUSTER_WINDOW_MS: u64 = 1000;

/// Retention window (ms) for recent pool transactions
const RECENT_TX_RETENTION_MS: u64 = 2000;

/// Minimum unique wallets to flag a sybil cluster
const MIN_SYBIL_WALLETS: usize = 4;

/// Configuration for Gene Mapper analysis
#[derive(Debug, Clone)]
pub struct GeneMapperConfig {
    /// Maximum bytes to scan (performance limiter)
    pub max_scan_depth: usize,

    /// Enable hash-based malicious program detection
    pub enable_hash_lookup: bool,

    /// Enable opcode pattern scanning
    pub enable_opcode_scan: bool,

    /// Minimum severity to report (filters low-risk patterns)
    pub min_severity_threshold: f64,
}

impl Default for GeneMapperConfig {
    fn default() -> Self {
        Self {
            max_scan_depth: DEFAULT_MAX_SCAN_DEPTH,
            enable_hash_lookup: true,
            enable_opcode_scan: true,
            min_severity_threshold: 0.0,
        }
    }
}

/// Result of gene mapping analysis
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GeneAnalysisResult {
    /// Overall risk score (0.0 = safe, 1.0 = critical)
    pub risk_score: f64,

    /// Risk level classification
    pub risk_level: RiskLevel,

    /// Program hash (blake3)
    pub program_hash: [u8; 32],

    /// Whether program matches known malicious hash
    pub is_known_malicious: bool,

    /// Detected dangerous patterns
    pub detected_patterns: Vec<DetectedPattern>,

    /// Human-readable threat summary
    pub threat_summary: String,

    /// Number of bytes analyzed
    pub bytes_scanned: usize,
}

/// Risk level classification
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum RiskLevel {
    /// No risk detected
    Safe,
    /// Low risk (informational)
    Low,
    /// Medium risk (caution advised)
    Medium,
    /// High risk (abort recommended)
    High,
    /// Critical risk (known malicious)
    Critical,
}

/// Detected dangerous pattern
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DetectedPattern {
    /// Pattern name
    pub name: String,
    /// Severity level
    pub severity: f64,
    /// Pattern description
    pub description: String,
}

#[derive(Debug, Clone)]
struct PoolTransaction {
    wallet: String,
    amount_sol: f64,
    timestamp_ms: u64,
}

impl GeneAnalysisResult {
    /// Check if result indicates high risk
    pub fn is_high_risk(&self) -> bool {
        matches!(self.risk_level, RiskLevel::High | RiskLevel::Critical)
    }

    /// Check if result indicates critical risk
    pub fn is_critical(&self) -> bool {
        matches!(self.risk_level, RiskLevel::Critical)
    }

    /// Get recommended action based on risk level
    pub fn recommended_action(&self) -> &'static str {
        match self.risk_level {
            RiskLevel::Safe => "PROCEED",
            RiskLevel::Low => "PROCEED_WITH_CAUTION",
            RiskLevel::Medium => "CAUTION",
            RiskLevel::High => "ABORT",
            RiskLevel::Critical => "ABORT_IMMEDIATELY",
        }
    }
}

/// Gene Mapper - Main bytecode analyzer
pub struct GeneMapper {
    config: GeneMapperConfig,
    recent_pool_transactions: HashMap<String, Vec<PoolTransaction>>,
}

impl GeneMapper {
    /// Create a new Gene Mapper with default configuration
    pub fn new() -> Self {
        Self {
            config: GeneMapperConfig::default(),
            recent_pool_transactions: HashMap::new(),
        }
    }

    /// Create a new Gene Mapper with custom configuration
    pub fn with_config(config: GeneMapperConfig) -> Self {
        Self {
            config,
            recent_pool_transactions: HashMap::new(),
        }
    }

    fn record_transaction(
        &mut self,
        pool_id: &str,
        wallet: &str,
        amount_sol: f64,
        timestamp_ms: u64,
    ) {
        let entry = self
            .recent_pool_transactions
            .entry(pool_id.to_string())
            .or_default();

        entry.push(PoolTransaction {
            wallet: wallet.to_string(),
            amount_sol,
            timestamp_ms,
        });

        let cutoff = timestamp_ms.saturating_sub(RECENT_TX_RETENTION_MS);
        entry.retain(|tx| tx.timestamp_ms >= cutoff);
    }

    fn detect_sybil_cluster(&self, pool_id: &str, now_ms: u64) -> Option<(f64, usize)> {
        let txs = self.recent_pool_transactions.get(pool_id)?;

        let mut grouped: HashMap<i64, Vec<&PoolTransaction>> = HashMap::new();
        for tx in txs.iter() {
            let amount_key = (tx.amount_sol * 1_000_000_000.0).round() as i64;
            grouped.entry(amount_key).or_default().push(tx);
        }

        for (amount_key, mut records) in grouped {
            if records.len() < MIN_SYBIL_WALLETS {
                continue;
            }

            records.sort_by_key(|tx| tx.timestamp_ms);

            let mut wallet_counts: HashMap<&str, usize> = HashMap::new();
            let mut start = 0usize;

            for end in 0..records.len() {
                let wallet = records[end].wallet.as_str();
                *wallet_counts.entry(wallet).or_insert(0) += 1;

                while records[end]
                    .timestamp_ms
                    .saturating_sub(records[start].timestamp_ms)
                    > SYBIL_CLUSTER_WINDOW_MS
                {
                    let start_wallet = records[start].wallet.as_str();
                    if let Some(count) = wallet_counts.get_mut(start_wallet) {
                        if *count <= 1 {
                            wallet_counts.remove(start_wallet);
                        } else {
                            *count -= 1;
                        }
                    }
                    start += 1;
                }

                if wallet_counts.len() >= MIN_SYBIL_WALLETS {
                    let amount_sol = amount_key as f64 / 1_000_000_000.0;
                    return Some((amount_sol, wallet_counts.len()));
                }
            }
        }

        None
    }

    /// Analyze program bytecode for malicious patterns
    ///
    /// # Arguments
    ///
    /// * `bytecode` - Program account data to analyze
    ///
    /// # Returns
    ///
    /// `GeneAnalysisResult` containing risk assessment
    ///
    /// # Example
    ///
    /// ```
    /// use ghost_brain::security::GeneMapper;
    ///
    /// let mapper = GeneMapper::new();
    /// let bytecode = vec![0x0e, 0x06]; // FreezeAccount + SetAuthority
    /// let result = mapper.analyze(&bytecode);
    /// assert!(result.is_high_risk());
    /// ```
    pub fn analyze(&self, bytecode: &[u8]) -> GeneAnalysisResult {
        // Limit scan depth for performance
        let scan_limit = std::cmp::min(bytecode.len(), self.config.max_scan_depth);
        let scan_data = &bytecode[..scan_limit];

        // Calculate program hash
        let program_hash = self.calculate_hash(bytecode);

        // Stage 1: Hash-based lookup
        let is_known_malicious = if self.config.enable_hash_lookup {
            is_malicious_program(&program_hash)
        } else {
            false
        };

        // If known malicious, return critical risk immediately
        if is_known_malicious {
            return GeneAnalysisResult {
                risk_score: 1.0,
                risk_level: RiskLevel::Critical,
                program_hash,
                is_known_malicious: true,
                detected_patterns: vec![],
                threat_summary: "CRITICAL: Known malicious program detected in database"
                    .to_string(),
                bytes_scanned: scan_limit,
            };
        }

        // Stage 2: Opcode scanning
        let mut detected_patterns = Vec::new();
        let mut max_severity = 0.0;

        if self.config.enable_opcode_scan {
            let patterns = scan_dangerous_opcodes(scan_data);

            for pattern in patterns {
                // Filter by minimum severity threshold
                if pattern.severity >= self.config.min_severity_threshold {
                    detected_patterns.push(DetectedPattern {
                        name: pattern.name.to_string(),
                        severity: pattern.severity,
                        description: pattern.description.to_string(),
                    });

                    if pattern.severity > max_severity {
                        max_severity = pattern.severity;
                    }
                }
            }
        }

        // Calculate aggregate risk score
        let risk_score = if detected_patterns.is_empty() {
            0.0
        } else {
            // Use max severity as base, with bonus for multiple patterns
            let pattern_count_bonus = ((detected_patterns.len() - 1) as f64 * 0.05).min(0.15);
            (max_severity + pattern_count_bonus).min(1.0)
        };

        // Classify risk level
        let risk_level = classify_risk(risk_score);

        // Generate threat summary
        let threat_summary = self.generate_threat_summary(&detected_patterns, risk_score);

        GeneAnalysisResult {
            risk_score,
            risk_level,
            program_hash,
            is_known_malicious: false,
            detected_patterns,
            threat_summary,
            bytes_scanned: scan_limit,
        }
    }

    /// Analyze bytecode with recent transaction context for sybil cluster detection
    pub fn analyze_with_transactions(
        &mut self,
        bytecode: &[u8],
        pool_id: &str,
        wallet: &str,
        amount_sol: f64,
        timestamp_ms: u64,
    ) -> GeneAnalysisResult {
        self.record_transaction(pool_id, wallet, amount_sol, timestamp_ms);

        let mut result = self.analyze(bytecode);

        if let Some((amount, wallet_count)) = self.detect_sybil_cluster(pool_id, timestamp_ms) {
            result.risk_score = 0.0;
            result.risk_level = RiskLevel::Critical;
            result.threat_summary = format!(
                "Sybil cluster detected: {} wallets bought {:.8} SOL within 1s",
                wallet_count, amount
            );

            result.detected_patterns.push(DetectedPattern {
                name: "SybilCluster".to_string(),
                severity: 1.0,
                description: "Coordinated identical-volume buys within 1s window".to_string(),
            });
        }

        result
    }

    /// Calculate blake3 hash of bytecode
    ///
    /// For performance, limits hash calculation to first MAX_HASH_SIZE bytes.
    /// This prevents DoS from excessively large inputs while maintaining
    /// accurate program identification (malicious code is typically at start).
    fn calculate_hash(&self, bytecode: &[u8]) -> [u8; 32] {
        let hash_limit = std::cmp::min(bytecode.len(), MAX_HASH_SIZE);
        let hash_data = &bytecode[..hash_limit];

        let mut hasher = Hasher::new();
        hasher.update(hash_data);
        *hasher.finalize().as_bytes()
    }

    /// Generate human-readable threat summary
    fn generate_threat_summary(&self, patterns: &[DetectedPattern], risk_score: f64) -> String {
        if patterns.is_empty() {
            return "No dangerous patterns detected".to_string();
        }

        let pattern_names: Vec<&str> = patterns.iter().map(|p| p.name.as_str()).collect();

        match patterns.len() {
            1 => format!(
                "Detected {} (severity: {:.2})",
                pattern_names[0], patterns[0].severity
            ),
            2 => format!(
                "Detected {} and {} (risk: {:.2})",
                pattern_names[0], pattern_names[1], risk_score
            ),
            _ => format!(
                "Detected {} dangerous patterns including {} (risk: {:.2})",
                patterns.len(),
                pattern_names[0],
                risk_score
            ),
        }
    }

    /// Quick hash-only check (for fast pre-screening)
    ///
    /// Useful when you only need to check against known malicious programs
    /// without performing full opcode analysis.
    pub fn quick_check(&self, bytecode: &[u8]) -> bool {
        let hash = self.calculate_hash(bytecode);
        is_malicious_program(&hash)
    }
}

impl Default for GeneMapper {
    fn default() -> Self {
        Self::new()
    }
}

/// Classify risk score into risk level
fn classify_risk(score: f64) -> RiskLevel {
    if score >= HIGH_RISK_THRESHOLD {
        RiskLevel::High
    } else if score >= MEDIUM_RISK_THRESHOLD {
        RiskLevel::Medium
    } else if score > 0.0 {
        RiskLevel::Low
    } else {
        RiskLevel::Safe
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gene_mapper_creation() {
        let mapper = GeneMapper::new();
        assert_eq!(mapper.config.max_scan_depth, DEFAULT_MAX_SCAN_DEPTH);
    }

    #[test]
    fn test_clean_bytecode() {
        let mapper = GeneMapper::new();
        let clean_code = vec![0x01, 0x02, 0x03, 0x04, 0x05];

        let result = mapper.analyze(&clean_code);

        assert_eq!(result.risk_level, RiskLevel::Safe);
        assert_eq!(result.risk_score, 0.0);
        assert!(!result.is_high_risk());
        assert!(result.detected_patterns.is_empty());
    }

    #[test]
    fn test_freeze_account_detection() {
        let mapper = GeneMapper::new();
        let malicious_code = vec![0x00, 0x0e, 0x01]; // Contains FreezeAccount

        let result = mapper.analyze(&malicious_code);

        assert!(result.risk_score > 0.0);
        assert!(!result.detected_patterns.is_empty());
        assert!(result
            .detected_patterns
            .iter()
            .any(|p| p.name == "FreezeAccount"));
    }

    #[test]
    fn test_authority_hijack_detection() {
        let mapper = GeneMapper::new();
        let malicious_code = vec![0x06, 0x03, 0x00]; // SetAuthority + Transfer

        let result = mapper.analyze(&malicious_code);

        assert!(result.risk_score > 0.0);
        assert!(result
            .detected_patterns
            .iter()
            .any(|p| p.name == "AuthorityHijack"));
        assert!(result.is_high_risk());
    }

    #[test]
    fn test_freeze_and_seize_detection() {
        let mapper = GeneMapper::new();
        let malicious_code = vec![0x0e, 0x06]; // FreezeAccount + SetAuthority

        let result = mapper.analyze(&malicious_code);

        assert!(result
            .detected_patterns
            .iter()
            .any(|p| p.name == "FreezeAndSeize"));
        assert_eq!(result.risk_level, RiskLevel::High);
        assert!(result.is_high_risk());
        assert_eq!(result.recommended_action(), "ABORT");
    }

    #[test]
    fn test_known_malicious_program() {
        let mapper = GeneMapper::new();

        // Create bytecode that will hash to a known malicious value
        // For testing, we'll use the hash directly
        let malicious_hash = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];

        // Check if this hash is detected as malicious
        assert!(is_malicious_program(&malicious_hash));
    }

    #[test]
    fn test_multiple_patterns_increase_risk() {
        let mapper = GeneMapper::new();

        // Bytecode with multiple dangerous patterns
        let multi_threat = vec![
            0x06, // SetAuthority
            0x07, // MintTo
            0x08, // Burn
            0x09, // CloseAccount
        ];

        let result = mapper.analyze(&multi_threat);

        // Multiple patterns should increase risk score
        assert!(result.detected_patterns.len() >= 3);
        assert!(result.risk_score > 0.5);
    }

    #[test]
    fn test_scan_depth_limit() {
        let mut config = GeneMapperConfig::default();
        config.max_scan_depth = 10;
        let mapper = GeneMapper::with_config(config);

        // Create large bytecode
        let mut large_code = vec![0x00; 100];
        large_code[50] = 0x0e; // FreezeAccount at position 50 (beyond scan limit)

        let result = mapper.analyze(&large_code);

        // Should only scan first 10 bytes
        assert_eq!(result.bytes_scanned, 10);
        // Should not detect pattern beyond scan depth
        assert!(result.detected_patterns.is_empty());
    }

    #[test]
    fn test_min_severity_filter() {
        let mut config = GeneMapperConfig::default();
        config.min_severity_threshold = 0.9; // Only report very high severity
        let mapper = GeneMapper::with_config(config);

        let code = vec![0x08]; // Burn (severity 0.5)
        let result = mapper.analyze(&code);

        // Should filter out low severity patterns
        assert!(result.detected_patterns.is_empty());
    }

    #[test]
    fn test_disabled_hash_lookup() {
        let mut config = GeneMapperConfig::default();
        config.enable_hash_lookup = false;
        let mapper = GeneMapper::with_config(config);

        // Any bytecode analyzed with hash lookup disabled
        let code = vec![0x01, 0x02];
        let result = mapper.analyze(&code);

        assert!(!result.is_known_malicious);
    }

    #[test]
    fn test_disabled_opcode_scan() {
        let mut config = GeneMapperConfig::default();
        config.enable_opcode_scan = false;
        let mapper = GeneMapper::with_config(config);

        let malicious_code = vec![0x0e, 0x06]; // Should be high risk
        let result = mapper.analyze(&malicious_code);

        // With opcode scan disabled, no patterns detected
        assert!(result.detected_patterns.is_empty());
    }

    #[test]
    fn test_sybil_cluster_detection_sets_score_zero() {
        let mut mapper = GeneMapper::new();
        let pool_id = "pool-1";
        let base_time_ms = 1_000u64;

        let mut last_result = None;
        for i in 0..4 {
            let wallet = format!("wallet{}", i);
            last_result = Some(mapper.analyze_with_transactions(
                &[],
                pool_id,
                &wallet,
                0.1,
                base_time_ms + (i as u64) * 200,
            ));
        }

        let result = last_result.unwrap();
        assert_eq!(result.risk_score, 0.0);
        assert!(result.is_high_risk());
        assert!(result.threat_summary.contains("Sybil cluster"));
        assert!(result
            .detected_patterns
            .iter()
            .any(|p| p.name == "SybilCluster"));
    }

    #[test]
    fn test_quick_check() {
        let mapper = GeneMapper::new();

        // Clean program
        let clean = vec![0xff; 100];
        assert!(!mapper.quick_check(&clean));
    }

    #[test]
    fn test_risk_classification() {
        assert_eq!(classify_risk(0.0), RiskLevel::Safe);
        assert_eq!(classify_risk(0.3), RiskLevel::Low);
        assert_eq!(classify_risk(0.6), RiskLevel::Medium);
        assert_eq!(classify_risk(0.8), RiskLevel::High);
    }

    #[test]
    fn test_threat_summary_generation() {
        let mapper = GeneMapper::new();

        // Single pattern
        let single = vec![DetectedPattern {
            name: "Test".to_string(),
            severity: 0.5,
            description: "Test pattern".to_string(),
        }];
        let summary = mapper.generate_threat_summary(&single, 0.5);
        assert!(summary.contains("Test"));

        // Multiple patterns
        let multiple = vec![
            DetectedPattern {
                name: "Pattern1".to_string(),
                severity: 0.7,
                description: "First".to_string(),
            },
            DetectedPattern {
                name: "Pattern2".to_string(),
                severity: 0.8,
                description: "Second".to_string(),
            },
            DetectedPattern {
                name: "Pattern3".to_string(),
                severity: 0.6,
                description: "Third".to_string(),
            },
        ];
        let summary = mapper.generate_threat_summary(&multiple, 0.85);
        assert!(summary.contains("3"));
    }

    #[test]
    fn test_recommended_actions() {
        let safe = GeneAnalysisResult {
            risk_score: 0.0,
            risk_level: RiskLevel::Safe,
            program_hash: [0; 32],
            is_known_malicious: false,
            detected_patterns: vec![],
            threat_summary: "Safe".to_string(),
            bytes_scanned: 100,
        };
        assert_eq!(safe.recommended_action(), "PROCEED");

        let high = GeneAnalysisResult {
            risk_score: 0.9,
            risk_level: RiskLevel::High,
            program_hash: [0; 32],
            is_known_malicious: false,
            detected_patterns: vec![],
            threat_summary: "High risk".to_string(),
            bytes_scanned: 100,
        };
        assert_eq!(high.recommended_action(), "ABORT");
    }
}
