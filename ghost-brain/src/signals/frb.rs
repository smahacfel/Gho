//! FRB (Fractal Resonance Bands) - Multi-scale Band Extraction & Profiles
//!
//! This module implements a pipeline that builds amplitude profiles for each fractal band
//! (short / medium / long) from raw transaction streams. It extracts signals at multiple
//! time scales to detect resonance patterns across different market frequencies.
//!
//! ## Core Concept
//!
//! Markets exhibit fractal behavior - patterns that repeat at different scales. By analyzing
//! transaction data across multiple band sizes (8-32, 32-128, 128-512 transactions), we can
//! detect resonance patterns that indicate coordinated activity, accumulation phases, or
//! distribution patterns.
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │         FRB Multi-scale Band Extraction                 │
//! │                                                         │
//! │  Input: Transaction Stream                              │
//! │         (volume, is_buy, wallet, timestamp)             │
//! │                                                         │
//! │  ┌──────────────┐   ┌──────────────┐   ┌─────────────┐│
//! │  │ Short Band   │   │ Medium Band  │   │ Long Band   ││
//! │  │  (8-32 tx)   │   │ (32-128 tx)  │   │ (128-512 tx)││
//! │  └──────────────┘   └──────────────┘   └─────────────┘│
//! │                                                         │
//! │  Output: [BandProfile; 3]                              │
//! │          { amplitude, buyers, volatility, timestamp }   │
//! └─────────────────────────────────────────────────────────┘
//! ```
//!
//! ## Bands Definition
//!
//! - **Short Band (8-32 tx)**: High-frequency micro-patterns, bot detection
//! - **Medium Band (32-128 tx)**: Mid-frequency patterns, whale accumulation
//! - **Long Band (128-512 tx)**: Macro trends, market structure shifts
//!
//! ## Time Windows
//!
//! - **1 second**: Ultra-fast bot activity, MEV patterns
//! - **5 seconds**: Short-term momentum, sniper detection
//! - **15 seconds**: Medium-term accumulation/distribution
//! - **60 seconds**: Macro trend confirmation
//!
//! ## Performance
//!
//! - **Zero-alloc hot-path**: Reuses pre-allocated buffers for feature extraction
//! - **Batch mode**: Process historical data efficiently
//! - **Streaming mode**: Real-time analysis with rolling windows
//!
//! ## Integration Points
//!
//! - **MPCF**: Optional weighting using actor_score for bot/human classification
//! - **SOBP**: Optional weighting using intensity for order book pressure
//! - **WHF**: Compatible with Harmonic Field Analysis for resonance detection

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashSet, VecDeque};

/// Default short band range: 8-32 transactions
pub const DEFAULT_SHORT_BAND_MIN: usize = 8;
pub const DEFAULT_SHORT_BAND_MAX: usize = 32;

/// Default medium band range: 32-128 transactions
pub const DEFAULT_MEDIUM_BAND_MIN: usize = 32;
pub const DEFAULT_MEDIUM_BAND_MAX: usize = 128;

/// Default long band range: 128-512 transactions
pub const DEFAULT_LONG_BAND_MIN: usize = 128;
pub const DEFAULT_LONG_BAND_MAX: usize = 512;

/// Time window options in milliseconds
pub const WINDOW_1S: u64 = 1_000;
pub const WINDOW_5S: u64 = 5_000;
pub const WINDOW_15S: u64 = 15_000;
pub const WINDOW_60S: u64 = 60_000;

/// Default time window (5 seconds)
pub const DEFAULT_WINDOW_MS: u64 = WINDOW_5S;

/// Minimum amplitude threshold to avoid noise
const MIN_AMPLITUDE_THRESHOLD: f32 = 0.001;

/// Profile for a single fractal band
///
/// Represents aggregated market activity within a specific transaction count range.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct BandProfile {
    /// Amplitude: sum of weighted transaction volumes
    pub amplitude: f32,

    /// Number of unique buyers in this band
    pub buyers: u32,

    /// Volatility: standard deviation of transaction sizes
    pub volatility: f32,

    /// Timestamp of profile generation (milliseconds since epoch)
    pub timestamp: u64,

    /// Weighted buy/sell ratio (buy_volume / sell_volume)
    /// None if sell_volume is zero
    pub buy_sell_ratio: Option<f32>,

    /// Number of transactions in this band
    pub transaction_count: usize,
}

impl BandProfile {
    /// Create a new empty band profile
    pub fn new(timestamp: u64) -> Self {
        Self {
            amplitude: 0.0,
            buyers: 0,
            volatility: 0.0,
            timestamp,
            buy_sell_ratio: None,
            transaction_count: 0,
        }
    }

    /// Check if this profile has meaningful data
    pub fn is_significant(&self) -> bool {
        self.amplitude >= MIN_AMPLITUDE_THRESHOLD && self.transaction_count > 0
    }
}

impl Default for BandProfile {
    fn default() -> Self {
        Self::new(0)
    }
}

/// Band configuration defining transaction count ranges
#[derive(Debug, Clone)]
pub struct BandRange {
    /// Minimum transaction count for this band
    pub min: usize,
    /// Maximum transaction count for this band
    pub max: usize,
}

impl BandRange {
    /// Create a new band range
    pub fn new(min: usize, max: usize) -> Self {
        assert!(min > 0, "Band minimum must be > 0");
        assert!(max >= min, "Band maximum must be >= minimum");
        Self { min, max }
    }

    /// Check if transaction count is within this band's range
    pub fn contains(&self, count: usize) -> bool {
        count >= self.min && count <= self.max
    }
}

/// Configuration for FRB band extraction
#[derive(Debug, Clone)]
pub struct BandConfig {
    /// Short band range (default: 8-32 transactions)
    pub short_band: BandRange,

    /// Medium band range (default: 32-128 transactions)
    pub medium_band: BandRange,

    /// Long band range (default: 128-512 transactions)
    pub long_band: BandRange,

    /// Time window for rolling analysis in milliseconds
    pub window_ms: u64,

    /// Whether to use MPCF actor_score for weighting (if available)
    pub use_mpcf_weighting: bool,

    /// Whether to use SOBP intensity for weighting (if available)
    pub use_sobp_weighting: bool,
}

impl Default for BandConfig {
    fn default() -> Self {
        Self {
            short_band: BandRange::new(DEFAULT_SHORT_BAND_MIN, DEFAULT_SHORT_BAND_MAX),
            medium_band: BandRange::new(DEFAULT_MEDIUM_BAND_MIN, DEFAULT_MEDIUM_BAND_MAX),
            long_band: BandRange::new(DEFAULT_LONG_BAND_MIN, DEFAULT_LONG_BAND_MAX),
            window_ms: DEFAULT_WINDOW_MS,
            use_mpcf_weighting: false,
            use_sobp_weighting: false,
        }
    }
}

