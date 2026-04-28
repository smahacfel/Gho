//! Parameter Fitting
//!
//! Ridge regression for calibrating QEDD model parameters (α, β, γ, δ).

use crate::calibration::dataset::DataPoint;
use crate::config::qedd_config::QeddConfig;
use anyhow::{Context, Result};
use nalgebra::{DMatrix, DVector};
use serde::{Deserialize, Serialize};

/// Result from calibration
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalibrationResult {
    /// Fitted alpha (SOBP drop coefficient)
    pub alpha_sobp_drop: f32,

    /// Fitted beta (outflow coefficient)
    pub beta_outflow: f32,

    /// Fitted gamma (resonance risk coefficient)
    pub gamma_resonance: f32,

    /// Fitted delta (deviation risk coefficient)
    pub delta_deviation: f32,

    /// Fitted lambda base
    pub lambda_base: f32,

    /// Training R² score
    pub train_r2: f32,

    /// Test R² score (if test set provided)
    pub test_r2: Option<f32>,

    /// Mean squared error on training set
    pub train_mse: f32,

    /// Mean squared error on test set (if provided)
    pub test_mse: Option<f32>,

    /// Number of training samples
    pub n_train: usize,

    /// Number of test samples
    pub n_test: Option<usize>,
}

impl CalibrationResult {
    /// Create a QeddConfig from calibration results
    pub fn to_qedd_config(&self) -> QeddConfig {
        let mut config = QeddConfig::default();
        config.alpha_sobp_drop = self.alpha_sobp_drop;
        config.beta_outflow = self.beta_outflow;
        config.gamma_resonance = self.gamma_resonance;
        config.delta_deviation = self.delta_deviation;
        config.lambda_base = self.lambda_base;
        config
    }
}

/// Ridge regression calibrator for QEDD parameters
pub struct RidgeCalibrator {
    /// Ridge regularization parameter (lambda)
    alpha: f64,

    /// Train/test split ratio (0.0 to 1.0)
    train_ratio: f64,
}

impl RidgeCalibrator {
    /// Create a new ridge calibrator
    pub fn new(alpha: f64, train_ratio: f64) -> Self {
        Self { alpha, train_ratio }
    }

    /// Create with default parameters
    pub fn default() -> Self {
        Self {
            alpha: 0.1,       // Small regularization
            train_ratio: 0.8, // 80% train, 20% test
        }
    }

    /// Fit QEDD parameters using ridge regression
    ///
    /// Formula: λ(t) = λ_base + α * SOBP_drop + β * outflow + γ * resonance_risk + δ * dev_risk
    ///
    /// We fit the parameters to predict actual lambda values that best explain survival outcomes.
    pub fn fit(&self, data: &[DataPoint]) -> Result<CalibrationResult> {
        if data.is_empty() {
            anyhow::bail!("Cannot fit with empty dataset");
        }

        // Split data into train/test sets
        let split_idx = (data.len() as f64 * self.train_ratio) as usize;
        let (train_data, test_data) = data.split_at(split_idx);

        if train_data.is_empty() {
            anyhow::bail!("Training set is empty");
        }

        // Build feature matrix X and target vector y for training
        let (x_train, y_train) = self.build_matrices(train_data)?;

        // Fit ridge regression: w = (X^T X + α I)^-1 X^T y
        let coefficients = self.ridge_fit(&x_train, &y_train)?;

        // Extract parameters
        let lambda_base = coefficients[0] as f32;
        let alpha_sobp_drop = coefficients[1] as f32;
        let beta_outflow = coefficients[2] as f32;
        let gamma_resonance = coefficients[3] as f32;
        let delta_deviation = coefficients[4] as f32;

        // Compute training metrics
        let y_pred_train = &x_train * &coefficients;
        let train_r2 = self.compute_r2(&y_train, &y_pred_train);
        let train_mse = self.compute_mse(&y_train, &y_pred_train);

        // Compute test metrics if test data available
        let (test_r2, test_mse, n_test) = if !test_data.is_empty() {
            let (x_test, y_test) = self.build_matrices(test_data)?;
            let y_pred_test = &x_test * &coefficients;
            let test_r2 = self.compute_r2(&y_test, &y_pred_test);
            let test_mse = self.compute_mse(&y_test, &y_pred_test);
            (
                Some(test_r2 as f32),
                Some(test_mse as f32),
                Some(test_data.len()),
            )
        } else {
            (None, None, None)
        };

        Ok(CalibrationResult {
            alpha_sobp_drop,
            beta_outflow,
            gamma_resonance,
            delta_deviation,
            lambda_base,
            train_r2: train_r2 as f32,
            test_r2,
            train_mse: train_mse as f32,
            test_mse,
            n_train: train_data.len(),
            n_test,
        })
    }

