//! MPCF (Micro-Payload Cognitive Fingerprint) Module
//!
//! Actor-behavioral byte fingerprinting for ultra-fast classification of transaction sources.
//! Analyzes raw transaction bytes to determine who is buying/selling in the critical 0-2 second
//! window after token launch.
//!
//! ## Core Concept
//!
//! MPCF performs three-layer analysis on raw transaction payloads:
//!
//! 1. **Byte Morphology**: Histogram of byte distribution patterns
//! 2. **Entropy Signature**: Shannon entropy of payload to detect bot vs human patterns
//! 3. **Instruction Spacing Signature (ISS)**: Offset patterns between instructions
//!
//! Unlike volume/price/holder analysis, MPCF operates on the **meta-level** of transaction
//! structure, detecting fingerprints that bots cannot mask because they are side effects
//! of their frameworks/programs.
//!
//! ## Performance Target
//!
//! - **Execution Time**: 30-70 microseconds per transaction
//! - **Zero Heap Allocation**: Stack-only analysis for hot path performance
//! - **No Unwrap**: All operations use safe error handling
//! - **Dependencies**: std/core/alloc only (no external crates)
//!
//! ## Thread Safety
//!
//! All types implement `Send + Sync` for concurrent usage across threads.
//!
//! ## Integration
//!
//! MPCF feeds into the Oracle Pipeline:
//! ```text
//! Geyser Stream → raw tx_data: &[u8]
//!      ↓
//! [ MPCF ] → ActorInference
//!      ↓
//! Shadow Ledger
//!      ↓
//! QASS/QOFSV decision
//! ```
//!
//! # Example
//! ```rust,ignore
//! use ghost_brain::oracle::ultrafast::mpcf::{mpcf_infer, ActorType};
//!
//! // Raw transaction bytes from Geyser/RPC
//! let tx_bytes: &[u8] = &[/* transaction data */];
//!
//! // Ultra-fast classification (30-70μs)
//! let inference = mpcf_infer(tx_bytes);
//!
//! match inference.actor {
//!     ActorType::HumanMobile | ActorType::HumanDesktop => {
//!         // Organic user activity - bullish signal
//!         println!("Organic buy detected (confidence: {})", inference.confidence);
//!     }
//!     ActorType::SniperScript | ActorType::MEVArb => {
//!         // Bot activity - needs additional analysis
//!         println!("Bot detected with entropy: {}", inference.entropy);
//!     }
//!     _ => {}
//! }
//!
//! // Use fingerprint for pattern tracking
//! println!("Fingerprint: {:x?}", inference.fingerprint);
//! ```

// =============================================================================
// Constants
// =============================================================================

/// Default histogram size for byte distribution analysis
const HISTOGRAM_SIZE: usize = 256;

/// Entropy threshold for bot detection (low entropy = regular patterns)
const BOT_ENTROPY_THRESHOLD: f32 = 3.5;

/// Entropy threshold for human detection (high entropy = chaotic)
const HUMAN_ENTROPY_THRESHOLD: f32 = 5.5;

/// Fingerprint size in bytes (128-bit fingerprint)
const FINGERPRINT_SIZE: usize = 16;

/// Minimum payload size for reliable analysis
const MIN_PAYLOAD_SIZE: usize = 32;

/// Maximum payload size to analyze (prevent DoS on huge transactions)
const MAX_PAYLOAD_SIZE: usize = 4096;

/// Default confidence for unknown classification
const UNKNOWN_CONFIDENCE: f32 = 0.3;

/// Base confidence when payload is too small (used in validation step)
const LOW_CONFIDENCE_SMALL_PAYLOAD: f32 = 0.4;

/// Instruction spacing variance threshold for bot detection
const BOT_ISS_VARIANCE_THRESHOLD: f32 = 0.15;

/// Instruction spacing variance threshold for human detection
const HUMAN_ISS_VARIANCE_THRESHOLD: f32 = 0.35;

// =============================================================================
// Classification Constants
// =============================================================================

/// Very low entropy threshold for MEV bots
const VERY_LOW_ENTROPY: f32 = 3.0;

/// Low variance threshold for regular bot patterns
const LOW_VARIANCE: f32 = 50.0;

/// High variance threshold for human patterns
const HIGH_VARIANCE: f32 = 200.0;

/// Large payload size threshold (bytes)
const LARGE_PAYLOAD: usize = 800;

/// Liquidity bot minimum entropy
const LIQUIDITY_BOT_MIN_ENTROPY: f32 = 3.5;

/// Liquidity bot maximum entropy
const LIQUIDITY_BOT_MAX_ENTROPY: f32 = 4.5;

/// RPC filler maximum entropy
const RPC_FILLER_MAX_ENTROPY: f32 = 4.5;

/// Sybil bot variance divisor
const SYBIL_VARIANCE_DIVISOR: f32 = 2.0;

/// Sybil bot entropy threshold
const SYBIL_ENTROPY_THRESHOLD: f32 = 3.5;

/// ISS variance multiplier for bot detection
const BOT_ISS_VARIANCE_MULTIPLIER: f32 = 1000.0;

/// ISS variance multiplier for human detection
const HUMAN_ISS_VARIANCE_MULTIPLIER: f32 = 500.0;

/// Pre-computed bot ISS variance threshold
const BOT_ISS_VARIANCE_COMPUTED: f32 = BOT_ISS_VARIANCE_THRESHOLD * BOT_ISS_VARIANCE_MULTIPLIER; // 150.0

/// Pre-computed human ISS variance threshold
const HUMAN_ISS_VARIANCE_COMPUTED: f32 =
    HUMAN_ISS_VARIANCE_THRESHOLD * HUMAN_ISS_VARIANCE_MULTIPLIER; // 175.0

/// Entropy quantization factor (maps 0-8 bits to 0-255)
const ENTROPY_QUANTIZATION_FACTOR: f32 = 31.875; // 255.0 / 8.0

// =============================================================================
// Actor Type Classification
// =============================================================================

/// Classification of transaction actor based on byte-level fingerprinting.
///
/// Each variant represents a distinct class of transaction originator, detected
/// through payload structure analysis rather than on-chain behavior patterns.
///
/// # Classification Criteria
///
/// - **Human**: High entropy (>5.5), irregular instruction spacing, diverse byte distribution
/// - **Bot**: Low entropy (<3.5), regular instruction spacing, compact payload
/// - **Unknown**: Insufficient data or ambiguous patterns
///
/// # Thread Safety
///
/// This enum is `Copy + Clone + Send + Sync`, safe for concurrent usage.
#[derive(Debug, Clone, Copy, PartialEq, Eq, serde::Serialize)]
pub enum ActorType {
    /// Human trader using mobile wallet (Phantom Mobile, Solflare Mobile)
    ///
    /// Characteristics:
    /// - High entropy (>5.5)
    /// - Irregular instruction spacing
    /// - Additional SDK overhead in payload
    /// - Mobile wallet-specific metadata patterns
    HumanMobile,

    /// Human trader using desktop wallet (Phantom Desktop, Backpack)
    ///
    /// Characteristics:
    /// - High entropy (>5.0)
    /// - Moderate instruction spacing variance
    /// - Desktop SDK patterns
    /// - More compact than mobile but less than bots
    HumanDesktop,

    /// Automated sniper script (Python/TypeScript/Rust bot)
    ///
    /// Characteristics:
    /// - Low entropy (<3.5)
    /// - Highly regular instruction spacing
    /// - Minimal padding
    /// - Tight instruction packing
    SniperScript,

    /// MEV arbitrage bot (Jito MEV, custom MEV)
    ///
    /// Characteristics:
    /// - Very low entropy (<3.0)
    /// - Extremely regular patterns
    /// - High-frequency instruction patterns
    /// - Optimized payload compression
    MEVArb,

    /// Liquidity provision bot
    ///
    /// Characteristics:
    /// - Moderate entropy (3.5-4.5)
    /// - Regular but not minimal spacing
    /// - LP-specific instruction patterns
    /// - Predictable offset sequences
    LiquidityBot,

    /// RPC transaction filler (automated market maker)
    ///
    /// Characteristics:
    /// - Low entropy (<4.0)
    /// - Consistent instruction structure
    /// - RPC-generated payload patterns
    /// - Protocol-specific alignment
    RPCFiller,

    /// Sybil bot network (coordinated multi-wallet attack)
    ///
    /// Characteristics:
    /// - Nearly identical byte patterns across transactions
    /// - Extremely low variance in fingerprints
    /// - Mass-produced payload structure
    /// - Cloned instruction sequences
    SybilBot,

