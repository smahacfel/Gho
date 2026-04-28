//! Feature Selection and Importance Ranking
//!
//! This module provides tools for selecting the most important features
//! and ranking features by their predictive power.

use super::extractors::FeatureVector;
use anyhow::Result;
use std::collections::HashMap;

/// Feature importance score
pub type ImportanceScore = f64;

/// Feature importance ranking
#[derive(Debug, Clone)]
pub struct FeatureImportance {
    /// Feature name to importance score mapping
    pub scores: HashMap<String, ImportanceScore>,
}

impl FeatureImportance {
    /// Create new feature importance ranking
    pub fn new() -> Self {
        Self {
            scores: HashMap::new(),
        }
    }

    /// Set importance score for a feature
    pub fn set_importance(&mut self, feature: String, score: ImportanceScore) {
        self.scores.insert(feature, score);
    }

    /// Get importance score for a feature
    pub fn get_importance(&self, feature: &str) -> Option<ImportanceScore> {
        self.scores.get(feature).copied()
    }

    /// Get top N most important features
    pub fn top_features(&self, n: usize) -> Vec<(String, ImportanceScore)> {
        let mut features: Vec<_> = self.scores.iter().map(|(k, v)| (k.clone(), *v)).collect();
        features.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));
        features.into_iter().take(n).collect()
    }

    /// Get features above a certain importance threshold
    pub fn features_above_threshold(&self, threshold: ImportanceScore) -> Vec<String> {
        self.scores
            .iter()
            .filter(|(_, &score)| score >= threshold)
            .map(|(name, _)| name.clone())
            .collect()
    }

    /// Normalize importance scores to [0, 1]
    pub fn normalize(&mut self) {
        let max_score = self
            .scores
            .values()
            .max_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal))
            .copied()
            .unwrap_or(1.0);

        if max_score > 0.0 {
            for score in self.scores.values_mut() {
                *score /= max_score;
            }
        }
    }
}

impl Default for FeatureImportance {
    fn default() -> Self {
        Self::new()
    }
}

/// Feature selection methods
pub enum SelectionMethod {
    /// Select top K features by importance
    TopK(usize),
    /// Select features above threshold
    Threshold(f64),
    /// Select features by percentile
    Percentile(f64),
    /// Select all features
    All,
}

/// Feature selector
pub struct FeatureSelector {
    importance: FeatureImportance,
}

impl FeatureSelector {
    /// Create new feature selector with importance rankings
    pub fn new(importance: FeatureImportance) -> Self {
        Self { importance }
    }

    /// Select features based on selection method
    pub fn select(&self, method: SelectionMethod) -> Vec<String> {
        match method {
            SelectionMethod::TopK(k) => self
                .importance
                .top_features(k)
                .into_iter()
                .map(|(name, _)| name)
                .collect(),
            SelectionMethod::Threshold(threshold) => {
                self.importance.features_above_threshold(threshold)
            }
            SelectionMethod::Percentile(percentile) => {
                let scores: Vec<f64> = self.importance.scores.values().copied().collect();
                let threshold = self.calculate_percentile(&scores, percentile);
                self.importance.features_above_threshold(threshold)
            }
            SelectionMethod::All => self.importance.scores.keys().cloned().collect(),
        }
    }

    /// Filter feature vector to only include selected features
    pub fn filter_features(
        &self,
        features: &FeatureVector,
        method: SelectionMethod,
    ) -> FeatureVector {
        let selected = self.select(method);
        features
            .iter()
            .filter(|(name, _)| selected.contains(name))
            .map(|(k, v)| (k.clone(), *v))
            .collect()
    }

    /// Calculate percentile of a sorted list of values
    fn calculate_percentile(&self, values: &[f64], percentile: f64) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mut sorted = values.to_vec();
        sorted.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

        let index = ((percentile / 100.0) * (sorted.len() - 1) as f64).round() as usize;
        sorted[index.min(sorted.len() - 1)]
    }
}

/// Correlation-based feature selection
pub struct CorrelationSelector;

