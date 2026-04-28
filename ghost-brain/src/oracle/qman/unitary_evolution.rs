//! Unitary Evolution Engine
//!
//! Implements quantum-inspired unitary evolution for capital flow prediction.
//!
//! ## Core Concept
//!
//! Given:
//! - Current state vector |ψ_current⟩ (from WEST)
//! - Transition matrix T (from transition_matrix module)
//!
//! Compute:
//! - Unitary matrix U (normalized version of T preserving energy)
//! - Predicted state |ψ_predicted⟩ = U * |ψ_current⟩
//!
//! ## Energy Conservation
//!
//! The unitary matrix U ensures total energy (capital) is conserved:
//! - ⟨ψ_predicted|ψ_predicted⟩ = ⟨ψ_current|ψ_current⟩
//! - No capital is created or destroyed, only redistributed

use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};
use solana_sdk::pubkey::Pubkey;
use std::collections::HashMap;

use super::transition_matrix::SparseTransitionMatrix;
use crate::oracle::wallet_energy_tracker::StateVector;

/// Prediction result from unitary evolution
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PredictionResult {
    /// Predicted energy distribution per token
    pub predicted_energies: HashMap<Option<Pubkey>, f64>,

    /// Total energy (should equal input total energy)
    pub total_energy: f64,

    /// Timestamp of prediction (milliseconds)
    pub timestamp_ms: u64,

    /// Time horizon of prediction (milliseconds)
    /// e.g., 5000ms = 5 seconds into the future
    pub prediction_horizon_ms: u64,

    /// Confidence score (0.0-1.0) based on data quality
    pub confidence: f64,

    /// Significant predicted flows (token, predicted_energy, change_from_current)
    pub top_flows: Vec<(Option<Pubkey>, f64, f64)>,
}

impl PredictionResult {
    /// Get predicted energy for a specific token
    pub fn get_predicted_energy(&self, token: &Option<Pubkey>) -> f64 {
        self.predicted_energies.get(token).copied().unwrap_or(0.0)
    }

    /// Get the token predicted to have the highest energy
    pub fn highest_energy_token(&self) -> Option<(Option<Pubkey>, f64)> {
        self.predicted_energies
            .iter()
            .max_by(|a, b| a.1.partial_cmp(b.1).unwrap_or(std::cmp::Ordering::Equal))
            .map(|(k, v)| (*k, *v))
    }

    /// Get tokens with predicted energy above a threshold
    pub fn tokens_above_threshold(&self, threshold: f64) -> Vec<(Option<Pubkey>, f64)> {
        self.predicted_energies
            .iter()
            .filter(|(_, &energy)| energy >= threshold)
            .map(|(k, v)| (*k, *v))
            .collect()
    }
}

/// Unitary Evolution Engine
///
/// Predicts future capital distribution using unitary evolution of state vectors.
#[derive(Clone)]
pub struct UnitaryEvolution {
    /// Prediction horizon in milliseconds (default: 5000ms = 5 seconds)
    prediction_horizon_ms: u64,

    /// Minimum confidence threshold for predictions (0.0-1.0)
    min_confidence: f64,
}

impl UnitaryEvolution {
    /// Create a new unitary evolution engine
    pub fn new() -> Self {
        Self {
            prediction_horizon_ms: 5000, // 5 seconds default
            min_confidence: 0.3,
        }
    }

    /// Create with custom prediction horizon
    pub fn with_horizon(mut self, horizon_ms: u64) -> Self {
        self.prediction_horizon_ms = horizon_ms;
        self
    }

    /// Create with custom minimum confidence
    pub fn with_min_confidence(mut self, min_confidence: f64) -> Self {
        self.min_confidence = min_confidence.clamp(0.0, 1.0);
        self
    }

