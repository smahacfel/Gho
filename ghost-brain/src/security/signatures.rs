//! Malicious Program Signature Database
//!
//! This module contains a curated database of known malicious program signatures
//! and dangerous opcode patterns used for bytecode analysis.
//!
//! ## Signature Types
//!
//! 1. **Program Hashes**: SHA256 hashes of known malicious programs
//! 2. **Opcode Patterns**: Byte sequences representing dangerous instructions
//!
//! ## Usage
//!
//! ```rust
//! use ghost_brain::security::signatures::{is_malicious_program, scan_dangerous_opcodes};
//!
//! let program_hash = [0u8; 32];
//! if is_malicious_program(&program_hash) {
//!     println!("Known malicious program detected!");
//! }
//! ```

use once_cell::sync::Lazy;
use std::collections::HashSet;

/// Known malicious program hashes (SHA256)
///
/// This list is maintained through community reports and security analysis.
/// Programs are added here after verification of malicious behavior such as:
/// - Rug pulls
/// - Honey pots
/// - Unauthorized token freezing
/// - Authority manipulation
static MALICIOUS_PROGRAM_HASHES: Lazy<HashSet<[u8; 32]>> = Lazy::new(|| {
    let mut set = HashSet::new();

    // Example malicious program hashes
    // In production, these would be populated from:
    // 1. Community reports
    // 2. On-chain analysis
    // 3. Security audits
    // 4. Rug pull databases

    // Placeholder examples (would be real hashes in production)
    set.insert([
        0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d, 0x0e,
        0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b, 0x1c, 0x1d,
        0x1e, 0x1f,
    ]);

    set
});

/// Dangerous opcode patterns that indicate risky program behavior
///
/// These patterns are derived from Solana's instruction set and BPF bytecode.
/// Detection of these patterns doesn't necessarily mean malicious intent,
/// but warrants additional scrutiny.
#[derive(Debug, Clone)]
pub struct OpcodePattern {
    /// Human-readable name of the pattern
    pub name: &'static str,
    /// Byte sequence to search for
    pub pattern: &'static [u8],
    /// Severity level (0.0 = low, 1.0 = critical)
    pub severity: f64,
    /// Description of what this pattern does
    pub description: &'static str,
}

/// Database of known dangerous opcode patterns
static DANGEROUS_OPCODES: Lazy<Vec<OpcodePattern>> = Lazy::new(|| {
    vec![
        // SPL Token Program: FreezeAccount instruction
        // Discriminator: 14 (0x0E)
        OpcodePattern {
            name: "FreezeAccount",
            pattern: &[0x0e],
            severity: 0.9,
            description: "Can freeze token accounts, preventing transfers",
        },
        // SPL Token Program: SetAuthority instruction
        // Discriminator: 6 (0x06)
        OpcodePattern {
            name: "SetAuthority",
            pattern: &[0x06],
            severity: 0.8,
            description: "Can change authority over accounts",
        },
        // SPL Token Program: MintTo instruction
        // Discriminator: 7 (0x07)
        OpcodePattern {
            name: "MintTo",
            pattern: &[0x07],
            severity: 0.6,
            description: "Can mint new tokens (potential inflation attack)",
        },
        // SPL Token Program: Burn instruction
        // Discriminator: 8 (0x08)
        OpcodePattern {
            name: "Burn",
            pattern: &[0x08],
            severity: 0.5,
            description: "Can burn tokens from accounts",
        },
        // SPL Token Program: CloseAccount instruction
        // Discriminator: 9 (0x09)
        OpcodePattern {
            name: "CloseAccount",
            pattern: &[0x09],
            severity: 0.7,
            description: "Can close token accounts and reclaim rent",
        },
        // Suspicious multi-instruction patterns
        // SetAuthority followed by Transfer (potential authority hijack)
        OpcodePattern {
            name: "AuthorityHijack",
            pattern: &[0x06, 0x03], // SetAuthority + Transfer
            severity: 0.95,
            description: "SetAuthority immediately followed by Transfer (suspicious)",
        },
        // FreezeAccount followed by SetAuthority (potential lockout)
        OpcodePattern {
            name: "FreezeAndSeize",
            pattern: &[0x0e, 0x06], // FreezeAccount + SetAuthority
            severity: 1.0,
            description: "Freeze followed by authority change (classic rug pull pattern)",
        },
    ]
});

/// Check if a program hash matches a known malicious program
///
/// # Arguments
///
/// * `hash` - SHA256 hash of the program bytecode
///
/// # Returns
///
/// `true` if the hash is in the malicious program database
///
/// # Example
///
/// ```
/// use ghost_brain::security::signatures::is_malicious_program;
///
/// let hash = [0u8; 32];
/// if is_malicious_program(&hash) {
///     println!("Malicious program detected!");
/// }
/// ```
pub fn is_malicious_program(hash: &[u8; 32]) -> bool {
    MALICIOUS_PROGRAM_HASHES.contains(hash)
}

/// Add a new malicious program hash to the runtime database
///
/// **Note:** This is a placeholder for future implementation.
/// Currently not functional as the signature database is immutable.
///
/// For production use, this would require:
/// - Wrapping MALICIOUS_PROGRAM_HASHES in RwLock
/// - Persistent storage (Redis/SQLite)
/// - Thread-safe updates
///
/// # Arguments
///
/// * `hash` - SHA256 hash to add to the malicious list
#[allow(unused_variables)]
pub fn add_malicious_program(hash: [u8; 32]) {
    // TODO: Implement runtime database updates
    // Requires refactoring to use:
    // static MALICIOUS_PROGRAMS: Lazy<RwLock<HashSet<[u8; 32]>>> = ...
    unimplemented!("Dynamic signature updates not yet implemented. Use static database for now.")
}