impl CorrelationSelector {
    /// Calculate correlation between two feature vectors
    pub fn correlation(x: &[f64], y: &[f64]) -> f64 {
        if x.len() != y.len() || x.is_empty() {
            return 0.0;
        }

        let n = x.len() as f64;
        let mean_x = x.iter().sum::<f64>() / n;
        let mean_y = y.iter().sum::<f64>() / n;

        let cov: f64 = x
            .iter()
            .zip(y.iter())
            .map(|(xi, yi)| (xi - mean_x) * (yi - mean_y))
            .sum();

        let var_x: f64 = x.iter().map(|xi| (xi - mean_x).powi(2)).sum();
        let var_y: f64 = y.iter().map(|yi| (yi - mean_y).powi(2)).sum();

        if var_x == 0.0 || var_y == 0.0 {
            return 0.0;
        }

        cov / (var_x.sqrt() * var_y.sqrt())
    }

    /// Remove highly correlated features (keep one from each correlated pair)
    pub fn remove_correlated_features(
        feature_vectors: &[FeatureVector],
        threshold: f64,
    ) -> Result<Vec<String>> {
        if feature_vectors.is_empty() {
            return Ok(vec![]);
        }

        // Collect all feature names
        let feature_names: Vec<String> = feature_vectors[0].keys().cloned().collect();
        let mut selected_features = vec![];
        let mut removed_features = std::collections::HashSet::new();

        for (i, feature1) in feature_names.iter().enumerate() {
            if removed_features.contains(feature1) {
                continue;
            }

            selected_features.push(feature1.clone());

            // Check correlation with other features
            for feature2 in feature_names.iter().skip(i + 1) {
                if removed_features.contains(feature2) {
                    continue;
                }

                // Collect values for both features
                let values1: Vec<f64> = feature_vectors
                    .iter()
                    .filter_map(|fv| fv.get(feature1).copied())
                    .collect();

                let values2: Vec<f64> = feature_vectors
                    .iter()
                    .filter_map(|fv| fv.get(feature2).copied())
                    .collect();

                if values1.len() == values2.len() {
                    let corr = Self::correlation(&values1, &values2).abs();
                    if corr > threshold {
                        removed_features.insert(feature2.clone());
                    }
                }
            }
        }

        Ok(selected_features)
    }
}

/// Variance-based feature selection
pub struct VarianceSelector;

impl VarianceSelector {
    /// Calculate variance of a feature across samples
    pub fn calculate_variance(values: &[f64]) -> f64 {
        if values.is_empty() {
            return 0.0;
        }

        let mean = values.iter().sum::<f64>() / values.len() as f64;
        values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64
    }

    /// Select features with variance above threshold
    pub fn select_by_variance(
        feature_vectors: &[FeatureVector],
        threshold: f64,
    ) -> Result<Vec<String>> {
        if feature_vectors.is_empty() {
            return Ok(vec![]);
        }

        let feature_names: Vec<String> = feature_vectors[0].keys().cloned().collect();
        let mut selected_features = vec![];

        for feature_name in feature_names {
            let values: Vec<f64> = feature_vectors
                .iter()
                .filter_map(|fv| fv.get(&feature_name).copied())
                .collect();

            let variance = Self::calculate_variance(&values);
            if variance > threshold {
                selected_features.push(feature_name);
            }
        }

        Ok(selected_features)
    }
}

/// Mutual information-based feature selection (simplified version)
pub struct MutualInformationSelector;

impl MutualInformationSelector {
    /// Estimate mutual information between feature and target (simplified)
    /// Higher values indicate stronger relationship
    pub fn mutual_information(feature_values: &[f64], target_values: &[f64]) -> f64 {
        if feature_values.len() != target_values.len() || feature_values.is_empty() {
            return 0.0;
        }

        // Simplified MI estimation using correlation as proxy
        CorrelationSelector::correlation(feature_values, target_values).abs()
    }

