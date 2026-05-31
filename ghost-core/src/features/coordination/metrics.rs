use crate::features::coordination::config::CoordinationRiskConfig;
use crate::features::coordination::evidence::{
    severity_high, severity_low, CoordinationMetricBreakdowns, CoordinationMetricName,
    CoordinationRiskEvidenceUnit, CoordinationRiskFeatures, CoordinationSnapshotMode,
    DegradedReason, FundingVisibility, MetricEvidenceRecord, MetricEvidenceStatus,
    MetricPolicyMode, MetricValue, SkippedMetric,
};
use crate::features::coordination::samples::{
    sequence_buys, summarize_observed_buy_txs, unique_first_buys_by_signer, SequenceBuildError,
};
use crate::features::coordination::stats::{
    cv, diversity_from_hhi_norm, kendall_tau_b, median, normalized_hhi_from_counts, robust_cv,
};
use crate::features::coordination::types::{
    BseBreakdown, CoordinationSampleSummary, CpvBreakdown, CpvSignerIntensity, CucdBreakdown,
    CucdBucketCount, DbiaBreakdown, DesBreakdown, DevFingerprintEvidence, DevFingerprintMode,
    EconomicSpend, EconomicSpendSource, FeeTopologyCount, FtdiBreakdown, InfraFingerprint,
    ObservedBuyTx, SfdBreakdown, SfdSourceCount, SignerCrossPoolActivity,
};
use crate::tx_intelligence::types::{FscEvidenceStatus, FscSnapshotMode, FscV2Evidence};
use smallvec::{smallvec, SmallVec};
use solana_sdk::pubkey::Pubkey;
use std::collections::{BTreeMap, HashMap};

#[derive(Debug, Clone, PartialEq)]
pub struct MetricComputation<T> {
    pub value: Option<MetricValue>,
    pub evidence: MetricEvidenceRecord<T>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CoordinationRiskEvidenceInput {
    pub schema_version: u16,
    pub scope_id: String,
    pub run_id: Option<String>,
    pub candidate_id: Option<String>,
    pub pool_id: Pubkey,
    pub mint: Pubkey,
    pub decision_id: Option<String>,
    pub decision_ts_ms: u64,
    pub decision_slot: Option<u64>,
    pub snapshot_mode: CoordinationSnapshotMode,
    pub snapshot_available: bool,
    pub feature_cutoff_ts_ms: u64,
    pub feature_cutoff_slot: Option<u64>,
    pub source_buffer_watermark_slot: Option<u64>,
    pub computed_at_recv_ts_ns: u128,
    pub gatekeeper_version: Option<String>,
    pub source_snapshot_hash: Option<String>,
    pub sample_summary: CoordinationSampleSummary,
    pub funding_visibility: FundingVisibility,
    pub features: CoordinationRiskFeatures,
    pub metric_breakdowns: CoordinationMetricBreakdowns,
}

#[derive(Debug, Clone, PartialEq)]
pub struct FrozenCoordinationDecisionSnapshot {
    pub schema_version: u16,
    pub scope_id: String,
    pub run_id: Option<String>,
    pub candidate_id: Option<String>,
    pub pool_id: Pubkey,
    pub mint: Pubkey,
    pub decision_id: Option<String>,
    pub decision_ts_ms: u64,
    pub decision_slot: Option<u64>,
    pub snapshot_mode: CoordinationSnapshotMode,
    pub feature_cutoff_ts_ms: u64,
    pub feature_cutoff_slot: Option<u64>,
    pub source_buffer_watermark_slot: Option<u64>,
    pub computed_at_recv_ts_ns: u128,
    pub gatekeeper_version: Option<String>,
    pub source_snapshot_hash: Option<String>,
    pub txs: SmallVec<[ObservedBuyTx; 32]>,
    pub dev_reference: Option<DevFingerprintEvidence>,
    pub signer_activity: SmallVec<[SignerCrossPoolActivity; 16]>,
    pub rolling_state_ready: bool,
    pub fsc_v2: Option<FscV2Evidence>,
}

pub fn compute_ftdi_v2(
    txs: &[ObservedBuyTx],
    config: &CoordinationRiskConfig,
) -> MetricComputation<FtdiBreakdown> {
    let buyers = unique_first_buys_by_signer(txs);
    let unique_count = buyers.len();
    let mut breakdown = FtdiBreakdown {
        unique_buyer_count: saturating_u8(unique_count),
        ..FtdiBreakdown::default()
    };

    if unique_count < usize::from(config.min_unique_buyers_for_diagnostics) {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientUniqueSigners],
        );
    }

    let mut counts = BTreeMap::new();
    for tx in buyers {
        if let Some(fingerprint) = tx.fee_topology_fp {
            let entry = counts.entry(fingerprint).or_insert(0_u8);
            *entry = entry.saturating_add(1);
        } else {
            breakdown.missing_fingerprint_count =
                breakdown.missing_fingerprint_count.saturating_add(1);
        }
    }

    let available_count: u8 = counts.values().copied().sum();
    breakdown.fingerprint_coverage = coverage(usize::from(available_count), unique_count);
    breakdown.topology_counts = counts
        .into_iter()
        .map(|(fingerprint, count)| FeeTopologyCount { fingerprint, count })
        .collect();

    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    if breakdown.fingerprint_coverage < config.clean_coverage_min() {
        reasons.push(DegradedReason::LowCoverage);
    }
    if available_count < 2 {
        reasons.push(DegradedReason::InsufficientBuys);
    }

    if !reasons.is_empty() {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }

    let count_values: SmallVec<[u8; 16]> = breakdown
        .topology_counts
        .iter()
        .map(|entry| entry.count)
        .collect();
    let Some(hhi_norm) = normalized_hhi_from_counts(&count_values) else {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::DenominatorZero],
        );
    };

    let diversity = diversity_from_hhi_norm(hhi_norm);
    let value = MetricValue::new(
        diversity,
        severity_low(diversity, config.ftdi_low_threshold()),
        breakdown.fingerprint_coverage,
        saturating_u8(unique_count),
        breakdown.fingerprint_coverage,
        MetricEvidenceStatus::Clean,
    );

    clean_metric(value, breakdown)
}