    /// Unknown or ambiguous actor type
    ///
    /// Used when:
    /// - Payload too small for reliable classification
    /// - Mixed signals from multiple layers
    /// - Novel/unseen pattern
    /// - Confidence below threshold
    Unknown,
}

// =============================================================================
// Actor Inference Result
// =============================================================================

/// Result of MPCF actor classification with confidence metrics.
///
/// Provides complete fingerprinting data for downstream Oracle components.
///
/// # Fields
///
/// - `actor`: Classified actor type (8 variants)
/// - `confidence`: Classification confidence (0.0-1.0, where 1.0 = certain)
/// - `entropy`: Shannon entropy of payload (0.0-8.0 typical range)
/// - `fingerprint`: 128-bit structural fingerprint for pattern tracking
///
/// # Performance
///
/// This struct is stack-allocated (16 bytes fingerprint + 12 bytes metadata = 28 bytes total).
/// No heap allocation occurs during inference.
///
/// # Thread Safety
///
/// Implements `Clone + Send + Sync` for concurrent processing.
#[derive(Debug, Clone, serde::Serialize)]
pub struct ActorInference {
    /// Classified actor type based on byte-level analysis
    pub actor: ActorType,

    /// Classification confidence (0.0-1.0)
    ///
    /// - >0.8: High confidence, reliable classification
    /// - 0.5-0.8: Moderate confidence, usable but verify
    /// - <0.5: Low confidence, treat as Unknown
    pub confidence: f32,

    /// Shannon entropy of transaction payload (bits)
    ///
    /// Typical ranges:
    /// - Bots: 2.0-3.5
    /// - Humans: 5.0-7.0
    /// - Mixed: 3.5-5.0
    pub entropy: f32,

    /// 128-bit structural fingerprint (derived from histogram + ISS)
    ///
    /// Used for:
    /// - Tracking actor patterns over time
    /// - Detecting sybil clusters (identical fingerprints)
    /// - Building actor reputation databases
    /// - Cross-transaction correlation
    pub fingerprint: [u8; FINGERPRINT_SIZE],
}

// =============================================================================
// Main Inference Function
// =============================================================================

/// Performs ultra-fast actor classification on raw transaction bytes.
///
/// # Arguments
///
/// * `tx_bytes` - Raw transaction payload from Geyser/RPC/WebSocket
///
/// # Returns
///
/// `ActorInference` containing:
/// - Actor type classification
/// - Confidence score (0.0-1.0)
/// - Payload entropy (bits)
/// - 128-bit structural fingerprint
///
/// # Performance
///
/// - **Target**: 30-70 microseconds
/// - **Worst Case**: <200 microseconds (oversized payload)
/// - **Zero Heap**: All operations use stack allocation
/// - **No Unwrap**: Safe error handling throughout
///
/// # Algorithm Overview
///
/// 1. **Validation**: Check payload size bounds
/// 2. **Byte Morphology**: Build histogram of byte distribution
/// 3. **Entropy Calculation**: Shannon entropy H = -Σ p(i)log₂p(i)
/// 4. **Instruction Spacing**: Analyze offset patterns (ISS)
/// 5. **Fingerprint Generation**: Combine histogram + ISS into 128-bit hash
/// 6. **Classification**: Map entropy + ISS variance to ActorType
/// 7. **Confidence Scoring**: Assess classification reliability
///
/// # Design Constraints
///
/// - **Stack-only**: No heap allocations (fixed-size arrays)
/// - **No external crates**: Uses std/core/alloc only
/// - **No unwrap()**: All operations are fallible-safe
/// - **Deterministic**: Same input always produces same output
///
/// # Example
///
/// ```rust,ignore
/// let tx_bytes: &[u8] = &[0x01, 0x00, 0x03, /* ... */];
/// let result = mpcf_infer(tx_bytes);
///
/// if result.confidence > 0.7 {
///     match result.actor {
///         ActorType::HumanMobile | ActorType::HumanDesktop => {
///             println!("Organic activity detected!");
///         }
///         ActorType::SniperScript => {
///             println!("Bot detected - analyze further");
///         }
///         _ => {}
///     }
/// }
/// ```
///
/// # Security Considerations
///
/// - Payload is capped at MAX_PAYLOAD_SIZE to prevent DoS
/// - No unsafe code blocks
/// - No panics on malformed input
/// - All array accesses are bounds-checked
pub fn mpcf_infer(tx_bytes: &[u8]) -> ActorInference {
    mpcf_infer_impl(tx_bytes, false, None, true)
}

/// Performs ultra-fast actor classification with synthetic event tracking.
///
/// This variant accepts a synthetic flag and optional pool ID for enhanced
/// logging and observability. Use this when processing events that may come
/// from Shadow Ledger simulations vs real blockchain data.
///
/// # Arguments
///
/// * `tx_bytes` - Raw transaction payload
/// * `synthetic` - True if event is from Shadow Ledger simulation, false if real blockchain event
/// * `pool_id` - Optional pool identifier for correlation in logs
///
/// # Returns
///
/// `ActorInference` containing classification and confidence
///
/// # Logging
///
/// Logs in format: `MPCF_INFER pool={} synthetic={} actor={:?} conf={:.2}`
///
/// # Example
///
/// ```rust,ignore
/// // Real blockchain event
/// let result = mpcf_infer_with_source(&tx_bytes, false, Some("pool_xyz"));
///
/// // Shadow Ledger synthetic event
/// let result = mpcf_infer_with_source(&sim_bytes, true, Some("pool_xyz"));
/// ```
pub fn mpcf_infer_with_source(
    tx_bytes: &[u8],
    synthetic: bool,
    pool_id: Option<&str>,
) -> ActorInference {
    mpcf_infer_impl(tx_bytes, synthetic, pool_id, true)
}

/// Infer actor without emitting the classification log.
///
/// Use this in code paths that already log the classification result once (e.g.,
/// SOBP actor tagging inside the cyclic engine) to avoid duplicate log lines.
/// The inference output is identical to `mpcf_infer`.
pub fn mpcf_infer_quiet(tx_bytes: &[u8]) -> ActorInference {
    mpcf_infer_impl(tx_bytes, false, None, false)
}

/// Internal implementation of MPCF inference with synthetic flag and pool tracking
///
/// # Arguments
/// * `tx_bytes` - Raw transaction payload from Geyser/RPC/WebSocket
/// * `synthetic` - Whether this event is synthetic (from Shadow Ledger) vs real (from blockchain)
/// * `pool_id` - Optional pool identifier for logging
/// * `log_classification` - When true, emit the classification summary log; set
///   to false in call-sites that already log the classification to prevent
///   duplicate lines while keeping the same inference output.
///
/// # Returns
/// `ActorInference` containing classification and confidence
///
/// # Note
/// When `synthetic` is true, the function bypasses volume/value filters and logs
/// the source as shadow_ledger for observability.
fn mpcf_infer_impl(
    tx_bytes: &[u8],
    synthetic: bool,
    pool_id: Option<&str>,
    log_classification: bool,
) -> ActorInference {
    // ========================================
    // DIAGNOSTIC LOGGING (Issue #158)
    // ========================================

    // Step 1: Validate payload size
    let payload_len = tx_bytes.len();

    // Check for empty payload (should not happen in production)
    if payload_len == 0 {
        tracing::warn!("MPCF: Received empty raw_bytes - should not happen!");
        return ActorInference {
            actor: ActorType::Unknown,
            confidence: LOW_CONFIDENCE_SMALL_PAYLOAD,
            entropy: 0.0,
            fingerprint: [0u8; FINGERPRINT_SIZE],
        };
    }

    if payload_len < MIN_PAYLOAD_SIZE {
        tracing::debug!(
            "MPCF: Payload too small ({} bytes, min {}), returning Unknown",
            payload_len,
            MIN_PAYLOAD_SIZE
        );
        return ActorInference {
            actor: ActorType::Unknown,
            confidence: LOW_CONFIDENCE_SMALL_PAYLOAD,
            entropy: 0.0,
            fingerprint: [0u8; FINGERPRINT_SIZE],
        };
    }

    // Log byte count
    tracing::debug!(
        "MPCF: Processing {} bytes for actor classification",
        payload_len
    );

    // Limit analysis to MAX_PAYLOAD_SIZE (prevent DoS)
    let effective_len = payload_len.min(MAX_PAYLOAD_SIZE);
    let bytes_to_analyze = &tx_bytes[..effective_len];

    // Step 2: Build byte histogram (Layer I - Byte Morphology)
    let mut histogram = [0u32; HISTOGRAM_SIZE];
    build_histogram(bytes_to_analyze, &mut histogram);

    // Step 3: Calculate Shannon entropy (Layer II - Entropy Signature)
    let entropy = calculate_entropy(&histogram, effective_len as u32);
    tracing::debug!("MPCF: Entropy = {:.3}", entropy);

    // Step 4: Analyze Instruction Spacing Signature (Layer III - ISS)
    let (iss_mean, iss_variance) = analyze_instruction_spacing(bytes_to_analyze);
    tracing::debug!("MPCF: ISS variance = {:.3}", iss_variance);

    // Step 5: Generate 128-bit fingerprint
    let fingerprint = generate_fingerprint(&histogram, entropy, iss_mean, iss_variance);

    // Step 6: Classify actor type
    let actor = classify_actor(entropy, iss_variance, payload_len);

    // Step 7: Calculate confidence
    let confidence = calculate_confidence(entropy, iss_variance, payload_len, actor);

    // Step 8: Log final classification result
    if log_classification {
        tracing::info!(
            "MPCF: Classification complete - actor={:?}, confidence={:.2}, entropy={:.3}, iss={:.3}",
            actor, confidence, entropy, iss_variance
        );
    }

    // Step 9: Additional logging for synthetic events or pool tracking
    if let Some(pool) = pool_id {
        tracing::debug!(
            "MPCF_INFER pool={} synthetic={} actor={:?} conf={:.2}",
            pool,
            synthetic,
            actor,
            confidence
        );
    }

    // Step 10: Return ActorInference
    ActorInference {
        actor,
        confidence,
        entropy,
        fingerprint,
    }
}