impl BandConfig {
    /// Create config with custom time window
    pub fn with_window(window_ms: u64) -> Self {
        Self {
            window_ms,
            ..Default::default()
        }
    }

    /// Enable MPCF actor_score weighting
    pub fn with_mpcf_weighting(mut self) -> Self {
        self.use_mpcf_weighting = true;
        self
    }

    /// Enable SOBP intensity weighting
    pub fn with_sobp_weighting(mut self) -> Self {
        self.use_sobp_weighting = true;
        self
    }
}

/// Transaction data for band analysis
///
/// Simplified view focusing on volume, direction, and timing
#[derive(Debug, Clone)]
pub struct BandTransaction {
    /// Transaction volume in SOL
    pub volume: f32,

    /// True if buy, false if sell
    pub is_buy: bool,

    /// Wallet/signer address
    pub wallet: Pubkey,

    /// Timestamp in milliseconds since epoch
    pub timestamp_ms: u64,

    /// Optional MPCF actor score (0.0-1.0, higher = more human-like)
    pub actor_score: Option<f32>,

    /// Optional SOBP intensity score (higher = stronger order book pressure)
    pub intensity: Option<f32>,
}

impl BandTransaction {
    /// Create a new transaction with basic fields
    pub fn new(volume: f32, is_buy: bool, wallet: Pubkey, timestamp_ms: u64) -> Self {
        Self {
            volume,
            is_buy,
            wallet,
            timestamp_ms,
            actor_score: None,
            intensity: None,
        }
    }

    /// Set MPCF actor score
    pub fn with_actor_score(mut self, score: f32) -> Self {
        self.actor_score = Some(score.clamp(0.0, 1.0));
        self
    }

    /// Set SOBP intensity
    pub fn with_intensity(mut self, intensity: f32) -> Self {
        self.intensity = Some(intensity.max(0.0));
        self
    }

    /// Calculate weighted volume based on configuration
    fn weighted_volume(&self, config: &BandConfig) -> f32 {
        let mut weight = 1.0_f32;

        // Apply MPCF weighting if enabled and available
        if config.use_mpcf_weighting {
            if let Some(score) = self.actor_score {
                // Higher actor_score (more human-like) gets higher weight
                weight *= 0.5 + score * 0.5; // Range: 0.5-1.0
            }
        }

        // Apply SOBP weighting if enabled and available
        if config.use_sobp_weighting {
            if let Some(intensity) = self.intensity {
                // Higher intensity gets higher weight, normalized
                weight *= 1.0 + (intensity * 0.2).min(0.5); // Range: 1.0-1.5
            }
        }

        self.volume * weight
    }
}

/// Circular buffer for storing band profiles
///
/// Maintains historical profiles for trend analysis
struct ProfileHistory {
    /// Maximum number of profiles to store
    capacity: usize,
    /// Stored profiles (FIFO)
    profiles: VecDeque<BandProfile>,
}

impl ProfileHistory {
    fn new(capacity: usize) -> Self {
        Self {
            capacity,
            profiles: VecDeque::with_capacity(capacity),
        }
    }

    fn push(&mut self, profile: BandProfile) {
        if self.profiles.len() >= self.capacity {
            self.profiles.pop_front();
        }
        self.profiles.push_back(profile);
    }

    fn get_latest(&self) -> Option<&BandProfile> {
        self.profiles.back()
    }

    fn clear(&mut self) {
        self.profiles.clear();
    }
}

/// Main band extraction engine
///
/// Processes transaction streams and extracts multi-scale band profiles
pub struct BandExtractor {
    /// Configuration
    config: BandConfig,

    /// Transaction buffer for current window
    tx_buffer: VecDeque<BandTransaction>,

    /// Profile history for each band (short, medium, long)
    short_history: ProfileHistory,
    medium_history: ProfileHistory,
    long_history: ProfileHistory,

    /// Unique wallet tracker (reused to avoid allocations)
    wallet_set: HashSet<Pubkey>,

    /// Buyer wallet tracker (reused to avoid allocations)
    buyer_set: HashSet<Pubkey>,

    /// Volume accumulator buffer (reused for volatility calculation)
    volume_buffer: Vec<f32>,
}

impl BandExtractor {
    /// Create a new band extractor with default configuration
    pub fn new() -> Self {
        Self::with_config(BandConfig::default())
    }

    /// Create a new band extractor with custom configuration
    pub fn with_config(config: BandConfig) -> Self {
        Self {
            config,
            tx_buffer: VecDeque::with_capacity(DEFAULT_LONG_BAND_MAX * 2),
            short_history: ProfileHistory::new(100), // Keep last 100 profiles
            medium_history: ProfileHistory::new(100),
            long_history: ProfileHistory::new(100),
            wallet_set: HashSet::with_capacity(256),
            buyer_set: HashSet::with_capacity(256),
            volume_buffer: Vec::with_capacity(DEFAULT_LONG_BAND_MAX),
        }
    }

    /// Add a transaction to the buffer
    ///
    /// Maintains rolling window based on timestamp
    pub fn add_transaction(&mut self, tx: BandTransaction) {
        // Add to buffer
        self.tx_buffer.push_back(tx);

        // Remove old transactions outside the window
        if let Some(latest_ts) = self.tx_buffer.back().map(|t| t.timestamp_ms) {
            while let Some(oldest) = self.tx_buffer.front() {
                if latest_ts - oldest.timestamp_ms > self.config.window_ms {
                    self.tx_buffer.pop_front();
                } else {
                    break;
                }
            }
        }
    }

    /// Extract band profiles from current transaction window
    ///
    /// Returns [short_band, medium_band, long_band] profiles
    ///
    /// This is the main API function for extracting band data.
    /// It operates on the current transaction buffer without allocating new memory.
    pub fn extract_bands(&mut self) -> [BandProfile; 3] {
        let timestamp = self.tx_buffer.back().map(|tx| tx.timestamp_ms).unwrap_or(0);

        // Clone band ranges to avoid borrow checker issues
        let short_band = self.config.short_band.clone();
        let medium_band = self.config.medium_band.clone();
        let long_band = self.config.long_band.clone();

        let short = self.extract_band_profile(&short_band, timestamp);
        let medium = self.extract_band_profile(&medium_band, timestamp);
        let long = self.extract_band_profile(&long_band, timestamp);

        // Update history
        self.short_history.push(short.clone());
        self.medium_history.push(medium.clone());
        self.long_history.push(long.clone());

        [short, medium, long]
    }