pub fn compute_dbia_v2(
    buyer_samples: &[&ObservedBuyTx],
    dev_reference: Option<DevFingerprintEvidence>,
    config: &CoordinationRiskConfig,
) -> MetricComputation<DbiaBreakdown> {
    let buyers: SmallVec<[&ObservedBuyTx; 16]> = buyer_samples
        .iter()
        .copied()
        .filter(|tx| tx.is_buyer_sample_candidate())
        .collect();
    let mut breakdown = DbiaBreakdown::default();

    if buyers.len() < usize::from(config.min_unique_buyers_for_diagnostics) {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientUniqueSigners],
        );
    }

    let Some(dev_reference) = dev_reference else {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::MissingDevBuy],
        );
    };
    breakdown.dev_mode = dev_reference.mode;

    if dev_reference.mode == DevFingerprintMode::NotComparable {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::DevTxNotComparable],
        );
    }
    if dev_reference.mode == DevFingerprintMode::CreateTxSwapSliceOnly
        && !dev_reference.explicit_swap_slice
    {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::DevTxNotComparable],
        );
    }

    let Some(dev_fingerprint) = dev_reference.fingerprint else {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::MissingDevBuy],
        );
    };

    let mut comparable_count = 0_usize;
    let mut account_role_similarity = 0.0;
    let mut outer_program_similarity = 0.0;
    let mut inner_program_similarity = 0.0;
    let mut compute_budget_similarity = 0.0;
    let mut fee_topology_similarity = 0.0;
    let mut ata_wsol_similarity = 0.0;

    for tx in buyers.iter().copied() {
        let Some(buyer_fingerprint) = infra_fingerprint_from_buy(tx) else {
            continue;
        };

        comparable_count += 1;
        account_role_similarity += exact_match(
            buyer_fingerprint.account_role_pattern_hash,
            dev_fingerprint.account_role_pattern_hash,
        );
        outer_program_similarity += exact_match(
            buyer_fingerprint.outer_program_sequence_hash,
            dev_fingerprint.outer_program_sequence_hash,
        );
        inner_program_similarity += exact_match(
            buyer_fingerprint.inner_program_sequence_hash,
            dev_fingerprint.inner_program_sequence_hash,
        );
        compute_budget_similarity += exact_match(
            buyer_fingerprint.compute_budget_shape,
            dev_fingerprint.compute_budget_shape,
        );
        fee_topology_similarity += exact_match(
            buyer_fingerprint.fee_topology_hash,
            dev_fingerprint.fee_topology_hash,
        );
        ata_wsol_similarity += exact_match(
            buyer_fingerprint.ata_wsol_shape,
            dev_fingerprint.ata_wsol_shape,
        );
    }

    breakdown.buyer_fingerprint_coverage = coverage(comparable_count, buyers.len());
    if breakdown.buyer_fingerprint_coverage < config.clean_coverage_min() {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Degraded,
            smallvec![DegradedReason::LowCoverage],
        );
    }

    if comparable_count == 0 {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::MissingMeta],
        );
    }

    let denom = comparable_count as f64;
    breakdown.account_role_similarity = account_role_similarity / denom;
    breakdown.outer_program_similarity = outer_program_similarity / denom;
    breakdown.inner_program_similarity = inner_program_similarity / denom;
    breakdown.compute_budget_similarity = compute_budget_similarity / denom;
    breakdown.fee_topology_similarity = fee_topology_similarity / denom;
    breakdown.ata_wsol_similarity = ata_wsol_similarity / denom;

    let affinity = (breakdown.account_role_similarity
        + breakdown.outer_program_similarity
        + breakdown.inner_program_similarity
        + breakdown.compute_budget_similarity
        + breakdown.fee_topology_similarity
        + breakdown.ata_wsol_similarity)
        / 6.0;
    let value = MetricValue::new(
        affinity,
        severity_high(affinity, config.dbia_high_threshold()),
        dev_reference_confidence(dev_reference.mode) * breakdown.buyer_fingerprint_coverage,
        saturating_u8(comparable_count),
        breakdown.buyer_fingerprint_coverage,
        MetricEvidenceStatus::Clean,
    );

    clean_metric(value, breakdown)
}

