use std::cmp::Ordering;

#[must_use]
pub fn normalized_hhi_from_counts(counts: &[u8]) -> Option<f64> {
    let sample_n: u64 = counts.iter().map(|&count| u64::from(count)).sum();
    if counts.is_empty() || sample_n < 2 {
        return None;
    }

    let sample_n_f = sample_n as f64;
    let hhi: f64 = counts
        .iter()
        .filter(|&&count| count > 0)
        .map(|&count| {
            let p = f64::from(count) / sample_n_f;
            p * p
        })
        .sum();

    let min_hhi = 1.0 / sample_n_f;
    let denom = 1.0 - min_hhi;
    if denom <= 0.0 {
        return None;
    }

    Some(((hhi - min_hhi) / denom).clamp(0.0, 1.0))
}

#[must_use]
pub fn diversity_from_hhi_norm(hhi_norm: f64) -> f64 {
    (1.0 - hhi_norm).clamp(0.0, 1.0)
}

pub fn median(values: &mut [f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    values.sort_by(|left, right| left.total_cmp(right));

    let n = values.len();
    if n % 2 == 1 {
        Some(values[n / 2])
    } else {
        Some((values[n / 2 - 1] + values[n / 2]) / 2.0)
    }
}

pub fn mad(values: &[f64]) -> Option<f64> {
    if values.is_empty() || values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mut copy = values.to_vec();
    let med = median(&mut copy)?;

    let mut deviations: Vec<f64> = values.iter().map(|value| (value - med).abs()).collect();
    median(&mut deviations)
}

pub fn weighted_median(values: &[(f64, f64)]) -> Option<f64> {
    if values.is_empty()
        || values
            .iter()
            .any(|(value, weight)| !value.is_finite() || !weight.is_finite() || *weight < 0.0)
    {
        return None;
    }

    let total_weight: f64 = values.iter().map(|(_, weight)| *weight).sum();
    if total_weight <= 0.0 || !total_weight.is_finite() {
        return None;
    }

    let mut sorted = values.to_vec();
    sorted.sort_by(|left, right| left.0.total_cmp(&right.0));

    let mut cumulative = 0.0;
    let half = total_weight / 2.0;
    for (value, weight) in sorted {
        cumulative += weight;
        if cumulative >= half {
            return Some(value);
        }
    }

    None
}

pub fn weighted_mad(values: &[(f64, f64)]) -> Option<f64> {
    let med = weighted_median(values)?;
    let deviations: Vec<(f64, f64)> = values
        .iter()
        .map(|(value, weight)| ((value - med).abs(), *weight))
        .collect();

    weighted_median(&deviations)
}

pub fn kendall_tau_b(xs: &[f64], ys: &[f64]) -> Option<f64> {
    if xs.len() != ys.len() || xs.len() < 3 {
        return None;
    }

    if xs.iter().chain(ys.iter()).any(|value| !value.is_finite()) {
        return None;
    }

    let mut concordant: f64 = 0.0;
    let mut discordant: f64 = 0.0;
    let mut ties_x: f64 = 0.0;
    let mut ties_y: f64 = 0.0;

    for i in 0..xs.len() {
        for j in (i + 1)..xs.len() {
            let dx = xs[i].partial_cmp(&xs[j])?;
            let dy = ys[i].partial_cmp(&ys[j])?;

            match (dx, dy) {
                (Ordering::Equal, Ordering::Equal) => {}
                (Ordering::Equal, _) => ties_x += 1.0,
                (_, Ordering::Equal) => ties_y += 1.0,
                (Ordering::Less, Ordering::Less) | (Ordering::Greater, Ordering::Greater) => {
                    concordant += 1.0;
                }
                _ => discordant += 1.0,
            }
        }
    }

    let numerator = concordant - discordant;
    let denom_x = concordant + discordant + ties_x;
    let denom_y = concordant + discordant + ties_y;
    if denom_x <= 0.0 || denom_y <= 0.0 {
        return None;
    }

    let denom = (denom_x * denom_y).sqrt();
    if denom <= 0.0 || !denom.is_finite() {
        return None;
    }

    Some((numerator / denom).clamp(-1.0, 1.0))
}

pub fn cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 || values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mean = values.iter().sum::<f64>() / values.len() as f64;
    if mean.abs() < f64::EPSILON || !mean.is_finite() {
        return None;
    }

    let variance = values
        .iter()
        .map(|value| {
            let delta = value - mean;
            delta * delta
        })
        .sum::<f64>()
        / values.len() as f64;
    if !variance.is_finite() {
        return None;
    }

    let result = variance.sqrt() / mean.abs();
    result.is_finite().then_some(result)
}

pub fn robust_cv(values: &[f64]) -> Option<f64> {
    if values.len() < 2 || values.iter().any(|value| !value.is_finite()) {
        return None;
    }

    let mut copy = values.to_vec();
    let med = median(&mut copy)?;
    if med.abs() < f64::EPSILON {
        return None;
    }

    let mad_value = mad(values)?;
    let result = 1.4826 * mad_value / med.abs();
    result.is_finite().then_some(result)
}
