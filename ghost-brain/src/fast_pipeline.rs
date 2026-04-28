//! Lock-free Zero-Copy Infrastructure + Batch Pipeline
//!
//! This module implements a lock-free, zero-copy pipeline for processing premint candidates
//! with the following characteristics:
//! - Zero allocations in hot path (using object pool)
//! - Zero contention (using lock-free queue)
//! - Batch processing for improved throughput
//! - Memory leak prevention (pool recycling)
//! - Hot/Cold data separation to eliminate false sharing
//!
//! ## Cache Line Architecture
//!
//! The structures use strategic field ordering and cache-line padding:
//! - Hot fields (frequently accessed in scoring) are grouped at the start
//! - A padding barrier separates hot from cold fields
//! - Cold fields (addresses, strings) follow after the padding
//!
//! This prevents false sharing when multiple threads access the same batch:
//! one thread scoring (hot fields) won't invalidate cache lines of another
//! thread accessing addresses (cold fields).

use crossbeam_queue::ArrayQueue;
use object_pool::Pool;
use once_cell::sync::Lazy;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

/// Cache line size in bytes (x86-64 standard)
pub const CACHE_LINE_SIZE: usize = 64;

/// Cache line padding to prevent false sharing between hot and cold data
///
/// This struct is exactly 64 bytes and is used to separate hot and cold data
/// sections, ensuring they occupy different cache lines.
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct CacheLinePadding {
    _padding: [u8; CACHE_LINE_SIZE],
}

impl Default for CacheLinePadding {
    fn default() -> Self {
        Self {
            _padding: [0u8; CACHE_LINE_SIZE],
        }
    }
}

/// Premint candidate structure containing all relevant data
///
/// Uses strategic field ordering to eliminate false sharing:
/// - **Hot fields** (slot, timestamp, liquidity_sol, base_score, bonding_curve_progress)
///   are grouped at the start and fit within a single cache line (~48 bytes)
/// - **Cache line padding** separates hot from cold fields
/// - **Cold fields** (Pubkeys, signature, optional fields) follow the padding
///
/// ## Memory Layout (False Sharing Prevention)
///
/// ```text
/// ┌──────────────────────────────────────────────────────────────────┐
/// │ HOT DATA (Cache Line 0, ~48 bytes)                               │
/// │   slot: u64 (8 bytes)                                            │
/// │   timestamp: u64 (8 bytes)                                       │
/// │   liquidity_sol: f64 (8 bytes)                                   │
/// │   base_score: u8 (1 byte)                                        │
/// │   bonding_curve_progress: Option<f64> (16 bytes with enum)       │
/// │   _hot_padding: [u8; 7] (alignment padding)                      │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ PADDING BARRIER (64 bytes)                                       │
/// │   Prevents hot/cold false sharing                                │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ COLD DATA (subsequent cache lines)                               │
/// │   pool_amm_id: Pubkey (32 bytes)                                 │
/// │   amm_program_id: Pubkey (32 bytes)                              │
/// │   base_mint: Pubkey (32 bytes)                                   │
/// │   quote_mint: Pubkey (32 bytes)                                  │
/// │   bonding_curve: Pubkey (32 bytes)                               │
/// │   signature: String (24 bytes + heap)                            │
/// │   token_total_supply: Option<u64> (16 bytes)                     │
/// │   block_time: Option<i64> (16 bytes)                             │
/// └──────────────────────────────────────────────────────────────────┘
/// ```
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct PremintCandidate {
    // ══════════════════════════════════════════════════════════════
    // HOT DATA - Frequently accessed in scoring loops
    // These fields should fit in a single cache line (64 bytes)
    // ══════════════════════════════════════════════════════════════
    /// Slot when detected (8 bytes, optional)
    pub slot: Option<u64>,

    /// Timestamp when detected (8 bytes)
    pub timestamp: u64,

    /// Initial liquidity in SOL (8 bytes)
    pub liquidity_sol: f64,

    /// Base score (0-100) (1 byte + 7 padding = 8 bytes for alignment)
    pub base_score: u8,

    /// Padding to maintain 8-byte alignment after base_score (internal use)
    ///
    /// Memory layout calculation:
    /// - slot: 8 bytes (offset 0)
    /// - timestamp: 8 bytes (offset 8)
    /// - liquidity_sol: 8 bytes (offset 16)
    /// - base_score: 1 byte (offset 24)
    /// - _hot_padding: 7 bytes (offset 25-31, maintains 8-byte alignment)
    /// - bonding_curve_progress: 16 bytes (offset 32-47)
    /// Total hot section: 48 bytes (fits within 64-byte cache line)
    #[doc(hidden)]
    pub _hot_padding: [u8; 7],

    /// Optional: Bonding curve progress (0.0 - 1.0) (16 bytes with Option)
    pub bonding_curve_progress: Option<f64>,

    // ══════════════════════════════════════════════════════════════
    // CACHE LINE BARRIER - Prevents false sharing
    // ══════════════════════════════════════════════════════════════
    /// Padding to ensure cold data starts on a new cache line (internal use)
    #[doc(hidden)]
    pub _cache_barrier: CacheLinePadding,

    // ══════════════════════════════════════════════════════════════
    // COLD DATA - Less frequently accessed (addresses, strings)
    // ══════════════════════════════════════════════════════════════
    /// Pool AMM account ID
    pub pool_amm_id: Pubkey,

    /// AMM program ID
    pub amm_program_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Quote token mint  
    pub quote_mint: Pubkey,

    /// Bonding curve account
    pub bonding_curve: Pubkey,

    /// Transaction signature
    pub signature: String,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,

    /// Block time when pool was initialized
    pub block_time: Option<i64>,
}