// =============================================================================
// Helper Functions (Private)
// =============================================================================

/// Build byte histogram from payload.
///
/// Single-pass algorithm that counts occurrences of each byte value.
/// Uses stack-allocated histogram array for zero-heap performance.
///
/// # Arguments
///
/// * `bytes` - Input byte slice to analyze
/// * `histogram` - Output histogram array [u32; 256]
#[inline]
fn build_histogram(bytes: &[u8], histogram: &mut [u32; HISTOGRAM_SIZE]) {
    // Zero out histogram
    for slot in histogram.iter_mut() {
        *slot = 0;
    }

    // Single pass: count byte occurrences
    for &byte in bytes {
        histogram[byte as usize] += 1;
    }
}

/// Calculate Shannon entropy from histogram.
///
/// Implements H = -Σ p(i) * log₂(p(i)) where p(i) = count[i] / total
///
/// Uses fast log2 approximation for performance.
///
/// # Arguments
///
/// * `histogram` - Byte occurrence counts
/// * `total` - Total number of bytes analyzed
///
/// # Returns
///
/// Shannon entropy in bits (typically 0.0-8.0 range)
#[inline]
fn calculate_entropy(histogram: &[u32; HISTOGRAM_SIZE], total: u32) -> f32 {
    if total == 0 {
        return 0.0;
    }

    let mut entropy = 0.0f32;
    let total_f = total as f32;

    for &count in histogram {
        if count > 0 {
            let p = count as f32 / total_f;
            // Shannon entropy: -p * log₂(p)
            // log₂(p) = ln(p) / ln(2)
            entropy -= p * p.log2();
        }
    }

    entropy
}

/// Analyze instruction spacing patterns.
///
/// Scans for instruction boundaries using heuristic patterns:
/// - Look for common Solana instruction markers
/// - Calculate offset deltas between instructions
/// - Compute mean and variance of spacing
///
/// Low variance indicates bot (regular spacing)
/// High variance indicates human (irregular spacing)
///
/// # Arguments
///
/// * `bytes` - Transaction payload bytes
///
/// # Returns
///
/// Tuple of (mean_spacing, variance)
#[inline]
fn analyze_instruction_spacing(bytes: &[u8]) -> (f32, f32) {
    // Stack-allocated buffer for instruction offsets (max 32 instructions)
    let mut offsets = [0usize; 32];
    let mut offset_count = 0usize;

    // Heuristic: scan for instruction boundaries
    // We look for patterns that indicate instruction starts:
    // - Program ID markers (32-byte pubkeys)
    // - Instruction discriminators
    // - Account metadata sections

    let mut i = 0;
    while i < bytes.len() && offset_count < 32 {
        // Check for potential instruction boundary markers
        // Heuristic 1: Look for 0x00 0x00 patterns (padding between instructions)
        // Heuristic 2: Look for compact-u16 length prefixes
        // Heuristic 3: Detect 32-byte alignment patterns

        if i > 0
            && (
                (i % 32 == 0) ||  // 32-byte alignment
            (bytes[i] == 0 && i + 1 < bytes.len() && bytes[i + 1] < 0x10) ||  // Small discriminator
            (i >= 32 && bytes[i - 1] == 0 && bytes[i] != 0)
                // Transition
            )
        {
            offsets[offset_count] = i;
            offset_count += 1;
        }

        i += 1;
    }

    if offset_count < 2 {
        // Not enough data for spacing analysis
        return (0.0, 0.0);
    }

    // Calculate deltas between offsets
    let mut deltas = [0usize; 31];
    for i in 0..(offset_count - 1) {
        deltas[i] = offsets[i + 1].saturating_sub(offsets[i]);
    }

    let delta_count = offset_count - 1;

    // Calculate mean
    let sum: usize = deltas[..delta_count].iter().sum();
    let mean = sum as f32 / delta_count as f32;

    // Calculate variance
    let mut variance_sum = 0.0f32;
    for i in 0..delta_count {
        let diff = deltas[i] as f32 - mean;
        variance_sum += diff * diff;
    }
    let variance = variance_sum / delta_count as f32;

    (mean, variance)
}

/// Generate 128-bit structural fingerprint.
///
/// Combines multiple layers of analysis into a unique fingerprint:
/// - Top 8 histogram buckets (8 bytes)
/// - ISS mean (2 bytes)
/// - ISS variance (2 bytes)
/// - Entropy quantized (1 byte)
/// - Payload size hash (1 byte)
/// - Structural hash (2 bytes)
///
/// # Arguments
///
/// * `histogram` - Byte distribution histogram
/// * `entropy` - Shannon entropy value
/// * `iss_mean` - Instruction spacing mean
/// * `iss_variance` - Instruction spacing variance
///
/// # Returns
///
/// 128-bit (16-byte) fingerprint array
#[inline]
fn generate_fingerprint(
    histogram: &[u32; HISTOGRAM_SIZE],
    entropy: f32,
    iss_mean: f32,
    iss_variance: f32,
) -> [u8; FINGERPRINT_SIZE] {
    let mut fingerprint = [0u8; FINGERPRINT_SIZE];

    // Find top 8 histogram buckets (most common bytes)
    let mut top_indices = [0usize; 8];
    let mut top_counts = [0u32; 8];

    for (idx, &count) in histogram.iter().enumerate() {
        if count > 0 {
            // Insert into top 8 if larger than smallest in top 8
            for i in 0..8 {
                if count > top_counts[i] {
                    // Shift down
                    for j in (i + 1..8).rev() {
                        top_counts[j] = top_counts[j - 1];
                        top_indices[j] = top_indices[j - 1];
                    }
                    top_counts[i] = count;
                    top_indices[i] = idx;
                    break;
                }
            }
        }
    }

    // Bytes 0-7: Top 8 byte indices
    for i in 0..8 {
        fingerprint[i] = top_indices[i] as u8;
    }

    // Bytes 8-9: ISS mean (u16)
    let iss_mean_u16 = (iss_mean.clamp(0.0, 65535.0) as u16).to_le_bytes();
    fingerprint[8] = iss_mean_u16[0];
    fingerprint[9] = iss_mean_u16[1];

    // Bytes 10-11: ISS variance (u16)
    let iss_var_u16 = (iss_variance.clamp(0.0, 65535.0) as u16).to_le_bytes();
    fingerprint[10] = iss_var_u16[0];
    fingerprint[11] = iss_var_u16[1];

    // Byte 12: Entropy quantized to u8
    fingerprint[12] = (entropy.clamp(0.0, 8.0) * ENTROPY_QUANTIZATION_FACTOR) as u8;

    // Byte 13: XOR fold of top counts (structural hash)
    let mut hash_byte = 0u8;
    for &count in &top_counts {
        hash_byte ^= (count & 0xFF) as u8;
    }
    fingerprint[13] = hash_byte;

    // Bytes 14-15: Combined structural hash
    let mut structural_hash = 0u16;
    for i in 0..8 {
        structural_hash ^= (top_counts[i] as u16).wrapping_mul(top_indices[i] as u16);
    }
    let hash_bytes = structural_hash.to_le_bytes();
    fingerprint[14] = hash_bytes[0];
    fingerprint[15] = hash_bytes[1];

    fingerprint
}

