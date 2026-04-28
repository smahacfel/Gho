//! Harmonic & Field Analysis Module (WHF Part 2)
//!
//! This module provides mathematical field analysis on wallet energy distributions,
//! detecting market patterns through vector calculus and harmonic analysis.
//!
//! ## Concepts
//!
//! - **Curl**: Measures rotation/vorticity in the energy field (wash trading detection)
//! - **Divergence**: Measures flow concentration (accumulation vs distribution)
//! - **Resonance**: Statistical detection of periodic trading patterns (bot detection)
//!
//! ## Architecture
//!
//! ```text
//! ┌─────────────────────────────────────────────────────────┐
//! │         Harmonic & Field Analysis (WHF Part 2)          │
//! │                                                         │
//! │  Input: WalletEnergyTracker Data                       │
//! │         (wallet positions, energy flows)                │
//! │                                                         │
//! │  ┌──────────────┐   ┌──────────────┐   ┌─────────────┐│
//! │  │ Field Ops    │   │ CV Analysis  │   │ Indicators  ││
//! │  │ (curl/div)   │   │ (resonance)  │   │ (stability) ││
//! │  └──────────────┘   └──────────────┘   └─────────────┘│
//! │                                                         │
//! │  Output: HarmonicFieldAnalysis                         │
//! │          { curl, divergence, resonance_score }         │
//! └─────────────────────────────────────────────────────────┘
//! ```

use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

/// Default buffer size for timestamp analysis window
pub const DEFAULT_BUFFER_SIZE: usize = 128;

/// Result of harmonic and field analysis
///
/// This structure contains the key metrics from analyzing the wallet energy field.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HarmonicFieldAnalysis {
    /// Curl measurement: rotation/vorticity in energy flow
    /// High values indicate wash trading or circular flows
    /// Range: 0.0 (no rotation) to 1.0+ (high vorticity)
    pub curl: f32,

    /// Divergence measurement: flow concentration
    /// Positive = capital accumulation (buying pressure)
    /// Negative = capital distribution (selling pressure)
    /// Range: -1.0 (strong outflow) to +1.0 (strong inflow)
    pub divergence: f32,

    /// Resonance score: periodic pattern strength
    /// Detected via FFT analysis of trading intervals
    /// High values indicate bot activity or coordinated trading
    /// Range: 0.0 (random) to 1.0 (highly periodic)
    pub resonance_score: f32,

    /// Timestamp of analysis (milliseconds since epoch)
    pub timestamp_ms: u64,
}

/// Represents a wallet's position in 2D energy space
///
/// For field analysis, we map wallet energy to 2D coordinates:
/// - X-axis: Energy magnitude (SOL balance × activity)
/// - Y-axis: Token concentration (number of unique tokens held)
#[derive(Debug, Clone)]
struct WalletFieldPoint {
    /// Energy magnitude
    energy: f64,

    /// Token state: None = free liquidity, Some = locked in token
    token: Option<Pubkey>,

    /// Velocity: change in energy over time (buy = positive, sell = negative)
    velocity: f64,

    /// Last action timestamp
    timestamp_ms: u64,
}

/// Field extractor: converts wallet data into analyzable field points
struct FieldExtractor;

impl FieldExtractor {
    /// Extract field points from wallet energy data
    ///
    /// # Arguments
    ///
    /// * `wallet_data` - Map of wallet pubkey to (energy, token, timestamp)
    ///
    /// # Returns
    ///
    /// Vector of field points suitable for curl/div analysis
    fn extract_field_points(
        wallet_data: &HashMap<Pubkey, (f64, Option<Pubkey>, u64)>,
        previous_data: Option<&HashMap<Pubkey, (f64, Option<Pubkey>, u64)>>,
    ) -> Vec<WalletFieldPoint> {
        let mut points = Vec::with_capacity(wallet_data.len());

        for (wallet, (energy, token, timestamp_ms)) in wallet_data {
            // Calculate velocity (energy change rate)
            let velocity = if let Some(prev) = previous_data {
                if let Some((prev_energy, _, prev_time)) = prev.get(wallet) {
                    let time_delta = (*timestamp_ms - *prev_time) as f64;
                    if time_delta > 0.0 {
                        (*energy - *prev_energy) / (time_delta / 1000.0) // Energy change per second
                    } else {
                        0.0
                    }
                } else {
                    *energy // New wallet, assume initial velocity = energy
                }
            } else {
                0.0 // No previous data
            };

            points.push(WalletFieldPoint {
                energy: *energy,
                token: *token,
                velocity,
                timestamp_ms: *timestamp_ms,
            });
        }

        points
    }
}