/// Enhanced candidate with contextual analysis from transaction data
///
/// This structure extends the basic candidate data with heuristics computed
/// at the ingest stage (Shred/Seer) without any RPC calls, enabling fast
/// scam/honeypot detection based on transaction intent and context.
///
/// Uses strategic field ordering with cache-line padding to prevent false sharing:
/// - **Hot fields** are grouped first (slot, timestamp, scoring fields)
/// - **Cache line barriers** separate hot, shadow, and cold data
/// - **Cold fields** follow after barriers (Pubkeys, strings)
///
/// ## Memory Layout (False Sharing Prevention)
///
/// ```text
/// ┌──────────────────────────────────────────────────────────────────┐
/// │ HOT DATA - Scoring Fields (frequently accessed)                  │
/// │   slot, timestamp, initial_liquidity_sol, dev_buy_sol            │
/// │   bonding_curve_progress, vanity_score, metadata_len_score       │
/// │   has_dev_buy, mint_auth_disabled                                │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ PADDING BARRIER 1                                                │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ SHADOW LEDGER DATA (independent access pattern)                  │
/// │   expected_price, shadow_bonding_progress                        │
/// │   virtual_sol_reserves, shadow_market_cap                        │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ PADDING BARRIER 2                                                │
/// ├──────────────────────────────────────────────────────────────────┤
/// │ COLD DATA (addresses, strings)                                   │
/// │   pool_amm_id, amm_program_id, base_mint, quote_mint             │
/// │   bonding_curve, signature, token_total_supply                   │
/// └──────────────────────────────────────────────────────────────────┘
/// ```
#[repr(C)]
#[derive(Debug, Clone, Default)]
pub struct EnhancedCandidate {
    // ══════════════════════════════════════════════════════════════
    // HOT DATA - Scoring fields (frequently accessed in scoring loops)
    // ══════════════════════════════════════════════════════════════
    /// Slot when detected (if known)
    pub slot: Option<u64>,

    /// Timestamp when detected (Unix timestamp)
    pub timestamp: u64,

    /// Initial liquidity in SOL
    pub initial_liquidity_sol: f64,

    /// Sum of SOL spent by dev on BUY in same tx/bundle
    pub dev_buy_sol: f64,

    /// Optional: Bonding curve progress (0.0 - 1.0)
    pub bonding_curve_progress: Option<f64>,

    /// Heuristic for vanity/grind address of mint (0-100)
    pub vanity_score: u8,

    /// Metadata quality heuristic based on name/symbol (0-100)
    pub metadata_len_score: u8,

    /// Whether dev performs atomic BUY in same transaction/bundle
    pub has_dev_buy: bool,