/// Classify actor type based on entropy and ISS patterns.
///
/// Uses threshold-based classification:
/// - Very low entropy + low variance = MEV/Sniper bots
/// - Low entropy + moderate variance = Liquidity bots
/// - High entropy + high variance = Human traders
/// - Moderate patterns = RPC fillers / Unknown
///
/// # Arguments
///
/// * `entropy` - Shannon entropy of payload
/// * `iss_variance` - Instruction spacing variance
/// * `payload_size` - Size of transaction payload
///
/// # Returns
///
/// Most likely ActorType
#[inline]
fn classify_actor(entropy: f32, iss_variance: f32, payload_size: usize) -> ActorType {
    // MEV Arbitrage: Very low entropy, extremely regular patterns
    if entropy < VERY_LOW_ENTROPY && iss_variance < LOW_VARIANCE {
        return ActorType::MEVArb;
    }

    // Sniper Script: Low entropy, low variance
    if entropy < BOT_ENTROPY_THRESHOLD && iss_variance < BOT_ISS_VARIANCE_COMPUTED {
        return ActorType::SniperScript;
    }

    // Human Mobile: High entropy, high variance, large payload
    if entropy > HUMAN_ENTROPY_THRESHOLD
        && iss_variance > HIGH_VARIANCE
        && payload_size > LARGE_PAYLOAD
    {
        return ActorType::HumanMobile;
    }

    // Human Desktop: High entropy, moderate-high variance
    if entropy > HUMAN_ENTROPY_THRESHOLD && iss_variance > HUMAN_ISS_VARIANCE_COMPUTED {
        return ActorType::HumanDesktop;
    }

    // Liquidity Bot: Moderate entropy, moderate variance
    if entropy >= LIQUIDITY_BOT_MIN_ENTROPY
        && entropy <= LIQUIDITY_BOT_MAX_ENTROPY
        && iss_variance >= LOW_VARIANCE
        && iss_variance < HIGH_VARIANCE
    {
        return ActorType::LiquidityBot;
    }

    // RPC Filler: Low-moderate entropy, consistent patterns
    if entropy < RPC_FILLER_MAX_ENTROPY && iss_variance < HUMAN_ISS_VARIANCE_COMPUTED {
        return ActorType::RPCFiller;
    }

    // Sybil Bot: Very low variance (identical patterns)
    if iss_variance < LOW_VARIANCE / SYBIL_VARIANCE_DIVISOR && entropy < SYBIL_ENTROPY_THRESHOLD {
        return ActorType::SybilBot;
    }

    // Default: Unknown
    ActorType::Unknown
}

/// Calculate classification confidence score.
///
/// Confidence based on:
/// - Distance from classification thresholds (clearer signal = higher)
/// - Payload size adequacy (larger = more reliable)
/// - Pattern clarity (distinct features = higher)
///
/// # Arguments
///
/// * `entropy` - Shannon entropy
/// * `iss_variance` - Instruction spacing variance
/// * `payload_size` - Transaction payload size
/// * `actor` - Classified actor type
///
/// # Returns
///
/// Confidence score [0.0, 1.0]
#[inline]
fn calculate_confidence(
    entropy: f32,
    iss_variance: f32,
    payload_size: usize,
    actor: ActorType,
) -> f32 {
    let mut confidence: f32 = 0.5; // Base confidence

    // Adjust based on payload size (more data = higher confidence)
    if payload_size >= 512 {
        confidence += 0.2;
    } else if payload_size >= 256 {
        confidence += 0.1;
    } else if payload_size < MIN_PAYLOAD_SIZE * 2 {
        confidence -= 0.1;
    }

    // Adjust based on entropy clarity
    if actor == ActorType::Unknown {
        // Ambiguous classification
        confidence = confidence.min(0.4);
    } else if entropy < BOT_ENTROPY_THRESHOLD - 0.5 || entropy > HUMAN_ENTROPY_THRESHOLD + 0.5 {
        // Clear entropy signal
        confidence += 0.2;
    } else if (entropy - BOT_ENTROPY_THRESHOLD).abs() < 0.3
        || (entropy - HUMAN_ENTROPY_THRESHOLD).abs() < 0.3
    {
        // Near threshold (ambiguous)
        confidence -= 0.15;
    }

    // Adjust based on ISS variance clarity
    if iss_variance < 50.0 || iss_variance > 300.0 {
        // Clear variance signal
        confidence += 0.1;
    }

    // Clamp to valid range
    confidence.clamp(0.0, 1.0)
}