    /// Build feature matrix X and target vector y from data points
    ///
    /// X = [1, sobp_drop, outflow, resonance_risk, deviation_risk] (n × 5 matrix)
    /// y = computed lambda from survival probabilities (n × 1 vector)
    fn build_matrices(&self, data: &[DataPoint]) -> Result<(DMatrix<f64>, DVector<f64>)> {
        // Constants for lambda estimation from survival outcomes
        const LAMBDA_SURVIVED: f64 = 0.5; // Low hazard rate for tokens that survived
        const LAMBDA_RUGGED: f64 = 1.5; // High hazard rate for tokens that rugged
        const LAMBDA_UNKNOWN: f64 = 0.8; // Moderate hazard rate when outcome unknown

        let n = data.len();
        let mut x_data = vec![0.0; n * 5];
        let mut y_data = Vec::with_capacity(n);

        for (i, point) in data.iter().enumerate() {
            // Feature vector: [1, sobp_drop, outflow, resonance_risk, deviation_risk]
            x_data[i * 5 + 0] = 1.0; // Intercept (lambda_base)
            x_data[i * 5 + 1] = point.signals.sobp.drop;
            x_data[i * 5 + 2] = point.signals.flow.outflow;
            x_data[i * 5 + 3] = point.signals.resonance.risk;
            x_data[i * 5 + 4] = point.signals.deviation.risk;

            // Target: derive lambda from survival probability if available
            // Using S(1s) = exp(-λ * 1) => λ = -ln(S(1s))
            // If survival not available, we'll need to compute it from price changes
            let lambda = if let Some(survived) = point.survived_1s {
                if survived {
                    LAMBDA_SURVIVED // Token survived - low lambda
                } else {
                    LAMBDA_RUGGED // Token rugged - high lambda
                }
            } else {
                LAMBDA_UNKNOWN // No ground truth, use placeholder
            };

            y_data.push(lambda);
        }

        let x = DMatrix::from_row_slice(n, 5, &x_data);
        let y = DVector::from_vec(y_data);

        Ok((x, y))
    }

    /// Perform ridge regression: w = (X^T X + α I)^-1 X^T y
    fn ridge_fit(&self, x: &DMatrix<f64>, y: &DVector<f64>) -> Result<DVector<f64>> {
        let xt = x.transpose();
        let xtx = &xt * x;

        // Add ridge penalty: α I
        let identity = DMatrix::identity(xtx.nrows(), xtx.ncols());
        let xtx_ridge = xtx + identity * self.alpha;

        // Solve: (X^T X + α I) w = X^T y
        let xty = &xt * y;

        let decomp = xtx_ridge.lu();
        let w = decomp
            .solve(&xty)
            .context("Failed to solve ridge regression system")?;

        Ok(w)
    }

    /// Compute R² score
    fn compute_r2(&self, y_true: &DVector<f64>, y_pred: &DVector<f64>) -> f64 {
        let mean = y_true.mean();
        let ss_tot: f64 = y_true.iter().map(|&y| (y - mean).powi(2)).sum();
        let ss_res: f64 = y_true
            .iter()
            .zip(y_pred.iter())
            .map(|(&yt, &yp)| (yt - yp).powi(2))
            .sum();

        if ss_tot == 0.0 {
            return 0.0;
        }

        1.0 - (ss_res / ss_tot)
    }

    /// Compute mean squared error
    fn compute_mse(&self, y_true: &DVector<f64>, y_pred: &DVector<f64>) -> f64 {
        let n = y_true.len() as f64;
        let ss_res: f64 = y_true
            .iter()
            .zip(y_pred.iter())
            .map(|(&yt, &yp)| (yt - yp).powi(2))
            .sum();
        ss_res / n
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::signals::MarketSignals;

    #[test]
    fn test_calibrator_creation() {
        let calibrator = RidgeCalibrator::default();
        assert_eq!(calibrator.alpha, 0.1);
        assert_eq!(calibrator.train_ratio, 0.8);
    }

    #[test]
    fn test_fit_with_mock_data() {
        let calibrator = RidgeCalibrator::default();

        // Create mock data
        let mut data = Vec::new();
        for i in 0..100 {
            let mut dp = DataPoint::new(i as u64, MarketSignals::mock());
            dp.survived_1s = Some(i % 2 == 0); // Alternate survival
            data.push(dp);
        }

        let result = calibrator.fit(&data);
        assert!(result.is_ok());

        let result = result.unwrap();
        // R² can be negative if the model performs worse than a horizontal line
        // We just check that it's finite and MSE is non-negative
        assert!(result.train_r2.is_finite());
        assert!(result.train_mse >= 0.0);
    }
}
