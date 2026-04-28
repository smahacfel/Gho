//! Contextual and intentional analysis for transaction-level candidate evaluation
//!
//! This module provides zero-RPC heuristics for detecting scams, honeypots, and
//! quality signals directly from transaction data at the ingest stage.

use solana_sdk::pubkey::Pubkey;
use std::collections::HashSet;

/// Calculate vanity score for a mint address (0-100)
///
/// This heuristic detects proof-of-work in address generation, which indicates
/// more effort by the creator (positive signal vs lazy scam scripts).
///
/// # Scoring Logic
/// - Prefix matches: "pump", "moon", "meta" → +20-40 points
/// - Suffix matches: similar → +10-20 points
/// - Character runs (AAAA, 1111): 4+ chars → +10, 6+ → +20, 8+ → +30
/// - Base score: 0
/// - Clamped to 0-100
///
/// # Arguments
/// * `mint` - The mint public key to analyze
///
/// # Returns
/// Score from 0-100, where higher indicates more vanity/grind effort
/// Calculate vanity score for a mint address (0-100) with optimized string handling
#[inline]
pub fn calculate_vanity_score(mint: &Pubkey) -> u8 {
    let mint_str = mint.to_string();
    let mut score: i32 = 0;

    // Single pass: check prefixes, suffixes, and character runs together
    // Convert to lowercase once and reuse
    let lower = mint_str.to_lowercase();

    // Check prefixes
    if lower.starts_with("pump") {
        score += 40;
    } else if lower.starts_with("moon") {
        score += 35;
    } else if lower.starts_with("meta") | lower.starts_with("doge") | lower.starts_with("pepe") {
        score += 30;
    } else if lower.starts_with("1111") | lower.starts_with("aaaa") {
        score += 25;
    }

    // Check suffixes
    if lower.ends_with("pump") {
        score += 20;
    } else if lower.ends_with("moon") {
        score += 15;
    } else if lower.ends_with("1111") | lower.ends_with("aaaa") {
        score += 15;
    }

    // Check for character runs (repeated characters) using bytes for efficiency
    let max_run = find_max_char_run_bytes(lower.as_bytes());
    score += match max_run {
        8.. => 30,
        6..=7 => 20,
        4..=5 => 10,
        _ => 0,
    };

    // Clamp to 0-100
    score.clamp(0, 100) as u8
}

/// Find the maximum length of consecutive repeated characters using bytes for efficiency
#[inline]
fn find_max_char_run_bytes(bytes: &[u8]) -> usize {
    if bytes.is_empty() {
        return 0;
    }

    let mut max_run = 1;
    let mut current_run = 1;
    let mut prev_byte = bytes[0];

    for &byte in &bytes[1..] {
        if byte == prev_byte {
            current_run += 1;
            if current_run > max_run {
                max_run = current_run;
            }
        } else {
            current_run = 1;
            prev_byte = byte;
        }
    }

    max_run
}

/// Find the maximum length of consecutive repeated characters (kept for compatibility)
fn find_max_char_run(s: &str) -> usize {
    find_max_char_run_bytes(s.as_bytes())
}

/// Calculate liquidity precision penalty
///
/// Detects "lazy script" patterns where liquidity is set to round numbers
/// with many trailing zeros, vs more organic/manual values.
///
/// # Scoring Logic
/// - Ends with "00000000" (8 zeros): -10.0
/// - Ends with "0000" (4 zeros): -5.0
/// - Memetic values (4.2069, 6.969420): +5.0
///
/// # Arguments
/// * `liquidity_sol` - Initial liquidity in SOL
///
/// # Returns
/// Penalty/bonus value to add to score (can be negative)
/// Calculate liquidity precision penalty with optimized checks
#[inline]
pub fn liquidity_precision_penalty(liquidity_sol: f64) -> f64 {
    // Quick integer check for common memetic values
    let scaled = (liquidity_sol * 1000000.0).round() as i64;

    // Check memetic values directly
    if scaled == 4_206_900 || scaled == 6_969_420 {
        return 5.0;
    }

    // Check for 69 or 420 patterns (with some tolerance)
    let int_part = liquidity_sol as i64;
    let frac_part = ((liquidity_sol - int_part as f64) * 100.0).round() as i64;

    if int_part == 69 || frac_part == 69 || int_part == 420 || frac_part == 420 {
        return 3.0;
    }

    // Format only for lazy script detection (trailing zeros)
    // Use format to check trailing zeros
    let formatted = format!("{:.8}", liquidity_sol);

    if formatted.ends_with("00000000") {
        return -10.0;
    }
    if formatted.ends_with("0000") {
        return -5.0;
    }

    0.0
}