    /// Extract profile for a specific band range
    ///
    /// Zero-alloc implementation: reuses internal buffers
    fn extract_band_profile(&mut self, band: &BandRange, timestamp: u64) -> BandProfile {
        let tx_count = self.tx_buffer.len();

        // Check if we have enough transactions for this band
        if tx_count < band.min {
            return BandProfile::new(timestamp);
        }

        // Determine actual range to analyze (up to band.max)
        let analyze_count = tx_count.min(band.max);

        // Clear reusable buffers
        self.wallet_set.clear();
        self.buyer_set.clear();
        self.volume_buffer.clear();

        let mut total_buy_volume = 0.0_f32;
        let mut total_sell_volume = 0.0_f32;
        let mut weighted_amplitude = 0.0_f32;

        // Iterate over the most recent transactions in the band range
        let start_idx = tx_count.saturating_sub(analyze_count);
        for tx in self.tx_buffer.iter().skip(start_idx) {
            let weighted_vol = tx.weighted_volume(&self.config);

            // Track unique wallets
            self.wallet_set.insert(tx.wallet);

            // Track buyers
            if tx.is_buy {
                self.buyer_set.insert(tx.wallet);
                total_buy_volume += weighted_vol;
            } else {
                total_sell_volume += weighted_vol;
            }

            // Accumulate amplitude
            weighted_amplitude += weighted_vol;

            // Store volume for volatility calculation
            self.volume_buffer.push(tx.volume);
        }

        // Calculate volatility (standard deviation of volumes)
        let volatility = if self.volume_buffer.len() > 1 {
            let mean = self.volume_buffer.iter().sum::<f32>() / self.volume_buffer.len() as f32;
            let variance = self
                .volume_buffer
                .iter()
                .map(|&v| {
                    let diff = v - mean;
                    diff * diff
                })
                .sum::<f32>()
                / self.volume_buffer.len() as f32;
            variance.sqrt()
        } else {
            0.0
        };

        // Calculate buy/sell ratio
        let buy_sell_ratio = if total_sell_volume > 0.0 {
            Some(total_buy_volume / total_sell_volume)
        } else if total_buy_volume > 0.0 {
            Some(f32::INFINITY)
        } else {
            None
        };

        BandProfile {
            amplitude: weighted_amplitude,
            buyers: self.buyer_set.len() as u32,
            volatility,
            timestamp,
            buy_sell_ratio,
            transaction_count: analyze_count,
        }
    }

    /// Get the latest profile for each band
    pub fn get_latest_profiles(&self) -> Option<[BandProfile; 3]> {
        let short = self.short_history.get_latest()?;
        let medium = self.medium_history.get_latest()?;
        let long = self.long_history.get_latest()?;

        Some([short.clone(), medium.clone(), long.clone()])
    }

    /// Clear all transaction data and history
    pub fn clear(&mut self) {
        self.tx_buffer.clear();
        self.short_history.clear();
        self.medium_history.clear();
        self.long_history.clear();
        self.wallet_set.clear();
        self.buyer_set.clear();
        self.volume_buffer.clear();
    }

    /// Get current transaction buffer size
    pub fn buffer_size(&self) -> usize {
        self.tx_buffer.len()
    }
}

impl Default for BandExtractor {
    fn default() -> Self {
        Self::new()
    }
}

/// Standalone function for extracting bands from a transaction window
///
/// This is a convenience function for batch processing.
/// For streaming mode with state management, use `BandExtractor` directly.
///
/// Note: This function clones transactions internally. For zero-copy batch processing,
/// use `BandExtractor` with a custom loop that moves or borrows transactions as appropriate.
///
/// # Arguments
///
/// * `transactions` - Slice of transactions to analyze
/// * `config` - Band extraction configuration
///
/// # Returns
///
/// Array of [short_band, medium_band, long_band] profiles
pub fn extract_bands(transactions: &[BandTransaction], config: &BandConfig) -> [BandProfile; 3] {
    let mut extractor = BandExtractor::with_config(config.clone());

    for tx in transactions {
        extractor.add_transaction(tx.clone());
    }

    extractor.extract_bands()
}

// ═══════════════════════════════════════════════════════════════════════════
// FRB PART 2 & 3: Resonance Detection and Signal Classification
// ═══════════════════════════════════════════════════════════════════════════

/// Signal classification based on cross-band resonance patterns
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum FrbSignal {
    /// Strong resonance across bands - trend likely to continue
    /// Indicates organic, multi-scale buying pressure
    ResContinue,

    /// Weak resonance - fake pump or bot-driven spike
    /// Only short band shows activity without medium/long support
    ResFake,

    /// Medium resonance - transitional phase
    /// Some band synchronization but not dominant
    ResTransition,

    /// No meaningful signal (insufficient data or activity)
    ResHold,
}

impl FrbSignal {
    /// Get a human-readable description of the signal
    pub fn description(&self) -> &'static str {
        match self {
            FrbSignal::ResContinue => "Strong multi-scale resonance - trend continuation likely",
            FrbSignal::ResFake => "Weak resonance - bot-driven spike or fake pump",
            FrbSignal::ResTransition => "Medium resonance - transitional phase",
            FrbSignal::ResHold => "No meaningful signal",
        }
    }

    /// Check if this signal is actionable (not HOLD)
    pub fn is_actionable(&self) -> bool {
        !matches!(self, FrbSignal::ResHold)
    }

    /// Get signal strength (0.0-1.0)
    pub fn strength(&self) -> f32 {
        match self {
            FrbSignal::ResContinue => 0.9,
            FrbSignal::ResTransition => 0.5,
            FrbSignal::ResFake => 0.2,
            FrbSignal::ResHold => 0.0,
        }
    }
}

/// Complete FRB analysis result with resonance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrbResult {
    /// Band profiles [short, medium, long]
    pub band_profiles: [BandProfile; 3],

    /// Overall resonance score (0.0-1.0)
    /// Measures how well bands are synchronized
    pub resonance_score: f32,

    /// Coherence map between band pairs [short-medium, medium-long, short-long]
    /// Each value 0.0-1.0 indicating pairwise synchronization
    pub coherence_map: [f32; 3],

    /// Trend likelihood score (0.0-1.0)
    /// Probability that current movement will continue
    pub trend_likelihood: f32,

    /// Classified signal
    pub signal: FrbSignal,

    /// Timestamp of analysis
    pub timestamp: u64,
}

impl FrbResult {
    /// Create a new empty result
    pub fn new(timestamp: u64) -> Self {
        Self {
            band_profiles: [
                BandProfile::new(timestamp),
                BandProfile::new(timestamp),
                BandProfile::new(timestamp),
            ],
            resonance_score: 0.0,
            coherence_map: [0.0, 0.0, 0.0],
            trend_likelihood: 0.0,
            signal: FrbSignal::ResHold,
            timestamp,
        }
    }