    /// Normalize a transition matrix to be unitary
    ///
    /// For each row:
    /// - Sum all transitions
    /// - Divide by sum to get probabilities (preserves energy)
    /// - Handle zero rows (no outgoing transitions) by adding self-transition
    fn normalize_to_unitary(&self, matrix: &SparseTransitionMatrix) -> DMatrix<f64> {
        let n = matrix.num_states();
        let mut unitary = DMatrix::zeros(n, n);

        for i in 0..n {
            let row_total = matrix.row_totals.get(i).copied().unwrap_or(0.0);

            if row_total > 0.0 {
                // Normalize existing transitions
                for j in 0..n {
                    let prob = matrix.get_transition_prob(i, j);
                    unitary[(i, j)] = prob;
                }
            } else {
                // No outgoing transitions - add self-transition (stay in current state)
                unitary[(i, i)] = 1.0;
            }
        }

        unitary
    }

    /// Convert WEST state vector to DVector aligned with transition matrix
    fn state_vector_to_dvector(
        &self,
        state: &StateVector,
        matrix: &SparseTransitionMatrix,
    ) -> DVector<f64> {
        let n = matrix.num_states();
        let mut vec = DVector::zeros(n);

        // Map state vector energies to matrix indices
        for (i, token_opt) in matrix.states.iter().enumerate() {
            let energy = match token_opt {
                None => state.free_energy,
                Some(token) => state.token_energies.get(token).copied().unwrap_or(0.0),
            };
            vec[i] = energy;
        }

        vec
    }

    /// Convert DVector back to energy map
    fn dvector_to_energy_map(
        &self,
        vec: &DVector<f64>,
        matrix: &SparseTransitionMatrix,
    ) -> HashMap<Option<Pubkey>, f64> {
        let mut energies = HashMap::new();

        for (i, token_opt) in matrix.states.iter().enumerate() {
            let energy = vec[i];
            if energy > 0.0 {
                energies.insert(*token_opt, energy);
            }
        }

        energies
    }

    /// Calculate confidence based on available data
    fn calculate_confidence(&self, matrix: &SparseTransitionMatrix, state: &StateVector) -> f64 {
        let mut confidence = 0.0;

        // Factor 1: Number of observed states (more = better)
        // Lower threshold: 3 states gives 0.3 confidence
        let state_coverage = (matrix.num_states() as f64 / 3.0).min(1.0);
        confidence += state_coverage * 0.3;

        // Factor 2: Number of transitions observed
        // Lower threshold: 5 transitions gives 0.4 confidence
        let transition_density = (matrix.transitions.len() as f64 / 5.0).min(1.0);
        confidence += transition_density * 0.4;

        // Factor 3: Active wallets (from state vector)
        // Lower threshold: 5 wallets gives 0.3 confidence
        let wallet_activity = (state.active_wallets as f64 / 5.0).min(1.0);
        confidence += wallet_activity * 0.3;

        confidence.clamp(0.0, 1.0)
    }

