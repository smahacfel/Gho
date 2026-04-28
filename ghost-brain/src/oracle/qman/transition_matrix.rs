//! Transition Matrix Construction
//!
//! Builds a sparse transition matrix T from observed wallet movements between tokens.
//! If wallets massively swap A -> B, the matrix element T[A][B] grows.
//!
//! ## Design
//!
//! - **Sparse Matrix**: Most tokens don't directly transition to each other
//! - **Time Decay**: Recent transitions have more weight than old ones
//! - **Normalization**: Rows sum to 1.0 (probability distribution)

use nalgebra::DMatrix;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::{HashMap, VecDeque};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Maximum number of tokens to track in the transition matrix
/// Beyond this, we only track the most active tokens
const MAX_TOKENS: usize = 256;

/// Time window for transition observations (5 minutes)
const TRANSITION_WINDOW_MS: u64 = 300_000;

/// Minimum transition count to be considered significant
const MIN_TRANSITION_COUNT: usize = 3;

/// A single observed transition between states
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Transition {
    /// Source token (None = SOL/free liquidity)
    pub from_token: Option<Pubkey>,

    /// Destination token (None = SOL/free liquidity)
    pub to_token: Option<Pubkey>,

    /// Energy (capital) involved in the transition
    pub energy: f64,

    /// Timestamp when transition was observed (milliseconds)
    pub timestamp_ms: u64,

    /// Wallet that made the transition
    pub wallet: Pubkey,
}

/// Sparse representation of transition matrix for memory efficiency
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SparseTransitionMatrix {
    /// Ordered list of token states (index -> token pubkey)
    /// Index 0 is always None (free liquidity/SOL)
    pub states: Vec<Option<Pubkey>>,

    /// Sparse matrix data: (from_idx, to_idx) -> transition_weight
    /// Only stores non-zero transitions
    pub transitions: HashMap<(usize, usize), f64>,

    /// Total transition count per row for normalization
    pub row_totals: Vec<f64>,

    /// Last update timestamp
    pub last_update_ms: u64,
}

impl SparseTransitionMatrix {
    /// Create a new empty sparse transition matrix
    pub fn new() -> Self {
        Self {
            states: vec![None], // Index 0 = free liquidity
            transitions: HashMap::new(),
            row_totals: vec![0.0],
            last_update_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis() as u64,
        }
    }

    /// Get or create index for a token state
    fn get_or_create_state_index(&mut self, token: Option<Pubkey>) -> usize {
        // Check if token already exists
        if let Some(idx) = self.states.iter().position(|t| *t == token) {
            return idx;
        }

        // Add new token if we haven't hit the limit
        if self.states.len() < MAX_TOKENS {
            let idx = self.states.len();
            self.states.push(token);
            self.row_totals.push(0.0);
            idx
        } else {
            // If we hit the limit, map to free liquidity (index 0)
            // This is a simplification - in production we'd track most active tokens
            0
        }
    }

    /// Get index for an existing token (returns None if token not tracked)
    pub fn get_state_index(&self, token: &Option<Pubkey>) -> Option<usize> {
        self.states.iter().position(|t| t == token)
    }

    /// Get token at a given index
    pub fn get_state(&self, index: usize) -> Option<&Option<Pubkey>> {
        self.states.get(index)
    }

    /// Get transition probability from state i to state j
    pub fn get_transition_prob(&self, from_idx: usize, to_idx: usize) -> f64 {
        if from_idx >= self.states.len() || to_idx >= self.states.len() {
            return 0.0;
        }

        let weight = self
            .transitions
            .get(&(from_idx, to_idx))
            .copied()
            .unwrap_or(0.0);
        let total = self.row_totals.get(from_idx).copied().unwrap_or(0.0);

        if total > 0.0 {
            weight / total
        } else {
            0.0
        }
    }

    /// Get all non-zero transitions from a given state
    pub fn get_transitions_from(&self, from_idx: usize) -> Vec<(usize, f64)> {
        let mut result = Vec::new();

        for ((f, t), weight) in &self.transitions {
            if *f == from_idx {
                let prob = if let Some(total) = self.row_totals.get(from_idx) {
                    if *total > 0.0 {
                        weight / total
                    } else {
                        0.0
                    }
                } else {
                    0.0
                };

                if prob > 0.0 {
                    result.push((*t, prob));
                }
            }
        }

        result
    }

    /// Number of tracked states
    pub fn num_states(&self) -> usize {
        self.states.len()
    }

    /// Convert to dense matrix (for testing/debugging)
    pub fn to_dense(&self) -> DMatrix<f64> {
        let n = self.num_states();
        let mut matrix = DMatrix::zeros(n, n);

        for i in 0..n {
            for j in 0..n {
                matrix[(i, j)] = self.get_transition_prob(i, j);
            }
        }

        matrix
    }
}