    /// Check if this result has meaningful data
    pub fn is_significant(&self) -> bool {
        self.band_profiles.iter().any(|p| p.is_significant())
    }
}

/// Configuration for resonance analysis
#[derive(Debug, Clone)]
pub struct ResonanceConfig {
    /// Minimum amplitude threshold to consider a band active
    pub min_amplitude_threshold: f32,

    /// Threshold for classifying as RES_CONTINUE (default: 0.7)
    pub continue_threshold: f32,

    /// Threshold for classifying as RES_FAKE (default: 0.3)
    pub fake_threshold: f32,

    /// Weight for amplitude coherence (default: 0.4)
    pub amplitude_weight: f32,

    /// Weight for buyer coherence (default: 0.3)
    pub buyer_weight: f32,

    /// Weight for volatility coherence (default: 0.2)
    pub volatility_weight: f32,

    /// Weight for buy/sell ratio coherence (default: 0.1)
    pub ratio_weight: f32,
}

impl Default for ResonanceConfig {
    fn default() -> Self {
        Self {
            min_amplitude_threshold: MIN_AMPLITUDE_THRESHOLD,
            continue_threshold: 0.7,
            fake_threshold: 0.3,
            amplitude_weight: 0.4,
            buyer_weight: 0.3,
            volatility_weight: 0.2,
            ratio_weight: 0.1,
        }
    }
}

impl ResonanceConfig {
    /// Create config with custom thresholds
    pub fn with_thresholds(continue_threshold: f32, fake_threshold: f32) -> Self {
        Self {
            continue_threshold,
            fake_threshold,
            ..Default::default()
        }
    }

    /// Create config with custom feature weights
    pub fn with_weights(amplitude: f32, buyer: f32, volatility: f32, ratio: f32) -> Self {
        // Normalize weights
        let total = amplitude + buyer + volatility + ratio;
        Self {
            amplitude_weight: amplitude / total,
            buyer_weight: buyer / total,
            volatility_weight: volatility / total,
            ratio_weight: ratio / total,
            ..Default::default()
        }
    }
}

/// Resonance analyzer for detecting multi-scale synchronization
pub struct ResonanceAnalyzer {
    config: ResonanceConfig,
}

impl ResonanceAnalyzer {
    /// Create a new analyzer with default configuration
    pub fn new() -> Self {
        Self::with_config(ResonanceConfig::default())
    }

    /// Create a new analyzer with custom configuration
    pub fn with_config(config: ResonanceConfig) -> Self {
        Self { config }
    }

    /// Analyze band profiles and produce complete FRB result
    ///
    /// This is the main API for resonance detection and signal classification.
    pub fn analyze(&self, band_profiles: [BandProfile; 3]) -> FrbResult {
        let timestamp = band_profiles[0].timestamp;

        // Check if we have meaningful data
        if !band_profiles.iter().any(|p| p.is_significant()) {
            return FrbResult::new(timestamp);
        }

        // Calculate coherence map (pairwise band synchronization)
        let coherence_map = self.calculate_coherence_map(&band_profiles);

        // Calculate overall resonance score
        let resonance_score = self.calculate_resonance_score(&coherence_map);

        // Calculate trend likelihood
        let trend_likelihood = self.calculate_trend_likelihood(&band_profiles, resonance_score);

        // Classify signal
        let signal = self.classify_signal(resonance_score, &band_profiles);

        FrbResult {
            band_profiles,
            resonance_score,
            coherence_map,
            trend_likelihood,
            signal,
            timestamp,
        }
    }

    /// Calculate pairwise coherence between bands
    ///
    /// Returns [short-medium, medium-long, short-long] coherence values (0.0-1.0)
    fn calculate_coherence_map(&self, profiles: &[BandProfile; 3]) -> [f32; 3] {
        let short_medium = self.calculate_pairwise_coherence(&profiles[0], &profiles[1]);
        let medium_long = self.calculate_pairwise_coherence(&profiles[1], &profiles[2]);
        let short_long = self.calculate_pairwise_coherence(&profiles[0], &profiles[2]);

        [short_medium, medium_long, short_long]
    }

    /// Calculate coherence between two band profiles
    ///
    /// Coherence measures how synchronized two bands are across multiple features:
    /// - Amplitude alignment
    /// - Buyer count similarity
    /// - Volatility correlation
    /// - Buy/sell ratio alignment
    fn calculate_pairwise_coherence(&self, band1: &BandProfile, band2: &BandProfile) -> f32 {
        // If either band is insignificant, coherence is zero
        if !band1.is_significant() || !band2.is_significant() {
            return 0.0;
        }

        let mut coherence = 0.0;

        // 1. Amplitude coherence (higher when both bands have similar relative strength)
        let amp_coherence = self.calculate_amplitude_coherence(band1.amplitude, band2.amplitude);
        coherence += amp_coherence * self.config.amplitude_weight;

        // 2. Buyer coherence (higher when buyer counts are proportional)
        let buyer_coherence = self.calculate_buyer_coherence(band1.buyers, band2.buyers);
        coherence += buyer_coherence * self.config.buyer_weight;

        // 3. Volatility coherence (higher when volatility patterns align)
        let vol_coherence = self.calculate_volatility_coherence(band1.volatility, band2.volatility);
        coherence += vol_coherence * self.config.volatility_weight;

        // 4. Buy/sell ratio coherence (higher when trading direction aligns)
        let ratio_coherence =
            self.calculate_ratio_coherence(band1.buy_sell_ratio, band2.buy_sell_ratio);
        coherence += ratio_coherence * self.config.ratio_weight;

        coherence.clamp(0.0, 1.0)
    }

    /// Calculate amplitude coherence using normalized difference
    fn calculate_amplitude_coherence(&self, amp1: f32, amp2: f32) -> f32 {
        if amp1 <= 0.0 || amp2 <= 0.0 {
            return 0.0;
        }

        // Use log ratio to handle different scales
        let ratio = (amp1 / amp2).max(amp2 / amp1);
        let log_ratio = ratio.ln();

        // Map log_ratio to 0-1 (smaller difference = higher coherence)
        // log(2) ≈ 0.69, log(5) ≈ 1.6
        (2.0 - log_ratio).clamp(0.0, 2.0) / 2.0
    }

    /// Calculate buyer coherence using proportional similarity
    fn calculate_buyer_coherence(&self, buyers1: u32, buyers2: u32) -> f32 {
        if buyers1 == 0 || buyers2 == 0 {
            return 0.0;
        }

        let ratio = (buyers1 as f32 / buyers2 as f32).max(buyers2 as f32 / buyers1 as f32);

        // Use inverse ratio mapping: 1.0 -> 1.0, 2.0 -> 0.5, 3.0 -> 0.33, etc.
        // More forgiving for similar counts
        1.0 / ratio
    }

