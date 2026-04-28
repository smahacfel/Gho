//! Feature Normalization
//!
//! This module provides various normalization techniques for features.

use super::extractors::FeatureVector;
use anyhow::Result;
use std::collections::HashMap;

/// Normalization method
#[derive(Debug, Clone, Copy)]
pub enum NormalizationMethod {
    /// Min-max scaling to [0, 1]
    MinMax,
    /// Z-score normalization (mean=0, std=1)
    ZScore,
    /// Log normalization
    Log,
    /// Sigmoid normalization
    Sigmoid,
    /// Robust scaling (using median and IQR)
    Robust,
    /// No normalization
    None,
}

/// Feature normalizer with per-feature statistics
pub struct FeatureNormalizer {
    /// Statistics for each feature: (min, max, mean, std, median, iqr)
    feature_stats: HashMap<String, FeatureStats>,
    /// Default normalization method
    default_method: NormalizationMethod,
}

#[derive(Debug, Clone)]
pub struct FeatureStats {
    pub min: f64,
    pub max: f64,
    pub mean: f64,
    pub std: f64,
    pub median: f64,
    pub iqr: f64,
}

impl FeatureNormalizer {
    /// Create a new normalizer with default method
    pub fn new(default_method: NormalizationMethod) -> Self {
        Self {
            feature_stats: HashMap::new(),
            default_method,
        }
    }

    /// Fit normalizer to training data
    pub fn fit(&mut self, feature_vectors: &[FeatureVector]) -> Result<()> {
        if feature_vectors.is_empty() {
            return Ok(());
        }

        // Collect all feature names
        let mut all_features: std::collections::HashSet<String> = std::collections::HashSet::new();
        for fv in feature_vectors {
            all_features.extend(fv.keys().cloned());
        }

        // Calculate statistics for each feature
        for feature_name in all_features {
            let mut values: Vec<f64> = feature_vectors
                .iter()
                .filter_map(|fv| fv.get(&feature_name).copied())
                .collect();

            if values.is_empty() {
                continue;
            }

            values.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));

            let min = *values.first().unwrap();
            let max = *values.last().unwrap();
            let mean = values.iter().sum::<f64>() / values.len() as f64;

            let variance =
                values.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / values.len() as f64;
            let std = variance.sqrt();

            let median = if values.len() % 2 == 0 {
                (values[values.len() / 2 - 1] + values[values.len() / 2]) / 2.0
            } else {
                values[values.len() / 2]
            };

            let q1 = values[values.len() / 4];
            let q3 = values[3 * values.len() / 4];
            let iqr = q3 - q1;

            self.feature_stats.insert(
                feature_name,
                FeatureStats {
                    min,
                    max,
                    mean,
                    std,
                    median,
                    iqr,
                },
            );
        }

        Ok(())
    }

    /// Normalize a feature vector
    pub fn normalize(&self, features: &FeatureVector) -> Result<FeatureVector> {
        let mut normalized = HashMap::new();

        for (name, &value) in features {
            let normalized_value = if let Some(stats) = self.feature_stats.get(name) {
                self.normalize_value(value, stats, self.default_method)
            } else {
                // No stats available, use default normalization
                self.normalize_value_default(value, self.default_method)
            };

            normalized.insert(name.clone(), normalized_value);
        }

        Ok(normalized)
    }

    /// Normalize a single value using statistics
    fn normalize_value(
        &self,
        value: f64,
        stats: &FeatureStats,
        method: NormalizationMethod,
    ) -> f64 {
        match method {
            NormalizationMethod::MinMax => {
                if (stats.max - stats.min).abs() < 1e-10 {
                    0.5
                } else {
                    ((value - stats.min) / (stats.max - stats.min)).clamp(0.0, 1.0)
                }
            }
            NormalizationMethod::ZScore => {
                if stats.std < 1e-10 {
                    0.0
                } else {
                    (value - stats.mean) / stats.std
                }
            }
            NormalizationMethod::Log => {
                if value > 0.0 {
                    (value.ln() - stats.min.max(1e-10).ln())
                        / (stats.max.max(1e-10).ln() - stats.min.max(1e-10).ln())
                } else {
                    0.0
                }
            }
            NormalizationMethod::Sigmoid => 1.0 / (1.0 + (-value).exp()),
            NormalizationMethod::Robust => {
                if stats.iqr < 1e-10 {
                    0.5
                } else {
                    (value - stats.median) / stats.iqr
                }
            }
            NormalizationMethod::None => value,
        }
    }

    /// Normalize value without statistics (fallback)
    fn normalize_value_default(&self, value: f64, method: NormalizationMethod) -> f64 {
        match method {
            NormalizationMethod::Sigmoid => 1.0 / (1.0 + (-value).exp()),
            NormalizationMethod::Log => {
                if value > 0.0 {
                    value.ln().max(0.0)
                } else {
                    0.0
                }
            }
            _ => value.clamp(0.0, 1.0),
        }
    }

    /// Get statistics for a feature
    pub fn get_stats(&self, feature_name: &str) -> Option<&FeatureStats> {
        self.feature_stats.get(feature_name)
    }
}

impl Default for FeatureNormalizer {
    fn default() -> Self {
        Self::new(NormalizationMethod::MinMax)
    }
}