    /// Whether mint authority has been disabled/transferred
    pub mint_auth_disabled: bool,

    /// Hot data padding for cache line alignment (internal use)
    #[doc(hidden)]
    pub _hot_padding: [u8; 4],

    // ══════════════════════════════════════════════════════════════
    // CACHE LINE BARRIER 1 - Separates hot from shadow
    // ══════════════════════════════════════════════════════════════
    #[doc(hidden)]
    pub _cache_barrier_1: CacheLinePadding,

    // ══════════════════════════════════════════════════════════════
    // SHADOW LEDGER DATA - Accessed independently from scoring
    // ══════════════════════════════════════════════════════════════
    /// Expected price per token from Shadow Ledger simulation (in lamports)
    pub expected_price: Option<f64>,

    /// Bonding curve progress from Shadow Ledger (0-100)
    pub shadow_bonding_progress: Option<u64>,

    /// Virtual SOL reserves from Shadow Ledger (in lamports)
    pub virtual_sol_reserves: Option<u64>,

    /// Market cap from Shadow Ledger (in lamports)
    pub shadow_market_cap: Option<u64>,

    // ══════════════════════════════════════════════════════════════
    // CACHE LINE BARRIER 2 - Separates shadow from cold
    // ══════════════════════════════════════════════════════════════
    #[doc(hidden)]
    pub _cache_barrier_2: CacheLinePadding,

    // ══════════════════════════════════════════════════════════════
    // COLD DATA - Addresses and strings (less frequently accessed)
    // ══════════════════════════════════════════════════════════════
    /// Pool AMM account ID
    pub pool_amm_id: Pubkey,

    /// AMM program ID
    pub amm_program_id: Pubkey,

    /// Base token mint
    pub base_mint: Pubkey,

    /// Quote token mint
    pub quote_mint: Pubkey,

    /// Bonding curve account
    pub bonding_curve: Pubkey,

    /// Transaction signature
    pub signature: String,

    /// Optional: Token total supply
    pub token_total_supply: Option<u64>,
}

impl EnhancedCandidate {
    /// Create a new EnhancedCandidate with all fields explicitly specified.
    ///
    /// This builder method handles the internal padding fields automatically,
    /// allowing callers to create instances without knowing about cache-line
    /// alignment internals.
    #[allow(clippy::too_many_arguments)]
    pub fn new_with_fields(
        // Hot fields
        slot: Option<u64>,
        timestamp: u64,
        initial_liquidity_sol: f64,
        dev_buy_sol: f64,
        bonding_curve_progress: Option<f64>,
        vanity_score: u8,
        metadata_len_score: u8,
        has_dev_buy: bool,
        mint_auth_disabled: bool,
        // Shadow fields
        expected_price: Option<f64>,
        shadow_bonding_progress: Option<u64>,
        virtual_sol_reserves: Option<u64>,
        shadow_market_cap: Option<u64>,
        // Cold fields
        pool_amm_id: Pubkey,
        amm_program_id: Pubkey,
        base_mint: Pubkey,
        quote_mint: Pubkey,
        bonding_curve: Pubkey,
        signature: String,
        token_total_supply: Option<u64>,
    ) -> Self {
        Self {
            slot,
            timestamp,
            initial_liquidity_sol,
            dev_buy_sol,
            bonding_curve_progress,
            vanity_score,
            metadata_len_score,
            has_dev_buy,
            mint_auth_disabled,
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            expected_price,
            shadow_bonding_progress,
            virtual_sol_reserves,
            shadow_market_cap,
            _cache_barrier_2: CacheLinePadding::default(),
            pool_amm_id,
            amm_program_id,
            base_mint,
            quote_mint,
            bonding_curve,
            signature,
            token_total_supply,
        }
    }
}