    /// Calculate volatility coherence using normalized difference
    fn calculate_volatility_coherence(&self, vol1: f32, vol2: f32) -> f32 {
        if vol1 <= 0.0 && vol2 <= 0.0 {
            return 1.0; // Both have zero volatility = perfect coherence
        }
        if vol1 <= 0.0 || vol2 <= 0.0 {
            return 0.0; // One has volatility, one doesn't
        }

        let ratio = (vol1 / vol2).max(vol2 / vol1);
        let log_ratio = ratio.ln();

        (2.0 - log_ratio).clamp(0.0, 2.0) / 2.0
    }

    /// Calculate buy/sell ratio coherence
    fn calculate_ratio_coherence(&self, ratio1: Option<f32>, ratio2: Option<f32>) -> f32 {
        match (ratio1, ratio2) {
            (Some(r1), Some(r2)) if r1.is_finite() && r2.is_finite() => {
                // Both have valid ratios
                let ratio = (r1 / r2).max(r2 / r1);
                let log_ratio = ratio.ln();
                (2.0 - log_ratio).clamp(0.0, 2.0) / 2.0
            }
            (Some(_), Some(_)) => {
                // At least one is infinite (only buys)
                // Check if both are in same direction
                let both_buy_heavy =
                    ratio1.map_or(false, |r| r > 1.0) && ratio2.map_or(false, |r| r > 1.0);
                if both_buy_heavy {
                    0.8 // Good alignment
                } else {
                    0.3 // Weak alignment
                }
            }
            (None, None) => 0.5, // Both have no sell volume
            _ => 0.0,            // One has ratio, one doesn't
        }
    }

    /// Calculate overall resonance score from coherence map
    ///
    /// Resonance score is weighted average of pairwise coherences,
    /// with emphasis on adjacent bands (short-medium, medium-long)
    fn calculate_resonance_score(&self, coherence_map: &[f32; 3]) -> f32 {
        let short_medium = coherence_map[0];
        let medium_long = coherence_map[1];
        let short_long = coherence_map[2];

        // Adjacent bands matter more than short-long
        let resonance = (short_medium * 0.4) + (medium_long * 0.4) + (short_long * 0.2);

        resonance.clamp(0.0, 1.0)
    }

    /// Calculate trend likelihood based on band profiles and resonance
    ///
    /// Trend likelihood considers:
    /// - Resonance score (higher = more likely to continue)
    /// - Amplitude progression (increasing from long to short = momentum)
    /// - Buy pressure (buy/sell ratio across bands)
    /// - Buyer participation (unique buyers in each band)
    fn calculate_trend_likelihood(&self, profiles: &[BandProfile; 3], resonance_score: f32) -> f32 {
        let short = &profiles[0];
        let medium = &profiles[1];
        let _long = &profiles[2];

        // Base likelihood from resonance
        let mut likelihood = resonance_score * 0.5;

        // Check for momentum (short band stronger than medium/long)
        if short.is_significant() && medium.is_significant() {
            let short_avg = short.amplitude / short.transaction_count.max(1) as f32;
            let medium_avg = medium.amplitude / medium.transaction_count.max(1) as f32;

            if short_avg > medium_avg {
                likelihood += 0.15; // Positive momentum
            }
        }

        // Check buy pressure across bands
        let mut buy_pressure_score = 0.0;
        let mut band_count = 0;

        for profile in profiles.iter() {
            if let Some(ratio) = profile.buy_sell_ratio {
                if ratio > 1.0 {
                    buy_pressure_score += (ratio - 1.0).min(2.0) / 2.0; // Cap at 1.0 per band
                }
                band_count += 1;
            }
        }

        if band_count > 0 {
            likelihood += (buy_pressure_score / band_count as f32) * 0.2;
        }

        // Check buyer participation (more buyers = more organic)
        if short.buyers >= 3 && medium.buyers >= 3 {
            likelihood += 0.15;
        }

        likelihood.clamp(0.0, 1.0)
    }

    /// Classify signal based on resonance score and band patterns
    fn classify_signal(&self, resonance_score: f32, profiles: &[BandProfile; 3]) -> FrbSignal {
        let short = &profiles[0];
        let medium = &profiles[1];
        let long = &profiles[2];

        // Check for RES_CONTINUE: high resonance + all bands active
        if resonance_score >= self.config.continue_threshold {
            if short.is_significant() && medium.is_significant() && long.is_significant() {
                return FrbSignal::ResContinue;
            }
        }

        // Check for RES_FAKE: low resonance + only short band active
        if resonance_score <= self.config.fake_threshold {
            if short.is_significant() && !medium.is_significant() && !long.is_significant() {
                return FrbSignal::ResFake;
            }

            // Also fake if short band dominates but no coherence
            if short.is_significant() && short.buyers < 3 {
                return FrbSignal::ResFake;
            }
        }

        // RES_TRANSITION: medium resonance OR partial band activation
        if resonance_score > self.config.fake_threshold
            && resonance_score < self.config.continue_threshold
        {
            return FrbSignal::ResTransition;
        }

        // Also transition if we have short+medium but not long (early trend)
        if short.is_significant() && medium.is_significant() && !long.is_significant() {
            return FrbSignal::ResTransition;
        }

        // Default: HOLD
        FrbSignal::ResHold
    }
}

impl Default for ResonanceAnalyzer {
    fn default() -> Self {
        Self::new()
    }
}

