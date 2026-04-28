use super::gatekeeper::{
    AlphaGateDiagnostics, AlphaRejectTrigger, BondingCurveDynamics, DevBehaviorProfile,
    GatekeeperAssessment, GatekeeperDecision, GatekeeperStrength, GatekeeperVerdictType,
    ProsperityFilterDiagnostics, ProsperityRejectTrigger, SignerDiversityProfile, SoftSignals,
    SybilInterferencePattern, SybilLeadSignal, SybilPolicyDiagnostics, SybilSoftSignals,
    VelocityProfile, VolumeSanityProfile,
};
use ghost_brain::config::GatekeeperV2Config;
use ghost_core::checkpoint::{MaterializedFeatureSet, SybilResistanceFeatures, TrendDirection};
use ghost_core::tx_intelligence::types::{
    CPV_INSUFFICIENT_SIGNERS_REASON, CPV_ROLLING_STATE_UNAVAILABLE_REASON,
    DBIA_INSUFFICIENT_BUYERS_REASON, DBIA_NO_DEV_BUY_REASON,
    DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON, DES_CURVE_DATA_UNAVAILABLE_REASON,
    DES_INSUFFICIENT_BUYS_REASON, DES_SLOT_ORDER_UNAVAILABLE_REASON,
    FSC_FUNDING_STREAM_UNAVAILABLE_REASON, FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
    FSC_ROLLING_STATE_UNAVAILABLE_REASON, FTDI_INSUFFICIENT_BUYS_REASON,
    FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON, SFD_INSUFFICIENT_BUYS_REASON,
};
use ghost_core::ShadowLedgerStaleFallback;
use seer::early_fingerprint::EarlyFingerprintMetrics;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HardFailReason {
    DevSold,
    SellImpact,
    TxPriceImpact,
    PriceChange,
    MarketCapTooLow,
    ExtremeHhi,
    ExtremeBundling,
    ExtremeTop3Dominance,
    ExtremeBotTiming,
    FailedTxRatio,
    SlowPool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CurveGateOutcome {
    Ready,
    Pending {
        reason_label: &'static str,
    },
    Reject {
        reason_label: &'static str,
        terminal_outcome: &'static str,
        reason: &'static str,
    },
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct PolicyEvaluationContext {
    pub finalize_lag_ms: u64,
    pub eval_count: usize,
}

fn clamp01(value: f64) -> f64 {
    value.clamp(0.0, 1.0)
}

fn norm_up(value: f64, low: f64, high: f64) -> f64 {
    if high <= low {
        return 0.0;
    }
    clamp01((value - low) / (high - low))
}

fn norm_down(value: f64, low: f64, high: f64) -> f64 {
    if high <= low {
        return 0.0;
    }
    clamp01((high - value) / (high - low))
}

fn option_bool_label(value: Option<bool>) -> &'static str {
    match value {
        Some(true) => "true",
        Some(false) => "false",
        None => "null",
    }
}

fn format_matched_branches(branches: &[&'static str]) -> String {
    if branches.is_empty() {
        "none".to_string()
    } else {
        branches.join(",")
    }
}

fn compute_momentum(features: &MaterializedFeatureSet) -> f64 {
    let tx = &features.tx_intel_features;
    let alpha = &features.alpha_fingerprint;

    let base = 0.36 * norm_up(tx.burst_ratio, 0.08, 0.45)
        + 0.34 * norm_down(tx.avg_interval_ms, 90.0, 700.0)
        + 0.20 * norm_up(tx.timing_entropy, 1.05, 2.35)
        + 0.10 * norm_up(tx.buy_count as f64, 10.0, 25.0);
    let jito_boost = 1.0 + 0.12 * norm_up(alpha.jito_tip_intensity.unwrap_or_default(), 0.05, 0.45);
    let dominance_boost = 1.0
        + 0.15
            * norm_up(
                alpha.early_slot_volume_dominance_buy.unwrap_or_default(),
                0.55,
                0.90,
            );

    clamp01(base * jito_boost * dominance_boost)
}

fn compute_demand(features: &MaterializedFeatureSet) -> f64 {
    let tx = &features.tx_intel_features;
    let alpha = &features.alpha_fingerprint;

    let base = 0.35 * norm_up(tx.buy_count as f64, 10.0, 25.0)
        + 0.35 * norm_up(tx.unique_signers as f64, 8.0, 20.0)
        + 0.30 * norm_up(tx.buy_ratio, 0.55, 0.92);
    let fixed_size_penalty =
        1.0 - 0.28 * norm_up(alpha.fixed_size_buy_ratio.unwrap_or_default(), 0.20, 0.75);
    let flipper_penalty =
        1.0 - 0.30 * norm_up(alpha.flipper_presence_ratio.unwrap_or_default(), 0.05, 0.40);

    clamp01(base * fixed_size_penalty * flipper_penalty)
}

fn evaluate_alpha_gate(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> AlphaGateDiagnostics {
    if !config.enable_alpha_gate {
        return AlphaGateDiagnostics::not_run(false);
    }

    if features.tx_intel_features.buy_count < config.min_alpha_sample as u64 {
        return AlphaGateDiagnostics::skipped(true, "insufficient_sample");
    }

    let alpha = &features.alpha_fingerprint;
    if alpha.jito_tip_intensity.is_none()
        || alpha.early_slot_volume_dominance_buy.is_none()
        || alpha.fixed_size_buy_ratio.is_none()
        || alpha.flipper_presence_ratio.is_none()
    {
        return AlphaGateDiagnostics::skipped(true, "missing_alpha_inputs");
    }

    let momentum = compute_momentum(features);
    let demand = compute_demand(features);
    let joint = clamp01(momentum * demand);
    let reject_trigger = if momentum < config.min_momentum {
        Some(AlphaRejectTrigger::LowMomentum)
    } else if demand < config.min_demand {
        Some(AlphaRejectTrigger::LowDemand)
    } else if joint < config.min_alpha_joint {
        Some(AlphaRejectTrigger::LowJoint)
    } else {
        None
    };

    AlphaGateDiagnostics::evaluated(
        true,
        momentum,
        demand,
        joint,
        reject_trigger.is_none(),
        reject_trigger,
    )
}

fn evaluate_prosperity_filter(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> ProsperityFilterDiagnostics {
    if !config.enable_prosperity_filter {
        return ProsperityFilterDiagnostics::not_run(false);
    }

    let curve = match assessment
        .phase6_curve
        .as_ref()
        .filter(|curve| curve.curve_data_known)
    {
        Some(curve) => curve,
        None => {
            let mut diagnostics = ProsperityFilterDiagnostics::rejected_missing(
                true,
                ProsperityRejectTrigger::MissingMarketCap,
                None,
                None,
            );
            diagnostics.overlay_enabled = config.enable_prosperity_overlay;
            return diagnostics;
        }
    };

    let market_cap_floor_pass =
        curve.current_market_cap_sol >= config.prosperity_min_market_cap_sol;
    let sybil = &assessment.feature_snapshot.sybil_resistance;
    let overlay_enabled = config.enable_prosperity_overlay;
    if !sybil_metric_is_actionable(sybil, SybilMetric::Cpv) {
        let mut diagnostics = ProsperityFilterDiagnostics::rejected_missing(
            true,
            ProsperityRejectTrigger::MissingSignerCrossPoolVelocity,
            Some(market_cap_floor_pass),
            None,
        );
        diagnostics.overlay_enabled = overlay_enabled;
        return diagnostics;
    }

    let cpv_pass = sybil
        .signer_cross_pool_velocity
        .is_some_and(|value| value <= config.prosperity_max_signer_cross_pool_velocity);
    let fingerprint = assessment.early_fingerprint.as_ref();
    let block0_sniped_supply_pct = fingerprint.and_then(|metrics| metrics.block0_sniped_supply_pct);
    let sell_buy_ratio = fingerprint.and_then(|metrics| metrics.sell_buy_ratio);
    let early_slot_volume_dominance_buy = assessment
        .feature_snapshot
        .alpha_fingerprint
        .early_slot_volume_dominance_buy
        .or_else(|| fingerprint.and_then(|metrics| metrics.early_slot_volume_dominance_buy));
    let hhi = assessment
        .phase3_diversity
        .as_ref()
        .map(|diversity| diversity.hhi)
        .unwrap_or(assessment.feature_snapshot.tx_intel_features.hhi);
    let fee_topology_diversity_index = sybil.fee_topology_diversity_index;

    let branch1_pass = block0_sniped_supply_pct
        .is_some_and(|value| value >= config.prosperity_branch1_min_block0_sniped_supply_pct)
        && sell_buy_ratio
            .is_some_and(|value| value <= config.prosperity_branch1_max_sell_buy_ratio);
    let branch2_pass = curve.current_market_cap_sol >= config.prosperity_branch2_min_market_cap_sol
        && early_slot_volume_dominance_buy.is_some_and(|value| {
            value >= config.prosperity_branch2_min_early_slot_volume_dominance_buy
        });
    let branch3_pass = hhi <= config.prosperity_branch3_max_hhi
        && fee_topology_diversity_index.is_some_and(|value| {
            value >= config.prosperity_branch3_min_fee_topology_diversity_index
        });

    let overlay_price_change_pass = overlay_enabled
        .then_some(curve.price_change_ratio <= config.prosperity_overlay_max_price_change_ratio);
    let overlay_bonding_progress_pass = overlay_enabled.then_some(
        curve.bonding_progress_pct <= config.prosperity_overlay_max_bonding_progress_pct,
    );
    let overlay_ftdi_actionable =
        !overlay_enabled || sybil_metric_is_actionable(sybil, SybilMetric::Ftdi);
    let overlay_fee_topology_diversity_pass = if overlay_enabled && overlay_ftdi_actionable {
        Some(fee_topology_diversity_index.is_some_and(|value| {
            value >= config.prosperity_overlay_min_fee_topology_diversity_index
        }))
    } else {
        None
    };
    let overlay_branch23_sell_buy_required = overlay_enabled && (branch2_pass || branch3_pass);
    let overlay_branch23_sell_buy_pass = if overlay_enabled {
        if overlay_branch23_sell_buy_required {
            Some(sell_buy_ratio.is_some_and(|value| {
                value <= config.prosperity_overlay_branch23_max_sell_buy_ratio
            }))
        } else {
            Some(true)
        }
    } else {
        None
    };
    let overlay_branch2_price_change_pass = if overlay_enabled {
        if branch2_pass {
            Some(
                curve.price_change_ratio
                    <= config.prosperity_overlay_branch2_max_price_change_ratio,
            )
        } else {
            Some(true)
        }
    } else {
        None
    };
    let overlay_globals_pass = !overlay_enabled
        || (overlay_price_change_pass == Some(true)
            && overlay_bonding_progress_pass == Some(true)
            && overlay_fee_topology_diversity_pass == Some(true));
    let overlay_branch23_pass = !overlay_enabled || overlay_branch23_sell_buy_pass == Some(true);
    let overlay_branch2_price_pass =
        !overlay_enabled || overlay_branch2_price_change_pass == Some(true);

    let mut base_matched_branches = Vec::new();
    if branch1_pass {
        base_matched_branches.push("conviction_clean_sells");
    }
    if branch2_pass {
        base_matched_branches.push("large_cap_buy_dominance");
    }
    if branch3_pass {
        base_matched_branches.push("organic_structure");
    }

    let mut matched_branches = Vec::new();
    if branch1_pass && overlay_globals_pass {
        matched_branches.push("conviction_clean_sells");
    }
    if branch2_pass && overlay_globals_pass && overlay_branch23_pass && overlay_branch2_price_pass {
        matched_branches.push("large_cap_buy_dominance");
    }
    if branch3_pass && overlay_globals_pass && overlay_branch23_pass {
        matched_branches.push("organic_structure");
    }

    let reject_trigger = if !market_cap_floor_pass {
        Some(ProsperityRejectTrigger::BelowMinMarketCap)
    } else if !cpv_pass {
        Some(ProsperityRejectTrigger::HighSignerCrossPoolVelocity)
    } else if base_matched_branches.is_empty() {
        Some(ProsperityRejectTrigger::NoBalancedBranch)
    } else if overlay_enabled && !overlay_ftdi_actionable {
        Some(ProsperityRejectTrigger::MissingFeeTopologyDiversityIndex)
    } else if overlay_price_change_pass == Some(false) {
        Some(ProsperityRejectTrigger::AboveOverlayMaxPriceChange)
    } else if overlay_bonding_progress_pass == Some(false) {
        Some(ProsperityRejectTrigger::AboveOverlayMaxBondingProgress)
    } else if overlay_fee_topology_diversity_pass == Some(false) {
        Some(ProsperityRejectTrigger::BelowOverlayMinFeeTopologyDiversityIndex)
    } else if overlay_branch23_sell_buy_required && sell_buy_ratio.is_none() {
        Some(ProsperityRejectTrigger::MissingSellBuyRatio)
    } else if branch2_pass
        && !branch3_pass
        && overlay_branch2_price_change_pass == Some(false)
        && matched_branches.is_empty()
    {
        Some(ProsperityRejectTrigger::AboveOverlayBranch2MaxPriceChange)
    } else if overlay_branch23_sell_buy_pass == Some(false) && matched_branches.is_empty() {
        Some(ProsperityRejectTrigger::AboveOverlayMaxSellBuyRatio)
    } else if matched_branches.is_empty() {
        Some(ProsperityRejectTrigger::NoBalancedBranch)
    } else {
        None
    };

    let overlay_pass = if overlay_enabled
        && market_cap_floor_pass
        && cpv_pass
        && !base_matched_branches.is_empty()
    {
        Some(!matched_branches.is_empty())
    } else {
        None
    };

    ProsperityFilterDiagnostics {
        enabled: true,
        actionable: true,
        pass: Some(reject_trigger.is_none()),
        reject_trigger,
        market_cap_floor_pass: Some(market_cap_floor_pass),
        cpv_pass: Some(cpv_pass),
        branch1_pass: Some(branch1_pass),
        branch2_pass: Some(branch2_pass),
        branch3_pass: Some(branch3_pass),
        overlay_enabled,
        overlay_pass,
        overlay_price_change_pass,
        overlay_bonding_progress_pass,
        overlay_fee_topology_diversity_pass,
        overlay_branch23_sell_buy_pass,
        overlay_branch2_price_change_pass,
        matched_branches,
    }
}

pub fn build_assessment_from_features(
    features: MaterializedFeatureSet,
    config: &GatekeeperV2Config,
    context: PolicyEvaluationContext,
) -> GatekeeperAssessment {
    let phase1_passed = features.tx_intel_features.tx_count >= config.min_tx_count as u64
        && features.tx_intel_features.unique_signers >= config.min_unique_signers as u64
        && features.tx_intel_features.buy_count >= config.min_buy_count as u64;

    let phase2_velocity = velocity_profile_from_features(&features);
    let phase2_passed = phase2_velocity
        .as_ref()
        .map(|velocity| {
            velocity.interval_cv >= config.min_interval_cv
                && velocity.interval_cv <= config.max_interval_cv
                && velocity.burst_ratio <= config.max_burst_ratio
                && velocity.avg_interval_ms >= config.min_avg_interval_ms
                && velocity.avg_interval_ms <= config.max_avg_interval_ms
                && velocity.timing_entropy >= config.min_timing_entropy
                && velocity.timing_entropy <= config.max_timing_entropy
                && features.tx_intel_features.dust_tx_count >= config.min_dust_filtered_count
        })
        .unwrap_or(false);

    let phase3_diversity = signer_diversity_from_features(&features);
    let phase3_passed = phase3_diversity
        .as_ref()
        .map(|diversity| diversity_phase_passes(diversity, config))
        .unwrap_or(false);

    let phase4_volume = volume_sanity_from_features(&features);
    let phase4_fingerprint_ok =
        alpha_fingerprint_phase4_passes(&features.alpha_fingerprint, config);
    let phase4_passed = phase4_volume
        .as_ref()
        .map(|volume| volume_phase_passes_base(volume, phase4_fingerprint_ok, config))
        .unwrap_or(false);

    let phase5_dev = dev_behavior_from_features(&features);
    let phase5_passed = phase5_dev
        .as_ref()
        .map(|dev| {
            if !dev.dev_wallet_known {
                true
            } else {
                dev.dev_buy_total_sol <= config.max_dev_buy_sol
                    && dev.dev_buy_total_sol >= config.min_dev_buy_sol
                    && dev.dev_tx_ratio <= config.max_dev_tx_ratio
                    && dev.dev_tx_ratio >= config.min_dev_tx_ratio
                    && dev.dev_volume_ratio <= config.max_dev_volume_ratio
                    && dev.dev_volume_ratio >= config.min_dev_volume_ratio
                    && if config.reject_on_dev_sell {
                        !dev.dev_has_sold
                    } else {
                        true
                    }
            }
        })
        .unwrap_or(true);

    let phase6_curve = bonding_curve_from_features(&features);
    let phase6_passed = phase6_curve
        .as_ref()
        .map(|curve| {
            if curve.price_data_points < 2 {
                true
            } else {
                curve.price_change_ratio <= config.max_price_change_ratio
                    && curve.max_single_tx_price_impact_pct <= config.max_single_tx_price_impact_pct
                    && curve.max_single_sell_impact_pct <= config.max_single_sell_impact_pct
                    && if curve.curve_data_known {
                        curve.bonding_progress_pct <= config.max_bonding_progress_pct
                            && curve.bonding_progress_pct >= config.min_bonding_progress_pct
                            && curve.current_market_cap_sol >= config.min_market_cap_sol
                    } else {
                        true
                    }
            }
        })
        .unwrap_or(false);

    let checkpoint_count = features.checkpoint_features.trajectory_checkpoint_count;
    let trajectory_available =
        checkpoint_count > 0 || features.curve_readiness.price_sample_count > 1;
    let phases_passed = [
        phase1_passed,
        phase2_passed,
        phase3_passed,
        phase4_passed,
        phase5_passed,
        phase6_passed,
    ]
    .into_iter()
    .filter(|passed| *passed)
    .count() as u8;

    let mut assessment = GatekeeperAssessment {
        phase1_passed,
        phase2_velocity,
        phase2_passed,
        phase3_diversity,
        phase3_passed,
        phase4_volume,
        phase4_passed,
        phase5_dev,
        phase5_passed,
        phase6_curve,
        phase6_passed,
        phases_passed,
        hard_reject_reason: None,
        total_tx_evaluated: features.tx_intel_features.tx_count as usize,
        unique_tx_evaluated: features.tx_intel_features.tx_count as usize,
        unique_signers_evaluated: features.tx_intel_features.unique_signers as usize,
        observation_duration_ms: features.session_metadata.observation_duration_ms,
        finalize_lag_ms: context.finalize_lag_ms,
        dust_filtered_count: features.tx_intel_features.dust_tx_count,
        eval_count: context.eval_count,
        buy_count: features.tx_intel_features.buy_count as usize,
        decision: None,
        early_fingerprint: None,
        curve_t0_event_ts_ms: features.curve_readiness.t0_event_ts_ms,
        curve_t0_clock_source: None,
        curve_wait_elapsed_ms: features.curve_readiness.wait_elapsed_ms,
        feature_snapshot: features,
        checkpoint_count,
        trajectory_available,
    };
    assessment.hard_reject_reason = evaluate_hard_filters_from_assessment(&assessment, config)
        .map(|(_reason, reason_chain)| reason_chain);
    assessment
}

pub fn refresh_assessment_thresholds(
    assessment: &mut GatekeeperAssessment,
    config: &GatekeeperV2Config,
) {
    if let Some(diversity) = assessment.phase3_diversity.as_ref() {
        assessment.phase3_passed = diversity_phase_passes(diversity, config);
    }
    if let Some(volume) = assessment.phase4_volume.as_ref() {
        assessment.phase4_passed =
            volume_phase_passes(volume, assessment.early_fingerprint.as_ref(), config);
    }
    assessment.phases_passed = [
        assessment.phase1_passed,
        assessment.phase2_passed,
        assessment.phase3_passed,
        assessment.phase4_passed,
        assessment.phase5_passed,
        assessment.phase6_passed,
    ]
    .into_iter()
    .filter(|passed| *passed)
    .count() as u8;
    assessment.hard_reject_reason = evaluate_hard_filters_from_assessment(assessment, config)
        .map(|(_reason, reason_chain)| reason_chain);
}

pub fn evaluate_hard_filters(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> Option<(HardFailReason, String)> {
    let assessment = build_assessment_from_features(
        features.clone(),
        config,
        PolicyEvaluationContext::default(),
    );
    evaluate_hard_filters_from_assessment(&assessment, config)
}

fn evaluate_hard_filters_from_assessment(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> Option<(HardFailReason, String)> {
    if config.reject_on_dev_sell
        && assessment
            .phase5_dev
            .as_ref()
            .map(|dev| dev.dev_has_sold)
            .unwrap_or(false)
    {
        return Some((
            HardFailReason::DevSold,
            "HARD_FAIL: dev_has_sold".to_string(),
        ));
    }

    if let Some(curve) = assessment.phase6_curve.as_ref() {
        if curve.price_data_points >= 2
            && curve.max_single_sell_impact_pct > config.max_single_sell_impact_pct
        {
            return Some((
                HardFailReason::SellImpact,
                format!(
                    "HARD_FAIL: sell_impact={:.1}% > {:.1}%",
                    curve.max_single_sell_impact_pct, config.max_single_sell_impact_pct
                ),
            ));
        }

        if curve.price_data_points >= 2
            && curve.max_single_tx_price_impact_pct > config.max_single_tx_price_impact_pct
        {
            return Some((
                HardFailReason::TxPriceImpact,
                format!(
                    "HARD_FAIL: tx_price_impact={:.1}% > {:.1}%",
                    curve.max_single_tx_price_impact_pct, config.max_single_tx_price_impact_pct
                ),
            ));
        }

        if curve.price_data_points >= 2 && curve.price_change_ratio > config.max_price_change_ratio
        {
            return Some((
                HardFailReason::PriceChange,
                format!(
                    "HARD_FAIL: price_change_ratio={:.1} > {:.1}",
                    curve.price_change_ratio, config.max_price_change_ratio
                ),
            ));
        }

        if curve.curve_data_known
            && curve.price_data_points >= 2
            && curve.current_market_cap_sol < config.min_market_cap_sol
        {
            return Some((
                HardFailReason::MarketCapTooLow,
                format!(
                    "HARD_FAIL: market_cap={:.1} < {:.1}",
                    curve.current_market_cap_sol, config.min_market_cap_sol
                ),
            ));
        }
    }

    if let Some(diversity) = assessment.phase3_diversity.as_ref() {
        if diversity.hhi > config.hard_fail_hhi {
            return Some((
                HardFailReason::ExtremeHhi,
                format!(
                    "HARD_FAIL: hhi={:.3} > {:.3} (extreme cabal)",
                    diversity.hhi, config.hard_fail_hhi
                ),
            ));
        }

        if diversity.same_ms_tx_ratio > config.hard_fail_same_ms_tx_ratio {
            return Some((
                HardFailReason::ExtremeBundling,
                format!(
                    "HARD_FAIL: same_ms_ratio={:.2} > {:.2} (extreme bundling)",
                    diversity.same_ms_tx_ratio, config.hard_fail_same_ms_tx_ratio
                ),
            ));
        }

        if diversity.top3_volume_pct > config.hard_fail_top3_volume_pct {
            return Some((
                HardFailReason::ExtremeTop3Dominance,
                format!(
                    "HARD_FAIL: top3_vol={:.2} > {:.2} (extreme whale dominance)",
                    diversity.top3_volume_pct, config.hard_fail_top3_volume_pct
                ),
            ));
        }
    }

    if let Some(velocity) = assessment.phase2_velocity.as_ref() {
        if velocity.interval_cv < 0.08
            && velocity.avg_interval_ms < 30.0
            && assessment.total_tx_evaluated >= config.hard_fail_bot_min_tx as usize
            && assessment.observation_duration_ms >= config.hard_fail_bot_min_observation_ms
        {
            return Some((
                HardFailReason::ExtremeBotTiming,
                format!(
                    "HARD_FAIL: extreme_bot cv={:.3} avg={:.0}ms (n={} window={}ms)",
                    velocity.interval_cv,
                    velocity.avg_interval_ms,
                    assessment.total_tx_evaluated,
                    assessment.observation_duration_ms,
                ),
            ));
        }

        if velocity.avg_interval_ms > config.max_avg_interval_ms {
            return Some((
                HardFailReason::SlowPool,
                format!(
                    "HARD_FAIL: avg_interval={:.0}ms > {:.0}ms (slow/dead pool)",
                    velocity.avg_interval_ms, config.max_avg_interval_ms
                ),
            ));
        }
    }

    if let Some(threshold) = config.min_failed_tx_ratio_for_bot_flag {
        let total_with_failed = assessment.total_tx_evaluated as u64
            + assessment
                .feature_snapshot
                .tx_intel_features
                .failed_tx_count;
        if total_with_failed > 5 {
            let failed_ratio = assessment
                .feature_snapshot
                .tx_intel_features
                .failed_tx_count as f64
                / total_with_failed as f64;
            if failed_ratio > threshold {
                return Some((
                    HardFailReason::FailedTxRatio,
                    format!(
                        "HARD_FAIL: failed_tx_ratio={:.2} (bot spam, Yellowstone)",
                        failed_ratio
                    ),
                ));
            }
        }
    }

    None
}

pub fn evaluate_policy(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> GatekeeperDecision {
    let assessment = build_assessment_from_features(
        features.clone(),
        config,
        PolicyEvaluationContext::default(),
    );
    evaluate_policy_from_assessment(&assessment, config)
}

pub fn evaluate_policy_from_assessment(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> GatekeeperDecision {
    let diagnostics = build_policy_diagnostics(assessment, config);
    let total_soft_points = diagnostics.soft_points as u16 + diagnostics.sybil_policy.soft_points;

    if let Some((_reason, reason_chain)) = evaluate_hard_filters_from_assessment(assessment, config)
    {
        return GatekeeperDecision {
            hard_fail_reason: Some(reason_chain.clone()),
            core1_passed: diagnostics.core1_passed,
            core2_passed: diagnostics.core2_passed,
            core3_passed: diagnostics.core3_passed,
            soft_signals: diagnostics.soft_signals,
            soft_points: diagnostics.soft_points,
            max_soft_points_possible: diagnostics.max_soft_points_possible,
            effective_max_soft_points: diagnostics.effective_max_soft_points,
            dev_unknown: diagnostics.dev_unknown,
            sybil_policy: diagnostics.sybil_policy.clone(),
            alpha_gate: AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
            prosperity_filter: ProsperityFilterDiagnostics::not_run(
                config.enable_prosperity_filter,
            ),
            total_soft_points,
            verdict_type: GatekeeperVerdictType::RejectHardFail,
            verdict_buy: false,
            reason_chain,
            gatekeeper_strength: None,
        };
    }

    let (verdict_type, verdict_buy, reason_chain, alpha_gate, prosperity_filter) = if !diagnostics
        .core1_passed
        || !diagnostics.core2_passed
        || !diagnostics.core3_passed
    {
        (
            GatekeeperVerdictType::RejectCoreFail,
            false,
            format!(
                "CORE_FAIL: core1={} core2={} core3={}",
                diagnostics.core1_passed, diagnostics.core2_passed, diagnostics.core3_passed
            ),
            AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
            ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
        )
    } else if let Some(reason) = sybil_combo_veto_reason(&diagnostics.sybil_policy, config) {
        (
            GatekeeperVerdictType::RejectSybilInterference,
            false,
            reason,
            AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
            ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
        )
    } else if diagnostics.sybil_policy.enabled
        && diagnostics.sybil_policy.soft_points
            > diagnostics.sybil_policy.effective_max_soft_points as u16
    {
        (
            GatekeeperVerdictType::RejectSybilSoftExcess,
            false,
            format!(
                "SYBIL_SOFT_FAIL: sybil_soft_points={} > {} flags=[{}] patterns=[{}]",
                diagnostics.sybil_policy.soft_points,
                diagnostics.sybil_policy.effective_max_soft_points,
                diagnostics.sybil_policy.soft_signals.format_flags(),
                SybilInterferencePattern::format_patterns(
                    &diagnostics.sybil_policy.interference_patterns
                )
            ),
            AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
            ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
        )
    } else if diagnostics.soft_points > diagnostics.effective_max_soft_points {
        (
            GatekeeperVerdictType::RejectSoftExcess,
            false,
            format!(
                "SOFT_FAIL: soft_points={} > {} flags=[{}]",
                diagnostics.soft_points,
                diagnostics.effective_max_soft_points,
                diagnostics.soft_signals.format_flags()
            ),
            AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
            ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
        )
    } else {
        let alpha_gate = evaluate_alpha_gate(&assessment.feature_snapshot, config);
        if let Some(trigger) = alpha_gate.reject_trigger {
            (
                GatekeeperVerdictType::RejectLowAlpha,
                false,
                format!(
                    "ALPHA_FAIL: trigger={} momentum={:.3}/{} demand={:.3}/{} joint={:.3}/{}",
                    trigger,
                    alpha_gate.momentum.unwrap_or_default(),
                    config.min_momentum,
                    alpha_gate.demand.unwrap_or_default(),
                    config.min_demand,
                    alpha_gate.joint.unwrap_or_default(),
                    config.min_alpha_joint,
                ),
                alpha_gate,
                ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
            )
        } else {
            let prosperity_filter = evaluate_prosperity_filter(assessment, config);
            if let Some(trigger) = prosperity_filter.reject_trigger {
                (
                        GatekeeperVerdictType::RejectLowProsperity,
                        false,
                        format!(
                            "PROSPERITY_FAIL: trigger={} mcap_floor_pass={} cpv_pass={} b1={} b2={} b3={} matched=[{}]",
                            trigger,
                            option_bool_label(prosperity_filter.market_cap_floor_pass),
                            option_bool_label(prosperity_filter.cpv_pass),
                            option_bool_label(prosperity_filter.branch1_pass),
                            option_bool_label(prosperity_filter.branch2_pass),
                            option_bool_label(prosperity_filter.branch3_pass),
                            format_matched_branches(&prosperity_filter.matched_branches),
                        ),
                        alpha_gate,
                        prosperity_filter,
                    )
            } else {
                (
                        GatekeeperVerdictType::Buy,
                        true,
                        format!(
                            "BUY: soft_points={}/{} flags=[{}] alpha_pass={} momentum={} demand={} joint={} alpha_skip={} prosperity_pass={} mcap_floor_pass={} cpv_pass={} b1={} b2={} b3={} matched=[{}]",
                            diagnostics.soft_points,
                            diagnostics.effective_max_soft_points,
                            diagnostics.soft_signals.format_flags(),
                            alpha_gate.pass.unwrap_or(true),
                            alpha_gate
                                .momentum
                                .map(|value| format!("{value:.3}"))
                                .unwrap_or_else(|| "null".to_string()),
                            alpha_gate
                                .demand
                                .map(|value| format!("{value:.3}"))
                                .unwrap_or_else(|| "null".to_string()),
                            alpha_gate
                                .joint
                                .map(|value| format!("{value:.3}"))
                                .unwrap_or_else(|| "null".to_string()),
                            alpha_gate.skip_reason.unwrap_or("none"),
                            prosperity_filter.pass.unwrap_or(true),
                            option_bool_label(prosperity_filter.market_cap_floor_pass),
                            option_bool_label(prosperity_filter.cpv_pass),
                            option_bool_label(prosperity_filter.branch1_pass),
                            option_bool_label(prosperity_filter.branch2_pass),
                            option_bool_label(prosperity_filter.branch3_pass),
                            format_matched_branches(&prosperity_filter.matched_branches),
                        ),
                        alpha_gate,
                        prosperity_filter,
                    )
            }
        }
    };

    let gatekeeper_strength = verdict_buy.then(|| {
        let manipulation_flag_count = [
            diagnostics.soft_signals.bundle_suspicion,
            diagnostics.soft_signals.cabal_suspicion,
            diagnostics.soft_signals.top3_dominance,
        ]
        .into_iter()
        .filter(|flag| *flag)
        .count() as u8;

        if diagnostics.soft_points
            <= diagnostics
                .effective_max_soft_points
                .saturating_sub(config.iwim_veto_strong_margin)
            && manipulation_flag_count <= config.iwim_veto_strong_max_manip_flags
        {
            GatekeeperStrength::Strong
        } else {
            GatekeeperStrength::Borderline
        }
    });

    GatekeeperDecision {
        hard_fail_reason: None,
        core1_passed: diagnostics.core1_passed,
        core2_passed: diagnostics.core2_passed,
        core3_passed: diagnostics.core3_passed,
        soft_signals: diagnostics.soft_signals,
        soft_points: diagnostics.soft_points,
        max_soft_points_possible: diagnostics.max_soft_points_possible,
        effective_max_soft_points: diagnostics.effective_max_soft_points,
        dev_unknown: diagnostics.dev_unknown,
        sybil_policy: diagnostics.sybil_policy,
        alpha_gate,
        prosperity_filter,
        total_soft_points,
        verdict_type,
        verdict_buy,
        reason_chain,
        gatekeeper_strength,
    }
}

pub fn build_timeout_decision_from_assessment(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> GatekeeperDecision {
    let diagnostics = build_policy_diagnostics(assessment, config);
    let total_soft_points = diagnostics.soft_points as u16 + diagnostics.sybil_policy.soft_points;
    let hard_fail_reason = evaluate_hard_filters_from_assessment(assessment, config)
        .map(|(_reason, reason_chain)| reason_chain);
    let (verdict_type, reason_chain) = if assessment.total_tx_evaluated == 0 {
        (
            GatekeeperVerdictType::TimeoutNoData,
            format!(
                "{}: tx={}/{} signers={}/{} buys={}/{}",
                GatekeeperVerdictType::TimeoutNoData.tag(),
                assessment.total_tx_evaluated,
                config.min_tx_count,
                assessment.unique_signers_evaluated,
                config.min_unique_signers,
                assessment.buy_count,
                config.min_buy_count,
            ),
        )
    } else if !assessment.phase1_passed {
        (
            GatekeeperVerdictType::TimeoutPhase1,
            format!(
                "{}: tx={}/{} signers={}/{} buys={}/{}",
                GatekeeperVerdictType::TimeoutPhase1.tag(),
                assessment.total_tx_evaluated,
                config.min_tx_count,
                assessment.unique_signers_evaluated,
                config.min_unique_signers,
                assessment.buy_count,
                config.min_buy_count,
            ),
        )
    } else {
        (
            GatekeeperVerdictType::RejectCoreFail,
            format!(
                "TIMEOUT_AFTER_PHASE1: core1={} core2={} core3={} tx={}/{} signers={}/{} buys={}/{}",
                diagnostics.core1_passed,
                diagnostics.core2_passed,
                diagnostics.core3_passed,
                assessment.total_tx_evaluated,
                config.min_tx_count,
                assessment.unique_signers_evaluated,
                config.min_unique_signers,
                assessment.buy_count,
                config.min_buy_count,
            ),
        )
    };
    let (core1_passed, core2_passed, core3_passed) = if matches!(
        verdict_type,
        GatekeeperVerdictType::TimeoutNoData | GatekeeperVerdictType::TimeoutPhase1
    ) {
        (false, false, false)
    } else {
        (
            diagnostics.core1_passed,
            diagnostics.core2_passed,
            diagnostics.core3_passed,
        )
    };

    GatekeeperDecision {
        hard_fail_reason,
        core1_passed,
        core2_passed,
        core3_passed,
        soft_signals: diagnostics.soft_signals,
        soft_points: diagnostics.soft_points,
        max_soft_points_possible: diagnostics.max_soft_points_possible,
        effective_max_soft_points: diagnostics.effective_max_soft_points,
        dev_unknown: diagnostics.dev_unknown,
        sybil_policy: diagnostics.sybil_policy,
        alpha_gate: AlphaGateDiagnostics::not_run(config.enable_alpha_gate),
        prosperity_filter: ProsperityFilterDiagnostics::not_run(config.enable_prosperity_filter),
        total_soft_points,
        verdict_type,
        verdict_buy: false,
        reason_chain,
        gatekeeper_strength: None,
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SybilMetric {
    Ftdi,
    Dbia,
    Sfd,
    Des,
    Cpv,
    Fsc,
}

pub fn evaluate_curve_gate(
    features: &MaterializedFeatureSet,
    config: &GatekeeperV2Config,
) -> CurveGateOutcome {
    if features.curve_readiness.is_ready {
        return CurveGateOutcome::Ready;
    }

    match features.curve_readiness.freshness {
        ghost_core::CurveFreshnessState::Fresh | ghost_core::CurveFreshnessState::Committed => {
            CurveGateOutcome::Ready
        }
        ghost_core::CurveFreshnessState::Unknown => {
            if !config.curve_require_for_buy {
                CurveGateOutcome::Reject {
                    reason_label: "unknown_curve_reject",
                    terminal_outcome: "rejected",
                    reason: "HARD_FAIL: CURVE_UNKNOWN_REJECTED_BY_POLICY",
                }
            } else if features.curve_readiness.wait_elapsed_ms.unwrap_or_default()
                < config.curve_wait_ms
            {
                CurveGateOutcome::Pending {
                    reason_label: "unknown_curve_pending",
                }
            } else {
                CurveGateOutcome::Reject {
                    reason_label: "unknown_curve_timeout",
                    terminal_outcome: "timed_out",
                    reason: "HARD_FAIL: CURVE_NOT_READY_TIMEOUT",
                }
            }
        }
        ghost_core::CurveFreshnessState::Stale => match config.stale_fallback {
            ShadowLedgerStaleFallback::Reject => CurveGateOutcome::Reject {
                reason_label: "stale_curve_reject",
                terminal_outcome: "rejected",
                reason: "HARD_FAIL: CURVE_STALE_REJECTED",
            },
            ShadowLedgerStaleFallback::PendingCurve => {
                if features.curve_readiness.wait_elapsed_ms.unwrap_or_default()
                    < config.curve_wait_ms
                {
                    CurveGateOutcome::Pending {
                        reason_label: "stale_curve_pending",
                    }
                } else {
                    CurveGateOutcome::Reject {
                        reason_label: "stale_curve_timeout",
                        terminal_outcome: "timed_out",
                        reason: "HARD_FAIL: CURVE_STALE_TIMEOUT",
                    }
                }
            }
            ShadowLedgerStaleFallback::UseStaleWithWarning => {
                if features.curve_readiness.finality.is_finalized() {
                    CurveGateOutcome::Ready
                } else if features.curve_readiness.wait_elapsed_ms.unwrap_or_default()
                    < config.curve_wait_ms
                {
                    CurveGateOutcome::Pending {
                        reason_label: "stale_curve_pending",
                    }
                } else {
                    CurveGateOutcome::Reject {
                        reason_label: "stale_curve_timeout",
                        terminal_outcome: "timed_out",
                        reason: "HARD_FAIL: CURVE_STALE_TIMEOUT",
                    }
                }
            }
        },
    }
}

fn velocity_profile_from_features(features: &MaterializedFeatureSet) -> Option<VelocityProfile> {
    (features.tx_intel_features.tx_count >= 2).then(|| VelocityProfile {
        avg_interval_ms: features.tx_intel_features.avg_interval_ms,
        interval_std_dev: features.tx_intel_features.avg_interval_ms
            * features.tx_intel_features.interval_cv,
        interval_cv: features.tx_intel_features.interval_cv,
        burst_ratio: features.tx_intel_features.burst_ratio,
        timing_entropy: features.tx_intel_features.timing_entropy,
        is_accelerating: matches!(
            features.checkpoint_features.buy_pressure_trend,
            TrendDirection::Rising
        ),
    })
}

fn signer_diversity_from_features(
    features: &MaterializedFeatureSet,
) -> Option<SignerDiversityProfile> {
    (features.tx_intel_features.tx_count >= 2).then(|| SignerDiversityProfile {
        unique_ratio: features.tx_intel_features.unique_signer_ratio,
        hhi: features.tx_intel_features.hhi,
        max_tx_per_signer: features.tx_intel_features.max_tx_per_signer as usize,
        volume_gini: features.tx_intel_features.volume_gini,
        top3_volume_pct: features.tx_intel_features.top3_volume_pct,
        same_ms_tx_ratio: features.tx_intel_features.same_ms_tx_ratio,
    })
}

fn volume_sanity_from_features(features: &MaterializedFeatureSet) -> Option<VolumeSanityProfile> {
    (features.tx_intel_features.tx_count > 0).then(|| VolumeSanityProfile {
        buy_ratio: features.tx_intel_features.buy_ratio,
        avg_tx_sol: features.tx_intel_features.avg_tx_sol,
        volume_cv: features.tx_intel_features.volume_cv,
        total_volume_sol: features.tx_intel_features.total_volume_sol,
        min_tx_sol: features.tx_intel_features.min_tx_sol,
        max_tx_sol: features.tx_intel_features.max_tx_sol,
        sol_buy_ratio: features.tx_intel_features.sol_buy_ratio,
        max_consecutive_buys: features.tx_intel_features.max_consecutive_buys as usize,
    })
}

fn dev_behavior_from_features(features: &MaterializedFeatureSet) -> Option<DevBehaviorProfile> {
    (features.tx_intel_features.tx_count > 0).then(|| DevBehaviorProfile {
        dev_wallet_known: features.tx_intel_features.dev_wallet_known,
        dev_buy_total_sol: features.tx_intel_features.dev_buy_sol,
        dev_initial_buy_tokens: features.tx_intel_features.dev_initial_buy_tokens,
        dev_tx_count: features.tx_intel_features.dev_tx_count as usize,
        dev_tx_ratio: features.tx_intel_features.dev_tx_ratio,
        dev_volume_ratio: features.tx_intel_features.dev_volume_ratio,
        dev_has_sold: features.tx_intel_features.dev_has_sold,
        dev_is_first_buyer: features.tx_intel_features.dev_is_first_buyer,
    })
}

fn bonding_curve_from_features(features: &MaterializedFeatureSet) -> Option<BondingCurveDynamics> {
    let current_price = features.account_features.price_sol;
    let price_change_pct = if features
        .checkpoint_features
        .price_change_from_first_checkpoint_pct
        .abs()
        > f64::EPSILON
    {
        features
            .checkpoint_features
            .price_change_from_first_checkpoint_pct
    } else {
        features.account_features.price_change_since_t0_pct
    };
    let price_change_ratio = 1.0 + (price_change_pct / 100.0);
    let initial_price = if price_change_ratio > 0.0 {
        current_price / price_change_ratio
    } else {
        current_price
    };
    let max_price = features
        .checkpoint_features
        .price_trajectory
        .iter()
        .copied()
        .fold(current_price.max(initial_price), f64::max);
    let price_data_points = features
        .curve_readiness
        .price_sample_count
        .max(features.checkpoint_features.trajectory_checkpoint_count)
        as usize;

    (current_price > 0.0 || price_data_points > 0).then(|| BondingCurveDynamics {
        initial_price,
        current_price,
        max_price,
        price_change_ratio,
        max_single_tx_price_impact_pct: features.checkpoint_features.single_tx_max_price_impact_pct,
        max_single_sell_impact_pct: features.checkpoint_features.max_single_sell_impact_pct,
        current_market_cap_sol: features.account_features.market_cap_sol,
        market_cap_change_ratio: price_change_ratio,
        bonding_progress_pct: features.account_features.bonding_progress * 100.0,
        curve_data_known: features.curve_readiness.curve_data_known,
        curve_finality: features.curve_readiness.finality,
        price_data_points,
    })
}

fn compute_soft_signals(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> SoftSignals {
    let mut signals = SoftSignals::default();

    if let Some(velocity) = assessment.phase2_velocity.as_ref() {
        signals.low_interval_cv = velocity.interval_cv < config.min_interval_cv;
        signals.high_interval_cv = velocity.interval_cv > config.max_interval_cv;
        signals.low_timing_entropy = velocity.timing_entropy < config.min_timing_entropy;
        signals.high_timing_entropy = velocity.timing_entropy > config.max_timing_entropy;
        signals.avg_interval_out_of_range = velocity.avg_interval_ms < config.min_avg_interval_ms
            || velocity.avg_interval_ms > config.max_avg_interval_ms;
        signals.high_burst_ratio = velocity.burst_ratio > config.max_burst_ratio;
    }

    if let Some(diversity) = assessment.phase3_diversity.as_ref() {
        signals.bundle_suspicion = diversity.same_ms_tx_ratio > config.max_same_ms_tx_ratio;
        signals.cabal_suspicion = diversity.hhi > config.max_hhi;
        signals.top3_dominance = diversity.top3_volume_pct > config.max_top3_volume_pct;
        signals.high_volume_gini = diversity.volume_gini > config.max_volume_gini;
        signals.unique_ratio_out_of_range = diversity.unique_ratio < config.min_unique_ratio
            || diversity.unique_ratio > config.max_unique_ratio;
        signals.high_tx_per_signer =
            diversity.max_tx_per_signer as u64 > config.max_tx_per_signer as u64;
    }

    signals.low_dust_count = assessment.dust_filtered_count < config.min_dust_filtered_count;
    signals
}

fn has_degraded_reason(reasons: &[String], target: &str) -> bool {
    reasons.iter().any(|reason| reason == target)
}

fn sybil_metric_is_actionable(sybil: &SybilResistanceFeatures, metric: SybilMetric) -> bool {
    let value_present = match metric {
        SybilMetric::Ftdi => sybil.fee_topology_diversity_index.is_some(),
        SybilMetric::Dbia => sybil.dev_buyer_infrastructure_affinity.is_some(),
        SybilMetric::Sfd => sybil.spend_fraction_divergence.is_some(),
        SybilMetric::Des => sybil.demand_elasticity_score.is_some(),
        SybilMetric::Cpv => sybil.signer_cross_pool_velocity.is_some(),
        SybilMetric::Fsc => sybil.funding_source_concentration.is_some(),
    };
    if !value_present {
        return false;
    }

    let degraded = match metric {
        SybilMetric::Ftdi => {
            has_degraded_reason(&sybil.degraded_reasons, FTDI_INSUFFICIENT_BUYS_REASON)
                || has_degraded_reason(
                    &sybil.degraded_reasons,
                    FTDI_RAW_FEE_TOPOLOGY_UNAVAILABLE_REASON,
                )
        }
        SybilMetric::Dbia => {
            has_degraded_reason(&sybil.degraded_reasons, DBIA_NO_DEV_BUY_REASON)
                || has_degraded_reason(&sybil.degraded_reasons, DBIA_INSUFFICIENT_BUYERS_REASON)
                || has_degraded_reason(
                    &sybil.degraded_reasons,
                    DBIA_RAW_FINGERPRINT_UNAVAILABLE_REASON,
                )
        }
        SybilMetric::Sfd => {
            has_degraded_reason(&sybil.degraded_reasons, SFD_INSUFFICIENT_BUYS_REASON)
        }
        SybilMetric::Des => {
            has_degraded_reason(&sybil.degraded_reasons, DES_INSUFFICIENT_BUYS_REASON)
                || has_degraded_reason(&sybil.degraded_reasons, DES_CURVE_DATA_UNAVAILABLE_REASON)
                || has_degraded_reason(&sybil.degraded_reasons, DES_SLOT_ORDER_UNAVAILABLE_REASON)
        }
        SybilMetric::Cpv => {
            has_degraded_reason(
                &sybil.degraded_reasons,
                CPV_ROLLING_STATE_UNAVAILABLE_REASON,
            ) || has_degraded_reason(&sybil.degraded_reasons, CPV_INSUFFICIENT_SIGNERS_REASON)
        }
        SybilMetric::Fsc => {
            has_degraded_reason(
                &sybil.degraded_reasons,
                FSC_ROLLING_STATE_UNAVAILABLE_REASON,
            ) || has_degraded_reason(
                &sybil.degraded_reasons,
                FSC_INSUFFICIENT_KNOWN_SOURCES_REASON,
            ) || has_degraded_reason(
                &sybil.degraded_reasons,
                FSC_FUNDING_STREAM_UNAVAILABLE_REASON,
            )
        }
    };

    !degraded
}

fn compute_sybil_soft_signals(
    sybil: &SybilResistanceFeatures,
    config: &GatekeeperV2Config,
) -> SybilSoftSignals {
    let mut signals = SybilSoftSignals::default();

    if sybil_metric_is_actionable(sybil, SybilMetric::Ftdi) {
        signals.low_ftdi = sybil
            .fee_topology_diversity_index
            .is_some_and(|value| value < config.min_fee_topology_diversity_index);
    }
    if sybil_metric_is_actionable(sybil, SybilMetric::Dbia) {
        signals.high_dbia = sybil
            .dev_buyer_infrastructure_affinity
            .is_some_and(|value| value > config.max_dev_buyer_infrastructure_affinity);
    }
    if sybil_metric_is_actionable(sybil, SybilMetric::Sfd) {
        signals.low_sfd = sybil
            .spend_fraction_divergence
            .is_some_and(|value| value < config.min_spend_fraction_divergence);
    }
    if sybil_metric_is_actionable(sybil, SybilMetric::Des) {
        signals.low_des = sybil
            .demand_elasticity_score
            .is_some_and(|value| value < config.min_demand_elasticity_score);
    }
    if sybil_metric_is_actionable(sybil, SybilMetric::Cpv) {
        signals.high_cpv = sybil
            .signer_cross_pool_velocity
            .is_some_and(|value| value > config.max_signer_cross_pool_velocity);
    }
    if sybil_metric_is_actionable(sybil, SybilMetric::Fsc) {
        signals.high_fsc = sybil
            .funding_source_concentration
            .is_some_and(|value| value > config.max_funding_source_concentration);
    }

    signals
}

fn compute_sybil_interference_patterns(
    signals: &SybilSoftSignals,
) -> Vec<SybilInterferencePattern> {
    let mut patterns = Vec::new();
    if signals.high_dbia && signals.low_ftdi {
        patterns.push(SybilInterferencePattern::HighDbiaLowFtdi);
    }
    if signals.low_des && signals.low_sfd {
        patterns.push(SybilInterferencePattern::LowDesLowSfd);
    }
    if signals.high_cpv && signals.low_des {
        patterns.push(SybilInterferencePattern::HighCpvLowDes);
    }
    if signals.high_fsc && signals.high_cpv {
        patterns.push(SybilInterferencePattern::HighFscHighCpv);
    }
    if signals.high_dbia && signals.low_ftdi && signals.low_sfd {
        patterns.push(SybilInterferencePattern::HighDbiaLowFtdiLowSfd);
    }
    if signals.high_fsc && signals.high_cpv && (signals.low_des || signals.low_sfd) {
        patterns.push(SybilInterferencePattern::HighFscHighCpvLowDesOrLowSfd);
    }
    patterns
}

fn sybil_max_possible_points(config: &GatekeeperV2Config) -> u16 {
    [
        config.soft_penalty_low_ftdi as u16,
        config.soft_penalty_high_dbia as u16,
        config.soft_penalty_low_sfd as u16,
        config.soft_penalty_inelastic_demand as u16,
        config.soft_penalty_high_cpv as u16,
        config.soft_penalty_high_fsc as u16,
        config.soft_penalty_high_dbia_low_ftdi_combo as u16,
        config.soft_penalty_low_des_low_sfd_combo as u16,
        config.soft_penalty_high_cpv_low_des_combo as u16,
        config.soft_penalty_high_fsc_high_cpv_combo as u16,
    ]
    .into_iter()
    .sum()
}

fn calculate_sybil_soft_points(
    signals: &SybilSoftSignals,
    patterns: &[SybilInterferencePattern],
    config: &GatekeeperV2Config,
) -> u16 {
    let mut points = 0u16;

    if signals.low_ftdi {
        points += config.soft_penalty_low_ftdi as u16;
    }
    if signals.high_dbia {
        points += config.soft_penalty_high_dbia as u16;
    }
    if signals.low_sfd {
        points += config.soft_penalty_low_sfd as u16;
    }
    if signals.low_des {
        points += config.soft_penalty_inelastic_demand as u16;
    }
    if signals.high_cpv {
        points += config.soft_penalty_high_cpv as u16;
    }
    if signals.high_fsc {
        points += config.soft_penalty_high_fsc as u16;
    }
    if patterns.contains(&SybilInterferencePattern::HighDbiaLowFtdi) {
        points += config.soft_penalty_high_dbia_low_ftdi_combo as u16;
    }
    if patterns.contains(&SybilInterferencePattern::LowDesLowSfd) {
        points += config.soft_penalty_low_des_low_sfd_combo as u16;
    }
    if patterns.contains(&SybilInterferencePattern::HighCpvLowDes) {
        points += config.soft_penalty_high_cpv_low_des_combo as u16;
    }
    if patterns.contains(&SybilInterferencePattern::HighFscHighCpv) {
        points += config.soft_penalty_high_fsc_high_cpv_combo as u16;
    }

    points
}

fn collect_sybil_component_activity(
    signals: &SybilSoftSignals,
    patterns: &[SybilInterferencePattern],
    config: &GatekeeperV2Config,
) -> Vec<(SybilLeadSignal, u16)> {
    let mut components = Vec::new();

    if signals.low_ftdi {
        let points = config.soft_penalty_low_ftdi as u16;
        if points > 0 {
            components.push((SybilLeadSignal::LowFtdi, points));
        }
    }
    if signals.high_dbia {
        let points = config.soft_penalty_high_dbia as u16;
        if points > 0 {
            components.push((SybilLeadSignal::HighDbia, points));
        }
    }
    if signals.low_sfd {
        let points = config.soft_penalty_low_sfd as u16;
        if points > 0 {
            components.push((SybilLeadSignal::LowSfd, points));
        }
    }
    if signals.low_des {
        let points = config.soft_penalty_inelastic_demand as u16;
        if points > 0 {
            components.push((SybilLeadSignal::LowDes, points));
        }
    }
    if signals.high_cpv {
        let points = config.soft_penalty_high_cpv as u16;
        if points > 0 {
            components.push((SybilLeadSignal::HighCpv, points));
        }
    }
    if signals.high_fsc {
        let points = config.soft_penalty_high_fsc as u16;
        if points > 0 {
            components.push((SybilLeadSignal::HighFsc, points));
        }
    }

    for pattern in patterns {
        let (lead, points) = match pattern {
            SybilInterferencePattern::HighDbiaLowFtdi => (
                SybilLeadSignal::HighDbiaLowFtdi,
                config.soft_penalty_high_dbia_low_ftdi_combo as u16,
            ),
            SybilInterferencePattern::LowDesLowSfd => (
                SybilLeadSignal::LowDesLowSfd,
                config.soft_penalty_low_des_low_sfd_combo as u16,
            ),
            SybilInterferencePattern::HighCpvLowDes => (
                SybilLeadSignal::HighCpvLowDes,
                config.soft_penalty_high_cpv_low_des_combo as u16,
            ),
            SybilInterferencePattern::HighFscHighCpv => (
                SybilLeadSignal::HighFscHighCpv,
                config.soft_penalty_high_fsc_high_cpv_combo as u16,
            ),
            SybilInterferencePattern::HighDbiaLowFtdiLowSfd => (
                SybilLeadSignal::HighDbiaLowFtdiLowSfd,
                config.soft_penalty_high_dbia_low_ftdi_combo as u16,
            ),
            SybilInterferencePattern::HighFscHighCpvLowDesOrLowSfd => (
                SybilLeadSignal::HighFscHighCpvLowDesOrLowSfd,
                config.soft_penalty_high_fsc_high_cpv_combo as u16,
            ),
        };
        if points > 0 {
            components.push((lead, points));
        }
    }

    components
}

fn sybil_lead_priority(signal: SybilLeadSignal) -> u8 {
    match signal {
        SybilLeadSignal::LowDesLowSfd => 0,
        SybilLeadSignal::LowDes => 1,
        SybilLeadSignal::HighDbiaLowFtdiLowSfd => 2,
        SybilLeadSignal::HighDbiaLowFtdi => 3,
        SybilLeadSignal::LowSfd => 4,
        SybilLeadSignal::HighCpvLowDes => 5,
        SybilLeadSignal::HighFscHighCpvLowDesOrLowSfd => 6,
        SybilLeadSignal::HighFscHighCpv => 7,
        SybilLeadSignal::HighDbia => 8,
        SybilLeadSignal::LowFtdi => 9,
        SybilLeadSignal::HighFsc => 10,
        SybilLeadSignal::HighCpv => 11,
    }
}

fn select_sybil_lead_signal(components: &[(SybilLeadSignal, u16)]) -> Option<SybilLeadSignal> {
    let mut best: Option<(SybilLeadSignal, u16)> = None;
    for &(signal, points) in components {
        match best {
            None => best = Some((signal, points)),
            Some((best_signal, best_points)) => {
                if points > best_points
                    || (points == best_points
                        && sybil_lead_priority(signal) < sybil_lead_priority(best_signal))
                {
                    best = Some((signal, points));
                }
            }
        }
    }
    best.map(|(signal, _)| signal)
}

pub(crate) fn build_sybil_policy_diagnostics(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
    dev_unknown: bool,
) -> SybilPolicyDiagnostics {
    let sybil = &assessment.feature_snapshot.sybil_resistance;
    let soft_signals = compute_sybil_soft_signals(sybil, config);
    let interference_patterns = compute_sybil_interference_patterns(&soft_signals);
    let component_activity =
        collect_sybil_component_activity(&soft_signals, &interference_patterns, config);
    let soft_points = calculate_sybil_soft_points(&soft_signals, &interference_patterns, config);
    let effective_max_soft_points = if dev_unknown {
        config.dev_unknown_max_sybil_soft_points
    } else {
        config.max_sybil_soft_points
    };

    SybilPolicyDiagnostics {
        enabled: config.enable_sybil_interference_layer,
        combo_veto_enabled: config.enable_sybil_combo_veto,
        soft_signals,
        soft_points,
        max_soft_points_possible: sybil_max_possible_points(config),
        effective_max_soft_points,
        lead_signal: select_sybil_lead_signal(&component_activity),
        interference_patterns,
        meta_score: config.emit_sybil_meta_score.then_some(soft_points),
        metric_degraded_reasons: sybil.degraded_reasons.clone(),
    }
}

pub(crate) fn sybil_combo_veto_reason(
    diagnostics: &SybilPolicyDiagnostics,
    config: &GatekeeperV2Config,
) -> Option<String> {
    if !diagnostics.enabled || !diagnostics.combo_veto_enabled || !config.enable_sybil_combo_veto {
        return None;
    }

    let signals = &diagnostics.soft_signals;
    if diagnostics
        .interference_patterns
        .contains(&SybilInterferencePattern::HighDbiaLowFtdiLowSfd)
    {
        return Some("SYBIL_INTERFERENCE: pattern=HIGH_DBIA_LOW_FTDI_LOW_SFD".to_string());
    }
    if signals.low_des && signals.low_sfd && (signals.high_dbia || signals.low_ftdi) {
        return Some("SYBIL_INTERFERENCE: pattern=LOW_DES_LOW_SFD_STRUCTURAL_SUPPORT".to_string());
    }
    let fsc_ready = !config.require_ready_fsc_for_combo_veto
        || !diagnostics.metric_degraded_reasons.iter().any(|reason| {
            reason == FSC_ROLLING_STATE_UNAVAILABLE_REASON
                || reason == FSC_INSUFFICIENT_KNOWN_SOURCES_REASON
                || reason == FSC_FUNDING_STREAM_UNAVAILABLE_REASON
        });
    if fsc_ready && signals.high_fsc && signals.high_cpv && (signals.low_des || signals.low_sfd) {
        return Some(
            "SYBIL_INTERFERENCE: pattern=HIGH_FSC_HIGH_CPV_LOW_DES_OR_LOW_SFD".to_string(),
        );
    }

    None
}

fn diversity_phase_passes(diversity: &SignerDiversityProfile, config: &GatekeeperV2Config) -> bool {
    diversity.unique_ratio >= config.min_unique_ratio
        && diversity.unique_ratio <= config.max_unique_ratio
        && diversity.hhi <= config.max_hhi
        && diversity.max_tx_per_signer as u64 <= config.max_tx_per_signer as u64
        && diversity.volume_gini >= config.min_volume_gini
        && diversity.volume_gini <= config.max_volume_gini
        && diversity.top3_volume_pct <= config.max_top3_volume_pct
        && diversity.same_ms_tx_ratio <= config.max_same_ms_tx_ratio
}

fn phase4_fingerprint_thresholds_pass(
    avg_inner_ix_count_50tx: Option<f64>,
    sell_buy_ratio: Option<f64>,
    compute_unit_cluster_dominance: Option<f64>,
    static_fee_profile_ratio: Option<f64>,
    fixed_size_buy_ratio: Option<f64>,
    jito_tip_intensity: Option<f64>,
    early_slot_volume_dominance_buy: Option<f64>,
    early_top3_buy_volume_pct_3s: Option<f64>,
    config: &GatekeeperV2Config,
) -> bool {
    avg_inner_ix_count_50tx.map_or(true, |v| v >= config.min_avg_inner_ix_count_50tx)
        && avg_inner_ix_count_50tx.map_or(true, |v| v <= config.max_avg_inner_ix_count_50tx)
        && sell_buy_ratio.map_or(true, |v| v >= config.min_sell_buy_ratio)
        && sell_buy_ratio.map_or(true, |v| v <= config.max_sell_buy_ratio)
        && compute_unit_cluster_dominance
            .map_or(true, |v| v >= config.min_compute_unit_cluster_dominance)
        && compute_unit_cluster_dominance
            .map_or(true, |v| v <= config.max_compute_unit_cluster_dominance)
        && static_fee_profile_ratio.map_or(true, |v| v >= config.min_static_fee_profile_ratio)
        && static_fee_profile_ratio.map_or(true, |v| v <= config.max_static_fee_profile_ratio)
        && fixed_size_buy_ratio.map_or(true, |v| v >= config.min_fixed_size_buy_ratio)
        && jito_tip_intensity.map_or(true, |v| v >= config.min_jito_tip_intensity)
        && jito_tip_intensity.map_or(true, |v| v <= config.max_jito_tip_intensity)
        && early_slot_volume_dominance_buy
            .map_or(true, |v| v <= config.max_early_slot_volume_dominance_buy)
        && early_top3_buy_volume_pct_3s
            .map_or(true, |v| v <= config.max_early_top3_buy_volume_pct_3s)
}

fn alpha_fingerprint_phase4_passes(
    alpha: &ghost_core::checkpoint::AlphaFingerprintFeatures,
    config: &GatekeeperV2Config,
) -> bool {
    phase4_fingerprint_thresholds_pass(
        alpha.avg_inner_ix_count_50tx,
        alpha.sell_buy_ratio,
        alpha.compute_unit_cluster_dominance,
        alpha.static_fee_profile_ratio,
        alpha.fixed_size_buy_ratio,
        alpha.jito_tip_intensity,
        alpha.early_slot_volume_dominance_buy,
        alpha.early_top3_buy_volume_pct_3s,
        config,
    )
}

fn early_fingerprint_phase4_passes(
    fp: &EarlyFingerprintMetrics,
    config: &GatekeeperV2Config,
) -> bool {
    phase4_fingerprint_thresholds_pass(
        fp.avg_inner_ix_count_50tx,
        fp.sell_buy_ratio,
        fp.compute_unit_cluster_dominance,
        fp.static_fee_profile_ratio,
        fp.fixed_size_buy_ratio,
        fp.jito_tip_intensity,
        fp.early_slot_volume_dominance_buy,
        fp.early_top3_buy_volume_pct_3s,
        config,
    )
}

fn volume_phase_passes_base(
    volume: &VolumeSanityProfile,
    fingerprint_ok: bool,
    config: &GatekeeperV2Config,
) -> bool {
    volume.buy_ratio >= config.min_buy_ratio
        && volume.buy_ratio <= config.max_buy_ratio
        && volume.avg_tx_sol >= config.min_avg_tx_sol
        && volume.avg_tx_sol <= config.max_avg_tx_sol
        && volume.volume_cv >= config.min_volume_cv
        && volume.volume_cv <= config.max_volume_cv
        && volume.total_volume_sol >= config.min_total_volume_sol
        && volume.total_volume_sol <= config.max_total_volume_sol
        && volume.sol_buy_ratio >= config.min_sol_buy_ratio
        && volume.max_consecutive_buys as u64 >= config.min_consecutive_buys as u64
        && fingerprint_ok
}

fn volume_phase_passes(
    volume: &VolumeSanityProfile,
    fingerprint: Option<&EarlyFingerprintMetrics>,
    config: &GatekeeperV2Config,
) -> bool {
    let fingerprint_ok = fingerprint.map_or(true, |fp| early_fingerprint_phase4_passes(fp, config));
    volume_phase_passes_base(volume, fingerprint_ok, config)
}

fn compute_core3_pass(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
    dev_unknown: bool,
) -> bool {
    if dev_unknown {
        let Some(curve) = assessment.phase6_curve.as_ref() else {
            return false;
        };
        if curve.price_data_points < 2 {
            return false;
        }

        let price_ok = curve.price_change_ratio <= config.max_price_change_ratio
            && curve.max_single_tx_price_impact_pct
                <= config.dev_unknown_max_single_tx_price_impact_pct
            && curve.max_single_sell_impact_pct <= config.max_single_sell_impact_pct
            && if curve.curve_data_known {
                curve.current_market_cap_sol >= config.dev_unknown_min_market_cap_sol
            } else {
                true
            };

        let bonding_ok = if curve.curve_data_known {
            curve.bonding_progress_pct >= config.min_bonding_progress_pct
                && curve.bonding_progress_pct <= config.max_bonding_progress_pct
        } else {
            true
        };

        assessment.phase4_passed && price_ok && bonding_ok
    } else {
        assessment.phase5_passed && assessment.phase6_passed
    }
}

#[derive(Debug, Clone)]
struct PolicyDiagnostics {
    core1_passed: bool,
    core2_passed: bool,
    core3_passed: bool,
    soft_signals: SoftSignals,
    soft_points: u8,
    max_soft_points_possible: u8,
    effective_max_soft_points: u8,
    dev_unknown: bool,
    sybil_policy: SybilPolicyDiagnostics,
}

fn build_policy_diagnostics(
    assessment: &GatekeeperAssessment,
    config: &GatekeeperV2Config,
) -> PolicyDiagnostics {
    let dev_unknown = assessment
        .phase5_dev
        .as_ref()
        .map(|dev| !dev.dev_wallet_known)
        .unwrap_or(true);
    let core1_passed = assessment.phase1_passed;
    let core2_passed = assessment.phase4_passed;
    let core3_passed = compute_core3_pass(assessment, config, dev_unknown);
    let soft_signals = compute_soft_signals(assessment, config);
    let max_soft_points_possible = SoftSignals::max_possible_points(
        config.soft_weight_timing,
        config.soft_weight_manipulation,
        config.soft_weight_diversity,
        config.soft_weight_ecosystem,
    );
    let soft_points = soft_signals.weighted_score(
        config.soft_weight_timing,
        config.soft_weight_manipulation,
        config.soft_weight_diversity,
        config.soft_weight_ecosystem,
    );
    let effective_max_soft_points = if dev_unknown {
        config.dev_unknown_max_soft_points
    } else {
        config.max_soft_points
    };
    let sybil_policy = build_sybil_policy_diagnostics(assessment, config, dev_unknown);

    PolicyDiagnostics {
        core1_passed,
        core2_passed,
        core3_passed,
        soft_signals,
        soft_points,
        max_soft_points_possible,
        effective_max_soft_points,
        dev_unknown,
        sybil_policy,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ghost_core::checkpoint::AlphaFingerprintFeatures;
    use ghost_core::tx_intelligence::types::{
        SFD_PARTIAL_BALANCE_COVERAGE_REASON, SFD_ZERO_PREBALANCE_SKIPPED_REASON,
    };

    fn assessment_with_sybil(sybil: SybilResistanceFeatures) -> GatekeeperAssessment {
        let mut feature_snapshot = MaterializedFeatureSet::default();
        feature_snapshot.sybil_resistance = sybil;

        GatekeeperAssessment {
            phase1_passed: false,
            phase2_velocity: None,
            phase2_passed: false,
            phase3_diversity: None,
            phase3_passed: false,
            phase4_volume: None,
            phase4_passed: false,
            phase5_dev: None,
            phase5_passed: false,
            phase6_curve: None,
            phase6_passed: false,
            phases_passed: 0,
            hard_reject_reason: None,
            total_tx_evaluated: 0,
            unique_tx_evaluated: 0,
            unique_signers_evaluated: 0,
            observation_duration_ms: 0,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 0,
            buy_count: 0,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot,
            checkpoint_count: 0,
            trajectory_available: false,
        }
    }

    fn alpha_config() -> GatekeeperV2Config {
        let mut config = GatekeeperV2Config::default();
        config.enable_alpha_gate = true;
        config.enable_sybil_interference_layer = false;
        config.enable_sybil_combo_veto = false;
        config.max_soft_points = u8::MAX;
        config.dev_unknown_max_soft_points = u8::MAX;
        config.max_sybil_soft_points = u8::MAX;
        config.dev_unknown_max_sybil_soft_points = u8::MAX;
        config
    }

    fn alpha_ready_assessment() -> GatekeeperAssessment {
        let mut feature_snapshot = MaterializedFeatureSet::default();
        feature_snapshot.tx_intel_features.tx_count = 30;
        feature_snapshot.tx_intel_features.buy_count = 20;
        feature_snapshot.tx_intel_features.unique_signers = 18;
        feature_snapshot.tx_intel_features.buy_ratio = 0.78;
        feature_snapshot.tx_intel_features.avg_interval_ms = 150.0;
        feature_snapshot.tx_intel_features.timing_entropy = 2.10;
        feature_snapshot.tx_intel_features.burst_ratio = 0.32;
        feature_snapshot.alpha_fingerprint = AlphaFingerprintFeatures {
            avg_inner_ix_count_50tx: None,
            sell_buy_ratio: None,
            compute_unit_cluster_dominance: None,
            static_fee_profile_ratio: None,
            jito_tip_intensity: Some(0.25),
            early_slot_volume_dominance_buy: Some(0.72),
            early_top3_buy_volume_pct_3s: None,
            fixed_size_buy_ratio: Some(0.12),
            flipper_presence_ratio: Some(0.06),
        };

        GatekeeperAssessment {
            phase1_passed: true,
            phase2_velocity: Some(VelocityProfile {
                avg_interval_ms: 150.0,
                interval_std_dev: 60.0,
                interval_cv: 0.40,
                burst_ratio: 0.32,
                timing_entropy: 2.10,
                is_accelerating: true,
            }),
            phase2_passed: true,
            phase3_diversity: Some(SignerDiversityProfile {
                unique_ratio: 0.72,
                hhi: 0.08,
                max_tx_per_signer: 2,
                volume_gini: 0.34,
                top3_volume_pct: 0.42,
                same_ms_tx_ratio: 0.08,
            }),
            phase3_passed: true,
            phase4_volume: Some(VolumeSanityProfile {
                buy_ratio: 0.78,
                avg_tx_sol: 0.60,
                volume_cv: 0.28,
                total_volume_sol: 6.2,
                min_tx_sol: 0.03,
                max_tx_sol: 1.20,
                sol_buy_ratio: 0.78,
                max_consecutive_buys: 6,
            }),
            phase4_passed: true,
            phase5_dev: Some(DevBehaviorProfile {
                dev_wallet_known: true,
                dev_buy_total_sol: 0.30,
                dev_initial_buy_tokens: Some(5_000.0),
                dev_tx_count: 1,
                dev_tx_ratio: 0.03,
                dev_volume_ratio: 0.04,
                dev_has_sold: false,
                dev_is_first_buyer: false,
            }),
            phase5_passed: true,
            phase6_curve: Some(BondingCurveDynamics {
                initial_price: 0.0001,
                current_price: 0.00017,
                max_price: 0.00018,
                price_change_ratio: 1.70,
                max_single_tx_price_impact_pct: 8.0,
                max_single_sell_impact_pct: 9.0,
                current_market_cap_sol: 120.0,
                market_cap_change_ratio: 1.6,
                bonding_progress_pct: 8.0,
                curve_data_known: true,
                curve_finality: ghost_core::CurveFinality::Finalized,
                price_data_points: 6,
            }),
            phase6_passed: true,
            phases_passed: 6,
            hard_reject_reason: None,
            total_tx_evaluated: 30,
            unique_tx_evaluated: 28,
            unique_signers_evaluated: 18,
            observation_duration_ms: 8_000,
            finalize_lag_ms: 0,
            dust_filtered_count: 0,
            eval_count: 1,
            buy_count: 20,
            decision: None,
            early_fingerprint: None,
            curve_t0_event_ts_ms: None,
            curve_t0_clock_source: None,
            curve_wait_elapsed_ms: None,
            feature_snapshot,
            checkpoint_count: 0,
            trajectory_available: false,
        }
    }

    #[test]
    fn partial_balance_coverage_keeps_sfd_actionable() {
        let mut config = GatekeeperV2Config::default();
        config.min_spend_fraction_divergence = 0.25;

        let assessment = assessment_with_sybil(SybilResistanceFeatures {
            spend_fraction_divergence: Some(0.10),
            degraded_reasons: vec![SFD_PARTIAL_BALANCE_COVERAGE_REASON.to_string()],
            buy_sample_count: 4,
            signer_sample_count: 4,
            ..SybilResistanceFeatures::default()
        });

        let diagnostics = build_sybil_policy_diagnostics(&assessment, &config, false);

        assert!(diagnostics.soft_signals.low_sfd);
        assert_eq!(diagnostics.soft_points, config.soft_penalty_low_sfd as u16);
    }

    #[test]
    fn zero_prebalance_skip_keeps_sfd_actionable() {
        let mut config = GatekeeperV2Config::default();
        config.min_spend_fraction_divergence = 0.25;

        let assessment = assessment_with_sybil(SybilResistanceFeatures {
            spend_fraction_divergence: Some(0.12),
            degraded_reasons: vec![SFD_ZERO_PREBALANCE_SKIPPED_REASON.to_string()],
            buy_sample_count: 4,
            signer_sample_count: 4,
            ..SybilResistanceFeatures::default()
        });

        let diagnostics = build_sybil_policy_diagnostics(&assessment, &config, false);

        assert!(diagnostics.soft_signals.low_sfd);
        assert_eq!(diagnostics.soft_points, config.soft_penalty_low_sfd as u16);
    }

    #[test]
    fn insufficient_buys_reason_blocks_sfd_signal() {
        let mut config = GatekeeperV2Config::default();
        config.min_spend_fraction_divergence = 0.25;

        let assessment = assessment_with_sybil(SybilResistanceFeatures {
            spend_fraction_divergence: Some(0.10),
            degraded_reasons: vec![SFD_INSUFFICIENT_BUYS_REASON.to_string()],
            buy_sample_count: 2,
            signer_sample_count: 2,
            ..SybilResistanceFeatures::default()
        });

        let diagnostics = build_sybil_policy_diagnostics(&assessment, &config, false);

        assert!(!diagnostics.soft_signals.low_sfd);
        assert_eq!(diagnostics.soft_points, 0);
    }

    #[test]
    fn alpha_gate_rejects_low_momentum() {
        let config = alpha_config();
        let mut assessment = alpha_ready_assessment();
        assessment
            .feature_snapshot
            .tx_intel_features
            .avg_interval_ms = 620.0;
        assessment.feature_snapshot.tx_intel_features.burst_ratio = 0.02;
        assessment.feature_snapshot.tx_intel_features.timing_entropy = 1.20;

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectLowAlpha);
        assert_eq!(
            decision.alpha_gate.reject_trigger,
            Some(AlphaRejectTrigger::LowMomentum)
        );
        assert!(decision.alpha_gate.actionable);
        assert!(decision.alpha_gate.momentum.unwrap() < config.min_momentum);
    }

    #[test]
    fn alpha_gate_rejects_low_demand() {
        let config = alpha_config();
        let mut assessment = alpha_ready_assessment();
        assessment.feature_snapshot.tx_intel_features.unique_signers = 8;
        assessment.feature_snapshot.tx_intel_features.buy_ratio = 0.56;
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .fixed_size_buy_ratio = Some(0.90);
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .flipper_presence_ratio = Some(0.45);

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectLowAlpha);
        assert_eq!(
            decision.alpha_gate.reject_trigger,
            Some(AlphaRejectTrigger::LowDemand)
        );
        assert!(decision.alpha_gate.demand.unwrap() < config.min_demand);
    }

    #[test]
    fn alpha_gate_rejects_low_joint_when_scalars_individually_pass() {
        let mut config = alpha_config();
        config.min_alpha_joint = 0.35;

        let mut assessment = alpha_ready_assessment();
        assessment.feature_snapshot.tx_intel_features.buy_count = 18;
        assessment.feature_snapshot.tx_intel_features.unique_signers = 16;
        assessment.feature_snapshot.tx_intel_features.buy_ratio = 0.80;
        assessment
            .feature_snapshot
            .tx_intel_features
            .avg_interval_ms = 260.0;
        assessment.feature_snapshot.tx_intel_features.timing_entropy = 1.60;
        assessment.feature_snapshot.tx_intel_features.burst_ratio = 0.22;
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .jito_tip_intensity = Some(0.20);
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .early_slot_volume_dominance_buy = Some(0.68);
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .fixed_size_buy_ratio = Some(0.25);
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .flipper_presence_ratio = Some(0.08);
        assessment.buy_count = 18;

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::RejectLowAlpha);
        assert_eq!(
            decision.alpha_gate.reject_trigger,
            Some(AlphaRejectTrigger::LowJoint)
        );
        assert!(decision.alpha_gate.momentum.unwrap() >= config.min_momentum);
        assert!(decision.alpha_gate.demand.unwrap() >= config.min_demand);
        assert!(decision.alpha_gate.joint.unwrap() < config.min_alpha_joint);
    }

    #[test]
    fn alpha_gate_skips_when_sample_is_too_small() {
        let config = alpha_config();
        let mut assessment = alpha_ready_assessment();
        assessment.feature_snapshot.tx_intel_features.buy_count = 10;
        assessment
            .feature_snapshot
            .tx_intel_features
            .avg_interval_ms = 700.0;
        assessment.feature_snapshot.tx_intel_features.burst_ratio = 0.01;
        assessment.buy_count = 10;

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
        assert_eq!(decision.alpha_gate.skip_reason, Some("insufficient_sample"));
        assert_eq!(decision.alpha_gate.pass, Some(true));
        assert!(!decision.alpha_gate.actionable);
    }

    #[test]
    fn alpha_gate_skips_when_required_inputs_are_missing() {
        let config = alpha_config();
        let mut assessment = alpha_ready_assessment();
        assessment
            .feature_snapshot
            .alpha_fingerprint
            .jito_tip_intensity = None;

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
        assert_eq!(
            decision.alpha_gate.skip_reason,
            Some("missing_alpha_inputs")
        );
        assert_eq!(decision.alpha_gate.pass, Some(true));
        assert!(!decision.alpha_gate.actionable);
    }

    #[test]
    fn disabled_alpha_gate_preserves_buy_path() {
        let mut config = alpha_config();
        config.enable_alpha_gate = false;
        let mut assessment = alpha_ready_assessment();
        assessment
            .feature_snapshot
            .tx_intel_features
            .avg_interval_ms = 700.0;
        assessment.feature_snapshot.tx_intel_features.burst_ratio = 0.01;

        let decision = evaluate_policy_from_assessment(&assessment, &config);

        assert_eq!(decision.verdict_type, GatekeeperVerdictType::Buy);
        assert_eq!(decision.alpha_gate.pass, None);
        assert!(!decision.alpha_gate.enabled);
    }
}