/// Harmonic & Field Analyzer
///
/// Main analysis engine that computes curl, divergence, and resonance scores
pub struct HarmonicFieldAnalyzer {
    /// Buffer for storing transaction timestamps (for resonance analysis)
    timestamp_buffer: Vec<u64>,

    /// Maximum buffer size (sliding window)
    max_buffer_size: usize,

    /// Previous wallet state for velocity calculation
    previous_state: Option<HashMap<Pubkey, (f64, Option<Pubkey>, u64)>>,
}

impl HarmonicFieldAnalyzer {
    /// Create a new harmonic field analyzer
    ///
    /// # Arguments
    ///
    /// * `buffer_size` - Size of sliding window for timestamp analysis (default: 128)
    pub fn new(buffer_size: usize) -> Self {
        Self {
            timestamp_buffer: Vec::with_capacity(buffer_size),
            max_buffer_size: buffer_size,
            previous_state: None,
        }
    }

    /// Analyze wallet energy field and compute harmonic/field metrics
    ///
    /// # Arguments
    ///
    /// * `wallet_data` - Current wallet state: map of pubkey -> (energy, token, timestamp)
    /// * `timestamp_ms` - Current analysis timestamp
    ///
    /// # Returns
    ///
    /// HarmonicFieldAnalysis containing curl, divergence, and resonance scores
    pub fn analyze(
        &mut self,
        wallet_data: &HashMap<Pubkey, (f64, Option<Pubkey>, u64)>,
        timestamp_ms: u64,
    ) -> HarmonicFieldAnalysis {
        // Extract field points with velocity information
        let field_points =
            FieldExtractor::extract_field_points(wallet_data, self.previous_state.as_ref());

        // Calculate curl (vorticity)
        let curl = self.calculate_curl(&field_points);

        // Calculate divergence (flow concentration)
        let divergence = self.calculate_divergence(&field_points);

        // Update timestamp buffer for resonance analysis
        self.update_timestamp_buffer(wallet_data.values().map(|(_, _, ts)| *ts).collect());

        // Calculate resonance score via FFT
        let resonance_score = self.calculate_resonance();

        // Store current state for next iteration
        self.previous_state = Some(wallet_data.clone());

        HarmonicFieldAnalysis {
            curl,
            divergence,
            resonance_score,
            timestamp_ms,
        }
    }

    /// Calculate curl (rotation) in the energy field
    ///
    /// Curl measures the tendency for energy to flow in circular patterns,
    /// which is indicative of wash trading or manipulative behavior.
    ///
    /// Algorithm:
    /// 1. Analyze velocity patterns across all wallets
    /// 2. Detect sign changes in velocity (buy-sell alternation)
    /// 3. Weight by energy magnitude
    /// 4. Normalize to [0.0, 1.0]
    fn calculate_curl(&self, field_points: &[WalletFieldPoint]) -> f32 {
        if field_points.len() < 2 {
            return 0.0;
        }

        // Sort by timestamp to analyze temporal patterns
        let mut sorted_points: Vec<_> = field_points.iter().collect();
        sorted_points.sort_by_key(|p| p.timestamp_ms);

        // Detect velocity sign changes (buy-sell-buy patterns)
        let mut sign_changes = 0;
        let mut total_transitions = 0;

        for i in 1..sorted_points.len() {
            let prev_vel = sorted_points[i - 1].velocity;
            let curr_vel = sorted_points[i].velocity;

            // Skip zero velocities
            if prev_vel.abs() < 0.001 || curr_vel.abs() < 0.001 {
                continue;
            }

            total_transitions += 1;

            // Check for sign change
            if (prev_vel > 0.0) != (curr_vel > 0.0) {
                sign_changes += 1;
            }
        }

        if total_transitions == 0 {
            return 0.0;
        }

        // High sign change rate indicates circular trading
        (sign_changes as f64 / total_transitions as f64) as f32
    }

    /// Calculate divergence (flow concentration)
    ///
    /// Divergence measures whether capital is accumulating (positive)
    /// or distributing (negative) in the market.
    ///
    /// Algorithm:
    /// 1. Sum all positive velocities (inflows)
    /// 2. Sum all negative velocities (outflows)
    /// 3. Calculate net flow normalized by total energy
    fn calculate_divergence(&self, field_points: &[WalletFieldPoint]) -> f32 {
        if field_points.is_empty() {
            return 0.0;
        }

        let mut total_inflow = 0.0;
        let mut total_outflow = 0.0;
        let mut total_energy = 0.0;

        for point in field_points {
            total_energy += point.energy;

            if point.velocity > 0.0 {
                total_inflow += point.velocity;
            } else if point.velocity < 0.0 {
                total_outflow += point.velocity.abs();
            }
        }

        if total_energy == 0.0 {
            return 0.0;
        }

        // Net flow normalized by total energy
        let net_flow = total_inflow - total_outflow;
        let divergence = net_flow / total_energy;

        // Clamp to [-1.0, 1.0] range
        divergence.max(-1.0).min(1.0) as f32
    }

