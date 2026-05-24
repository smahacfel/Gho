use ghost_core::features::coordination::{
    cv, diversity_from_hhi_norm, kendall_tau_b, mad, median, normalized_hhi_from_counts, robust_cv,
    weighted_mad, weighted_median,
};

#[test]
fn hhi_empty_is_none() {
    assert_eq!(normalized_hhi_from_counts(&[]), None);
}

#[test]
fn hhi_single_sample_is_none() {
    assert_eq!(normalized_hhi_from_counts(&[1]), None);
}

#[test]
fn hhi_all_same_bucket_is_one() {
    assert_approx_eq(normalized_hhi_from_counts(&[5]).unwrap(), 1.0);
}

#[test]
fn hhi_all_unique_is_zero() {
    assert_approx_eq(normalized_hhi_from_counts(&[1, 1, 1, 1, 1]).unwrap(), 0.0);
}

#[test]
fn hhi_four_one_uses_sample_count_denominator() {
    assert_approx_eq(normalized_hhi_from_counts(&[4, 1]).unwrap(), 0.60);
}

#[test]
fn diversity_inverts_normalized_hhi_with_clamp() {
    assert_approx_eq(diversity_from_hhi_norm(0.60), 0.40);
    assert_eq!(diversity_from_hhi_norm(-1.0), 1.0);
    assert_eq!(diversity_from_hhi_norm(2.0), 0.0);
}

#[test]
fn median_handles_even_and_odd_finite_samples() {
    let mut odd = [3.0, 1.0, 2.0];
    assert_eq!(median(&mut odd), Some(2.0));

    let mut even = [10.0, 2.0, 4.0, 8.0];
    assert_eq!(median(&mut even), Some(6.0));
}

#[test]
fn median_rejects_empty_or_non_finite_samples() {
    assert_eq!(median(&mut []), None);

    let mut with_nan = [1.0, f64::NAN, 3.0];
    assert_eq!(median(&mut with_nan), None);
}

#[test]
fn mad_and_robust_cv_resist_single_outlier_cluster() {
    let values = [50_000.0, 50_000.0, 50_000.0, 50_000.0, 100_000.0];

    assert_eq!(mad(&values), Some(0.0));
    assert!(cv(&values).unwrap() > 0.30);
    assert_approx_eq(robust_cv(&values).unwrap(), 0.0);
}

#[test]
fn cv_and_robust_cv_return_none_when_undefined() {
    assert_eq!(cv(&[1.0]), None);
    assert_eq!(cv(&[-1.0, 1.0]), None);
    assert_eq!(robust_cv(&[1.0]), None);
    assert_eq!(robust_cv(&[-1.0, 0.0, 1.0]), None);
}

#[test]
fn weighted_median_and_weighted_mad_use_positive_finite_weights() {
    let values = [(10.0, 1.0), (20.0, 2.0), (30.0, 1.0)];

    assert_eq!(weighted_median(&values), Some(20.0));
    assert_eq!(weighted_mad(&values), Some(0.0));
    assert_eq!(weighted_median(&[(1.0, 0.0), (2.0, 0.0)]), None);
    assert_eq!(weighted_median(&[(1.0, -1.0)]), None);
    assert_eq!(weighted_median(&[(f64::INFINITY, 1.0)]), None);
}

#[test]
fn tau_b_len_less_than_three_is_none() {
    assert_eq!(kendall_tau_b(&[1.0, 2.0], &[1.0, 2.0]), None);
}

#[test]
fn tau_b_all_x_ties_is_none() {
    assert_eq!(kendall_tau_b(&[1.0, 1.0, 1.0], &[1.0, 2.0, 3.0]), None);
}

#[test]
fn tau_b_all_y_ties_is_none() {
    assert_eq!(kendall_tau_b(&[1.0, 2.0, 3.0], &[1.0, 1.0, 1.0]), None);
}

#[test]
fn tau_b_perfect_positive_monotonic_is_one() {
    assert_approx_eq(
        kendall_tau_b(&[1.0, 2.0, 3.0, 4.0], &[10.0, 20.0, 30.0, 40.0]).unwrap(),
        1.0,
    );
}

#[test]
fn tau_b_perfect_negative_monotonic_is_minus_one() {
    assert_approx_eq(
        kendall_tau_b(&[1.0, 2.0, 3.0, 4.0], &[40.0, 30.0, 20.0, 10.0]).unwrap(),
        -1.0,
    );
}

#[test]
fn tau_b_ties_in_y_remain_defined_when_denominator_is_nonzero() {
    let tau = kendall_tau_b(&[1.0, 2.0, 3.0, 4.0], &[1.0, 1.0, 2.0, 3.0]).unwrap();

    assert!(tau > 0.90);
    assert!(tau < 1.0);
}

#[test]
fn tau_b_same_slot_like_repeated_values_never_panic() {
    let result = kendall_tau_b(&[1.0, 2.0, 3.0, 4.0], &[10.0, 10.0, 10.0, 11.0]);

    assert!(result.is_some());
}

#[test]
fn tau_b_rejects_mismatched_or_non_finite_inputs() {
    assert_eq!(kendall_tau_b(&[1.0, 2.0, 3.0], &[1.0, 2.0]), None);
    assert_eq!(kendall_tau_b(&[1.0, f64::NAN, 3.0], &[1.0, 2.0, 3.0]), None);
}

fn assert_approx_eq(actual: f64, expected: f64) {
    assert!(
        (actual - expected).abs() < 1e-12,
        "actual={actual}, expected={expected}"
    );
}