/// Scan bytecode for dangerous opcode patterns
///
/// # Arguments
///
/// * `bytecode` - The program bytecode to scan
///
/// # Returns
///
/// Vector of detected patterns with their severities
///
/// # Example
///
/// ```
/// use ghost_brain::security::signatures::scan_dangerous_opcodes;
///
/// let bytecode = vec![0x06, 0x03, 0x00]; // SetAuthority + Transfer
/// let detected = scan_dangerous_opcodes(&bytecode);
/// for pattern in detected {
///     println!("Detected: {} (severity: {})", pattern.name, pattern.severity);
/// }
/// ```
pub fn scan_dangerous_opcodes(bytecode: &[u8]) -> Vec<&OpcodePattern> {
    let mut detected = Vec::new();

    // Scan for each pattern
    for pattern in DANGEROUS_OPCODES.iter() {
        if contains_pattern(bytecode, pattern.pattern) {
            detected.push(pattern);
        }
    }

    detected
}

/// Helper function to check if bytecode contains a specific pattern
///
/// Uses a sliding window approach for efficient pattern matching
fn contains_pattern(haystack: &[u8], needle: &[u8]) -> bool {
    if needle.is_empty() || haystack.len() < needle.len() {
        return false;
    }

    haystack
        .windows(needle.len())
        .any(|window| window == needle)
}

/// Get the maximum severity from a list of detected patterns
///
/// # Arguments
///
/// * `patterns` - List of detected opcode patterns
///
/// # Returns
///
/// Maximum severity value, or 0.0 if no patterns detected
pub fn get_max_severity(patterns: &[&OpcodePattern]) -> f64 {
    patterns
        .iter()
        .map(|p| p.severity)
        .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
        .unwrap_or(0.0)
}

/// Get all dangerous opcode patterns (for inspection/testing)
pub fn get_all_patterns() -> Vec<OpcodePattern> {
    DANGEROUS_OPCODES.clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_malicious_program() {
        // Known malicious hash
        let malicious_hash = [
            0x00, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0a, 0x0b, 0x0c, 0x0d,
            0x0e, 0x0f, 0x10, 0x11, 0x12, 0x13, 0x14, 0x15, 0x16, 0x17, 0x18, 0x19, 0x1a, 0x1b,
            0x1c, 0x1d, 0x1e, 0x1f,
        ];
        assert!(is_malicious_program(&malicious_hash));

        // Unknown hash
        let clean_hash = [0xff; 32];
        assert!(!is_malicious_program(&clean_hash));
    }

    #[test]
    fn test_contains_pattern() {
        let bytecode = vec![0x01, 0x02, 0x06, 0x03, 0x04];

        // Should find SetAuthority (0x06)
        assert!(contains_pattern(&bytecode, &[0x06]));

        // Should find SetAuthority + Transfer (0x06, 0x03)
        assert!(contains_pattern(&bytecode, &[0x06, 0x03]));

        // Should not find non-existent pattern
        assert!(!contains_pattern(&bytecode, &[0xff, 0xfe]));

        // Empty pattern
        assert!(!contains_pattern(&bytecode, &[]));

        // Pattern longer than bytecode
        assert!(!contains_pattern(&[0x01], &[0x01, 0x02, 0x03]));
    }

    #[test]
    fn test_scan_dangerous_opcodes() {
        // Bytecode with FreezeAccount instruction
        let bytecode = vec![0x00, 0x0e, 0x01, 0x02];
        let detected = scan_dangerous_opcodes(&bytecode);

        assert!(!detected.is_empty());
        assert!(detected.iter().any(|p| p.name == "FreezeAccount"));
    }

    #[test]
    fn test_scan_authority_hijack_pattern() {
        // SetAuthority followed by Transfer
        let bytecode = vec![0x00, 0x06, 0x03, 0x01];
        let detected = scan_dangerous_opcodes(&bytecode);

        assert!(!detected.is_empty());
        assert!(detected.iter().any(|p| p.name == "AuthorityHijack"));
    }

    #[test]
    fn test_scan_freeze_and_seize_pattern() {
        // FreezeAccount followed by SetAuthority (rug pull pattern)
        let bytecode = vec![0x0e, 0x06, 0x00];
        let detected = scan_dangerous_opcodes(&bytecode);

        assert!(!detected.is_empty());
        let freeze_seize = detected.iter().find(|p| p.name == "FreezeAndSeize");
        assert!(freeze_seize.is_some());
        assert_eq!(freeze_seize.unwrap().severity, 1.0);
    }

    #[test]
    fn test_clean_bytecode() {
        // Clean bytecode without dangerous patterns
        let bytecode = vec![0x01, 0x02, 0x03, 0x04, 0x05];
        let detected = scan_dangerous_opcodes(&bytecode);

        // Should not detect any patterns (unless pattern is subset of these bytes)
        // This specific sequence shouldn't match our patterns
        assert!(detected.is_empty());
    }

    #[test]
    fn test_get_max_severity() {
        let patterns = get_all_patterns();
        let pattern_refs: Vec<&OpcodePattern> = patterns.iter().collect();

        let max_sev = get_max_severity(&pattern_refs);
        assert!(max_sev > 0.0);
        assert!(max_sev <= 1.0);

        // Empty list should return 0.0
        assert_eq!(get_max_severity(&[]), 0.0);
    }

    #[test]
    fn test_all_patterns_have_valid_severity() {
        let patterns = get_all_patterns();
        for pattern in patterns {
            assert!(pattern.severity >= 0.0 && pattern.severity <= 1.0);
            assert!(!pattern.name.is_empty());
            assert!(!pattern.pattern.is_empty());
            assert!(!pattern.description.is_empty());
        }
    }
}