    /// Update the timestamp buffer with new transaction timestamps
    ///
    /// This is primarily used internally by `analyze()`, but can be called
    /// directly for testing or manual buffer management.
    pub fn update_timestamp_buffer(&mut self, timestamps: Vec<u64>) {
        // Add new timestamps
        self.timestamp_buffer.extend(timestamps);

        // Keep only the most recent timestamps (sliding window)
        if self.timestamp_buffer.len() > self.max_buffer_size {
            let start_idx = self.timestamp_buffer.len() - self.max_buffer_size;
            self.timestamp_buffer = self.timestamp_buffer[start_idx..].to_vec();
        }

        // Sort for interval analysis
        self.timestamp_buffer.sort_unstable();
    }

    /// Calculate resonance score using coefficient of variation on transaction intervals
    ///
    /// Resonance measures the presence of periodic patterns in trading activity.
    /// High resonance indicates bot activity or coordinated trading.
    ///
    /// This method can be called independently after populating the timestamp buffer
    /// for direct resonance analysis.
    ///
    /// Algorithm:
    /// 1. Calculate intervals between consecutive transactions
    /// 2. Compute coefficient of variation (CV = std_dev / mean)
    /// 3. Convert CV to resonance score [0.0, 1.0]
    pub fn calculate_resonance(&self) -> f32 {
        if self.timestamp_buffer.len() < 8 {
            // Need at least 8 samples for meaningful FFT
            return 0.0;
        }

        // Calculate intervals between consecutive timestamps
        let mut intervals: Vec<f64> = Vec::new();
        for i in 1..self.timestamp_buffer.len() {
            let interval = (self.timestamp_buffer[i] - self.timestamp_buffer[i - 1]) as f64;
            if interval > 0.0 {
                intervals.push(interval);
            }
        }

        if intervals.len() < 4 {
            return 0.0;
        }

        // Calculate coefficient of variation (CV) as a simple periodicity measure
        // Low CV = consistent intervals = high periodicity
        let mean: f64 = intervals.iter().sum::<f64>() / intervals.len() as f64;

        if mean == 0.0 {
            return 0.0;
        }

        let variance: f64 = intervals
            .iter()
            .map(|x| {
                let diff = x - mean;
                diff * diff
            })
            .sum::<f64>()
            / intervals.len() as f64;

        let std_dev = variance.sqrt();
        let cv = std_dev / mean;

        // Convert CV to resonance score
        // CV near 0 = highly periodic (resonance = 1.0)
        // CV > 1 = very random (resonance = 0.0)
        let resonance_score = if cv < 0.001 {
            1.0 // Perfect periodicity
        } else if cv > 1.5 {
            0.0 // Highly random
        } else {
            // Map CV [0, 1.5] to resonance [1.0, 0.0]
            (1.0 - (cv / 1.5)).max(0.0)
        };

        resonance_score as f32
    }
}

impl Default for HarmonicFieldAnalyzer {
    fn default() -> Self {
        Self::new(DEFAULT_BUFFER_SIZE)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solana_sdk::pubkey::Pubkey;

    /// Test buffer size (smaller than default for faster tests)
    const TEST_BUFFER_SIZE: usize = 64;

    /// Maximum expected resonance for random patterns
    const MAX_RANDOM_RESONANCE: f32 = 0.7;
    #[test]
    fn test_field_analysis_empty_data() {
        let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);
        let wallet_data = HashMap::new();

        let result = analyzer.analyze(&wallet_data, 1000000);

        assert_eq!(result.curl, 0.0);
        assert_eq!(result.divergence, 0.0);
        assert_eq!(result.resonance_score, 0.0);
    }

    #[test]
    fn test_divergence_accumulation() {
        let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

        // First snapshot: wallets with energy
        let mut state1 = HashMap::new();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();
        state1.insert(wallet1, (100.0, None, 1000));
        state1.insert(wallet2, (100.0, None, 1000));

        analyzer.analyze(&state1, 1000);

        // Second snapshot: energy increased (accumulation)
        let mut state2 = HashMap::new();
        state2.insert(wallet1, (150.0, None, 2000)); // +50 in 1 second
        state2.insert(wallet2, (180.0, None, 2000)); // +80 in 1 second

        let result = analyzer.analyze(&state2, 2000);

        // Divergence should be positive (accumulation)
        assert!(
            result.divergence > 0.0,
            "Expected positive divergence for accumulation"
        );
    }