impl Default for SparseTransitionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

/// Builder for constructing transition matrices from observed transitions
#[derive(Clone)]
pub struct TransitionMatrixBuilder {
    /// Recent transitions in the observation window
    transitions: Arc<RwLock<VecDeque<Transition>>>,

    /// Time decay factor (exponential decay for older transitions)
    /// Default: 0.95 (5% decay per time unit)
    decay_factor: f64,

    /// Current matrix
    matrix: Arc<RwLock<SparseTransitionMatrix>>,
}

impl TransitionMatrixBuilder {
    /// Create a new transition matrix builder
    pub fn new() -> Self {
        Self {
            transitions: Arc::new(RwLock::new(VecDeque::new())),
            decay_factor: 0.95,
            matrix: Arc::new(RwLock::new(SparseTransitionMatrix::new())),
        }
    }

    /// Create a builder with custom decay factor
    pub fn with_decay(mut self, decay_factor: f64) -> Self {
        self.decay_factor = decay_factor.clamp(0.0, 1.0);
        self
    }

    /// Record a transition observation
    pub fn observe_transition(
        &self,
        wallet: Pubkey,
        from_token: Option<Pubkey>,
        to_token: Option<Pubkey>,
        energy: f64,
    ) {
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        let transition = Transition {
            from_token,
            to_token,
            energy,
            timestamp_ms,
            wallet,
        };

        // Add to transition history
        let mut transitions = self.transitions.write();
        transitions.push_back(transition);

        // Cleanup old transitions (outside the time window)
        let cutoff_time = timestamp_ms.saturating_sub(TRANSITION_WINDOW_MS);
        while let Some(front) = transitions.front() {
            if front.timestamp_ms < cutoff_time {
                transitions.pop_front();
            } else {
                break;
            }
        }
    }

    /// Rebuild the transition matrix from observed transitions
    pub fn rebuild_matrix(&self) {
        let transitions = self.transitions.read();
        let current_time = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        // Create new matrix
        let mut matrix = SparseTransitionMatrix::new();
        matrix.last_update_ms = current_time;

        // Count transitions with time decay
        let mut transition_weights: HashMap<(Option<Pubkey>, Option<Pubkey>), f64> = HashMap::new();

        for t in transitions.iter() {
            // Calculate time-based weight (exponential decay)
            let age_ms = current_time.saturating_sub(t.timestamp_ms);
            let age_units = age_ms as f64 / 1000.0; // Convert to seconds
            let decay_weight = self.decay_factor.powf(age_units);

            // Weight by energy and time decay
            let weight = t.energy * decay_weight;

            *transition_weights
                .entry((t.from_token, t.to_token))
                .or_insert(0.0) += weight;
        }

        // Build sparse matrix from weighted transitions
        for ((from_token, to_token), weight) in transition_weights {
            if weight > 0.0 {
                let from_idx = matrix.get_or_create_state_index(from_token);
                let to_idx = matrix.get_or_create_state_index(to_token);

                *matrix.transitions.entry((from_idx, to_idx)).or_insert(0.0) += weight;
                matrix.row_totals[from_idx] += weight;
            }
        }

        // Update stored matrix
        *self.matrix.write() = matrix;
    }

    /// Get the current transition matrix
    pub fn get_matrix(&self) -> SparseTransitionMatrix {
        self.matrix.read().clone()
    }

    /// Get number of recent transitions
    pub fn transition_count(&self) -> usize {
        self.transitions.read().len()
    }

    /// Clear all transitions and reset matrix
    pub fn reset(&self) {
        self.transitions.write().clear();
        *self.matrix.write() = SparseTransitionMatrix::new();
    }
}

impl Default for TransitionMatrixBuilder {
    fn default() -> Self {
        Self::new()
    }
}

/// Main transition matrix coordinator
#[derive(Clone)]
pub struct TransitionMatrix {
    builder: TransitionMatrixBuilder,
}

impl TransitionMatrix {
    /// Create a new transition matrix
    pub fn new() -> Self {
        Self {
            builder: TransitionMatrixBuilder::new(),
        }
    }

    /// Create with custom decay factor
    pub fn with_decay(decay_factor: f64) -> Self {
        Self {
            builder: TransitionMatrixBuilder::new().with_decay(decay_factor),
        }
    }

    /// Observe a wallet transition
    pub fn observe_transition(
        &self,
        wallet: Pubkey,
        from_token: Option<Pubkey>,
        to_token: Option<Pubkey>,
        energy: f64,
    ) {
        self.builder
            .observe_transition(wallet, from_token, to_token, energy);
    }