pub fn compute_sfd_v2(
    txs: &[ObservedBuyTx],
    config: &CoordinationRiskConfig,
) -> MetricComputation<SfdBreakdown> {
    let buyers = unique_first_buys_by_signer(txs);
    let unique_count = buyers.len();
    let mut breakdown = SfdBreakdown {
        unique_buyer_count: saturating_u8(unique_count),
        ..SfdBreakdown::default()
    };

    if unique_count < usize::from(config.min_unique_buyers_for_diagnostics) {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientUniqueSigners],
        );
    }

    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    let mut confidence_sum = 0.0;
    let mut source_counts = BTreeMap::<u8, (EconomicSpendSource, u8)>::new();

    for tx in buyers {
        let Some(pre_balance) = tx.pre_balance_signer.filter(|value| *value > 0) else {
            push_unique_reason(&mut reasons, DegradedReason::MissingPrePostBalances);
            continue;
        };

        let Some(spend) = economic_spend_or_fallback(tx, &mut reasons) else {
            continue;
        };
        let fraction = spend.lamports as f64 / pre_balance as f64;
        if !fraction.is_finite() {
            push_unique_reason(&mut reasons, DegradedReason::ZeroOrInvalidMean);
            continue;
        }
        if fraction > config.sfd_sane_max() {
            breakdown.skipped_outlier_count = breakdown.skipped_outlier_count.saturating_add(1);
            push_unique_reason(&mut reasons, DegradedReason::SpendFractionOutOfRange);
            continue;
        }

        breakdown.spend_fractions.push(fraction);
        let confidence = spend.confidence.clamp(0.0, 1.0);
        if breakdown.spend_fractions.len() == 1 {
            breakdown.min_source_confidence = confidence;
        } else {
            breakdown.min_source_confidence = breakdown.min_source_confidence.min(confidence);
        }
        confidence_sum += confidence;

        let key = economic_spend_source_rank(spend.source);
        let entry = source_counts.entry(key).or_insert((spend.source, 0));
        entry.1 = entry.1.saturating_add(1);
    }

    breakdown.spend_coverage = coverage(breakdown.spend_fractions.len(), unique_count);
    breakdown.mean_source_confidence = if breakdown.spend_fractions.is_empty() {
        0.0
    } else {
        confidence_sum / breakdown.spend_fractions.len() as f64
    };
    breakdown.source_counts = source_counts
        .into_values()
        .map(|(source, count)| SfdSourceCount { source, count })
        .collect();

    if breakdown.spend_fractions.len() < usize::from(config.min_unique_buyers_for_diagnostics) {
        push_unique_reason(&mut reasons, DegradedReason::InsufficientBuys);
    }
    if breakdown.spend_coverage < config.economic_spend_coverage_min() {
        push_unique_reason(&mut reasons, DegradedReason::LowCoverage);
    }

    if !reasons.is_empty() && breakdown.spend_fractions.len() < 2 {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }
    if breakdown.spend_coverage < config.economic_spend_coverage_min() {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }

    let values = breakdown.spend_fractions.as_slice();
    let Some(divergence) = robust_cv(values).or_else(|| cv(values)) else {
        push_unique_reason(&mut reasons, DegradedReason::ZeroOrInvalidMean);
        return unavailable_metric(breakdown, MetricEvidenceStatus::Unavailable, reasons);
    };

    let status = if reasons.is_empty() {
        MetricEvidenceStatus::Clean
    } else {
        MetricEvidenceStatus::Degraded
    };
    let mut value = MetricValue::new(
        divergence,
        severity_low(divergence, config.sfd_low_threshold()),
        breakdown.spend_coverage * breakdown.mean_source_confidence,
        saturating_u8(values.len()),
        breakdown.spend_coverage,
        status,
    );
    value.degraded_reasons = reasons.clone();

    metric(value, breakdown, status, reasons)
}

/// Computes cross-pool velocity from a decision-cutoff rolling signer index.
pub fn compute_cpv_v2(
    txs: &[ObservedBuyTx],
    signer_activity: &[SignerCrossPoolActivity],
    rolling_state_ready: bool,
    config: &CoordinationRiskConfig,
) -> MetricComputation<CpvBreakdown> {
    let buyers = unique_first_buys_by_signer(txs);
    let unique_count = buyers.len();
    let mut breakdown = CpvBreakdown {
        unique_buyer_count: saturating_u8(unique_count),
        rolling_state_ready,
        ..CpvBreakdown::default()
    };

    if !rolling_state_ready {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::RollingStateUnavailable],
        );
    }

    if unique_count < usize::from(config.min_unique_buyers_for_diagnostics) {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientUniqueSigners],
        );
    }

    let activity: HashMap<Pubkey, &SignerCrossPoolActivity> = signer_activity
        .iter()
        .map(|entry| (entry.signer, entry))
        .collect();
    let cap = config.cpv_intensity_cap_pools.max(1);
    let mut intensity_sum = 0.0;
    let mut cutoff_proof_count = 0_usize;
    let mut current_pool_excluded_count = 0_usize;
    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();

    for tx in buyers {
        let activity = activity.get(&tx.signer).copied();
        let other_pool_count = activity.map_or(0, |entry| entry.other_pool_count);

        if let Some(activity) = activity {
            if activity.current_pool_excluded {
                current_pool_excluded_count += 1;
            } else {
                push_unique_reason(&mut reasons, DegradedReason::CurrentPoolNotExcluded);
            }

            match (activity.feature_cutoff_slot, activity.observed_until_slot) {
                (Some(cutoff), Some(observed_until)) if observed_until <= cutoff => {
                    cutoff_proof_count += 1;
                }
                (Some(_), Some(_)) => {
                    push_unique_reason(&mut reasons, DegradedReason::ActivityAfterCutoff);
                }
                _ => push_unique_reason(&mut reasons, DegradedReason::MissingFeatureCutoff),
            }
        } else {
            cutoff_proof_count += 1;
            current_pool_excluded_count += 1;
        }

        let intensity = f64::from(other_pool_count.min(cap)) / f64::from(cap);
        intensity_sum += intensity;
        breakdown.signer_intensities.push(CpvSignerIntensity {
            signer: tx.signer,
            other_pool_count,
            intensity,
        });
    }

    breakdown.activity_coverage = 1.0;
    breakdown.cutoff_proof_coverage = coverage(cutoff_proof_count, unique_count);
    breakdown.current_pool_exclusion_coverage = coverage(current_pool_excluded_count, unique_count);
    if breakdown.cutoff_proof_coverage < 1.0 || breakdown.current_pool_exclusion_coverage < 1.0 {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }

    let mean_intensity = intensity_sum / unique_count as f64;
    let value = MetricValue::new(
        mean_intensity,
        severity_high(mean_intensity, config.cpv_high_threshold()),
        breakdown.activity_coverage,
        saturating_u8(unique_count),
        breakdown.activity_coverage,
        MetricEvidenceStatus::Clean,
    );

    metric(
        value,
        breakdown,
        MetricEvidenceStatus::Clean,
        SmallVec::new(),
    )
}