    #[test]
    fn test_divergence_distribution() {
        let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

        // First snapshot
        let mut state1 = HashMap::new();
        let wallet1 = Pubkey::new_unique();
        state1.insert(wallet1, (200.0, None, 1000));

        analyzer.analyze(&state1, 1000);

        // Second snapshot: energy decreased (distribution)
        let mut state2 = HashMap::new();
        state2.insert(wallet1, (100.0, None, 2000)); // -100 in 1 second

        let result = analyzer.analyze(&state2, 2000);

        // Divergence should be negative (distribution)
        assert!(
            result.divergence < 0.0,
            "Expected negative divergence for distribution"
        );
    }

    #[test]
    fn test_curl_wash_trading_pattern() {
        let mut analyzer = HarmonicFieldAnalyzer::new(TEST_BUFFER_SIZE);

        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();
        let token = Pubkey::new_unique();

        // Simulate alternating buy/sell pattern (wash trading)
        let mut state1 = HashMap::new();
        state1.insert(wallet1, (100.0, Some(token), 1000));
        state1.insert(wallet2, (100.0, None, 1000));

        analyzer.analyze(&state1, 1000);

        // Reverse: wallet1 sells, wallet2 buys
        let mut state2 = HashMap::new();
        state2.insert(wallet1, (50.0, None, 2000)); // Negative velocity
        state2.insert(wallet2, (150.0, Some(token), 2000)); // Positive velocity

        analyzer.analyze(&state2, 2000);

        // Reverse again
        let mut state3 = HashMap::new();
        state3.insert(wallet1, (150.0, Some(token), 3000)); // Positive velocity
        state3.insert(wallet2, (50.0, None, 3000)); // Negative velocity

        let result = analyzer.analyze(&state3, 3000);

        // Curl should detect alternating pattern
        assert!(
            result.curl > 0.0,
            "Expected non-zero curl for wash trading pattern"
        );
    }

    #[test]
    fn test_resonance_periodic_pattern() {
        let mut analyzer = HarmonicFieldAnalyzer::new(DEFAULT_BUFFER_SIZE);

        // Create periodic timestamps (every 100ms)
        let mut timestamps = Vec::new();
        for i in 0..32 {
            timestamps.push(1000 + (i * 100));
        }

        analyzer.update_timestamp_buffer(timestamps);
        let resonance = analyzer.calculate_resonance();

        // Should detect strong periodicity
        assert!(
            resonance > 0.3,
            "Expected high resonance for periodic pattern, got {}",
            resonance
        );
    }

    #[test]
    fn test_resonance_random_pattern() {
        let mut analyzer = HarmonicFieldAnalyzer::new(DEFAULT_BUFFER_SIZE);

        // Create random timestamps with high variance
        use rand::Rng;
        let mut rng = rand::thread_rng();
        let mut timestamps = Vec::new();
        let mut time = 1000;
        for _ in 0..32 {
            time += rng.gen_range(10..1000); // Wide random intervals for high CV
            timestamps.push(time);
        }

        analyzer.update_timestamp_buffer(timestamps);
        let resonance = analyzer.calculate_resonance();

        // Should detect low periodicity
        assert!(
            resonance < MAX_RANDOM_RESONANCE,
            "Expected low resonance for random pattern, got {}",
            resonance
        );
    }

    #[test]
    fn test_field_extractor() {
        let mut wallet_data = HashMap::new();
        let wallet1 = Pubkey::new_unique();
        let wallet2 = Pubkey::new_unique();

        wallet_data.insert(wallet1, (100.0, None, 1000));
        wallet_data.insert(wallet2, (200.0, Some(Pubkey::new_unique()), 1000));

        let points = FieldExtractor::extract_field_points(&wallet_data, None);

        assert_eq!(points.len(), 2);
        assert!(points.iter().any(|p| p.energy == 100.0));
        assert!(points.iter().any(|p| p.energy == 200.0));
    }

    #[test]
    fn test_velocity_calculation() {
        let wallet = Pubkey::new_unique();

        // Previous state
        let mut prev_data = HashMap::new();
        prev_data.insert(wallet, (100.0, None, 1000));

        // Current state (energy increased)
        let mut curr_data = HashMap::new();
        curr_data.insert(wallet, (200.0, None, 2000)); // +100 in 1 second

        let points = FieldExtractor::extract_field_points(&curr_data, Some(&prev_data));

        assert_eq!(points.len(), 1);
        // Velocity should be 100.0 energy per second
        assert!((points[0].velocity - 100.0).abs() < 0.1);
    }
}