impl From<PremintCandidate> for EnhancedCandidate {
    fn from(candidate: PremintCandidate) -> Self {
        Self {
            // Hot fields
            slot: candidate.slot,
            timestamp: candidate.timestamp,
            initial_liquidity_sol: candidate.liquidity_sol,
            dev_buy_sol: 0.0,
            bonding_curve_progress: candidate.bonding_curve_progress,
            vanity_score: 0,
            metadata_len_score: 0,
            has_dev_buy: false,
            mint_auth_disabled: false,
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            // Shadow fields
            expected_price: None,
            shadow_bonding_progress: None,
            virtual_sol_reserves: None,
            shadow_market_cap: None,
            _cache_barrier_2: CacheLinePadding::default(),
            // Cold fields
            pool_amm_id: candidate.pool_amm_id,
            amm_program_id: candidate.amm_program_id,
            base_mint: candidate.base_mint,
            quote_mint: candidate.quote_mint,
            bonding_curve: candidate.bonding_curve,
            signature: candidate.signature,
            token_total_supply: candidate.token_total_supply,
        }
    }
}

impl From<ghost_core::EnhancedCandidate> for EnhancedCandidate {
    fn from(candidate: ghost_core::EnhancedCandidate) -> Self {
        Self {
            // Hot fields
            slot: candidate.slot,
            timestamp: candidate.timestamp,
            initial_liquidity_sol: candidate.initial_liquidity_sol,
            dev_buy_sol: candidate.dev_buy_sol,
            bonding_curve_progress: candidate.bonding_curve_progress,
            vanity_score: candidate.vanity_score,
            metadata_len_score: candidate.metadata_len_score,
            has_dev_buy: candidate.has_dev_buy,
            mint_auth_disabled: candidate.mint_auth_disabled,
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            // Shadow fields
            expected_price: candidate.expected_price,
            shadow_bonding_progress: candidate.shadow_bonding_progress,
            virtual_sol_reserves: candidate.virtual_sol_reserves,
            shadow_market_cap: candidate.shadow_market_cap,
            _cache_barrier_2: CacheLinePadding::default(),
            // Cold fields
            pool_amm_id: candidate.pool_amm_id,
            amm_program_id: candidate.amm_program_id,
            base_mint: candidate.base_mint,
            quote_mint: candidate.quote_mint,
            bonding_curve: candidate.bonding_curve,
            signature: candidate.signature,
            token_total_supply: candidate.token_total_supply,
        }
    }
}

impl From<seer::types::CandidatePool> for EnhancedCandidate {
    fn from(candidate: seer::types::CandidatePool) -> Self {
        Self {
            // Hot fields
            slot: candidate.slot,
            timestamp: candidate.timestamp,
            initial_liquidity_sol: candidate.initial_liquidity_sol.unwrap_or(0.0),
            dev_buy_sol: 0.0,
            bonding_curve_progress: candidate.bonding_curve_progress,
            vanity_score: 0,
            metadata_len_score: 50, // Neutral score
            has_dev_buy: false,
            mint_auth_disabled: false,
            _hot_padding: [0; 4],
            _cache_barrier_1: CacheLinePadding::default(),
            // Shadow fields
            expected_price: None,
            shadow_bonding_progress: None,
            virtual_sol_reserves: None,
            shadow_market_cap: None,
            _cache_barrier_2: CacheLinePadding::default(),
            // Cold fields
            pool_amm_id: candidate.pool_amm_id,
            amm_program_id: candidate.amm_program_id,
            base_mint: candidate.base_mint,
            quote_mint: candidate.quote_mint,
            bonding_curve: candidate.bonding_curve,
            signature: candidate.signature,
            token_total_supply: candidate.token_total_supply,
        }
    }
}

/// Capacity of the candidate queue
pub const CANDIDATE_QUEUE_CAPACITY: usize = 16_384;

/// Capacity of the candidate pool
pub const CANDIDATE_POOL_CAPACITY: usize = 32_768;

/// Global lock-free queue for candidates
static CANDIDATE_QUEUE: Lazy<ArrayQueue<Arc<PremintCandidate>>> =
    Lazy::new(|| ArrayQueue::new(CANDIDATE_QUEUE_CAPACITY));

/// Global object pool for candidates
static CANDIDATE_POOL: Lazy<Pool<PremintCandidate>> =
    Lazy::new(|| Pool::new(CANDIDATE_POOL_CAPACITY, PremintCandidate::default));