pub fn compute_des_v2(
    txs: &[ObservedBuyTx],
    config: &CoordinationRiskConfig,
) -> MetricComputation<DesBreakdown> {
    let sequence = match sequence_buys(txs) {
        Ok(sequence) => sequence,
        Err(error) => {
            let reason = sequence_error_reason(error);
            return unavailable_metric(
                DesBreakdown::default(),
                MetricEvidenceStatus::Unavailable,
                smallvec![reason],
            );
        }
    };
    let sequence_len = sequence.len();
    let mut breakdown = DesBreakdown {
        sequence_buy_count: saturating_u8(sequence_len),
        ..DesBreakdown::default()
    };

    if sequence_len < 4 {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientBuys],
        );
    }

    breakdown.same_slot_burst_ratio = same_slot_adjacent_ratio(&sequence);
    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    if breakdown.same_slot_burst_ratio >= config.same_slot_dominated_ratio() {
        reasons.push(DegradedReason::SameSlotDominated);
    }

    for window in sequence.windows(2) {
        let current = window[0];
        let next = window[1];
        let Some(impact) = price_impact(current) else {
            push_unique_reason(&mut reasons, DegradedReason::MissingCurveState);
            continue;
        };

        breakdown.impacts.push(impact);
        breakdown
            .gap_after_slots
            .push(next.slot.saturating_sub(current.slot) as f64);
    }
    breakdown.eligible_pairs = saturating_u8(breakdown.impacts.len());

    if breakdown.impacts.len() < 3 {
        push_unique_reason(&mut reasons, DegradedReason::InsufficientBuys);
        return unavailable_metric(breakdown, MetricEvidenceStatus::InsufficientSample, reasons);
    }

    let Some(tau) = kendall_tau_b(&breakdown.impacts, &breakdown.gap_after_slots) else {
        push_tie_reason(&mut reasons, &breakdown.impacts, &breakdown.gap_after_slots);
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    };
    let value_raw = tau.abs();
    let status = if reasons.is_empty() {
        MetricEvidenceStatus::Clean
    } else {
        MetricEvidenceStatus::Degraded
    };
    let mut value = MetricValue::new(
        value_raw,
        severity_high(value_raw, config.des_high_threshold()),
        1.0 - breakdown.same_slot_burst_ratio,
        breakdown.eligible_pairs,
        coverage(
            usize::from(breakdown.eligible_pairs),
            sequence_len.saturating_sub(1),
        ),
        status,
    );
    value.degraded_reasons = reasons.clone();

    metric(value, breakdown, status, reasons)
}

pub fn compute_bse_v2(
    txs: &[ObservedBuyTx],
    config: &CoordinationRiskConfig,
) -> MetricComputation<BseBreakdown> {
    let sequence = match sequence_buys(txs) {
        Ok(sequence) => sequence,
        Err(error) => {
            let reason = sequence_error_reason(error);
            return unavailable_metric(
                BseBreakdown::default(),
                MetricEvidenceStatus::Unavailable,
                smallvec![reason],
            );
        }
    };
    let sequence_len = sequence.len();
    let mut breakdown = BseBreakdown {
        sequence_buy_count: saturating_u8(sequence_len),
        ..BseBreakdown::default()
    };

    if sequence_len < 4 {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientBuys],
        );
    }

    let mut impacts = SmallVec::<[f64; 16]>::new();
    let mut next_spends = SmallVec::<[f64; 16]>::new();
    let mut spend_evidence_count = 0_usize;
    let mut price_evidence_count = 0_usize;

    for window in sequence.windows(2) {
        let current = window[0];
        let next = window[1];

        if next.economic_spent_lamports.is_some() {
            spend_evidence_count += 1;
        }
        if price_impact(current).is_some() {
            price_evidence_count += 1;
        }

        let Some(impact) = price_impact(current) else {
            continue;
        };
        let Some(next_spend) = next.economic_spent_lamports else {
            continue;
        };

        impacts.push(impact);
        next_spends.push(next_spend.lamports as f64);
    }

    let pair_denominator = sequence_len.saturating_sub(1);
    breakdown.economic_spend_coverage = coverage(spend_evidence_count, pair_denominator);
    breakdown.price_evidence_coverage = coverage(price_evidence_count, pair_denominator);
    breakdown.eligible_pairs = saturating_u8(impacts.len());

    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    if breakdown.economic_spend_coverage < config.economic_spend_coverage_min() {
        reasons.push(DegradedReason::LowCoverage);
    }
    if breakdown.price_evidence_coverage < config.price_evidence_coverage_min() {
        push_unique_reason(&mut reasons, DegradedReason::MissingCurveState);
    }
    if impacts.len() < 3 {
        push_unique_reason(&mut reasons, DegradedReason::InsufficientBuys);
    }
    if !reasons.is_empty() {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }

    let Some(tau) = kendall_tau_b(&impacts, &next_spends) else {
        push_tie_reason(&mut reasons, &impacts, &next_spends);
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    };

    let abs_tau = tau.abs();
    breakdown.tau_b_raw = Some(tau);
    breakdown.tau_b_abs = Some(abs_tau);
    let value = MetricValue::new(
        abs_tau,
        severity_high(abs_tau, config.bse_high_threshold()),
        breakdown
            .economic_spend_coverage
            .min(breakdown.price_evidence_coverage),
        breakdown.eligible_pairs,
        breakdown
            .economic_spend_coverage
            .min(breakdown.price_evidence_coverage),
        MetricEvidenceStatus::Clean,
    );

    clean_metric(value, breakdown)
}