/// Batch normalization utilities
pub struct BatchNormalizer;

impl BatchNormalizer {
    /// Normalize a batch of feature vectors (faster than individual normalization)
    pub fn normalize_batch(
        features: &[FeatureVector],
        method: NormalizationMethod,
    ) -> Result<Vec<FeatureVector>> {
        let mut normalizer = FeatureNormalizer::new(method);
        normalizer.fit(features)?;

        features.iter().map(|fv| normalizer.normalize(fv)).collect()
    }

    /// Normalize specific features in a vector
    pub fn normalize_features(
        features: &FeatureVector,
        feature_names: &[String],
        method: NormalizationMethod,
    ) -> Result<FeatureVector> {
        let mut normalized = features.clone();

        for name in feature_names {
            if let Some(&value) = features.get(name) {
                let normalized_value = match method {
                    NormalizationMethod::Sigmoid => 1.0 / (1.0 + (-value).exp()),
                    NormalizationMethod::Log => {
                        if value > 0.0 {
                            value.ln()
                        } else {
                            0.0
                        }
                    }
                    NormalizationMethod::MinMax => value.clamp(0.0, 1.0),
                    _ => value,
                };
                normalized.insert(name.clone(), normalized_value);
            }
        }

        Ok(normalized)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_minmax_normalization() {
        let mut normalizer = FeatureNormalizer::new(NormalizationMethod::MinMax);

        let training_data = vec![
            vec![
                ("feature1".to_string(), 10.0),
                ("feature2".to_string(), 100.0),
            ]
            .into_iter()
            .collect(),
            vec![
                ("feature1".to_string(), 20.0),
                ("feature2".to_string(), 200.0),
            ]
            .into_iter()
            .collect(),
            vec![
                ("feature1".to_string(), 30.0),
                ("feature2".to_string(), 300.0),
            ]
            .into_iter()
            .collect(),
        ];

        normalizer.fit(&training_data).unwrap();

        let test_data: FeatureVector = vec![
            ("feature1".to_string(), 15.0),
            ("feature2".to_string(), 150.0),
        ]
        .into_iter()
        .collect();

        let normalized = normalizer.normalize(&test_data).unwrap();

        // Value 15 should be normalized to 0.25 in range [10, 30]
        assert!((normalized.get("feature1").unwrap() - 0.25).abs() < 0.01);
        // Value 150 should be normalized to 0.25 in range [100, 300]
        assert!((normalized.get("feature2").unwrap() - 0.25).abs() < 0.01);
    }

    #[test]
    fn test_zscore_normalization() {
        let mut normalizer = FeatureNormalizer::new(NormalizationMethod::ZScore);

        let training_data = vec![
            vec![("feature1".to_string(), 10.0)].into_iter().collect(),
            vec![("feature1".to_string(), 20.0)].into_iter().collect(),
            vec![("feature1".to_string(), 30.0)].into_iter().collect(),
        ];

        normalizer.fit(&training_data).unwrap();

        let test_data: FeatureVector = vec![("feature1".to_string(), 20.0)].into_iter().collect();

        let normalized = normalizer.normalize(&test_data).unwrap();

        // Mean is 20, so normalized value should be close to 0
        assert!(normalized.get("feature1").unwrap().abs() < 0.01);
    }

    #[test]
    fn test_sigmoid_normalization() {
        let normalizer = FeatureNormalizer::new(NormalizationMethod::Sigmoid);

        let test_data: FeatureVector = vec![
            ("feature1".to_string(), 0.0),
            ("feature2".to_string(), 1.0),
            ("feature3".to_string(), -1.0),
        ]
        .into_iter()
        .collect();

        let normalized = normalizer.normalize(&test_data).unwrap();

        // sigmoid(0) = 0.5
        assert!((normalized.get("feature1").unwrap() - 0.5).abs() < 0.01);
        // sigmoid(1) ≈ 0.73
        assert!(*normalized.get("feature2").unwrap() > 0.7);
        // sigmoid(-1) ≈ 0.27
        assert!(*normalized.get("feature3").unwrap() < 0.3);
    }

    #[test]
    fn test_batch_normalization() {
        let features = vec![
            vec![("a".to_string(), 1.0), ("b".to_string(), 10.0)]
                .into_iter()
                .collect(),
            vec![("a".to_string(), 2.0), ("b".to_string(), 20.0)]
                .into_iter()
                .collect(),
            vec![("a".to_string(), 3.0), ("b".to_string(), 30.0)]
                .into_iter()
                .collect(),
        ];

        let normalized =
            BatchNormalizer::normalize_batch(&features, NormalizationMethod::MinMax).unwrap();

        assert_eq!(normalized.len(), 3);
        // First element should have minimum values normalized to 0
        assert!(*normalized[0].get("a").unwrap() < 0.1);
    }

    #[test]
    fn test_normalizer_with_no_data() {
        let normalizer = FeatureNormalizer::new(NormalizationMethod::MinMax);

        let test_data: FeatureVector = vec![("feature1".to_string(), 0.5)].into_iter().collect();

        let normalized = normalizer.normalize(&test_data).unwrap();

        // Should use default normalization
        assert!(normalized.contains_key("feature1"));
    }
}