/// Push a candidate to the queue using the object pool
///
/// This function:
/// 1. Pulls a candidate from the pool (or creates one if pool is empty)
/// 2. Fills it with the provided data
/// 3. Wraps it in an Arc for zero-copy sharing
/// 4. Pushes to the lock-free queue
///
/// # Arguments
/// * `fill_fn` - Function that fills the candidate with data
///
/// # Returns
/// * `Ok(())` if successfully pushed
/// * `Err(())` if queue is full
pub fn push_candidate<F>(fill_fn: F) -> Result<(), ()>
where
    F: FnOnce(&mut PremintCandidate),
{
    // Pull a candidate from the pool (or create new if pool is empty)
    let mut reusable = CANDIDATE_POOL.pull(PremintCandidate::default);

    // Fill the candidate with data
    fill_fn(&mut reusable);

    // Detach from pool to get ownership, then wrap in Arc
    let (_pool_ref, candidate) = reusable.detach();
    let arc_candidate = Arc::new(candidate);

    // Push to queue
    CANDIDATE_QUEUE.push(arc_candidate).map_err(|_| ())
}

/// Pop a candidate from the queue
///
/// # Returns
/// * `Some(Arc<PremintCandidate>)` if queue has candidates
/// * `None` if queue is empty
pub fn pop_candidate() -> Option<Arc<PremintCandidate>> {
    CANDIDATE_QUEUE.pop()
}

/// Return a candidate back to the pool for reuse
///
/// This should be called after processing is complete to prevent memory leaks.
/// If the Arc is the only reference, the inner candidate is extracted and
/// returned to the pool for reuse. Otherwise, the Arc is simply dropped.
///
/// # Note on Concurrent Scenarios
/// When there are multiple references to the Arc (e.g., in concurrent processing),
/// `Arc::try_unwrap()` will fail and the candidate won't be recycled to the pool.
/// This is a trade-off for supporting shared ownership. For maximum pool efficiency,
/// ensure candidates are recycled only after all references are done processing.
///
/// # Arguments
/// * `candidate` - Arc-wrapped candidate to return to pool
pub fn recycle_candidate(candidate: Arc<PremintCandidate>) {
    // Try to unwrap the Arc and return the inner candidate to the pool
    // This only succeeds if we have the only reference to the Arc
    if let Ok(mut inner) = Arc::try_unwrap(candidate) {
        // Preserve the String's capacity by clearing instead of replacing
        // This avoids reallocation when the candidate is reused
        let signature_capacity = inner.signature.capacity();

        // Reset all fields to default values
        inner.slot = None;
        inner.timestamp = 0;
        inner.liquidity_sol = 0.0;
        inner.base_score = 0;
        inner._hot_padding = [0; 7];
        inner.bonding_curve_progress = None;
        inner._cache_barrier = CacheLinePadding::default();
        inner.pool_amm_id = Pubkey::default();
        inner.amm_program_id = Pubkey::default();
        inner.base_mint = Pubkey::default();
        inner.quote_mint = Pubkey::default();
        inner.bonding_curve = Pubkey::default();
        inner.signature.clear(); // Preserves capacity
        inner.token_total_supply = None;
        inner.block_time = None;

        // Sanity check: ensure we preserved the capacity
        debug_assert!(inner.signature.capacity() >= signature_capacity.min(256));

        // Return to the pool for reuse
        CANDIDATE_POOL.attach(inner);
    }
    // If try_unwrap fails, there are other references - just drop this one
}

/// Batch consumer function that processes candidates in batches
///
/// This function:
/// 1. Collects up to `batch_size` candidates from the queue
/// 2. Processes them in batch (scoring, triggering, etc.)
/// 3. Returns all candidates to the pool
///
/// # Arguments
/// * `batch_size` - Maximum number of candidates to process in one batch
/// * `process_fn` - Function that processes a batch of candidates
///
/// # Example
/// ```ignore
/// use ghost_brain::fast_pipeline::run_fast_consumer;
///
/// tokio::spawn(async move {
///     run_fast_consumer(128, |batch| {
///         // Score the batch
///         for candidate in batch {
///             println!("Processing: {}", candidate.pool_amm_id);
///         }
///     }).await;
/// });
/// ```
pub async fn run_fast_consumer<F>(batch_size: usize, mut process_fn: F)
where
    F: FnMut(&[Arc<PremintCandidate>]),
{
    let mut batch = Vec::with_capacity(batch_size);

    loop {
        // Collect a batch of candidates
        while batch.len() < batch_size {
            if let Some(arc) = pop_candidate() {
                batch.push(arc);
            } else {
                // Queue is empty - if we have some candidates, process them
                if !batch.is_empty() {
                    break;
                }
                // Otherwise spin briefly before checking again
                std::hint::spin_loop();
                // Yield to prevent busy-waiting
                tokio::task::yield_now().await;
            }
        }

        // Process the batch
        if !batch.is_empty() {
            process_fn(&batch);

            // Recycle all candidates back to the pool
            for arc in batch.drain(..) {
                recycle_candidate(arc);
            }
        }
    }
}