pub fn compute_cucd_v2(
    txs: &[ObservedBuyTx],
    config: &CoordinationRiskConfig,
) -> MetricComputation<CucdBreakdown> {
    let buyers = unique_first_buys_by_signer(txs);
    let unique_count = buyers.len();
    let mut breakdown = CucdBreakdown {
        unique_buyer_count: saturating_u8(unique_count),
        ..CucdBreakdown::default()
    };

    if unique_count < usize::from(config.min_unique_buyers_for_diagnostics) {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::InsufficientSample,
            smallvec![DegradedReason::InsufficientUniqueSigners],
        );
    }

    let mut values = SmallVec::<[f64; 16]>::new();
    let mut raw_values = SmallVec::<[u64; 16]>::new();
    let mut buckets = BTreeMap::<u64, u8>::new();
    let bucket_size = config.cucd_bucket_size.max(1);

    for tx in buyers {
        let Some(compute_units) = tx.compute_units_consumed else {
            continue;
        };

        values.push(compute_units as f64);
        raw_values.push(compute_units);
        let bucket = compute_units / bucket_size;
        let entry = buckets.entry(bucket).or_insert(0);
        *entry = entry.saturating_add(1);
    }

    breakdown.compute_unit_coverage = coverage(values.len(), unique_count);
    breakdown.bucket_counts = buckets
        .iter()
        .map(|(bucket, count)| CucdBucketCount {
            bucket: *bucket,
            count: *count,
        })
        .collect();
    breakdown.min_cu = raw_values.iter().copied().min();
    breakdown.max_cu = raw_values.iter().copied().max();

    if !values.is_empty() {
        let mut median_values = values.to_vec();
        breakdown.median_cu = median(&mut median_values).map(|value| value.round() as u64);
    }
    breakdown.cv = cv(&values);
    breakdown.robust_cv = robust_cv(&values);

    let count_values: SmallVec<[u8; 16]> = breakdown
        .bucket_counts
        .iter()
        .map(|entry| entry.count)
        .collect();
    breakdown.cu_bucket_hhi_norm = normalized_hhi_from_counts(&count_values);
    breakdown.dominant_bucket_share = breakdown
        .bucket_counts
        .iter()
        .map(|entry| entry.count)
        .max()
        .map(|count| f64::from(count) / values.len() as f64);

    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    if breakdown.compute_unit_coverage < config.compute_unit_coverage_min() {
        reasons.push(DegradedReason::LowCoverage);
    }
    if values.len() < usize::from(config.min_unique_buyers_for_diagnostics) {
        push_unique_reason(&mut reasons, DegradedReason::MissingComputeUnits);
    }
    if !reasons.is_empty() {
        return unavailable_metric(breakdown, MetricEvidenceStatus::Degraded, reasons);
    }

    let Some(value_raw) = breakdown.robust_cv else {
        return unavailable_metric(
            breakdown,
            MetricEvidenceStatus::Unavailable,
            smallvec![DegradedReason::DenominatorZero],
        );
    };
    let value = MetricValue::new(
        value_raw,
        severity_low(value_raw, config.cucd_low_threshold()),
        breakdown.compute_unit_coverage,
        saturating_u8(values.len()),
        breakdown.compute_unit_coverage,
        MetricEvidenceStatus::Clean,
    );

    clean_metric(value, breakdown)
}

pub fn build_coordination_risk_evidence_unit(
    input: CoordinationRiskEvidenceInput,
) -> CoordinationRiskEvidenceUnit {
    let mut degraded_reasons = SmallVec::<[DegradedReason; 4]>::new();
    if !input.snapshot_available {
        degraded_reasons.push(DegradedReason::MissingDecisionSnapshot);
    }
    if input.snapshot_mode != CoordinationSnapshotMode::DecisionTime {
        push_unique_reason(&mut degraded_reasons, DegradedReason::MissingFrozenBuffer);
    }
    if input.source_snapshot_hash.is_none() {
        push_unique_reason(&mut degraded_reasons, DegradedReason::MissingFrozenBuffer);
    }
    append_snapshot_time_bound_reasons(
        &mut degraded_reasons,
        input.decision_ts_ms,
        input.decision_slot,
        input.feature_cutoff_ts_ms,
        input.feature_cutoff_slot,
        input.source_buffer_watermark_slot,
    );

    let skipped_metrics = skipped_phase06_metrics(DegradedReason::NotConfigured);
    let (features, metric_breakdowns) = if degraded_reasons.is_empty() {
        (
            sanitize_export_only_features(input.features),
            sanitize_metric_breakdowns(input.metric_breakdowns),
        )
    } else {
        let mut features = CoordinationRiskFeatures {
            funding_visibility: input.funding_visibility,
            ..CoordinationRiskFeatures::default()
        };
        features.degraded_reasons = degraded_reasons.clone();

        (
            features,
            skipped_phase06_breakdowns_with_skips(degraded_reasons[0], skipped_metrics.clone()),
        )
    };

    CoordinationRiskEvidenceUnit {
        schema_version: input.schema_version,
        scope_id: input.scope_id,
        run_id: input.run_id,
        candidate_id: input.candidate_id,
        pool_id: input.pool_id,
        mint: input.mint,
        decision_id: input.decision_id,
        decision_ts_ms: input.decision_ts_ms,
        decision_slot: input.decision_slot,
        snapshot_mode: input.snapshot_mode,
        feature_cutoff_ts_ms: input.feature_cutoff_ts_ms,
        feature_cutoff_slot: input.feature_cutoff_slot,
        source_buffer_watermark_slot: input.source_buffer_watermark_slot,
        computed_at_recv_ts_ns: input.computed_at_recv_ts_ns,
        gatekeeper_version: input.gatekeeper_version,
        source_snapshot_hash: input.source_snapshot_hash,
        sample_summary: input.sample_summary,
        funding_visibility: input.funding_visibility,
        features,
        metric_breakdowns,
        skipped_metrics,
        degraded_reasons,
    }
}