    /// Rebuild the matrix from recent observations
    pub fn update(&self) {
        self.builder.rebuild_matrix();
    }

    /// Get the current matrix
    pub fn get_matrix(&self) -> SparseTransitionMatrix {
        self.builder.get_matrix()
    }

    /// Get number of recent transitions
    pub fn transition_count(&self) -> usize {
        self.builder.transition_count()
    }

    /// Reset all data
    pub fn reset(&self) {
        self.builder.reset();
    }
}

impl Default for TransitionMatrix {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_pubkey(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    #[test]
    fn test_sparse_matrix_creation() {
        let matrix = SparseTransitionMatrix::new();
        assert_eq!(matrix.num_states(), 1); // Only free liquidity initially
        assert_eq!(matrix.states[0], None);
    }

    #[test]
    fn test_state_indexing() {
        let mut matrix = SparseTransitionMatrix::new();

        let token_a = Some(test_pubkey(1));
        let token_b = Some(test_pubkey(2));

        let idx_a = matrix.get_or_create_state_index(token_a);
        let idx_b = matrix.get_or_create_state_index(token_b);

        assert_eq!(idx_a, 1);
        assert_eq!(idx_b, 2);
        assert_eq!(matrix.num_states(), 3); // free + A + B
    }

    #[test]
    fn test_transition_probability() {
        let mut matrix = SparseTransitionMatrix::new();

        let free = None;
        let token_a = Some(test_pubkey(1));

        let idx_free = matrix.get_or_create_state_index(free);
        let idx_a = matrix.get_or_create_state_index(token_a);

        // Add transition: free -> token_a with weight 10.0
        matrix.transitions.insert((idx_free, idx_a), 10.0);
        matrix.row_totals[idx_free] = 10.0;

        let prob = matrix.get_transition_prob(idx_free, idx_a);
        assert!((prob - 1.0).abs() < 1e-6); // Should be 1.0 (100%)
    }

    #[test]
    fn test_transition_builder() {
        let builder = TransitionMatrixBuilder::new();

        let wallet = test_pubkey(10);
        let token_a = Some(test_pubkey(1));
        let token_b = Some(test_pubkey(2));

        // Observe transitions
        builder.observe_transition(wallet, None, token_a, 10.0); // Buy A
        builder.observe_transition(wallet, token_a, None, 8.0); // Sell A
        builder.observe_transition(wallet, None, token_b, 12.0); // Buy B

        assert_eq!(builder.transition_count(), 3);

        // Build matrix
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        assert!(matrix.num_states() >= 3); // free + A + B
    }

    #[test]
    fn test_transition_normalization() {
        let builder = TransitionMatrixBuilder::new();

        let wallet = test_pubkey(10);
        let token_a = Some(test_pubkey(1));
        let token_b = Some(test_pubkey(2));

        // From free liquidity: 60% -> A, 40% -> B
        builder.observe_transition(wallet, None, token_a, 6.0);
        builder.observe_transition(wallet, None, token_b, 4.0);

        builder.rebuild_matrix();
        let matrix = builder.get_matrix();

        let idx_free = matrix.get_state_index(&None).unwrap();
        let idx_a = matrix.get_state_index(&token_a).unwrap();
        let idx_b = matrix.get_state_index(&token_b).unwrap();

        let prob_a = matrix.get_transition_prob(idx_free, idx_a);
        let prob_b = matrix.get_transition_prob(idx_free, idx_b);

        // Should sum to 1.0 (or close to it)
        assert!((prob_a + prob_b - 1.0).abs() < 0.01);
        assert!(prob_a > prob_b); // A should have higher probability
    }

    #[test]
    fn test_dense_conversion() {
        let mut matrix = SparseTransitionMatrix::new();

        let token_a = Some(test_pubkey(1));
        let idx_free = 0;
        let idx_a = matrix.get_or_create_state_index(token_a);

        matrix.transitions.insert((idx_free, idx_a), 10.0);
        matrix.row_totals[idx_free] = 10.0;

        let dense = matrix.to_dense();
        assert_eq!(dense.nrows(), 2);
        assert_eq!(dense.ncols(), 2);
        assert!((dense[(idx_free, idx_a)] - 1.0).abs() < 1e-6);
    }

    #[test]
    fn test_transition_matrix_integration() {
        let tm = TransitionMatrix::new();

        let wallet = test_pubkey(10);
        let token_a = Some(test_pubkey(1));

        // Observe and update
        tm.observe_transition(wallet, None, token_a, 10.0);
        tm.update();

        let matrix = tm.get_matrix();
        assert!(matrix.num_states() >= 2);

        // Reset
        tm.reset();
        assert_eq!(tm.transition_count(), 0);
    }
}