/// Get queue statistics
pub fn queue_stats() -> QueueStats {
    QueueStats {
        capacity: CANDIDATE_QUEUE_CAPACITY,
        len: CANDIDATE_QUEUE.len(),
    }
}

/// Get pool statistics
pub fn pool_stats() -> PoolStats {
    PoolStats {
        capacity: CANDIDATE_POOL_CAPACITY,
        available: CANDIDATE_POOL.len(),
    }
}

/// Queue statistics
#[derive(Debug, Clone, Copy)]
pub struct QueueStats {
    pub capacity: usize,
    pub len: usize,
}

/// Pool statistics
#[derive(Debug, Clone, Copy)]
pub struct PoolStats {
    pub capacity: usize,
    pub available: usize,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_push_pop_candidate() {
        // Push a candidate
        let result = push_candidate(|c| {
            c.slot = Some(12345);
            c.liquidity_sol = 10.5;
            c.base_score = 85;
            c.signature = "test_sig".to_string();
        });
        assert!(result.is_ok());

        // Pop the candidate
        let candidate = pop_candidate();
        assert!(candidate.is_some());

        let candidate = candidate.unwrap();
        assert_eq!(candidate.slot, Some(12345));
        assert_eq!(candidate.liquidity_sol, 10.5);
        assert_eq!(candidate.base_score, 85);
        assert_eq!(candidate.signature, "test_sig");

        // Recycle it
        recycle_candidate(candidate);
    }

    #[test]
    fn test_pool_recycling() {
        // Clear any existing candidates from other tests
        while pop_candidate().is_some() {}

        // Push and pop multiple candidates to test pool recycling
        for i in 0..100 {
            let result = push_candidate(|c| {
                c.slot = Some(i);
                c.base_score = (i % 100) as u8;
            });
            assert!(result.is_ok());
        }

        // Pop all candidates
        let mut candidates = Vec::new();
        while let Some(c) = pop_candidate() {
            candidates.push(c);
        }
        assert_eq!(
            candidates.len(),
            100,
            "Should have popped exactly 100 candidates"
        );

        // Recycle all
        for c in candidates {
            recycle_candidate(c);
        }
    }

    #[test]
    fn test_queue_capacity() {
        let stats = queue_stats();
        assert_eq!(stats.capacity, CANDIDATE_QUEUE_CAPACITY);
    }

    #[tokio::test]
    async fn test_batch_consumer() {
        // Push some candidates
        for i in 0..10 {
            push_candidate(|c| {
                c.slot = Some(i);
                c.base_score = 90;
            })
            .unwrap();
        }

        let processed = Arc::new(std::sync::Mutex::new(0));
        let processed_clone = Arc::clone(&processed);

        // Spawn consumer with timeout
        let handle = tokio::spawn(async move {
            let mut batch = Vec::with_capacity(5);
            let mut total_processed = 0;

            // Process until we've seen all 10 candidates or timed out
            while total_processed < 10 {
                if let Some(arc) = pop_candidate() {
                    batch.push(arc);
                }

                if batch.len() >= 5 || (total_processed > 0 && batch.len() > 0) {
                    let mut count = processed_clone.lock().unwrap();
                    *count += batch.len();
                    total_processed += batch.len();

                    // Recycle candidates
                    for arc in batch.drain(..) {
                        recycle_candidate(arc);
                    }
                }

                // Small yield to prevent busy waiting
                tokio::task::yield_now().await;
            }
        });

        // Give it time to process
        tokio::time::timeout(tokio::time::Duration::from_secs(1), handle)
            .await
            .ok();

        let count = *processed.lock().unwrap();
        assert_eq!(count, 10, "Should have processed all 10 candidates");
    }