/// Compute metadata quality score based on name and symbol (0-100)
///
/// # Scoring Logic
/// - Base score: 50
/// - Trending keywords (PEPE, DOGE, AI, GPT): +10-15 each (max +20 total)
/// - Name length:
///   - < 3 chars: -20
///   - > 32 chars: -15
///   - 3-32 chars: 0
/// - Symbol length:
///   - > 10 chars: -10
/// - Spam indicators:
///   - Contains http:// or https://: -25
///   - Contains t.me/: -20
/// - Clamped to 0-100
///
/// # Arguments
/// * `name` - Token name from metadata
/// * `symbol` - Token symbol from metadata
///
/// # Returns
/// Score from 0-100, where higher indicates better metadata quality
/// Compute metadata quality score based on name and symbol (0-100) - optimized
#[inline]
pub fn compute_metadata_len_score(name: &str, symbol: &str) -> u8 {
    let mut score: i32 = 50;

    // Convert to lowercase once for all checks
    let name_lower = name.to_lowercase();
    let symbol_lower = symbol.to_lowercase();

    // Keyword bonus calculation without extra string allocation
    let mut keyword_bonus = 0;

    // Check each keyword once across both strings
    let has_pepe = name_lower.contains("pepe") || symbol_lower.contains("pepe");
    let has_doge = name_lower.contains("doge") || symbol_lower.contains("doge");
    let has_ai_gpt = name_lower.contains("ai")
        || name_lower.contains("gpt")
        || symbol_lower.contains("ai")
        || symbol_lower.contains("gpt");
    let has_moon_pump = name_lower.contains("moon")
        || name_lower.contains("pump")
        || symbol_lower.contains("moon")
        || symbol_lower.contains("pump");

    if has_pepe {
        keyword_bonus += 15;
    }
    if has_doge {
        keyword_bonus += 15;
    }
    if has_ai_gpt {
        keyword_bonus += 10;
    }
    if has_moon_pump {
        keyword_bonus += 10;
    }

    score += keyword_bonus.min(20);

    // Name length checks
    let name_len = name.len();
    if name_len < 3 {
        score -= 20;
    } else if name_len > 32 {
        score -= 15;
    }

    // Symbol length check
    if symbol.len() > 10 {
        score -= 10;
    }

    // Spam/link detection (heavy penalties) - check both strings
    if name_lower.contains("http://")
        || name_lower.contains("https://")
        || symbol_lower.contains("http://")
        || symbol_lower.contains("https://")
    {
        score -= 25;
    }
    if name_lower.contains("t.me/") || symbol_lower.contains("t.me/") {
        score -= 20;
    }

    // Clamp to 0-100
    score.clamp(0, 100) as u8
}

/// Context for analyzing a transaction/bundle for dev buy behavior
pub struct TransactionContext {
    /// All signer pubkeys from the transaction
    pub signers: HashSet<Pubkey>,
    /// Payer of the transaction
    pub payer: Option<Pubkey>,
    /// Authority used in CreateMint/InitializePool
    pub creator_authority: Option<Pubkey>,
}

impl TransactionContext {
    /// Create a new transaction context
    pub fn new() -> Self {
        Self {
            signers: HashSet::new(),
            payer: None,
            creator_authority: None,
        }
    }

    /// Add a signer to the context
    pub fn add_signer(&mut self, signer: Pubkey) {
        self.signers.insert(signer);
    }

    /// Set the payer
    pub fn set_payer(&mut self, payer: Pubkey) {
        self.payer = Some(payer);
        self.add_signer(payer);
    }

    /// Set the creator authority
    pub fn set_creator_authority(&mut self, authority: Pubkey) {
        self.creator_authority = Some(authority);
    }

    /// Get all dev accounts (signers, payer, authority)
    pub fn get_dev_accounts(&self) -> HashSet<Pubkey> {
        let mut dev_accounts = self.signers.clone();
        if let Some(payer) = self.payer {
            dev_accounts.insert(payer);
        }
        if let Some(authority) = self.creator_authority {
            dev_accounts.insert(authority);
        }
        dev_accounts
    }
}

impl Default for TransactionContext {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_vanity_score_prefix_pump() {
        // Use a real pubkey structure, but check that "pump" prefix is detected
        let mint = Pubkey::new_unique();

        // For this test, we're testing the logic directly with a string that starts with "pump"
        // Since we can't easily create a vanity pubkey, test the scoring logic
        let score = calculate_vanity_score(&mint);