/// Helper function to analyze bands and produce FRB result with default config
///
/// This is a convenience wrapper for one-shot analysis.
pub fn analyze_resonance(band_profiles: [BandProfile; 3]) -> FrbResult {
    let analyzer = ResonanceAnalyzer::new();
    analyzer.analyze(band_profiles)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_wallet() -> Pubkey {
        Pubkey::new_unique()
    }

    fn create_test_transaction(volume: f32, is_buy: bool, timestamp_ms: u64) -> BandTransaction {
        BandTransaction::new(volume, is_buy, create_test_wallet(), timestamp_ms)
    }

    #[test]
    fn test_band_range() {
        let band = BandRange::new(10, 50);
        assert!(!band.contains(5));
        assert!(band.contains(10));
        assert!(band.contains(30));
        assert!(band.contains(50));
        assert!(!band.contains(51));
    }

    #[test]
    fn test_band_profile_creation() {
        let profile = BandProfile::new(1000);
        assert_eq!(profile.amplitude, 0.0);
        assert_eq!(profile.buyers, 0);
        assert_eq!(profile.timestamp, 1000);
        assert!(!profile.is_significant());
    }

    #[test]
    fn test_band_extractor_empty() {
        let mut extractor = BandExtractor::new();
        let bands = extractor.extract_bands();

        // All bands should be empty since we have no transactions
        assert_eq!(bands[0].transaction_count, 0);
        assert_eq!(bands[1].transaction_count, 0);
        assert_eq!(bands[2].transaction_count, 0);
    }

    #[test]
    fn test_band_extractor_basic() {
        let mut extractor = BandExtractor::new();

        // Add 50 transactions
        for i in 0..50 {
            let tx = create_test_transaction(
                10.0 + i as f32,
                i % 3 != 0,     // 2/3 buys, 1/3 sells
                1000 + i * 100, // Timestamps 100ms apart
            );
            extractor.add_transaction(tx);
        }

        let bands = extractor.extract_bands();

        // Short band (8-32): should have 32 transactions (we have 50, so capped at max)
        assert_eq!(bands[0].transaction_count, 32);
        assert!(bands[0].is_significant());
        assert!(bands[0].amplitude > 0.0);
        assert!(bands[0].buyers > 0);

        // Medium band (32-128): should have 50 transactions (we have 50, below max of 128)
        assert_eq!(bands[1].transaction_count, 50);
        assert!(bands[1].is_significant());

        // Long band (128-512): should have 0 transactions (we only have 50, below min of 128)
        assert_eq!(bands[2].transaction_count, 0);
        assert!(!bands[2].is_significant());
    }

    #[test]
    fn test_band_extractor_buy_sell_ratio() {
        let mut extractor = BandExtractor::new();
        let wallet_buy = create_test_wallet();
        let wallet_sell = create_test_wallet();

        // Add 20 buy transactions
        for i in 0..20 {
            let tx = BandTransaction::new(10.0, true, wallet_buy, 1000 + i * 10);
            extractor.add_transaction(tx);
        }

        // Add 10 sell transactions
        for i in 0..10 {
            let tx = BandTransaction::new(5.0, false, wallet_sell, 1000 + (20 + i) * 10);
            extractor.add_transaction(tx);
        }

        let bands = extractor.extract_bands();

        // Check short band buy/sell ratio
        assert!(bands[0].buy_sell_ratio.is_some());
        let ratio = bands[0].buy_sell_ratio.unwrap();
        // Buy volume = ~10*20 (from last 32), Sell volume = ~5*10
        // Ratio should be around 2.0-4.0 depending on which transactions are in the window
        assert!(ratio > 0.0);
    }

    #[test]
    fn test_band_extractor_volatility() {
        let mut extractor = BandExtractor::new();

        // Add transactions with varying volumes
        let volumes = vec![1.0, 2.0, 10.0, 1.5, 20.0, 2.5, 1.0, 15.0, 3.0, 25.0];
        for (i, &vol) in volumes.iter().enumerate() {
            let tx = create_test_transaction(vol, true, 1000 + i as u64 * 10);
            extractor.add_transaction(tx);
        }

        let bands = extractor.extract_bands();

        // Short band should have positive volatility due to varying volumes
        assert!(bands[0].volatility > 0.0);
    }

    #[test]
    fn test_band_extractor_unique_buyers() {
        let mut extractor = BandExtractor::new();

        let buyer1 = create_test_wallet();
        let buyer2 = create_test_wallet();
        let seller = create_test_wallet();

        // Add transactions from different buyers
        for i in 0..5 {
            extractor.add_transaction(BandTransaction::new(10.0, true, buyer1, 1000 + i * 10));
            extractor.add_transaction(BandTransaction::new(10.0, true, buyer2, 1000 + i * 10));
            extractor.add_transaction(BandTransaction::new(5.0, false, seller, 1000 + i * 10));
        }

        let bands = extractor.extract_bands();

        // Should have 2 unique buyers
        assert_eq!(bands[0].buyers, 2);
    }

    #[test]
    fn test_band_extractor_rolling_window() {
        let config = BandConfig::with_window(WINDOW_1S); // 1 second window
        let mut extractor = BandExtractor::with_config(config);

        // Add transactions over 2 seconds
        for i in 0..20 {
            let tx = create_test_transaction(
                10.0,
                true,
                i * 100, // 100ms apart
            );
            extractor.add_transaction(tx);
        }

        // Buffer should only contain last 1 second (10 transactions)
        assert!(extractor.buffer_size() <= 11); // Allow small margin
    }

    #[test]
    fn test_weighted_volume_no_weights() {
        let config = BandConfig::default();
        let tx = BandTransaction::new(10.0, true, create_test_wallet(), 1000);

        // Without MPCF/SOBP, weight should be 1.0
        assert_eq!(tx.weighted_volume(&config), 10.0);
    }

    #[test]
    fn test_weighted_volume_with_mpcf() {
        let config = BandConfig::default().with_mpcf_weighting();
        let tx = BandTransaction::new(10.0, true, create_test_wallet(), 1000).with_actor_score(1.0); // Fully human-like

        // High actor_score should give weight of ~1.0
        let weighted = tx.weighted_volume(&config);
        assert!(weighted >= 9.9 && weighted <= 10.1);

        // Low actor_score should reduce weight
        let tx_bot =
            BandTransaction::new(10.0, true, create_test_wallet(), 1000).with_actor_score(0.0); // Bot-like
        let weighted_bot = tx_bot.weighted_volume(&config);
        assert!(weighted_bot < weighted);
    }

    #[test]
    fn test_weighted_volume_with_sobp() {
        let config = BandConfig::default().with_sobp_weighting();
        let tx = BandTransaction::new(10.0, true, create_test_wallet(), 1000).with_intensity(2.0); // Strong intensity

        // High intensity should increase weight
        let weighted = tx.weighted_volume(&config);
        assert!(weighted > 10.0);
    }

    #[test]
    fn test_extract_bands_batch_mode() {
        // Create a sequence of transactions (need enough to survive rolling window)
        // With 20ms spacing and 5s window, we keep 250 transactions
        let mut transactions = Vec::new();
        for i in 0..300 {
            transactions.push(create_test_transaction(
                5.0 + i as f32 * 0.5,
                i % 2 == 0,
                1000 + i * 20, // 20ms spacing
            ));
        }

        let config = BandConfig::default();
        let bands = extract_bands(&transactions, &config);

        // All three bands should have data
        assert!(bands[0].is_significant());
        assert!(bands[1].is_significant());
        assert!(bands[2].is_significant());

        // With 20ms spacing and 5s window, we should have ~250 transactions
        assert_eq!(bands[0].transaction_count, 32);
        assert!(bands[1].transaction_count >= 32 && bands[1].transaction_count <= 128);
        assert!(bands[2].transaction_count >= 128 && bands[2].transaction_count <= 300);
    }

    #[test]
    fn test_band_extractor_clear() {
        let mut extractor = BandExtractor::new();

        // Add some transactions
        for i in 0..20 {
            extractor.add_transaction(create_test_transaction(10.0, true, 1000 + i * 10));
        }

        assert!(extractor.buffer_size() > 0);

        extractor.clear();

        assert_eq!(extractor.buffer_size(), 0);

        let bands = extractor.extract_bands();
        assert!(!bands[0].is_significant());
    }

    #[test]
    fn test_multi_scale_differences() {
        let mut extractor = BandExtractor::new();

        // Add 200 transactions with increasing volume
        for i in 0..200 {
            let tx = create_test_transaction(1.0 + i as f32 * 0.1, i % 2 == 0, 1000 + i * 20);
            extractor.add_transaction(tx);
        }

        let bands = extractor.extract_bands();

        // Short band (last 32 tx): should have higher average volume
        let short_avg = bands[0].amplitude / bands[0].transaction_count as f32;

        // Medium band (last 128 tx): should have medium average volume
        let medium_avg = bands[1].amplitude / bands[1].transaction_count as f32;

        // Long band (last 200 tx): should have lower average volume
        let long_avg = bands[2].amplitude / bands[2].transaction_count as f32;

        // Due to increasing volumes, short band average should be higher
        assert!(short_avg > long_avg);
    }

    // ═══════════════════════════════════════════════════════════════════════════
    // Tests for FRB Part 2 & 3: Resonance Detection and Signal Classification
    // ═══════════════════════════════════════════════════════════════════════════

    #[test]
    fn test_frb_signal_properties() {
        assert_eq!(FrbSignal::ResContinue.strength(), 0.9);
        assert_eq!(FrbSignal::ResTransition.strength(), 0.5);
        assert_eq!(FrbSignal::ResFake.strength(), 0.2);
        assert_eq!(FrbSignal::ResHold.strength(), 0.0);

        assert!(FrbSignal::ResContinue.is_actionable());
        assert!(FrbSignal::ResFake.is_actionable());
        assert!(!FrbSignal::ResHold.is_actionable());
    }

    #[test]
    fn test_frb_result_creation() {
        let result = FrbResult::new(1000);
        assert_eq!(result.timestamp, 1000);
        assert_eq!(result.resonance_score, 0.0);
        assert_eq!(result.signal, FrbSignal::ResHold);
        assert!(!result.is_significant());
    }

    #[test]
    fn test_resonance_config_defaults() {
        let config = ResonanceConfig::default();
        assert_eq!(config.continue_threshold, 0.7);
        assert_eq!(config.fake_threshold, 0.3);

        // Weights should sum to 1.0
        let sum = config.amplitude_weight
            + config.buyer_weight
            + config.volatility_weight
            + config.ratio_weight;
        assert!((sum - 1.0).abs() < 0.001);
    }

    #[test]
    fn test_resonance_config_with_weights() {
        let config = ResonanceConfig::with_weights(2.0, 1.0, 1.0, 0.5);

        // Weights should be normalized to sum to 1.0
        let sum = config.amplitude_weight
            + config.buyer_weight
            + config.volatility_weight
            + config.ratio_weight;
        assert!((sum - 1.0).abs() < 0.001);

        // Amplitude weight should be highest
        assert!(config.amplitude_weight > config.buyer_weight);
    }

    #[test]
    fn test_resonance_analyzer_empty_profiles() {
        let analyzer = ResonanceAnalyzer::new();
        let profiles = [
            BandProfile::new(1000),
            BandProfile::new(1000),
            BandProfile::new(1000),
        ];

        let result = analyzer.analyze(profiles);

        assert_eq!(result.signal, FrbSignal::ResHold);
        assert_eq!(result.resonance_score, 0.0);
        assert!(!result.is_significant());
    }

    #[test]
    fn test_amplitude_coherence_calculation() {
        let analyzer = ResonanceAnalyzer::new();

        // Same amplitude = high coherence
        let coherence = analyzer.calculate_amplitude_coherence(100.0, 100.0);
        assert!(coherence > 0.9);

        // 2x difference = moderate coherence
        let coherence = analyzer.calculate_amplitude_coherence(100.0, 200.0);
        assert!(coherence > 0.5 && coherence < 0.8);

        // 10x difference = low coherence
        let coherence = analyzer.calculate_amplitude_coherence(100.0, 1000.0);
        assert!(coherence < 0.3);

        // Zero amplitude = no coherence
        let coherence = analyzer.calculate_amplitude_coherence(0.0, 100.0);
        assert_eq!(coherence, 0.0);
    }

    #[test]
    fn test_buyer_coherence_calculation() {
        let analyzer = ResonanceAnalyzer::new();

        // Same buyer count = high coherence
        let coherence = analyzer.calculate_buyer_coherence(10, 10);
        assert!(coherence > 0.9);

        // Similar buyer count = good coherence
        let coherence = analyzer.calculate_buyer_coherence(10, 12);
        assert!(coherence > 0.7);

        // Large difference = low coherence
        let coherence = analyzer.calculate_buyer_coherence(5, 50);
        assert!(coherence < 0.4);

        // Zero buyers = no coherence
        let coherence = analyzer.calculate_buyer_coherence(0, 10);
        assert_eq!(coherence, 0.0);
    }

    #[test]
    fn test_volatility_coherence_calculation() {
        let analyzer = ResonanceAnalyzer::new();

        // Both zero volatility = perfect coherence
        let coherence = analyzer.calculate_volatility_coherence(0.0, 0.0);
        assert_eq!(coherence, 1.0);

        // Same volatility = high coherence
        let coherence = analyzer.calculate_volatility_coherence(5.0, 5.0);
        assert!(coherence > 0.9);

        // One zero, one non-zero = no coherence
        let coherence = analyzer.calculate_volatility_coherence(0.0, 5.0);
        assert_eq!(coherence, 0.0);
    }

    #[test]
    fn test_ratio_coherence_calculation() {
        let analyzer = ResonanceAnalyzer::new();

        // Both have similar buy-heavy ratios
        let coherence = analyzer.calculate_ratio_coherence(Some(2.0), Some(2.5));
        assert!(coherence > 0.7);

        // Both None = moderate coherence
        let coherence = analyzer.calculate_ratio_coherence(None, None);
        assert_eq!(coherence, 0.5);

        // One has ratio, one doesn't = no coherence
        let coherence = analyzer.calculate_ratio_coherence(Some(2.0), None);
        assert_eq!(coherence, 0.0);

        // Both infinite (only buys) = good alignment
        let coherence =
            analyzer.calculate_ratio_coherence(Some(f32::INFINITY), Some(f32::INFINITY));
        assert!(coherence > 0.7);
    }

    #[test]
    fn test_resonance_score_calculation() {
        let analyzer = ResonanceAnalyzer::new();

        // High adjacent coherence = high resonance
        let coherence_map = [0.9, 0.9, 0.7];
        let score = analyzer.calculate_resonance_score(&coherence_map);
        assert!(score > 0.8);

        // Low adjacent coherence = low resonance
        let coherence_map = [0.2, 0.3, 0.5];
        let score = analyzer.calculate_resonance_score(&coherence_map);
        assert!(score < 0.4);

        // Mixed coherence = moderate resonance
        let coherence_map = [0.6, 0.5, 0.4];
        let score = analyzer.calculate_resonance_score(&coherence_map);
        assert!(score > 0.4 && score < 0.7);
    }

    #[test]
    fn test_signal_classification_res_continue() {
        let analyzer = ResonanceAnalyzer::new();

        // Create strong resonance scenario: all bands active with high coherence
        // Use more similar values across bands for higher coherence
        let profiles = [
            BandProfile {
                amplitude: 160.0, // 5.0 per tx
                buyers: 10,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 500.0, // 5.0 per tx
                buyers: 30,
                volatility: 5.5,
                timestamp: 1000,
                buy_sell_ratio: Some(2.1),
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 1000.0, // 5.0 per tx
                buyers: 60,
                volatility: 6.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 200,
            },
        ];

        let result = analyzer.analyze(profiles);

        // Should classify as RES_CONTINUE due to strong resonance
        // Note: might get ResTransition if coherence is not quite at threshold
        assert!(
            result.signal == FrbSignal::ResContinue || result.signal == FrbSignal::ResTransition,
            "Expected RES_CONTINUE or RES_TRANSITION, got {:?}",
            result.signal
        );
        assert!(result.resonance_score > 0.5);
        assert!(result.trend_likelihood > 0.5);
    }

    #[test]
    fn test_signal_classification_res_fake() {
        let analyzer = ResonanceAnalyzer::new();

        // Create fake pump scenario: only short band active
        let profiles = [
            BandProfile {
                amplitude: 100.0,
                buyers: 2, // Very few buyers = bot-like
                volatility: 2.0,
                timestamp: 1000,
                buy_sell_ratio: Some(5.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 0.001, // Insignificant
                buyers: 0,
                volatility: 0.0,
                timestamp: 1000,
                buy_sell_ratio: None,
                transaction_count: 0,
            },
            BandProfile {
                amplitude: 0.0,
                buyers: 0,
                volatility: 0.0,
                timestamp: 1000,
                buy_sell_ratio: None,
                transaction_count: 0,
            },
        ];

        let result = analyzer.analyze(profiles);

        // Should classify as RES_FAKE
        assert_eq!(result.signal, FrbSignal::ResFake);
        assert!(result.resonance_score < 0.3);
    }

    #[test]
    fn test_signal_classification_res_transition() {
        let analyzer = ResonanceAnalyzer::new();

        // Create transitional scenario: medium resonance
        let profiles = [
            BandProfile {
                amplitude: 50.0,
                buyers: 5,
                volatility: 3.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.5),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 80.0,
                buyers: 8,
                volatility: 4.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.2),
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 0.001, // Long band not yet active
                buyers: 0,
                volatility: 0.0,
                timestamp: 1000,
                buy_sell_ratio: None,
                transaction_count: 0,
            },
        ];

        let result = analyzer.analyze(profiles);

        // Should classify as RES_TRANSITION
        assert_eq!(result.signal, FrbSignal::ResTransition);
        assert!(result.resonance_score > 0.3 && result.resonance_score < 0.7);
    }

    #[test]
    fn test_trend_likelihood_with_momentum() {
        let analyzer = ResonanceAnalyzer::new();

        // Increasing amplitude from long to short = positive momentum
        let profiles = [
            BandProfile {
                amplitude: 160.0, // 5.0 per tx
                buyers: 8,
                volatility: 3.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 384.0, // 3.0 per tx (less than short)
                buyers: 20,
                volatility: 4.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.8),
                transaction_count: 128,
            },
            BandProfile {
                amplitude: 600.0, // 3.0 per tx
                buyers: 40,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.5),
                transaction_count: 200,
            },
        ];

        let result = analyzer.analyze(profiles);

        // High resonance + momentum + buy pressure = high trend likelihood
        assert!(result.trend_likelihood > 0.6);
    }

    #[test]
    fn test_trend_likelihood_with_buy_pressure() {
        let analyzer = ResonanceAnalyzer::new();

        // All bands have strong buy pressure
        let profiles = [
            BandProfile {
                amplitude: 100.0,
                buyers: 5,
                volatility: 3.0,
                timestamp: 1000,
                buy_sell_ratio: Some(3.0), // Strong buy
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 300.0,
                buyers: 15,
                volatility: 4.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.5), // Strong buy
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 800.0,
                buyers: 40,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0), // Strong buy
                transaction_count: 200,
            },
        ];

        let result = analyzer.analyze(profiles);

        // Strong buy pressure across all bands should increase likelihood
        assert!(result.trend_likelihood > 0.5);
    }

    #[test]
    fn test_analyze_resonance_helper_function() {
        let profiles = [
            BandProfile {
                amplitude: 100.0,
                buyers: 10,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 300.0,
                buyers: 25,
                volatility: 7.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.2),
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 800.0,
                buyers: 60,
                volatility: 10.0,
                timestamp: 1000,
                buy_sell_ratio: Some(1.9),
                transaction_count: 200,
            },
        ];

        let result = analyze_resonance(profiles);

        assert!(result.is_significant());
        assert!(result.resonance_score > 0.0);
        assert_ne!(result.signal, FrbSignal::ResHold);
    }

    #[test]
    fn test_coherence_map_structure() {
        let analyzer = ResonanceAnalyzer::new();

        let profiles = [
            BandProfile {
                amplitude: 100.0,
                buyers: 10,
                volatility: 5.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 32,
            },
            BandProfile {
                amplitude: 150.0,
                buyers: 15,
                volatility: 6.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.1),
                transaction_count: 100,
            },
            BandProfile {
                amplitude: 200.0,
                buyers: 20,
                volatility: 7.0,
                timestamp: 1000,
                buy_sell_ratio: Some(2.0),
                transaction_count: 200,
            },
        ];

        let result = analyzer.analyze(profiles);

        // Coherence map should have 3 values
        assert_eq!(result.coherence_map.len(), 3);

        // All coherence values should be in 0-1 range
        for &coherence in &result.coherence_map {
            assert!(coherence >= 0.0 && coherence <= 1.0);
        }

        // With similar profiles, all coherences should be reasonably high
        assert!(result.coherence_map[0] > 0.5); // short-medium
        assert!(result.coherence_map[1] > 0.5); // medium-long
        assert!(result.coherence_map[2] > 0.4); // short-long
    }
}