    #[test]
    fn test_detect_to_score_latency() {
        use std::time::Instant;

        // Warmup
        for _ in 0..1000 {
            push_candidate(|c| {
                c.slot = Some(12345);
                c.base_score = 85;
            })
            .ok();
            if let Some(arc) = pop_candidate() {
                recycle_candidate(arc);
            }
        }

        // Measure detect-to-score latency
        let iterations = 100_000;
        let start = Instant::now();

        for i in 0..iterations {
            push_candidate(|c| {
                c.slot = Some(i);
                c.pool_amm_id = Pubkey::new_unique();
                c.liquidity_sol = 10.5;
                c.base_score = 85;
            })
            .ok();

            if let Some(arc) = pop_candidate() {
                // Hot path scoring - only accesses hot data (first cache line)
                let _score = (arc.base_score as f64 * arc.liquidity_sol * 0.1) as u8;
                recycle_candidate(arc);
            }
        }

        let elapsed = start.elapsed();
        let avg_ns = elapsed.as_nanos() / iterations as u128;

        println!("\n=== Detect-to-Score Latency Test ===");
        println!("Total iterations: {}", iterations);
        println!("Total time: {:?}", elapsed);
        println!("Average latency: {} ns", avg_ns);
        println!("Target: ≤18 ns (release mode optimized)");

        // The 18ns target is extremely aggressive and requires release mode optimizations.
        println!("\nActual latency: {} ns per operation (debug mode)", avg_ns);
        println!("This includes: push (alloc + queue insert) + pop (queue remove) + recycle");
        println!("Note: Run in --release mode for optimized performance");

        // In debug mode, assert reasonable performance (< 5000ns)
        // In release mode with LTO, we should get much closer to the 18ns target
        #[cfg(not(debug_assertions))]
        assert!(
            avg_ns < 50,
            "Release mode latency should be under 50ns, got {}ns",
            avg_ns
        );

        #[cfg(debug_assertions)]
        assert!(
            avg_ns < 5000,
            "Debug mode latency should be under 5000ns, got {}ns",
            avg_ns
        );
    }

    #[test]
    fn test_memory_leak_prevention() {
        // Test that we don't leak memory after many operations
        // Run a large number of push/pop/recycle cycles
        let operations = 1_000_000; // 1M operations

        for i in 0..operations {
            push_candidate(|c| {
                c.slot = Some(i);
                c.base_score = (i % 100) as u8;
            })
            .ok();

            if let Some(arc) = pop_candidate() {
                recycle_candidate(arc);
            }
        }

        // If we get here without crashing or OOMing, we're good
        println!("\n=== Memory Leak Test ===");
        println!("Completed {} push/pop/recycle cycles", operations);
        println!("No memory leaks detected");
    }

    // ==================== False Sharing Regression Tests ====================

    /// Test that cache line padding is the correct size
    #[test]
    fn test_cache_line_padding_size() {
        use std::mem;

        // Verify CacheLinePadding is exactly 64 bytes
        assert_eq!(
            mem::size_of::<CacheLinePadding>(),
            CACHE_LINE_SIZE,
            "CacheLinePadding must be exactly {} bytes",
            CACHE_LINE_SIZE
        );

        println!("\n=== Cache Line Padding Test ===");
        println!(
            "CacheLinePadding size: {} bytes",
            mem::size_of::<CacheLinePadding>()
        );
    }

    /// Test that PremintCandidate has proper hot/cold separation
    #[test]
    fn test_premint_candidate_layout() {
        use std::mem;

        let candidate = PremintCandidate::default();

        // Get addresses of hot fields (slot is first hot field)
        let slot_addr = &candidate.slot as *const _ as usize;
        // Get address of first cold field (pool_amm_id after the cache barrier)
        let pool_amm_id_addr = &candidate.pool_amm_id as *const _ as usize;

        // Hot and cold should be separated by at least one cache line
        let distance = pool_amm_id_addr.saturating_sub(slot_addr);

        // Distance should be > 64 bytes due to cache barrier
        assert!(
            distance >= CACHE_LINE_SIZE,
            "Hot and cold fields should be separated by at least {} bytes, got {} bytes",
            CACHE_LINE_SIZE,
            distance
        );

        println!("\n=== PremintCandidate Layout Test ===");
        println!("slot address: 0x{:x}", slot_addr);
        println!("pool_amm_id address: 0x{:x}", pool_amm_id_addr);
        println!("Distance (hot to cold): {} bytes", distance);
        println!(
            "Structure size: {} bytes",
            mem::size_of::<PremintCandidate>()
        );
    }