pub fn build_coordination_risk_evidence_unit_from_snapshot(
    snapshot: FrozenCoordinationDecisionSnapshot,
    config: &CoordinationRiskConfig,
) -> CoordinationRiskEvidenceUnit {
    let snapshot_available = snapshot.snapshot_mode == CoordinationSnapshotMode::DecisionTime
        && snapshot.source_snapshot_hash.is_some()
        && snapshot_time_bounds_are_decision_safe(
            snapshot.decision_ts_ms,
            snapshot.decision_slot,
            snapshot.feature_cutoff_ts_ms,
            snapshot.feature_cutoff_slot,
            snapshot.source_buffer_watermark_slot,
        );
    let funding_visibility = FundingVisibility::from_fsc_v2_lane_health(snapshot.fsc_v2.as_ref());
    let sample_summary = summarize_observed_buy_txs(&snapshot.txs);

    let input = if snapshot_available {
        let ftdi = compute_ftdi_v2(&snapshot.txs, config);
        let buyer_samples = unique_first_buys_by_signer(&snapshot.txs);
        let dbia = compute_dbia_v2(&buyer_samples, snapshot.dev_reference, config);
        let sfd = compute_sfd_v2(&snapshot.txs, config);
        let cpv = compute_cpv_v2(
            &snapshot.txs,
            &snapshot.signer_activity,
            snapshot.rolling_state_ready,
            config,
        );
        let des = compute_des_v2(&snapshot.txs, config);
        let bse = compute_bse_v2(&snapshot.txs, config);
        let cucd = compute_cucd_v2(&snapshot.txs, config);

        let mut features = CoordinationRiskFeatures {
            funding_visibility,
            fee_topology_diversity_index: ftdi.value.clone(),
            dev_buyer_infra_affinity: dbia.value.clone(),
            spend_fraction_divergence: sfd.value.clone(),
            funding_source_concentration: funding_source_concentration_from_fsc_v2(
                snapshot.fsc_v2.as_ref(),
                config,
            ),
            signer_cross_pool_velocity: cpv.value.clone(),
            demand_elasticity_score: des.value.clone(),
            buy_sizing_elasticity: bse.value.clone(),
            compute_unit_consumption_dispersion: cucd.value.clone(),
            ..CoordinationRiskFeatures::default()
        };
        features.degraded_reasons.clear();
        if snapshot.fsc_v2.is_none() {
            features
                .degraded_reasons
                .push(DegradedReason::FundingLaneUnavailable);
        }

        CoordinationRiskEvidenceInput {
            schema_version: snapshot.schema_version,
            scope_id: snapshot.scope_id,
            run_id: snapshot.run_id,
            candidate_id: snapshot.candidate_id,
            pool_id: snapshot.pool_id,
            mint: snapshot.mint,
            decision_id: snapshot.decision_id,
            decision_ts_ms: snapshot.decision_ts_ms,
            decision_slot: snapshot.decision_slot,
            snapshot_mode: snapshot.snapshot_mode,
            snapshot_available,
            feature_cutoff_ts_ms: snapshot.feature_cutoff_ts_ms,
            feature_cutoff_slot: snapshot.feature_cutoff_slot,
            source_buffer_watermark_slot: snapshot.source_buffer_watermark_slot,
            computed_at_recv_ts_ns: snapshot.computed_at_recv_ts_ns,
            gatekeeper_version: snapshot.gatekeeper_version,
            source_snapshot_hash: snapshot.source_snapshot_hash,
            sample_summary,
            funding_visibility,
            features,
            metric_breakdowns: CoordinationMetricBreakdowns {
                fee_topology_diversity_index: Some(ftdi.evidence),
                dev_buyer_infra_affinity: Some(dbia.evidence),
                spend_fraction_divergence: Some(sfd.evidence),
                signer_cross_pool_velocity: Some(cpv.evidence),
                demand_elasticity_score: Some(des.evidence),
                buy_sizing_elasticity: Some(bse.evidence),
                compute_unit_consumption_dispersion: Some(cucd.evidence),
                skipped_metrics: skipped_phase06_metrics(DegradedReason::NotConfigured),
            },
        }
    } else {
        CoordinationRiskEvidenceInput {
            schema_version: snapshot.schema_version,
            scope_id: snapshot.scope_id,
            run_id: snapshot.run_id,
            candidate_id: snapshot.candidate_id,
            pool_id: snapshot.pool_id,
            mint: snapshot.mint,
            decision_id: snapshot.decision_id,
            decision_ts_ms: snapshot.decision_ts_ms,
            decision_slot: snapshot.decision_slot,
            snapshot_mode: snapshot.snapshot_mode,
            snapshot_available,
            feature_cutoff_ts_ms: snapshot.feature_cutoff_ts_ms,
            feature_cutoff_slot: snapshot.feature_cutoff_slot,
            source_buffer_watermark_slot: snapshot.source_buffer_watermark_slot,
            computed_at_recv_ts_ns: snapshot.computed_at_recv_ts_ns,
            gatekeeper_version: snapshot.gatekeeper_version,
            source_snapshot_hash: snapshot.source_snapshot_hash,
            sample_summary,
            funding_visibility,
            features: CoordinationRiskFeatures::default(),
            metric_breakdowns: CoordinationMetricBreakdowns::default(),
        }
    };

    build_coordination_risk_evidence_unit(input)
}