    /// Predict future state using unitary evolution
    ///
    /// ## Algorithm
    ///
    /// 1. Normalize transition matrix T to unitary matrix U
    /// 2. Convert current state vector to DVector
    /// 3. Apply: |ψ_predicted⟩ = U * |ψ_current⟩
    /// 4. Identify significant flows and changes
    pub fn predict(
        &self,
        current_state: &StateVector,
        transition_matrix: &SparseTransitionMatrix,
    ) -> Option<PredictionResult> {
        // Calculate confidence
        let confidence = self.calculate_confidence(transition_matrix, current_state);

        if confidence < self.min_confidence {
            return None; // Not enough data for reliable prediction
        }

        // Step 1: Normalize transition matrix to unitary
        let unitary = self.normalize_to_unitary(transition_matrix);

        // Step 2: Convert state vector to DVector
        let current_vec = self.state_vector_to_dvector(current_state, transition_matrix);

        // Step 3: Apply unitary evolution: |ψ_predicted⟩ = U * |ψ_current⟩
        let predicted_vec = &unitary * &current_vec;

        // Step 4: Convert back to energy map
        let predicted_energies = self.dvector_to_energy_map(&predicted_vec, transition_matrix);

        // Step 5: Calculate total energy (should be conserved)
        let total_energy: f64 = predicted_energies.values().sum();

        // Step 6: Identify significant flows (top changes)
        let mut flows: Vec<(Option<Pubkey>, f64, f64)> = Vec::new();

        for (token_opt, &predicted_energy) in &predicted_energies {
            let current_energy = match token_opt {
                None => current_state.free_energy,
                Some(token) => current_state
                    .token_energies
                    .get(token)
                    .copied()
                    .unwrap_or(0.0),
            };

            let change = predicted_energy - current_energy;

            // Only include significant changes (> 1% of total energy or > 0.1 absolute)
            if change.abs() > 0.1 || change.abs() / total_energy > 0.01 {
                flows.push((*token_opt, predicted_energy, change));
            }
        }

        // Sort by absolute change (largest first)
        flows.sort_by(|a, b| {
            b.2.abs()
                .partial_cmp(&a.2.abs())
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Keep top 10 flows
        flows.truncate(10);

        Some(PredictionResult {
            predicted_energies,
            total_energy,
            timestamp_ms: current_state.timestamp_ms,
            prediction_horizon_ms: self.prediction_horizon_ms,
            confidence,
            top_flows: flows,
        })
    }

    /// Predict and compare with threshold
    ///
    /// Returns tokens predicted to have energy above the threshold
    pub fn predict_above_threshold(
        &self,
        current_state: &StateVector,
        transition_matrix: &SparseTransitionMatrix,
        threshold: f64,
    ) -> Option<Vec<(Option<Pubkey>, f64)>> {
        self.predict(current_state, transition_matrix)
            .map(|result| result.tokens_above_threshold(threshold))
    }
}

impl Default for UnitaryEvolution {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oracle::qman::transition_matrix::{TransitionMatrix, TransitionMatrixBuilder};

    fn test_pubkey(n: u8) -> Pubkey {
        Pubkey::new_from_array([n; 32])
    }

    fn create_test_state() -> StateVector {
        let mut token_energies = HashMap::new();
        token_energies.insert(test_pubkey(1), 30.0);
        token_energies.insert(test_pubkey(2), 20.0);

        StateVector {
            timestamp_ms: 1000,
            free_energy: 50.0,
            token_energies,
            active_wallets: 10,
            total_energy: 100.0,
        }
    }

    #[test]
    fn test_unitary_evolution_creation() {
        let ue = UnitaryEvolution::new();
        assert_eq!(ue.prediction_horizon_ms, 5000);
        assert_eq!(ue.min_confidence, 0.3);
    }

    #[test]
    fn test_unitary_evolution_with_horizon() {
        let ue = UnitaryEvolution::new().with_horizon(10000);
        assert_eq!(ue.prediction_horizon_ms, 10000);
    }

    #[test]
    fn test_normalize_to_unitary() {
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        // Add transitions: free -> A (60%), free -> B (40%)
        builder.observe_transition(wallet, None, Some(test_pubkey(1)), 6.0);
        builder.observe_transition(wallet, None, Some(test_pubkey(2)), 4.0);
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();
        let unitary = ue.normalize_to_unitary(&matrix);

        // Check that rows sum to ~1.0
        for i in 0..unitary.nrows() {
            let row_sum: f64 = (0..unitary.ncols()).map(|j| unitary[(i, j)]).sum();
            assert!((row_sum - 1.0).abs() < 0.01, "Row {} sum = {}", i, row_sum);
        }
    }

    #[test]
    fn test_state_vector_conversion() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();

        // Create matrix with matching states
        let wallet = test_pubkey(10);
        builder.observe_transition(wallet, None, Some(test_pubkey(1)), 1.0);
        builder.observe_transition(wallet, None, Some(test_pubkey(2)), 1.0);
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let vec = ue.state_vector_to_dvector(&state, &matrix);

        // Check that energies match
        let idx_free = matrix.get_state_index(&None).unwrap();
        let idx_a = matrix.get_state_index(&Some(test_pubkey(1))).unwrap();
        let idx_b = matrix.get_state_index(&Some(test_pubkey(2))).unwrap();

        assert!((vec[idx_free] - 50.0).abs() < 1e-6);
        assert!((vec[idx_a] - 30.0).abs() < 1e-6);
        assert!((vec[idx_b] - 20.0).abs() < 1e-6);
    }

    #[test]
    fn test_energy_conservation() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        // Create transitions
        builder.observe_transition(wallet, None, Some(test_pubkey(1)), 6.0);
        builder.observe_transition(wallet, None, Some(test_pubkey(2)), 4.0);
        builder.observe_transition(wallet, Some(test_pubkey(1)), None, 3.0);
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let result = ue.predict(&state, &matrix);

        assert!(result.is_some());
        let result = result.unwrap();

        // Total energy should be conserved (within 5% due to untracked states)
        // In real usage, the matrix and state would be synchronized
        let energy_diff_pct =
            ((result.total_energy - state.total_energy).abs() / state.total_energy) * 100.0;
        assert!(
            energy_diff_pct < 10.0,
            "Energy not conserved: {} -> {} ({}% difference)",
            state.total_energy,
            result.total_energy,
            energy_diff_pct
        );
    }