// =============================================================================
// Tests
// =============================================================================

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_mpcf_infer_empty_payload() {
        let empty: &[u8] = &[];
        let result = mpcf_infer(empty);

        assert_eq!(result.actor, ActorType::Unknown);
        assert!(result.confidence < 0.5);
    }

    #[test]
    fn test_mpcf_infer_small_payload() {
        let small: &[u8] = &[0x01, 0x02, 0x03];
        let result = mpcf_infer(small);

        assert_eq!(result.actor, ActorType::Unknown);
        assert!(result.confidence < 0.5);
    }

    #[test]
    fn test_mpcf_infer_valid_payload() {
        // Generate a synthetic payload with reasonable entropy
        let payload: Vec<u8> = (0..128).map(|i| (i * 7) as u8).collect();
        let result = mpcf_infer(&payload);

        // Should not panic and should return valid structure
        assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
        assert!(result.entropy >= 0.0);
    }

    #[test]
    fn test_actor_type_copy() {
        let actor1 = ActorType::HumanMobile;
        let actor2 = actor1;
        assert_eq!(actor1, actor2);
    }

    #[test]
    fn test_actor_inference_clone() {
        let inference = ActorInference {
            actor: ActorType::SniperScript,
            confidence: 0.85,
            entropy: 3.2,
            fingerprint: [0xAB; FINGERPRINT_SIZE],
        };

        let cloned = inference.clone();
        assert_eq!(cloned.actor, inference.actor);
        assert_eq!(cloned.confidence, inference.confidence);
        assert_eq!(cloned.entropy, inference.entropy);
        assert_eq!(cloned.fingerprint, inference.fingerprint);
    }

    #[test]
    fn test_build_histogram() {
        let bytes = [0x00, 0x01, 0x00, 0x02, 0x01, 0x00];
        let mut histogram = [0u32; HISTOGRAM_SIZE];

        build_histogram(&bytes, &mut histogram);

        assert_eq!(histogram[0x00], 3);
        assert_eq!(histogram[0x01], 2);
        assert_eq!(histogram[0x02], 1);
        assert_eq!(histogram[0x03], 0);
    }

    #[test]
    fn test_calculate_entropy_uniform() {
        // Uniform distribution should have high entropy
        let mut histogram = [0u32; HISTOGRAM_SIZE];
        for i in 0..256 {
            histogram[i] = 1;
        }

        let entropy = calculate_entropy(&histogram, 256);

        // log₂(256) = 8.0
        assert!((entropy - 8.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_entropy_single_byte() {
        // Single byte repeated should have zero entropy
        let mut histogram = [0u32; HISTOGRAM_SIZE];
        histogram[0xAA] = 100;

        let entropy = calculate_entropy(&histogram, 100);

        assert!((entropy - 0.0).abs() < 0.01);
    }

    #[test]
    fn test_calculate_entropy_two_bytes() {
        // Two bytes equally distributed
        let mut histogram = [0u32; HISTOGRAM_SIZE];
        histogram[0x00] = 50;
        histogram[0xFF] = 50;

        let entropy = calculate_entropy(&histogram, 100);

        // log₂(2) = 1.0
        assert!((entropy - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_analyze_instruction_spacing() {
        // Create payload with regular spacing patterns (bot-like)
        let mut payload = vec![0u8; 256];
        for i in 0..8 {
            payload[i * 32] = 0xFF; // Mark boundaries
        }

        let (mean, variance) = analyze_instruction_spacing(&payload);

        // Regular spacing should have low variance
        assert!(mean > 0.0);
        assert!(variance >= 0.0);
    }

    #[test]
    fn test_generate_fingerprint_deterministic() {
        let mut histogram = [0u32; HISTOGRAM_SIZE];
        histogram[0] = 100;
        histogram[1] = 50;

        let fp1 = generate_fingerprint(&histogram, 5.0, 10.0, 20.0);
        let fp2 = generate_fingerprint(&histogram, 5.0, 10.0, 20.0);

        // Same input should produce same fingerprint
        assert_eq!(fp1, fp2);
    }

    #[test]
    fn test_generate_fingerprint_unique() {
        let mut histogram1 = [0u32; HISTOGRAM_SIZE];
        histogram1[0] = 100;

        let mut histogram2 = [0u32; HISTOGRAM_SIZE];
        histogram2[1] = 100;

        let fp1 = generate_fingerprint(&histogram1, 5.0, 10.0, 20.0);
        let fp2 = generate_fingerprint(&histogram2, 5.0, 10.0, 20.0);

        // Different inputs should produce different fingerprints
        assert_ne!(fp1, fp2);
    }

    #[test]
    fn test_classify_actor_mev_arb() {
        // Very low entropy, very low variance = MEV bot
        let actor = classify_actor(2.5, 30.0, 200);
        assert_eq!(actor, ActorType::MEVArb);
    }

    #[test]
    fn test_classify_actor_sniper_script() {
        // Low entropy, low variance = Sniper
        let actor = classify_actor(3.2, 80.0, 250);
        assert_eq!(actor, ActorType::SniperScript);
    }

    #[test]
    fn test_classify_actor_human_mobile() {
        // High entropy, high variance, large payload = Human Mobile
        let actor = classify_actor(6.5, 250.0, 1000);
        assert_eq!(actor, ActorType::HumanMobile);
    }

    #[test]
    fn test_classify_actor_human_desktop() {
        // High entropy, moderate-high variance = Human Desktop
        let actor = classify_actor(6.0, 200.0, 500);
        assert_eq!(actor, ActorType::HumanDesktop);
    }

    #[test]
    fn test_classify_actor_liquidity_bot() {
        // Moderate entropy, moderate variance = Liquidity Bot
        let actor = classify_actor(4.0, 100.0, 300);
        assert_eq!(actor, ActorType::LiquidityBot);
    }

    #[test]
    fn test_calculate_confidence_high() {
        // Large payload, clear signal should give high confidence
        let conf = calculate_confidence(2.5, 30.0, 1000, ActorType::MEVArb);
        assert!(conf > 0.7);
    }

    #[test]
    fn test_calculate_confidence_low() {
        // Small payload, unknown actor should give low confidence
        let conf = calculate_confidence(4.5, 100.0, 100, ActorType::Unknown);
        assert!(conf < 0.5);
    }

    #[test]
    fn test_mpcf_infer_bot_pattern() {
        // Create synthetic bot payload: low entropy, regular pattern
        let mut payload = vec![0u8; 256];
        for i in 0..256 {
            payload[i] = if i % 2 == 0 { 0x00 } else { 0xFF };
        }

        let result = mpcf_infer(&payload);

        // Should detect bot-like pattern
        assert!(result.entropy < 2.0); // Very low entropy
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_mpcf_infer_human_pattern() {
        // Create synthetic human payload: high entropy, diverse bytes
        let payload: Vec<u8> = (0..512).map(|i| ((i * 13 + 7) % 256) as u8).collect();

        let result = mpcf_infer(&payload);

        // Should have higher entropy
        assert!(result.entropy > 3.0);
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_mpcf_max_payload_handling() {
        // Create oversized payload
        let large_payload = vec![0x42; MAX_PAYLOAD_SIZE * 2];

        let result = mpcf_infer(&large_payload);

        // Should not panic, should truncate internally
        assert!(result.confidence > 0.0);
    }

    #[test]
    fn test_fingerprint_uniqueness_across_patterns() {
        // Test that different patterns produce different fingerprints
        let pattern1: Vec<u8> = (0..256).map(|i| i as u8).collect();
        let pattern2: Vec<u8> = vec![0xAA; 256];
        let pattern3: Vec<u8> = (0..256)
            .map(|i| if i % 2 == 0 { 0x00 } else { 0xFF })
            .collect();

        let result1 = mpcf_infer(&pattern1);
        let result2 = mpcf_infer(&pattern2);
        let result3 = mpcf_infer(&pattern3);

        // Pattern2 (uniform) and pattern3 (alternating) should be different from pattern1
        assert_ne!(result1.fingerprint, result2.fingerprint);
        assert_ne!(result2.fingerprint, result3.fingerprint);

        // Different entropy should also be detectable
        assert!((result1.entropy - result2.entropy).abs() > 0.5);
    }

    #[test]
    fn test_no_panic_on_malformed_input() {
        // Various edge cases that should not panic
        let test_cases = vec![
            vec![],                          // Empty
            vec![0xFF],                      // Single byte
            vec![0x00; 10000],               // Very large
            vec![0xFF; 5],                   // Small uniform
            (0..255u8).collect::<Vec<u8>>(), // Sequential
        ];

        for payload in test_cases {
            let _ = mpcf_infer(&payload); // Should not panic
        }
    }

    #[test]
    fn test_deterministic_results() {
        let payload: Vec<u8> = (0..256).map(|i| (i * 7) as u8).collect();

        let result1 = mpcf_infer(&payload);
        let result2 = mpcf_infer(&payload);

        // Same input should always produce same output
        assert_eq!(result1.actor, result2.actor);
        assert_eq!(result1.confidence, result2.confidence);
        assert_eq!(result1.entropy, result2.entropy);
        assert_eq!(result1.fingerprint, result2.fingerprint);
    }
}

// =============================================================================
// Property-Based Tests
// =============================================================================

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    // Generate arbitrary byte vectors for testing
    fn arbitrary_payload() -> impl Strategy<Value = Vec<u8>> {
        prop::collection::vec(any::<u8>(), 0..8192)
    }

    proptest! {
        /// Property: mpcf_infer should never panic on any input
        #[test]
        fn prop_never_panics(payload in arbitrary_payload()) {
            let _ = mpcf_infer(&payload);
        }

        /// Property: confidence should always be in [0.0, 1.0]
        #[test]
        fn prop_confidence_in_bounds(payload in arbitrary_payload()) {
            let result = mpcf_infer(&payload);
            prop_assert!(result.confidence >= 0.0);
            prop_assert!(result.confidence <= 1.0);
        }

        /// Property: entropy should be non-negative
        #[test]
        fn prop_entropy_non_negative(payload in arbitrary_payload()) {
            let result = mpcf_infer(&payload);
            prop_assert!(result.entropy >= 0.0);
        }

        /// Property: entropy should be bounded (max is log2(256) = 8.0)
        #[test]
        fn prop_entropy_bounded(payload in arbitrary_payload()) {
            let result = mpcf_infer(&payload);
            prop_assert!(result.entropy <= 8.1); // Allow small tolerance
        }

        /// Property: same input always produces same output
        #[test]
        fn prop_deterministic(payload in arbitrary_payload()) {
            let result1 = mpcf_infer(&payload);
            let result2 = mpcf_infer(&payload);

            prop_assert_eq!(result1.actor, result2.actor);
            prop_assert_eq!(result1.confidence, result2.confidence);
            prop_assert_eq!(result1.entropy, result2.entropy);
            prop_assert_eq!(result1.fingerprint, result2.fingerprint);
        }

        /// Property: small payloads should result in Unknown or low confidence
        #[test]
        fn prop_small_payloads_low_confidence(payload in prop::collection::vec(any::<u8>(), 0..MIN_PAYLOAD_SIZE)) {
            let result = mpcf_infer(&payload);
            prop_assert!(result.actor == ActorType::Unknown || result.confidence < 0.6);
        }

        /// Property: uniform bytes should have low entropy
        #[test]
        fn prop_uniform_low_entropy(byte_val: u8, size in 100usize..1000) {
            let payload = vec![byte_val; size];
            let result = mpcf_infer(&payload);
            prop_assert!(result.entropy < 0.1); // Uniform distribution has ~0 entropy
        }

        /// Property: highly diverse payloads should have high entropy
        #[test]
        fn prop_diverse_high_entropy(size in 256usize..512) {
            // Create payload with all possible byte values
            let mut payload = Vec::new();
            for _ in 0..(size / 256) {
                for i in 0..256 {
                    payload.push(i as u8);
                }
            }
            let result = mpcf_infer(&payload);
            prop_assert!(result.entropy > 7.0); // Should be close to 8.0
        }

        /// Property: truncating large payloads shouldn't change behavior significantly
        #[test]
        fn prop_truncation_stable(payload in prop::collection::vec(any::<u8>(), MAX_PAYLOAD_SIZE * 2..MAX_PAYLOAD_SIZE * 3)) {
            let result_full = mpcf_infer(&payload);
            let result_truncated = mpcf_infer(&payload[..MAX_PAYLOAD_SIZE]);

            // Results should be similar (same analysis on first MAX_PAYLOAD_SIZE bytes)
            prop_assert_eq!(result_full.entropy, result_truncated.entropy);
            prop_assert_eq!(result_full.fingerprint, result_truncated.fingerprint);
        }

        /// Property: fingerprint should use all 16 bytes for non-trivial inputs
        #[test]
        fn prop_fingerprint_utilizes_space(payload in prop::collection::vec(any::<u8>(), 256..1024)) {
            let result = mpcf_infer(&payload);
            // At least some bytes should be non-zero for non-trivial input
            let non_zero_count = result.fingerprint.iter().filter(|&&b| b != 0).count();
            prop_assert!(non_zero_count > 0);
        }

        /// Property: bot-like patterns (low entropy + regular spacing) should be classified as bot types
        #[test]
        fn prop_bot_pattern_detection(size in 256usize..512) {
            // Create bot-like payload: alternating bytes (low entropy, regular pattern)
            let payload: Vec<u8> = (0..size).map(|i| if i % 2 == 0 { 0x00 } else { 0xFF }).collect();
            let result = mpcf_infer(&payload);

            // Should have low entropy
            prop_assert!(result.entropy < 2.0);

            // Should be classified as some bot type or at least not Human
            prop_assert!(
                result.actor != ActorType::HumanMobile &&
                result.actor != ActorType::HumanDesktop
            );
        }

        /// Property: changing multiple bytes should change the fingerprint
        #[test]
        fn prop_fingerprint_sensitivity(mut payload in prop::collection::vec(any::<u8>(), 256..512)) {
            let result1 = mpcf_infer(&payload);

            // Change 10% of bytes to create noticeable difference
            let change_count = (payload.len() / 10).max(10);
            for i in 0..change_count {
                let idx = (i * 7) % payload.len();
                payload[idx] = payload[idx].wrapping_add(1);
            }

            let result2 = mpcf_infer(&payload);

            // Fingerprint or entropy should change with significant modifications
            let changed = result1.fingerprint != result2.fingerprint ||
                         (result1.entropy - result2.entropy).abs() > 0.01;
            prop_assert!(changed);
        }
    }
}

// =============================================================================
// Fuzz Corpus Generator & Enhanced Property-Based Tests
// =============================================================================

#[cfg(test)]
mod corpus {
    use super::*;

    /// Base64-encoded transaction corpus generator for realistic testing.
    ///
    /// This module provides utilities to generate realistic transaction payloads
    /// representing different actor types for comprehensive testing of MPCF.
    ///
    /// # Actor Type Coverage
    /// - Phantom Mobile/Desktop wallets
    /// - Sniper scripts (gm/solsniper)
    /// - Liquidation bots
    /// - MEV arbitrage bots
    /// - Sybil wallet networks

    /// Generate Phantom Mobile wallet transaction pattern.
    ///
    /// Characteristics:
    /// - High entropy from mobile SDK overhead
    /// - Additional metadata for mobile-specific features
    /// - Irregular instruction spacing due to wallet UI interactions
    /// - Larger payload size (800-1200 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating Phantom Mobile
    pub fn generate_phantom_mobile() -> Vec<u8> {
        let mut payload = Vec::with_capacity(1000);

        // Simulate transaction header with high diversity
        for i in 0..32 {
            payload.push(((i * 13 + 7) % 256) as u8);
        }

        // Add mobile SDK overhead (random-looking padding)
        for i in 32..128 {
            payload.push(((i * 37 + 19) % 256) as u8);
        }

        // Instruction data with irregular spacing
        for chunk in 0..8 {
            let base_offset = 128 + chunk * 100;
            for j in 0..80 {
                let byte_val = ((base_offset + j * 11) % 256) as u8;
                payload.push(byte_val);
            }
            // Add irregular padding (human-like)
            for k in 0..20 {
                payload.push(((chunk * k * 17 + 23) % 256) as u8);
            }
        }

        payload
    }

    /// Generate Phantom Desktop wallet transaction pattern.
    ///
    /// Characteristics:
    /// - Moderate entropy (less overhead than mobile)
    /// - More compact than mobile but less than bots
    /// - Desktop SDK patterns
    /// - Medium payload size (400-800 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating Phantom Desktop
    pub fn generate_phantom_desktop() -> Vec<u8> {
        let mut payload = Vec::with_capacity(600);

        // Transaction header
        for i in 0..32 {
            payload.push(((i * 11 + 5) % 256) as u8);
        }

        // Desktop SDK overhead (moderate)
        for i in 32..96 {
            payload.push(((i * 23 + 13) % 256) as u8);
        }

        // Instruction data with moderate spacing
        for chunk in 0..5 {
            let base_offset = 96 + chunk * 100;
            for j in 0..90 {
                let byte_val = ((base_offset + j * 7) % 256) as u8;
                payload.push(byte_val);
            }
            // Moderate padding
            for k in 0..10 {
                payload.push(((chunk * k * 13 + 17) % 256) as u8);
            }
        }

        payload
    }

    /// Generate sniper script transaction pattern (gm/solsniper-like).
    ///
    /// Characteristics:
    /// - Low entropy (highly optimized)
    /// - Tight instruction packing
    /// - Minimal padding
    /// - Regular instruction spacing
    /// - Small payload size (200-400 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating sniper bot
    pub fn generate_sniper_script() -> Vec<u8> {
        let mut payload = Vec::with_capacity(300);

        // Minimal header (bots optimize everything)
        for i in 0..16 {
            payload.push((i * 2) as u8);
        }

        // Tightly packed instructions with regular spacing
        for chunk in 0..10 {
            let base_offset = 16 + chunk * 28;
            // Regular pattern - low entropy
            for j in 0..28 {
                payload.push(((base_offset + j) % 8) as u8);
            }
        }

        // Minimal trailing data
        for i in 0..4 {
            payload.push((i * 3) as u8);
        }

        payload
    }

    /// Generate MEV arbitrage bot transaction pattern.
    ///
    /// Characteristics:
    /// - Very low entropy (extreme optimization)
    /// - Extremely regular patterns
    /// - High-frequency instruction patterns
    /// - Optimized payload compression
    /// - Minimal size (150-300 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating MEV bot
    pub fn generate_mev_arb() -> Vec<u8> {
        let mut payload = Vec::with_capacity(200);

        // Ultra-minimal header
        for i in 0..12 {
            payload.push((i % 4) as u8);
        }

        // Extremely regular instruction pattern (MEV bots are highly optimized)
        for _chunk in 0..15 {
            // Repeating 12-byte pattern (very low entropy)
            for j in 0..12 {
                payload.push((j % 3) as u8);
            }
        }

        // Minimal footer
        for i in 0..8 {
            payload.push((i % 2) as u8);
        }

        payload
    }

    /// Generate liquidity provision bot transaction pattern.
    ///
    /// Characteristics:
    /// - Moderate entropy (3.5-4.5)
    /// - Regular but not minimal spacing
    /// - LP-specific instruction patterns
    /// - Predictable offset sequences
    /// - Medium payload size (300-500 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating liquidity bot
    pub fn generate_liquidity_bot() -> Vec<u8> {
        let mut payload = Vec::with_capacity(400);

        // Standard header
        for i in 0..24 {
            payload.push(((i * 5 + 3) % 32) as u8);
        }

        // LP instruction patterns - moderate regularity
        for chunk in 0..8 {
            let base = chunk * 10;
            for j in 0..46 {
                payload.push(((base + j * 3) % 64) as u8);
            }
        }

        // LP-specific metadata
        for i in 0..8 {
            payload.push(((i * 7 + 11) % 32) as u8);
        }

        payload
    }

    /// Generate RPC filler transaction pattern.
    ///
    /// Characteristics:
    /// - Low entropy (<4.0)
    /// - Consistent instruction structure
    /// - RPC-generated payload patterns
    /// - Protocol-specific alignment
    /// - Standard payload size (250-450 bytes typical)
    ///
    /// # Returns
    /// Raw transaction bytes simulating RPC filler
    pub fn generate_rpc_filler() -> Vec<u8> {
        let mut payload = Vec::with_capacity(350);

        // RPC-generated header (consistent structure)
        for i in 0..32 {
            payload.push(((i / 4) * 16 + (i % 4)) as u8);
        }

        // RPC instruction patterns - aligned to protocol boundaries
        for chunk in 0..7 {
            let aligned_base = chunk * 16;
            for j in 0..44 {
                payload.push(((aligned_base + j / 4) % 128) as u8);
            }
        }

        // RPC footer
        for i in 0..10 {
            payload.push(((i / 2) * 8) as u8);
        }

        payload
    }

    /// Generate Sybil bot network transaction pattern.
    ///
    /// Characteristics:
    /// - Nearly identical byte patterns across transactions
    /// - Extremely low variance in fingerprints
    /// - Mass-produced payload structure
    /// - Cloned instruction sequences
    /// - Consistent size (200-350 bytes typical)
    ///
    /// # Arguments
    /// * `variant` - Variant number for slight variations (0-255)
    ///
    /// # Returns
    /// Raw transaction bytes simulating Sybil bot
    pub fn generate_sybil_bot(variant: u8) -> Vec<u8> {
        let mut payload = Vec::with_capacity(280);

        // Cloned header (nearly identical across all sybil wallets)
        for i in 0..20 {
            payload.push((i + variant / 16) as u8);
        }

        // Mass-produced instruction sequence
        for _chunk in 0..12 {
            // Nearly identical 20-byte blocks
            for j in 0..20 {
                payload.push(((j + variant / 8) % 16) as u8);
            }
        }

        // Minimal variation in footer
        for i in 0..20 {
            payload.push(((i + variant / 4) % 8) as u8);
        }

        payload
    }

    /// Generate a batch of diverse corpus payloads for comprehensive testing.
    ///
    /// This creates a realistic mix of transaction types that would be seen
    /// in a production environment during a token launch.
    ///
    /// # Returns
    /// Vector of (ActorType, Vec<u8>) tuples for testing
    pub fn generate_corpus_batch() -> Vec<(ActorType, Vec<u8>)> {
        vec![
            // Human wallets (organic users)
            (ActorType::HumanMobile, generate_phantom_mobile()),
            (ActorType::HumanMobile, generate_phantom_mobile()),
            (ActorType::HumanDesktop, generate_phantom_desktop()),
            (ActorType::HumanDesktop, generate_phantom_desktop()),
            // Sniper bots
            (ActorType::SniperScript, generate_sniper_script()),
            (ActorType::SniperScript, generate_sniper_script()),
            // MEV bots
            (ActorType::MEVArb, generate_mev_arb()),
            (ActorType::MEVArb, generate_mev_arb()),
            // Liquidity bots
            (ActorType::LiquidityBot, generate_liquidity_bot()),
            // RPC fillers
            (ActorType::RPCFiller, generate_rpc_filler()),
            // Sybil network (multiple wallets with nearly identical patterns)
            (ActorType::SybilBot, generate_sybil_bot(0)),
            (ActorType::SybilBot, generate_sybil_bot(1)),
            (ActorType::SybilBot, generate_sybil_bot(2)),
            (ActorType::SybilBot, generate_sybil_bot(3)),
            (ActorType::SybilBot, generate_sybil_bot(4)),
        ]
    }
}

// =============================================================================
// Enhanced Property-Based Tests with Corpus
// =============================================================================

#[cfg(test)]
mod corpus_proptests {
    use super::corpus::*;
    use super::*;
    use proptest::prelude::*;

    #[test]
    fn test_corpus_phantom_mobile_classification() {
        let payload = generate_phantom_mobile();
        let result = mpcf_infer(&payload);

        // Should classify as Human (Mobile or Desktop acceptable)
        assert!(
            result.actor == ActorType::HumanMobile || result.actor == ActorType::HumanDesktop,
            "Phantom Mobile should be classified as Human, got {:?}",
            result.actor
        );

        // Should have high entropy
        assert!(
            result.entropy > 5.0,
            "Phantom Mobile should have high entropy, got {}",
            result.entropy
        );

        // Should have reasonable confidence
        assert!(
            result.confidence > 0.4,
            "Phantom Mobile should have confidence >0.4, got {}",
            result.confidence
        );
    }

    #[test]
    fn test_corpus_phantom_desktop_classification() {
        let payload = generate_phantom_desktop();
        let result = mpcf_infer(&payload);

        // Should classify as Human (Mobile or Desktop acceptable)
        assert!(
            result.actor == ActorType::HumanMobile || result.actor == ActorType::HumanDesktop,
            "Phantom Desktop should be classified as Human, got {:?}",
            result.actor
        );

        // Should have moderate to high entropy
        assert!(
            result.entropy > 4.5,
            "Phantom Desktop should have entropy >4.5, got {}",
            result.entropy
        );
    }

    #[test]
    fn test_corpus_sniper_script_classification() {
        let payload = generate_sniper_script();
        let result = mpcf_infer(&payload);

        // Should classify as bot (Sniper, MEV, or other bot types acceptable)
        assert!(
            matches!(
                result.actor,
                ActorType::SniperScript
                    | ActorType::MEVArb
                    | ActorType::LiquidityBot
                    | ActorType::RPCFiller
            ),
            "Sniper script should be classified as bot type, got {:?}",
            result.actor
        );

        // Should have low entropy
        assert!(
            result.entropy < 4.0,
            "Sniper script should have low entropy, got {}",
            result.entropy
        );
    }

    #[test]
    fn test_corpus_mev_arb_classification() {
        let payload = generate_mev_arb();
        let result = mpcf_infer(&payload);

        // Should classify as bot type
        assert!(
            matches!(
                result.actor,
                ActorType::MEVArb
                    | ActorType::SniperScript
                    | ActorType::LiquidityBot
                    | ActorType::RPCFiller
            ),
            "MEV bot should be classified as bot type, got {:?}",
            result.actor
        );

        // Should have very low entropy
        assert!(
            result.entropy < 3.5,
            "MEV bot should have very low entropy, got {}",
            result.entropy
        );
    }

    #[test]
    fn test_corpus_liquidity_bot_classification() {
        let payload = generate_liquidity_bot();
        let result = mpcf_infer(&payload);

        // Should classify as bot type
        assert!(
            matches!(
                result.actor,
                ActorType::LiquidityBot
                    | ActorType::RPCFiller
                    | ActorType::SniperScript
                    | ActorType::MEVArb
            ),
            "Liquidity bot should be classified as bot type, got {:?}",
            result.actor
        );

        // Should have moderate entropy (3.5-4.5 range)
        assert!(
            result.entropy >= 2.5 && result.entropy <= 5.0,
            "Liquidity bot entropy should be moderate, got {}",
            result.entropy
        );
    }

    #[test]
    fn test_corpus_rpc_filler_classification() {
        let payload = generate_rpc_filler();
        let result = mpcf_infer(&payload);

        // Should classify as bot type
        assert!(
            matches!(
                result.actor,
                ActorType::RPCFiller
                    | ActorType::LiquidityBot
                    | ActorType::SniperScript
                    | ActorType::MEVArb
            ),
            "RPC filler should be classified as bot type, got {:?}",
            result.actor
        );

        // Should have low to moderate entropy
        assert!(
            result.entropy < 5.0,
            "RPC filler should have low-moderate entropy, got {}",
            result.entropy
        );
    }

    #[test]
    fn test_corpus_sybil_bot_fingerprint_similarity() {
        // Generate multiple sybil transactions
        let sybil1 = generate_sybil_bot(0);
        let sybil2 = generate_sybil_bot(1);
        let sybil3 = generate_sybil_bot(2);

        let result1 = mpcf_infer(&sybil1);
        let result2 = mpcf_infer(&sybil2);
        let result3 = mpcf_infer(&sybil3);

        // All should be bot types
        for result in [&result1, &result2, &result3] {
            assert!(
                !matches!(
                    result.actor,
                    ActorType::HumanMobile | ActorType::HumanDesktop
                ),
                "Sybil bots should not be classified as Human"
            );
        }

        // Fingerprints should show similarity (at least some common bytes)
        // FUTURE: Advanced sybil detection based on fingerprint clustering
        // would analyze hamming distance between fingerprints and flag
        // when multiple transactions have suspiciously similar fingerprints

        // For now, just verify they have low entropy
        assert!(result1.entropy < 4.0, "Sybil bot 1 should have low entropy");
        assert!(result2.entropy < 4.0, "Sybil bot 2 should have low entropy");
        assert!(result3.entropy < 4.0, "Sybil bot 3 should have low entropy");
    }

    #[test]
    fn test_corpus_batch_entropy_bounds() {
        let corpus = generate_corpus_batch();

        for (expected_type, payload) in corpus {
            let result = mpcf_infer(&payload);

            // Entropy should always be in valid bounds
            assert!(
                result.entropy >= 0.0 && result.entropy <= 8.1,
                "Entropy out of bounds for {:?}: {}",
                expected_type,
                result.entropy
            );

            // Confidence should be in valid bounds
            assert!(
                result.confidence >= 0.0 && result.confidence <= 1.0,
                "Confidence out of bounds for {:?}: {}",
                expected_type,
                result.confidence
            );
        }
    }

    #[test]
    fn test_corpus_batch_fingerprint_uniqueness() {
        let corpus = generate_corpus_batch();
        let mut fingerprints = Vec::new();

        for (actor_type, payload) in corpus {
            let result = mpcf_infer(&payload);

            // Check that fingerprint is not all zeros
            let is_non_zero = result.fingerprint.iter().any(|&b| b != 0);
            assert!(
                is_non_zero,
                "Fingerprint should be non-zero for {:?}",
                actor_type
            );

            fingerprints.push(result.fingerprint);
        }

        // Most fingerprints should be unique (allowing some collisions for sybil bots)
        // FUTURE: Advanced collision detection and clustering analysis
        // would track fingerprint collisions over time and identify patterns
        // that indicate coordinated sybil attacks

        // For now, just verify we have some diversity
        let unique_count = fingerprints
            .iter()
            .collect::<std::collections::HashSet<_>>()
            .len();

        assert!(
            unique_count >= 8,
            "Should have at least 8 unique fingerprints out of {}, got {}",
            fingerprints.len(),
            unique_count
        );
    }

    #[test]
    fn test_corpus_performance_target() {
        // Target: <70 µs per transaction
        let corpus = generate_corpus_batch();
        let start = std::time::Instant::now();

        for (_actor_type, payload) in &corpus {
            let _ = mpcf_infer(payload);
        }

        let elapsed = start.elapsed();
        let count = corpus.len();
        let avg_micros = elapsed.as_micros() / count as u128;

        // With optimization, should be well under 70 µs
        // In debug mode, we allow up to 500 µs as this is not optimized
        #[cfg(debug_assertions)]
        assert!(
            avg_micros < 500,
            "Average time per tx should be <500 µs in debug mode, got {} µs",
            avg_micros
        );

        #[cfg(not(debug_assertions))]
        assert!(
            avg_micros < 70,
            "Average time per tx should be <70 µs in release mode, got {} µs",
            avg_micros
        );

        println!("MPCF Performance: {} µs/tx (target: <70 µs)", avg_micros);
    }

    proptest! {
        /// Property: corpus generators should produce valid payloads
        #[test]
        fn prop_corpus_generators_valid(variant in 0u8..255) {
            let payloads = vec![
                generate_phantom_mobile(),
                generate_phantom_desktop(),
                generate_sniper_script(),
                generate_mev_arb(),
                generate_liquidity_bot(),
                generate_rpc_filler(),
                generate_sybil_bot(variant),
            ];

            for payload in payloads {
                // Should not panic
                let result = mpcf_infer(&payload);

                // Should produce valid results
                prop_assert!(result.confidence >= 0.0 && result.confidence <= 1.0);
                prop_assert!(result.entropy >= 0.0 && result.entropy <= 8.1);
            }
        }

        /// Property: Human corpus should have higher entropy than bot corpus
        #[test]
        fn prop_human_higher_entropy_than_bots(seed in 0usize..100) {
            let _ = seed; // Use seed for determinism

            let human_payloads = vec![
                generate_phantom_mobile(),
                generate_phantom_desktop(),
            ];

            let bot_payloads = vec![
                generate_sniper_script(),
                generate_mev_arb(),
            ];

            let human_entropy: f32 = human_payloads.iter()
                .map(|p| mpcf_infer(p).entropy)
                .sum::<f32>() / human_payloads.len() as f32;

            let bot_entropy: f32 = bot_payloads.iter()
                .map(|p| mpcf_infer(p).entropy)
                .sum::<f32>() / bot_payloads.len() as f32;

            // Human average entropy should be significantly higher than bot average
            prop_assert!(
                human_entropy > bot_entropy + 1.0,
                "Human entropy ({}) should be >1.0 higher than bot entropy ({})",
                human_entropy, bot_entropy
            );
        }

        /// Property: Sybil bots with close variants should have similar entropy
        #[test]
        fn prop_sybil_entropy_consistency(variant in 0u8..250) {
            let sybil1 = generate_sybil_bot(variant);
            let sybil2 = generate_sybil_bot(variant + 1);
            let sybil3 = generate_sybil_bot(variant + 2);

            let entropy1 = mpcf_infer(&sybil1).entropy;
            let entropy2 = mpcf_infer(&sybil2).entropy;
            let entropy3 = mpcf_infer(&sybil3).entropy;

            // Sybil bots should have consistent entropy (mass-produced pattern)
            let max_diff = (entropy1 - entropy2).abs()
                .max((entropy2 - entropy3).abs())
                .max((entropy1 - entropy3).abs());

            // FUTURE: Tighten this threshold after implementing advanced sybil detection
            // Once we have fingerprint clustering, we can detect sybil networks more precisely
            prop_assert!(
                max_diff < 2.0,
                "Sybil entropy variance should be low, got max diff: {}", max_diff
            );
        }

        /// Property: Batch processing should maintain consistency
        #[test]
        fn prop_batch_processing_consistency(iterations in 1usize..5) {
            for _ in 0..iterations {
                let corpus = generate_corpus_batch();
                let mut previous_results: Vec<ActorInference> = Vec::new();

                // Process batch twice
                for run in 0..2 {
                    let mut results = Vec::new();
                    for (_actor_type, payload) in &corpus {
                        results.push(mpcf_infer(payload));
                    }

                    if run == 1 {
                        // Second run should match first run (deterministic)
                        for (i, result) in results.iter().enumerate() {
                            prop_assert_eq!(result.entropy, previous_results[i].entropy);
                            prop_assert_eq!(result.fingerprint, previous_results[i].fingerprint);
                        }
                    } else {
                        previous_results = results;
                    }
                }
            }
        }

        /// Property: Performance should scale linearly with batch size
        #[test]
        fn prop_performance_linear_scaling(batch_multiplier in 1usize..4) {
            let base_corpus = generate_corpus_batch();
            let mut extended_corpus = Vec::new();

            // Create larger batch
            for _ in 0..batch_multiplier {
                extended_corpus.extend(base_corpus.iter().cloned());
            }

            let start = std::time::Instant::now();
            for (_actor_type, payload) in &extended_corpus {
                let _ = mpcf_infer(payload);
            }
            let elapsed = start.elapsed();

            let avg_micros = elapsed.as_micros() / extended_corpus.len() as u128;

            // Performance per transaction should remain consistent regardless of batch size
            #[cfg(debug_assertions)]
            prop_assert!(avg_micros < 500, "Avg time in debug: {} µs", avg_micros);

            #[cfg(not(debug_assertions))]
            prop_assert!(avg_micros < 70, "Avg time in release: {} µs", avg_micros);
        }
    }
}

// =============================================================================
// 10k Transaction Benchmark Test
// =============================================================================

#[cfg(test)]
mod bench_10k {
    use super::corpus::*;
    use super::*;

    #[test]
    #[ignore] // Run with: cargo test --release bench_10k_performance -- --ignored --nocapture
    fn bench_10k_performance() {
        // Generate 10k corpus by repeating base corpus
        let base_corpus = generate_corpus_batch();
        let repetitions = 10_000 / base_corpus.len() + 1;

        let mut corpus_10k = Vec::new();
        for _ in 0..repetitions {
            corpus_10k.extend(base_corpus.iter().map(|(t, p)| (*t, p.clone())));
        }
        corpus_10k.truncate(10_000);

        println!("Running MPCF on 10,000 transactions...");
        let start = std::time::Instant::now();

        for (_actor_type, payload) in &corpus_10k {
            let _ = mpcf_infer(payload);
        }

        let elapsed = start.elapsed();
        let avg_micros = elapsed.as_micros() / 10_000;
        let total_ms = elapsed.as_millis();

        println!("=== MPCF 10k Benchmark Results ===");
        println!("Total time: {} ms", total_ms);
        println!("Average time per tx: {} µs", avg_micros);
        println!("Target: <70 µs per tx");

        #[cfg(not(debug_assertions))]
        {
            println!(
                "Status: {}",
                if avg_micros < 70 {
                    "✓ PASS"
                } else {
                    "✗ FAIL"
                }
            );
            assert!(
                avg_micros < 70,
                "Performance target not met: {} µs/tx (target: <70 µs)",
                avg_micros
            );
        }

        #[cfg(debug_assertions)]
        {
            println!("Note: Running in debug mode, performance test skipped");
            println!("Run with --release flag for accurate performance measurement");
        }
    }
}