#[must_use]
pub fn skipped_phase06_breakdowns(reason: DegradedReason) -> CoordinationMetricBreakdowns {
    skipped_phase06_breakdowns_with_skips(reason, skipped_phase06_metrics(reason))
}

#[must_use]
pub fn funding_source_concentration_from_fsc_v2(
    evidence: Option<&FscV2Evidence>,
    config: &CoordinationRiskConfig,
) -> Option<MetricValue> {
    let evidence = evidence?;
    if evidence.snapshot_mode != FscSnapshotMode::DecisionTime
        || evidence.status != FscEvidenceStatus::Clean
        || evidence.gap_suspected
        || !evidence.capture_ready
        || !evidence.index_warm
        || evidence.excluded_reason.is_some()
    {
        return None;
    }

    let hhi_norm_count = evidence.hhi_norm_count?;
    Some(MetricValue::new(
        hhi_norm_count,
        severity_high(hhi_norm_count, config.fsc_high_threshold()),
        evidence
            .known_coverage
            .min(evidence.non_neutral_known_coverage)
            .min(evidence.attribution_confidence_mean.unwrap_or(1.0)),
        evidence.known_non_neutral_buyers,
        evidence.known_coverage,
        MetricEvidenceStatus::Clean,
    ))
}

#[must_use]
pub fn sample_summary_for_evidence(txs: &[ObservedBuyTx]) -> CoordinationSampleSummary {
    summarize_observed_buy_txs(txs)
}

fn sanitize_export_only_features(
    mut features: CoordinationRiskFeatures,
) -> CoordinationRiskFeatures {
    features.total_coordination_penalty = None;
    features.interaction_penalty = None;
    features.capital_template_concentration = None;
    features.cross_pool_cohort_recurrence = None;
    features.execution_template_concentration = None;
    features
}

fn snapshot_time_bounds_are_decision_safe(
    decision_ts_ms: u64,
    decision_slot: Option<u64>,
    feature_cutoff_ts_ms: u64,
    feature_cutoff_slot: Option<u64>,
    source_buffer_watermark_slot: Option<u64>,
) -> bool {
    let mut reasons = SmallVec::<[DegradedReason; 4]>::new();
    append_snapshot_time_bound_reasons(
        &mut reasons,
        decision_ts_ms,
        decision_slot,
        feature_cutoff_ts_ms,
        feature_cutoff_slot,
        source_buffer_watermark_slot,
    );
    reasons.is_empty()
}

fn append_snapshot_time_bound_reasons(
    degraded_reasons: &mut SmallVec<[DegradedReason; 4]>,
    decision_ts_ms: u64,
    decision_slot: Option<u64>,
    feature_cutoff_ts_ms: u64,
    feature_cutoff_slot: Option<u64>,
    source_buffer_watermark_slot: Option<u64>,
) {
    if feature_cutoff_ts_ms > decision_ts_ms {
        push_unique_reason(degraded_reasons, DegradedReason::FeatureCutoffAfterDecision);
    }

    if let (Some(feature_cutoff_slot), Some(decision_slot)) = (feature_cutoff_slot, decision_slot) {
        if feature_cutoff_slot > decision_slot {
            push_unique_reason(degraded_reasons, DegradedReason::FeatureCutoffAfterDecision);
        }
    }

    if let (Some(source_buffer_watermark_slot), Some(decision_slot)) =
        (source_buffer_watermark_slot, decision_slot)
    {
        if source_buffer_watermark_slot > decision_slot {
            push_unique_reason(
                degraded_reasons,
                DegradedReason::SourceWatermarkAfterDecision,
            );
        }
    }
}

fn sanitize_metric_breakdowns(
    mut breakdowns: CoordinationMetricBreakdowns,
) -> CoordinationMetricBreakdowns {
    breakdowns.skipped_metrics = skipped_phase06_metrics(DegradedReason::NotConfigured);
    breakdowns
}

fn skipped_phase06_breakdowns_with_skips(
    reason: DegradedReason,
    skipped_metrics: SmallVec<[SkippedMetric; 4]>,
) -> CoordinationMetricBreakdowns {
    CoordinationMetricBreakdowns {
        skipped_metrics: skipped_metrics
            .into_iter()
            .map(|mut skipped| {
                skipped.reason = reason;
                skipped
            })
            .collect(),
        ..CoordinationMetricBreakdowns::default()
    }
}

fn skipped_phase06_metrics(reason: DegradedReason) -> SmallVec<[SkippedMetric; 4]> {
    smallvec![
        SkippedMetric {
            metric: CoordinationMetricName::CapitalTemplateConcentration,
            reason,
        },
        SkippedMetric {
            metric: CoordinationMetricName::CrossPoolCohortRecurrence,
            reason,
        },
        SkippedMetric {
            metric: CoordinationMetricName::ExecutionTemplateConcentration,
            reason,
        },
    ]
}

fn clean_metric<T>(value: MetricValue, breakdown: T) -> MetricComputation<T> {
    metric(
        value,
        breakdown,
        MetricEvidenceStatus::Clean,
        SmallVec::new(),
    )
}

fn metric<T>(
    value: MetricValue,
    breakdown: T,
    evidence_status: MetricEvidenceStatus,
    degraded_reasons: SmallVec<[DegradedReason; 4]>,
) -> MetricComputation<T> {
    MetricComputation {
        value: Some(value),
        evidence: MetricEvidenceRecord {
            evidence_status,
            policy_mode: MetricPolicyMode::ExportOnly,
            score_eligible: false,
            degraded_reasons,
            breakdown,
        },
    }
}