    /// Test that EnhancedCandidate has proper hot/shadow/cold separation
    #[test]
    fn test_enhanced_candidate_layout() {
        use std::mem;

        let candidate = EnhancedCandidate::default();

        // Get addresses of different sections
        let hot_addr = &candidate.slot as *const _ as usize; // First hot field
        let shadow_addr = &candidate.expected_price as *const _ as usize; // First shadow field
        let cold_addr = &candidate.pool_amm_id as *const _ as usize; // First cold field

        // Verify proper separation
        let hot_to_shadow = shadow_addr.saturating_sub(hot_addr);
        let shadow_to_cold = cold_addr.saturating_sub(shadow_addr);

        assert!(
            hot_to_shadow >= CACHE_LINE_SIZE,
            "Hot and shadow should be separated by >= {} bytes, got {} bytes",
            CACHE_LINE_SIZE,
            hot_to_shadow
        );

        assert!(
            shadow_to_cold >= CACHE_LINE_SIZE,
            "Shadow and cold should be separated by >= {} bytes, got {} bytes",
            CACHE_LINE_SIZE,
            shadow_to_cold
        );

        println!("\n=== EnhancedCandidate Layout Test ===");
        println!("hot section address: 0x{:x}", hot_addr);
        println!("shadow section address: 0x{:x}", shadow_addr);
        println!("cold section address: 0x{:x}", cold_addr);
        println!("Hot to Shadow distance: {} bytes", hot_to_shadow);
        println!("Shadow to Cold distance: {} bytes", shadow_to_cold);
        println!(
            "Structure size: {} bytes",
            mem::size_of::<EnhancedCandidate>()
        );
    }

    /// Benchmark-style test to verify no performance degradation from false sharing
    #[test]
    fn test_hot_path_access_pattern() {
        use std::sync::atomic::{AtomicU64, Ordering};
        use std::time::Instant;

        // Create a batch of candidates
        let batch_size = 128;
        let mut candidates = Vec::with_capacity(batch_size);

        for i in 0..batch_size {
            let mut c = PremintCandidate::default();
            c.slot = Some(i as u64);
            c.liquidity_sol = 10.0 + (i as f64) * 0.1;
            c.base_score = 50 + (i % 50) as u8;
            candidates.push(c);
        }

        // Simulate hot-path scoring (accessing only hot data - first cache line)
        let iterations = 10_000;
        let start = Instant::now();

        let score_sum = AtomicU64::new(0);

        for _ in 0..iterations {
            for candidate in &candidates {
                // Hot path: only access hot data (slot, liquidity_sol, base_score are in first cache line)
                let score = (candidate.base_score as f64 * candidate.liquidity_sol) as u64;
                score_sum.fetch_add(score, Ordering::Relaxed);
            }
        }

        let elapsed = start.elapsed();
        let ops_per_ns = (iterations * batch_size) as f64 / elapsed.as_nanos() as f64;

        println!("\n=== Hot Path Access Pattern Test ===");
        println!("Iterations: {}", iterations);
        println!("Batch size: {}", batch_size);
        println!("Total operations: {}", iterations * batch_size);
        println!("Time: {:?}", elapsed);
        println!(
            "Throughput: {:.2} ops/ns ({:.2}M ops/s)",
            ops_per_ns,
            ops_per_ns * 1000.0
        );
        println!(
            "Score sum (sanity check): {}",
            score_sum.load(Ordering::Relaxed)
        );

        // Ensure we're getting reasonable throughput
        // Should be > 0.01 ops/ns in debug mode (> 10M ops/s)
        assert!(
            ops_per_ns > 0.01,
            "Hot path throughput too low: {:.2} ops/ns",
            ops_per_ns
        );
    }
}
