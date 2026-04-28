//! Report Generation
//!
//! Generate calibration reports with plots and metrics.

use crate::calibration::dataset::DataPoint;
use crate::calibration::fitting::CalibrationResult;
use crate::config::qedd_config::QeddConfig;
use crate::qedd::QeddEngine;
use anyhow::Result;
use serde::{Deserialize, Serialize};

/// Calibration report with metrics and diagnostics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationReport {
    /// Calibration result
    pub calibration: CalibrationResult,

    /// AUC (Area Under Curve) metrics for survival predictions
    pub auc_1s: Option<f32>,
    pub auc_5s: Option<f32>,
    pub auc_30s: Option<f32>,
    pub auc_60s: Option<f32>,

    /// Residual statistics
    pub residuals_mean: f32,
    pub residuals_std: f32,
    pub residuals_min: f32,
    pub residuals_max: f32,

    /// Summary statistics
    pub n_total_samples: usize,
    pub n_survived: usize,
    pub n_rugged: usize,

    /// Timestamp when report was generated
    pub generated_at: String,
}

impl CalibrationReport {
    /// Generate a text summary of the report
    pub fn to_text_summary(&self) -> String {
        let mut summary = String::new();

        summary.push_str("=== QEDD Calibration Report ===\n\n");
        summary.push_str(&format!("Generated: {}\n\n", self.generated_at));

        summary.push_str("## Fitted Parameters\n");
        summary.push_str(&format!(
            "  λ_base (lambda_base):     {:.4}\n",
            self.calibration.lambda_base
        ));
        summary.push_str(&format!(
            "  α (alpha_sobp_drop):      {:.4}\n",
            self.calibration.alpha_sobp_drop
        ));
        summary.push_str(&format!(
            "  β (beta_outflow):         {:.4}\n",
            self.calibration.beta_outflow
        ));
        summary.push_str(&format!(
            "  γ (gamma_resonance):      {:.4}\n",
            self.calibration.gamma_resonance
        ));
        summary.push_str(&format!(
            "  δ (delta_deviation):      {:.4}\n\n",
            self.calibration.delta_deviation
        ));

        summary.push_str("## Model Performance\n");
        summary.push_str(&format!(
            "  Training R²:              {:.4}\n",
            self.calibration.train_r2
        ));
        summary.push_str(&format!(
            "  Training MSE:             {:.4}\n",
            self.calibration.train_mse
        ));
        if let Some(test_r2) = self.calibration.test_r2 {
            summary.push_str(&format!("  Test R²:                  {:.4}\n", test_r2));
        }
        if let Some(test_mse) = self.calibration.test_mse {
            summary.push_str(&format!("  Test MSE:                 {:.4}\n", test_mse));
        }
        summary.push_str("\n");

        summary.push_str("## Survival Prediction AUC\n");
        if let Some(auc) = self.auc_1s {
            summary.push_str(&format!("  1s horizon:               {:.4}\n", auc));
        }
        if let Some(auc) = self.auc_5s {
            summary.push_str(&format!("  5s horizon:               {:.4}\n", auc));
        }
        if let Some(auc) = self.auc_30s {
            summary.push_str(&format!("  30s horizon:              {:.4}\n", auc));
        }
        if let Some(auc) = self.auc_60s {
            summary.push_str(&format!("  60s horizon:              {:.4}\n", auc));
        }
        summary.push_str("\n");

        summary.push_str("## Residuals Statistics\n");
        summary.push_str(&format!(
            "  Mean:                     {:.4}\n",
            self.residuals_mean
        ));
        summary.push_str(&format!(
            "  Std Dev:                  {:.4}\n",
            self.residuals_std
        ));
        summary.push_str(&format!(
            "  Min:                      {:.4}\n",
            self.residuals_min
        ));
        summary.push_str(&format!(
            "  Max:                      {:.4}\n\n",
            self.residuals_max
        ));

        summary.push_str("## Dataset Summary\n");
        summary.push_str(&format!(
            "  Total samples:            {}\n",
            self.n_total_samples
        ));
        summary.push_str(&format!(
            "  Survived:                 {} ({:.1}%)\n",
            self.n_survived,
            (self.n_survived as f32 / self.n_total_samples as f32) * 100.0
        ));
        summary.push_str(&format!(
            "  Rugged:                   {} ({:.1}%)\n",
            self.n_rugged,
            (self.n_rugged as f32 / self.n_total_samples as f32) * 100.0
        ));

        summary
    }
}

/// Report generator
pub struct ReportGenerator;

impl ReportGenerator {
    /// Generate a full calibration report
    pub fn generate(
        calibration: CalibrationResult,
        data: &[DataPoint],
        config: &QeddConfig,
    ) -> Result<CalibrationReport> {
        // Compute AUC metrics for each horizon
        let auc_1s = Self::compute_auc(data, config, |dp| dp.survived_1s, 0);
        let auc_5s = Self::compute_auc(data, config, |dp| dp.survived_5s, 1);
        let auc_30s = Self::compute_auc(data, config, |dp| dp.survived_30s, 2);
        let auc_60s = Self::compute_auc(data, config, |dp| dp.survived_60s, 3);

        // Compute residual statistics
        let residuals = Self::compute_residuals(data, config);
        let (residuals_mean, residuals_std, residuals_min, residuals_max) =
            Self::residual_stats(&residuals);

        // Count survival outcomes
        // Only count as survived if explicitly true; None or false are not counted as survived
        let n_survived = data
            .iter()
            .filter(|dp| dp.survived_1s == Some(true))
            .count();
        // Count as rugged only if explicitly false
        let n_rugged = data
            .iter()
            .filter(|dp| dp.survived_1s == Some(false))
            .count();

        let generated_at = chrono::Utc::now().to_rfc3339();

        Ok(CalibrationReport {
            calibration,
            auc_1s,
            auc_5s,
            auc_30s,
            auc_60s,
            residuals_mean,
            residuals_std,
            residuals_min,
            residuals_max,
            n_total_samples: data.len(),
            n_survived,
            n_rugged,
            generated_at,
        })
    }