fn unavailable_metric<T>(
    breakdown: T,
    evidence_status: MetricEvidenceStatus,
    degraded_reasons: SmallVec<[DegradedReason; 4]>,
) -> MetricComputation<T> {
    MetricComputation {
        value: None,
        evidence: MetricEvidenceRecord {
            evidence_status,
            policy_mode: MetricPolicyMode::ExportOnly,
            score_eligible: false,
            degraded_reasons,
            breakdown,
        },
    }
}

fn infra_fingerprint_from_buy(tx: &ObservedBuyTx) -> Option<InfraFingerprint> {
    let execution = tx.execution_template_fp?;
    Some(InfraFingerprint {
        account_role_pattern_hash: execution.account_role_pattern_hash,
        outer_program_sequence_hash: execution.outer_program_sequence_hash,
        inner_program_sequence_hash: execution.inner_program_sequence_hash,
        outer_ix_count_bucket: tx.outer_ix_count.unwrap_or_default(),
        inner_ix_group_count_bucket: tx.inner_ix_group_count.unwrap_or_default(),
        compute_budget_shape: execution.compute_budget_shape,
        fee_topology_hash: execution.fee_topology_hash,
        ata_wsol_shape: execution.ata_wsol_shape,
    })
}

fn economic_spend_or_fallback(
    tx: &ObservedBuyTx,
    reasons: &mut SmallVec<[DegradedReason; 4]>,
) -> Option<EconomicSpend> {
    if let Some(spend) = tx.economic_spent_lamports {
        return Some(spend);
    }

    if let Some(lamports) = tx.decoded_buy_sol_lamports {
        return Some(EconomicSpend {
            lamports,
            source: EconomicSpendSource::DecodedPumpInstruction,
            confidence: 0.9,
        });
    }

    if let Some(lamports) = tx.curve_sol_delta_lamports {
        return Some(EconomicSpend {
            lamports,
            source: EconomicSpendSource::CurveRealSolDelta,
            confidence: 0.75,
        });
    }

    push_unique_reason(reasons, DegradedReason::MissingEconomicSpend);
    let pre_balance = tx.pre_balance_signer?;
    let post_balance = tx.post_balance_signer?;
    let signer_delta = pre_balance.checked_sub(post_balance)?;
    let known_overheads = if let Some(fee_lamports) = tx.fee_lamports {
        fee_lamports
    } else {
        push_unique_reason(reasons, DegradedReason::MissingCostUnits);
        0
    };
    let lamports = signer_delta.saturating_sub(known_overheads);
    Some(EconomicSpend {
        lamports,
        source: EconomicSpendSource::SignerDeltaMinusKnownOverheads,
        confidence: 0.35,
    })
}

fn price_impact(tx: &ObservedBuyTx) -> Option<f64> {
    let before = tx.price_before?;
    let after = tx.price_after?;
    if !before.is_finite() || !after.is_finite() || before <= 0.0 {
        return None;
    }

    let impact = (after - before) / before;
    impact.is_finite().then_some(impact)
}

fn same_slot_adjacent_ratio(sequence: &[&ObservedBuyTx]) -> f64 {
    if sequence.len() < 2 {
        return 0.0;
    }

    let same_slot = sequence
        .windows(2)
        .filter(|window| window[0].slot == window[1].slot)
        .count();
    same_slot as f64 / sequence.len().saturating_sub(1) as f64
}

fn sequence_error_reason(error: SequenceBuildError) -> DegradedReason {
    match error {
        SequenceBuildError::MissingSlotIndex { .. } => DegradedReason::MissingSlotIndex,
        SequenceBuildError::DuplicateSlotIndex { .. } => DegradedReason::DuplicateSlotIndex,
    }
}

fn push_tie_reason(reasons: &mut SmallVec<[DegradedReason; 4]>, xs: &[f64], ys: &[f64]) {
    if all_tied(xs) {
        push_unique_reason(reasons, DegradedReason::AllXTies);
    }
    if all_tied(ys) {
        push_unique_reason(reasons, DegradedReason::AllYTies);
    }
    if !reasons.contains(&DegradedReason::AllXTies) && !reasons.contains(&DegradedReason::AllYTies)
    {
        push_unique_reason(reasons, DegradedReason::DenominatorZero);
    }
}

fn all_tied(values: &[f64]) -> bool {
    let Some(first) = values.first() else {
        return true;
    };

    values.iter().all(|value| value == first)
}

fn exact_match<T: PartialEq>(left: T, right: T) -> f64 {
    if left == right {
        1.0
    } else {
        0.0
    }
}

fn dev_reference_confidence(mode: DevFingerprintMode) -> f64 {
    match mode {
        DevFingerprintMode::ComparablePureBuy => 1.0,
        DevFingerprintMode::CreateTxSwapSliceOnly => 0.6,
        DevFingerprintMode::NotComparable => 0.0,
    }
}

fn push_unique_reason(reasons: &mut SmallVec<[DegradedReason; 4]>, reason: DegradedReason) {
    if !reasons.contains(&reason) {
        reasons.push(reason);
    }
}

fn economic_spend_source_rank(source: EconomicSpendSource) -> u8 {
    match source {
        EconomicSpendSource::DecodedPumpInstruction => 0,
        EconomicSpendSource::CurveRealSolDelta => 1,
        EconomicSpendSource::SignerDeltaMinusKnownOverheads => 2,
    }
}

fn coverage(available: usize, total: usize) -> f64 {
    if total == 0 {
        0.0
    } else {
        (available as f64 / total as f64).clamp(0.0, 1.0)
    }
}

fn saturating_u8(value: usize) -> u8 {
    u8::try_from(value).unwrap_or(u8::MAX)
}