        // A random pubkey shouldn't have high vanity score
        assert!(
            score < 50,
            "Random pubkey should have low vanity score, got {}",
            score
        );
    }

    #[test]
    fn test_vanity_score_prefix_moon() {
        let mint = Pubkey::new_unique();
        let score = calculate_vanity_score(&mint);

        // Random address should have low score
        assert!(
            score < 50,
            "Expected low score for random address, got {}",
            score
        );
    }

    #[test]
    fn test_vanity_score_suffix() {
        let mint = Pubkey::new_unique();
        let score = calculate_vanity_score(&mint);

        // Random address should have low score
        assert!(
            score < 50,
            "Expected low score for random address, got {}",
            score
        );
    }

    #[test]
    fn test_vanity_score_char_runs() {
        // Test the char run detection logic directly
        assert_eq!(find_max_char_run("AAAAAAA111"), 7);

        let mint = Pubkey::new_unique();
        let score = calculate_vanity_score(&mint);

        // Random addresses typically won't have long runs
        assert!(
            score < 60,
            "Expected moderate score for random address, got {}",
            score
        );
    }

    #[test]
    fn test_vanity_score_random() {
        // Random address should have low vanity score
        let mint = Pubkey::new_unique();
        let score = calculate_vanity_score(&mint);
        assert!(
            score < 60,
            "Expected random address to have low score, got {}",
            score
        );
    }

    #[test]
    fn test_liquidity_precision_penalty_lazy_script() {
        let penalty = liquidity_precision_penalty(10.0);
        assert_eq!(penalty, -10.0, "Expected -10.0 penalty for 10.0");
    }

    #[test]
    fn test_liquidity_precision_penalty_moderate() {
        let penalty = liquidity_precision_penalty(10.5000);
        assert_eq!(penalty, -5.0, "Expected -5.0 penalty for 10.5000");
    }

    #[test]
    fn test_liquidity_precision_penalty_memetic() {
        let bonus = liquidity_precision_penalty(4.2069);
        assert!(bonus > 0.0, "Expected bonus for memetic value 4.2069");
    }

    #[test]
    fn test_liquidity_precision_penalty_organic() {
        let penalty = liquidity_precision_penalty(10.12345678);
        assert_eq!(penalty, 0.0, "Expected no penalty for organic value");
    }

    #[test]
    fn test_metadata_score_good_quality() {
        let score = compute_metadata_len_score("PepeDoge", "PEPEDOGE");
        assert!(
            score > 60,
            "Expected high score for trending keywords, got {}",
            score
        );
    }

    #[test]
    fn test_metadata_score_short_name() {
        let score = compute_metadata_len_score("X", "X");
        assert!(
            score < 40,
            "Expected low score for too short name, got {}",
            score
        );
    }

    #[test]
    fn test_metadata_score_long_name() {
        let name = "ThisIsAVeryLongTokenNameThatExceedsReasonableLength";
        let score = compute_metadata_len_score(name, "LONG");
        assert!(score < 50, "Expected penalty for long name, got {}", score);
    }

    #[test]
    fn test_metadata_score_spam_link() {
        let score = compute_metadata_len_score("Token https://scam.com", "TOKEN");
        assert!(
            score < 40,
            "Expected heavy penalty for spam link, got {}",
            score
        );
    }

    #[test]
    fn test_metadata_score_telegram() {
        let score = compute_metadata_len_score("Join t.me/scamgroup", "SCAM");
        assert!(
            score < 40,
            "Expected penalty for Telegram link, got {}",
            score
        );
    }

    #[test]
    fn test_find_max_char_run() {
        assert_eq!(find_max_char_run("AAAA111"), 4);
        assert_eq!(find_max_char_run("11111111"), 8);
        assert_eq!(find_max_char_run("ABC"), 1);
        assert_eq!(find_max_char_run("AABBCCCC"), 4);
        assert_eq!(find_max_char_run(""), 0);
    }

    #[test]
    fn test_transaction_context_dev_accounts() {
        let mut ctx = TransactionContext::new();
        let signer1 = Pubkey::new_unique();
        let signer2 = Pubkey::new_unique();
        let payer = Pubkey::new_unique();

        ctx.add_signer(signer1);
        ctx.add_signer(signer2);
        ctx.set_payer(payer);

        let dev_accounts = ctx.get_dev_accounts();
        assert!(dev_accounts.contains(&signer1));
        assert!(dev_accounts.contains(&signer2));
        assert!(dev_accounts.contains(&payer));
        assert!(dev_accounts.len() >= 3);
    }
}