    /// Compute AUC (Area Under ROC Curve) for survival predictions
    fn compute_auc<F>(
        data: &[DataPoint],
        config: &QeddConfig,
        get_survived: F,
        horizon_idx: usize,
    ) -> Option<f32>
    where
        F: Fn(&DataPoint) -> Option<bool>,
    {
        let engine = QeddEngine::new(config.clone());

        // Collect (predicted_survival, actual_survival) pairs
        let mut pairs: Vec<(f32, bool)> = data
            .iter()
            .filter_map(|dp| {
                let survived = get_survived(dp)?;
                let result = engine.compute_qedd_sync(&dp.signals);

                // Use appropriate survival probability for the horizon
                let predicted_survival = match horizon_idx {
                    0 => result.survival_1s,
                    1 => result.survival_5s,
                    2 => result.survival_30s,
                    3 => result.survival_60s,
                    _ => result.survival_1s,
                };

                Some((predicted_survival, survived))
            })
            .collect();

        if pairs.is_empty() {
            return None;
        }

        // Sort by predicted survival (descending)
        pairs.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));

        // Compute AUC using trapezoidal rule
        let n_pos = pairs.iter().filter(|(_, survived)| *survived).count() as f32;
        let n_neg = pairs.len() as f32 - n_pos;

        if n_pos == 0.0 || n_neg == 0.0 {
            return None;
        }

        let mut tp = 0.0;
        let mut fp = 0.0;
        let mut auc = 0.0;
        let mut prev_fp = 0.0;

        for (_, survived) in pairs.iter() {
            if *survived {
                tp += 1.0;
                // Add area of trapezoid
                auc += (fp - prev_fp) * tp / n_pos;
                prev_fp = fp;
            } else {
                fp += 1.0;
            }
        }

        Some((auc / n_neg) as f32)
    }

    /// Compute residuals (predicted lambda - actual lambda derived from survival)
    fn compute_residuals(data: &[DataPoint], config: &QeddConfig) -> Vec<f32> {
        let engine = QeddEngine::new(config.clone());

        data.iter()
            .filter_map(|dp| {
                let result = engine.compute_qedd_sync(&dp.signals);
                let predicted_lambda = result.lambda_now;

                // Derive actual lambda from survival if available
                let actual_lambda = if let Some(survived) = dp.survived_1s {
                    if survived {
                        0.5
                    } else {
                        1.5
                    }
                } else {
                    return None;
                };

                Some(predicted_lambda - actual_lambda)
            })
            .collect()
    }

    /// Compute residual statistics
    fn residual_stats(residuals: &[f32]) -> (f32, f32, f32, f32) {
        if residuals.is_empty() {
            return (0.0, 0.0, 0.0, 0.0);
        }

        let mean = residuals.iter().sum::<f32>() / residuals.len() as f32;
        let variance =
            residuals.iter().map(|&r| (r - mean).powi(2)).sum::<f32>() / residuals.len() as f32;
        let std = variance.sqrt();
        let min = residuals.iter().copied().fold(f32::INFINITY, f32::min);
        let max = residuals.iter().copied().fold(f32::NEG_INFINITY, f32::max);

        (mean, std, min, max)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::MarketSignals;

    #[test]
    fn test_report_generation() {
        let calibration = CalibrationResult {
            alpha_sobp_drop: 0.3,
            beta_outflow: 0.25,
            gamma_resonance: 0.15,
            delta_deviation: 0.20,
            lambda_base: 0.5,
            train_r2: 0.85,
            test_r2: Some(0.80),
            train_mse: 0.05,
            test_mse: Some(0.06),
            n_train: 80,
            n_test: Some(20),
        };

        let mut data = Vec::new();
        for i in 0..10 {
            let mut dp = DataPoint::new(i, MarketSignals::mock());
            dp.survived_1s = Some(i % 2 == 0);
            data.push(dp);
        }

        let config = calibration.to_qedd_config();
        let report = ReportGenerator::generate(calibration, &data, &config);

        assert!(report.is_ok());
        let report = report.unwrap();
        assert_eq!(report.n_total_samples, 10);
    }

    #[test]
    fn test_text_summary() {
        let calibration = CalibrationResult {
            alpha_sobp_drop: 0.3,
            beta_outflow: 0.25,
            gamma_resonance: 0.15,
            delta_deviation: 0.20,
            lambda_base: 0.5,
            train_r2: 0.85,
            test_r2: None,
            train_mse: 0.05,
            test_mse: None,
            n_train: 100,
            n_test: None,
        };

        let report = CalibrationReport {
            calibration,
            auc_1s: Some(0.92),
            auc_5s: Some(0.88),
            auc_30s: Some(0.85),
            auc_60s: Some(0.82),
            residuals_mean: 0.01,
            residuals_std: 0.15,
            residuals_min: -0.5,
            residuals_max: 0.6,
            n_total_samples: 100,
            n_survived: 60,
            n_rugged: 40,
            generated_at: "2024-01-01T00:00:00Z".to_string(),
        };

        let summary = report.to_text_summary();
        assert!(summary.contains("Calibration Report"));
        assert!(summary.contains("0.3000")); // alpha
    }
}