    /// Rank features by mutual information with target
    pub fn rank_features(
        feature_vectors: &[FeatureVector],
        targets: &[f64],
    ) -> Result<FeatureImportance> {
        let mut importance = FeatureImportance::new();

        if feature_vectors.is_empty() || targets.is_empty() {
            return Ok(importance);
        }

        let feature_names: Vec<String> = feature_vectors[0].keys().cloned().collect();

        for feature_name in feature_names {
            let values: Vec<f64> = feature_vectors
                .iter()
                .filter_map(|fv| fv.get(&feature_name).copied())
                .collect();

            if values.len() == targets.len() {
                let mi = Self::mutual_information(&values, targets);
                importance.set_importance(feature_name, mi);
            }
        }

        importance.normalize();
        Ok(importance)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_feature_importance_ranking() {
        let mut importance = FeatureImportance::new();
        importance.set_importance("feature1".to_string(), 0.9);
        importance.set_importance("feature2".to_string(), 0.5);
        importance.set_importance("feature3".to_string(), 0.7);

        let top2 = importance.top_features(2);
        assert_eq!(top2.len(), 2);
        assert_eq!(top2[0].0, "feature1");
        assert_eq!(top2[1].0, "feature3");
    }

    #[test]
    fn test_feature_selector_topk() {
        let mut importance = FeatureImportance::new();
        importance.set_importance("a".to_string(), 0.9);
        importance.set_importance("b".to_string(), 0.5);
        importance.set_importance("c".to_string(), 0.7);

        let selector = FeatureSelector::new(importance);
        let selected = selector.select(SelectionMethod::TopK(2));

        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"a".to_string()));
        assert!(selected.contains(&"c".to_string()));
    }

    #[test]
    fn test_feature_selector_threshold() {
        let mut importance = FeatureImportance::new();
        importance.set_importance("a".to_string(), 0.9);
        importance.set_importance("b".to_string(), 0.5);
        importance.set_importance("c".to_string(), 0.7);

        let selector = FeatureSelector::new(importance);
        let selected = selector.select(SelectionMethod::Threshold(0.6));

        assert_eq!(selected.len(), 2);
        assert!(selected.contains(&"a".to_string()));
        assert!(selected.contains(&"c".to_string()));
    }

    #[test]
    fn test_correlation_calculation() {
        let x = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let y = vec![2.0, 4.0, 6.0, 8.0, 10.0]; // Perfect positive correlation

        let corr = CorrelationSelector::correlation(&x, &y);
        assert!((corr - 1.0).abs() < 0.01);
    }

    #[test]
    fn test_variance_calculation() {
        let values = vec![1.0, 2.0, 3.0, 4.0, 5.0];
        let variance = VarianceSelector::calculate_variance(&values);

        // Variance of [1,2,3,4,5] is 2.0
        assert!((variance - 2.0).abs() < 0.01);
    }

    #[test]
    fn test_variance_selector() {
        let features = vec![
            vec![("high_var".to_string(), 1.0), ("low_var".to_string(), 5.0)]
                .into_iter()
                .collect(),
            vec![("high_var".to_string(), 10.0), ("low_var".to_string(), 5.1)]
                .into_iter()
                .collect(),
            vec![("high_var".to_string(), 20.0), ("low_var".to_string(), 4.9)]
                .into_iter()
                .collect(),
        ];

        let selected = VarianceSelector::select_by_variance(&features, 1.0).unwrap();

        // high_var should be selected, low_var should not
        assert!(selected.contains(&"high_var".to_string()));
    }

    #[test]
    fn test_filter_features() {
        let mut importance = FeatureImportance::new();
        importance.set_importance("a".to_string(), 0.9);
        importance.set_importance("b".to_string(), 0.5);
        importance.set_importance("c".to_string(), 0.7);

        let selector = FeatureSelector::new(importance);

        let features: FeatureVector = vec![
            ("a".to_string(), 1.0),
            ("b".to_string(), 2.0),
            ("c".to_string(), 3.0),
        ]
        .into_iter()
        .collect();

        let filtered = selector.filter_features(&features, SelectionMethod::TopK(2));

        assert_eq!(filtered.len(), 2);
        assert!(filtered.contains_key("a"));
        assert!(filtered.contains_key("c"));
    }
}