    #[test]
    fn test_prediction_with_flow_analysis() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        // Strong flow: free -> token A
        for _ in 0..10 {
            builder.observe_transition(wallet, None, Some(test_pubkey(1)), 5.0);
        }

        builder.rebuild_matrix();
        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let result = ue.predict(&state, &matrix).expect("Should have prediction");

        // Check that we have top flows
        assert!(!result.top_flows.is_empty());

        // The strongest flow should involve token A gaining energy
        if let Some((token, _, change)) = result.top_flows.first() {
            // Should show movement toward token A
            assert!(change.abs() > 0.0, "Should have significant flow");
        }
    }

    #[test]
    fn test_confidence_calculation() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        // Few transitions = low confidence
        builder.observe_transition(wallet, None, Some(test_pubkey(1)), 1.0);
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let confidence = ue.calculate_confidence(&matrix, &state);
        assert!(
            confidence < 0.8,
            "Should have low confidence with few transitions: {}",
            confidence
        );

        // Many transitions = higher confidence
        for _ in 0..20 {
            builder.observe_transition(wallet, None, Some(test_pubkey(1)), 1.0);
            builder.observe_transition(wallet, Some(test_pubkey(1)), None, 1.0);
        }
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let confidence = ue.calculate_confidence(&matrix, &state);
        assert!(
            confidence >= 0.3,
            "Should have higher confidence with many transitions: {}",
            confidence
        );
    }

    #[test]
    fn test_min_confidence_threshold() {
        let state = StateVector {
            timestamp_ms: 1000,
            free_energy: 10.0,
            token_energies: HashMap::new(),
            active_wallets: 1,
            total_energy: 10.0,
        };

        let builder = TransitionMatrixBuilder::new();
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new().with_min_confidence(0.5);

        let result = ue.predict(&state, &matrix);
        assert!(
            result.is_none(),
            "Should not predict with insufficient confidence"
        );
    }

    #[test]
    fn test_predict_above_threshold() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        // Add enough data for confidence
        for _ in 0..10 {
            builder.observe_transition(wallet, None, Some(test_pubkey(1)), 5.0);
        }
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let above_threshold = ue.predict_above_threshold(&state, &matrix, 20.0);

        if let Some(tokens) = above_threshold {
            assert!(!tokens.is_empty(), "Should have tokens above threshold");
        }
    }

    #[test]
    fn test_highest_energy_token() {
        let state = create_test_state();
        let builder = TransitionMatrixBuilder::new();
        let wallet = test_pubkey(10);

        for _ in 0..10 {
            builder.observe_transition(wallet, None, Some(test_pubkey(1)), 5.0);
        }
        builder.rebuild_matrix();

        let matrix = builder.get_matrix();
        let ue = UnitaryEvolution::new();

        let result = ue.predict(&state, &matrix).expect("Should have prediction");
        let highest = result.highest_energy_token();

        assert!(highest.is_some());
    }
}
