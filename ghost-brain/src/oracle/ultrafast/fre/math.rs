//! Fractal Resonance Engine - Core Math Module
//!
//! Provides ultra-fast, zero-allocation implementations for key fractal metrics:
//! - Hurst exponent via R/S analysis
//! - Fractal roughness approximation
//! - Scale coherence normalizer
//! - Welford's online variance for streaming statistics

const SCALE_COHERENCE_MAX_STD: f64 = 0.25;

#[derive(Debug, Clone, Copy)]
pub struct WelfordVariance {
    count: u64,
    mean: f64,
    m2: f64,
}

impl WelfordVariance {
    pub const fn new() -> Self {
        Self {
            count: 0,
            mean: 0.0,
            m2: 0.0,
        }
    }

    #[inline]
    pub fn update(&mut self, value: f64) {
        self.count += 1;
        let delta = value - self.mean;
        self.mean += delta / self.count as f64;
        let delta2 = value - self.mean;
        self.m2 += delta * delta2;
    }

    #[inline]
    pub fn mean(&self) -> Option<f64> {
        (self.count > 0).then_some(self.mean)
    }

    #[inline]
    pub fn variance(&self) -> Option<f64> {
        if self.count > 1 {
            Some(self.m2 / (self.count - 1) as f64)
        } else {
            None
        }
    }

    #[inline]
    pub fn std_dev(&self) -> Option<f64> {
        self.variance().map(f64::sqrt)
    }
}

pub struct FractalMath;

impl FractalMath {
    /// Calculates the Hurst exponent using Rescaled Range (R/S) analysis.
    /// Returns None for insufficient data or degenerate inputs.
    pub fn calculate_rs_hurst(series: &[f64]) -> Option<f64> {
        let n = series.len();
        if n < 2 {
            return None;
        }

        let mut stats = WelfordVariance::new();
        for &value in series {
            stats.update(value);
        }
        let mean = stats.mean()?;
        let variance = stats.variance()?;
        if variance <= f64::EPSILON {
            return None;
        }

        let mut cumulative = 0.0f64;
        let mut min_cumulative = 0.0f64;
        let mut max_cumulative = 0.0f64;

        for &value in series {
            let deviation = value - mean;
            cumulative += deviation;
            if cumulative > max_cumulative {
                max_cumulative = cumulative;
            }
            if cumulative < min_cumulative {
                min_cumulative = cumulative;
            }
        }

        let range = max_cumulative - min_cumulative;
        if range <= f64::EPSILON {
            return None;
        }

        let rs = range / variance.sqrt();
        if rs <= f64::EPSILON {
            return None;
        }

        let hurst = (rs.ln()) / (n as f64).ln();
        Some(hurst.clamp(0.0, 1.0))
    }

    /// Approximates fractal roughness using sliding-window variance ratios.
    /// Returns None for insufficient data.
    pub fn calculate_roughness(series: &[f64]) -> Option<f64> {
        let n = series.len();
        if n < 2 {
            return None;
        }

        let mut global_stats = WelfordVariance::new();
        for &value in series {
            global_stats.update(value);
        }

        let global_var = global_stats.variance().unwrap_or(0.0);
        if global_var <= f64::EPSILON {
            return Some(0.0);
        }

        let window = n.min(8);
        let mut sum = 0.0;
        let mut sum_sq = 0.0;
        for &value in &series[..window] {
            sum += value;
            sum_sq += value * value;
        }

        let mut window_count = 1usize;
        let mut total_var = window_variance(sum, sum_sq, window);

        if window < n {
            for i in window..n {
                let entering = series[i];
                let leaving = series[i - window];
                sum += entering - leaving;
                sum_sq += entering * entering - leaving * leaving;
                total_var += window_variance(sum, sum_sq, window);
                window_count += 1;
            }
        }

        let avg_local_var = total_var / window_count as f64;
        let roughness = (avg_local_var.sqrt() / global_var.sqrt()).clamp(0.0, 1.0);
        Some(roughness)
    }

    /// Calculates scale coherence for multiple Hurst variants.
    pub fn calculate_scale_coherence(hurst_variants: &[f64]) -> Option<f64> {
        if hurst_variants.is_empty() {
            return None;
        }

        let mut stats = WelfordVariance::new();
        for &value in hurst_variants {
            stats.update(value);
        }

        let std_dev = stats.std_dev().unwrap_or(0.0);
        if SCALE_COHERENCE_MAX_STD <= f64::EPSILON {
            return None;
        }

        Some((1.0 - std_dev / SCALE_COHERENCE_MAX_STD).clamp(0.0, 1.0))
    }
}

#[inline]
fn window_variance(sum: f64, sum_sq: f64, len: usize) -> f64 {
    if len < 2 {
        return 0.0;
    }
    let mean = sum / len as f64;
    let variance = (sum_sq / len as f64) - mean * mean;
    if variance.is_sign_negative() {
        0.0
    } else {
        variance
    }
}

#[cfg(test)]
mod tests {
    use super::{FractalMath, WelfordVariance};
    use rand::rngs::StdRng;
    use rand::{Rng, SeedableRng};

    #[test]
    fn test_welford_accuracy() {
        let samples = [1.0, 2.0, 3.0, 4.0, 5.0];
        let mut welford = WelfordVariance::new();
        for &v in &samples {
            welford.update(v);
        }

        let classical_mean = samples.iter().sum::<f64>() / samples.len() as f64;
        let classical_var = samples
            .iter()
            .map(|v| (v - classical_mean).powi(2))
            .sum::<f64>()
            / (samples.len() as f64 - 1.0);

        let welford_var = welford.variance().unwrap();
        assert!((classical_var - welford_var).abs() < 1e-6);
    }

    #[test]
    fn test_hurst_random_walk() {
        let mut rng = StdRng::seed_from_u64(42);
        let mut series = Vec::with_capacity(512);
        let mut value = 0.0;
        for _ in 0..512 {
            let step = rng.gen_range(-1.0..1.0);
            value += step;
            series.push(value);
        }

        let hurst = FractalMath::calculate_rs_hurst(&series).unwrap();
        assert!((0.35..0.65).contains(&hurst), "hurst={}", hurst);
    }

    #[test]
    fn test_hurst_trending() {
        let mut series = Vec::with_capacity(256);
        let mut current = 0.0;
        for i in 0..256 {
            current += 0.05 + (i as f64 * 1e-4);
            series.push(current);
        }

        let hurst = FractalMath::calculate_rs_hurst(&series).unwrap();
        assert!(hurst > 0.65, "hurst={}", hurst);
    }

    #[test]
    fn test_hurst_mean_reverting() {
        let mut series = Vec::with_capacity(256);
        for i in 0..256 {
            let value = if i % 2 == 0 { 1.0 } else { -1.0 };
            series.push(value);
        }

        let hurst = FractalMath::calculate_rs_hurst(&series).unwrap();
        assert!(hurst < 0.35, "hurst={}", hurst);
    }
}
