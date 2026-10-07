use super::Token;
use crate::session::PoolObservationSession;
use anyhow::Result;
use ghost_brain::config::GatekeeperV2Config;
use serde_json::{json, Map, Value};

// All requested scalar columns exist on every row. Missing measurements are
// null, not silently omitted and never imputed to zero.
pub(super) const REQUIRED: &[&str] = &[
    "tx_count",
    "buy_count",
    "sell_count",
    "total_volume_sol",
    "avg_tx_sol",
    "volume_cv",
    "volume_gini",
    "top3_volume_pct",
    "sell_buy_ratio",
    "buy_ratio",
    "sol_buy_ratio",
    "fixed_size_buy_ratio",
    "fixed_size_buy_ratio_1e4",
    "failed_tx_ratio",
    "avg_interval_ms",
    "interval_cv",
    "burst_ratio",
    "timing_entropy",
    "same_ms_tx_ratio",
    "consecutive_buys",
    "early_slot_volume_dominance_buy",
    "early_top3_buy_volume_pct_3s",
    "unique_signers",
    "unique_ratio",
    "hhi",
    "tx_per_signer",
    "successful_buy_signers",
    "signer_cross_pool_velocity",
    "cpv_other_pool_activity",
    "fee_topology_diversity_index",
    "fee_topology_diversity_index_v2",
    "dev_buyer_infrastructure_affinity",
    "spend_fraction_divergence",
    "funding_source_concentration",
    "market_cap_sol",
    "bonding_progress_pct",
    "price_change_ratio",
    "single_tx_price_impact_pct",
    "single_sell_impact_pct",
    "single_sell_impact_pct_observed",
    "dev_buy_sol",
    "dev_tx_ratio",
    "dev_volume_ratio",
    "dev_paperhand_latency_ms",
    "jito_tip_intensity",
    "delta_jito_tip_intensity_1s_to_30s",
    "delta_jito_tip_intensity_31s_to_300s",
    "compute_unit_cluster_dominance",
    "static_fee_profile_ratio",
    "avg_inner_ix_count_50tx",
    "avg_cpi_depth_50tx",
    "flipper_presence_ratio",
    "whale_reversal_ratio_top1",
    "whale_reversal_ratio_top3",
    "demand_elasticity_score",
    "demand_elasticity_score_v2",
    "momentum",
    "demand",
    "alpha_joint",
    "alpha_sample",
];
fn scalars(out: &mut Map<String, Value>, value: Value) {
    if let Value::Object(fields) = value {
        for (key, value) in fields {
            if value.is_number() || value.is_null() || value.is_boolean() {
                out.insert(key, value);
            }
        }
    }
}
pub(super) fn build(
    session: &PoolObservationSession,
    token: &Token,
    at: u64,
    config: &GatekeeperV2Config,
) -> Result<Value> {
    let mfs = session.try_materialize_features_at_cutoff(at)?;
    let fingerprint = session.fingerprint_metrics();
    let alpha = crate::components::gatekeeper_policy::evaluate_alpha_gate(&mfs, config);
    let mut out: Map<String, Value> = REQUIRED
        .iter()
        .map(|name| (name.to_string(), Value::Null))
        .collect();
    scalars(&mut out, serde_json::to_value(&mfs.tx_intel_features)?);
    scalars(&mut out, serde_json::to_value(&mfs.alpha_fingerprint)?);
    scalars(&mut out, serde_json::to_value(&mfs.sybil_resistance)?);
    if let Some(f) = &fingerprint {
        scalars(&mut out, serde_json::to_value(f)?);
    }
    let tx = &mfs.tx_intel_features;
    let sybil = &mfs.sybil_resistance;
    let attempted = token.successful_tx_signatures.len() + token.failed_tx_signatures.len();
    let failed_ratio =
        (attempted > 0).then(|| token.failed_tx_signatures.len() as f64 / attempted as f64);
    for (name, value) in [
        ("unique_ratio", json!(tx.unique_signer_ratio)),
        (
            "unique_successful_tx_count",
            json!(token.successful_tx_signatures.len()),
        ),
        ("tx_per_signer", json!(tx.avg_tx_per_signer)),
        ("consecutive_buys", json!(tx.max_consecutive_buys)),
        ("alpha_sample", json!(tx.buy_count)),
        ("failed_tx_ratio", json!(failed_ratio)),
        ("failed_tx_count", json!(token.failed_tx_signatures.len())),
        (
            "unique_failed_tx_count",
            json!(token.failed_tx_signatures.len()),
        ),
        (
            "unique_attempted_tx_count",
            json!(token.failed_tx_signatures.len() + token.successful_tx_signatures.len()),
        ),
        // Jawny alias badawczy schema2. Stare MFS DES V1 i progi tradingowe
        // pozostają wyłączone; oba klucze tego datasetu opisują ten sam DES V2.
        (
            "demand_elasticity_score",
            json!(sybil
                .demand_elasticity_v2
                .as_ref()
                .and_then(|v| v.demand_elasticity_score)),
        ),
        ("successful_buy_signers", json!(sybil.signer_sample_count)),
        (
            "fee_topology_diversity_index_v2",
            json!(sybil
                .fee_topology_diversity_v2
                .as_ref()
                .and_then(|v| v.fee_topology_diversity_index)),
        ),
        (
            "demand_elasticity_score_v2",
            json!(sybil
                .demand_elasticity_v2
                .as_ref()
                .and_then(|v| v.demand_elasticity_score)),
        ),
        ("market_cap_sol", json!(token.last_market_cap_sol)),
        (
            "bonding_progress_pct",
            json!(
                (token.migration_ms.is_none() && mfs.curve_readiness.curve_data_known)
                    .then_some(mfs.account_features.bonding_progress * 100.0)
            ),
        ),
        (
            "price_change_ratio",
            json!(token
                .last_price_sol
                .zip(token.first_price_sol)
                .and_then(|(last, first)| (first > 0.0).then_some(last / first))),
        ),
        (
            "single_tx_price_impact_pct",
            json!(token.max_price_impact_pct),
        ),
        ("single_sell_impact_pct", json!(token.max_sell_impact_pct)),
        (
            "single_sell_impact_pct_observed",
            json!(token.max_sell_impact_pct),
        ),
        ("momentum", json!(alpha.momentum)),
        ("demand", json!(alpha.demand)),
        ("alpha_joint", json!(alpha.joint)),
    ] {
        out.insert(name.into(), value);
    }
    // Legacy producers expose default zeros for some undefined ratios. Keep
    // real zero counters, but do not present a missing denominator/dev as data.
    if tx.tx_count == 0 {
        for name in [
            "buy_ratio",
            "sol_buy_ratio",
            "avg_tx_sol",
            "volume_cv",
            "hhi",
            "volume_gini",
            "unique_ratio",
            "unique_signer_ratio",
            "tx_per_signer",
            "avg_tx_per_signer",
            "top3_volume_pct",
            "top3_signer_volume_ratio",
        ] {
            out.insert(name.into(), Value::Null);
        }
    }
    if tx.tx_count < 2 {
        for name in [
            "avg_interval_ms",
            "interval_cv",
            "burst_ratio",
            "timing_entropy",
            "same_ms_tx_ratio",
        ] {
            out.insert(name.into(), Value::Null);
        }
    }
    if !tx.dev_wallet_known {
        for name in [
            "dev_buy_sol",
            "dev_tx_ratio",
            "dev_volume_ratio",
            "dev_has_sold",
        ] {
            out.insert(name.into(), Value::Null);
        }
    }
    let current_jito = fingerprint.as_ref().and_then(|f| f.jito_tip_intensity);
    let age = at.saturating_sub(token.born_ms);
    let jito30 = token
        .jito_30s
        .or_else(|| (age == 30_000).then_some(current_jito).flatten());
    let jito300 = token
        .jito_300s
        .or_else(|| (age == 300_000).then_some(current_jito).flatten());
    out.insert(
        "delta_jito_tip_intensity_1s_to_30s".into(),
        json!(jito30.zip(token.jito_1s).map(|(b, a)| b - a)),
    );
    out.insert(
        "delta_jito_tip_intensity_31s_to_300s".into(),
        json!(jito300.zip(token.jito_31s).map(|(b, a)| b - a)),
    );
    let field_status = classify_fields(
        &out,
        token,
        &mfs,
        fingerprint.as_ref(),
        alpha.skip_reason.as_deref(),
        age,
    );
    let missing: Vec<_> = REQUIRED
        .iter()
        .filter(|name| out.get(**name).is_none_or(Value::is_null))
        .copied()
        .collect();
    Ok(
        json!({"metric_schema_version":2, "metrics":out, "venue":if token.migration_ms.is_some(){"pumpswap"}else{"pump_curve"},
        "quality":{"missing":missing,"fields":field_status,"sybil_reasons":sybil.degraded_reasons,"cpv":sybil.cpv_evidence,
            "cpv_source":session.cross_pool_velocity_index.source_diagnostics_at(at),
            "developer":{"pubkey":session.dev_wallet.map(|v|v.to_string()),
                "definition":"pump_create_protocol_creator_or_legacy_user"},
            "ftdi_v2":sybil.fee_topology_diversity_v2,"dbia":sybil.dbia_evidence_v1,"sfd":sybil.sfd_evidence_v1,
            "des_v2":sybil.demand_elasticity_v2,"fsc":sybil.funding_source_diagnostics,
            "fsc_v2":sybil.funding_source_v2,"fingerprint_degraded":fingerprint.as_ref().map(|f|f.fingerprint_degraded),
            "fingerprint_reason":fingerprint.as_ref().and_then(|f|f.fingerprint_reason.clone()),
            "alpha_skip_reason":alpha.skip_reason,"materialization":mfs.evidence_status,
            "failed_tx_ratio_source":"gate0_observation_success_and_failure",
            "price_sample_pairs":token.price_pairs,"sell_price_sample_pairs":token.sell_price_pairs,
            "input_order":order_diagnostics(session),"label_reasons":token.label_reasons},
        "definitions":{"fee_topology_diversity_index":"legacy_K_over_N","fee_topology_diversity_index_v2":"1_minus_HHI",
            "demand_elasticity_score":"next_buy_slot_tau_b_alias_v2","demand_elasticity_score_v2":"next_buy_slot_tau_b",
            "des_price_source":"verified_market_post_trade_price", "logical_market":"pump_curve_and_canonical_pumpswap_same_mint",
            "failed_tx_ratio":"unique_failed_transaction_signatures_over_unique_attempts",
            "single_tx_price_impact_pct":"max_abs_pre_post_successful_swap_price_change_pct",
            "single_sell_impact_pct":"max_observed_sell_price_drop_pct",
            "bonding_progress_pct":"not_applicable_after_canonical_migration",
            "volume_gini":"signer_volume","top3_volume_pct":"top3_signer_volume_ratio",
            "tx_count":"successful_swap_event_count","unique_successful_tx_count":"distinct_successful_transaction_signatures",
            "same_ms_tx_ratio":"legacy_adjacent_intervals_below_50ms",
            "avg_inner_ix_count_50tx":"first_50_eligible_transactions","avg_cpi_depth_50tx":"first_50_eligible_transactions",
            "early_metrics":"retain_named_early_window","static_fee_profile_ratio":"first_3_seconds",
            "main_window_ms":age,"metric_cutoff_ms":at}}),
    )
}

fn order_diagnostics(session: &PoolObservationSession) -> Value {
    let buys: Vec<_> = session
        .tx_buffer
        .iter()
        .filter(|tx| tx.is_confirmed_success() && tx.is_buy)
        .collect();
    let missing: Vec<_> = buys
        .iter()
        .filter(|tx| {
            tx.slot.is_none()
                || tx.tx_index.is_none()
                || tx.outer_instruction_index.is_none()
                || tx.inner_instruction_path.is_none()
        })
        .take(4)
        .map(|tx| {
            json!({"signature":tx.signature,"slot":tx.slot,"tx_index":tx.tx_index,
            "outer":tx.outer_instruction_index,"inner_path":tx.inner_instruction_path})
        })
        .collect();
    json!({"buys":buys.len(),"missing_slot":buys.iter().filter(|tx|tx.slot.is_none()).count(),
        "missing_tx_index":buys.iter().filter(|tx|tx.tx_index.is_none()).count(),
        "missing_outer":buys.iter().filter(|tx|tx.outer_instruction_index.is_none()).count(),
        "missing_inner_path":buys.iter().filter(|tx|tx.inner_instruction_path.is_none()).count(),"examples":missing})
}

fn classify_fields(
    out: &Map<String, Value>,
    token: &Token,
    mfs: &ghost_core::checkpoint::MaterializedFeatureSet,
    fingerprint: Option<&seer::early_fingerprint::EarlyFingerprintMetrics>,
    alpha_reason: Option<&str>,
    age: u64,
) -> Map<String, Value> {
    let sybil = &mfs.sybil_resistance;
    let mut result = Map::new();
    for name in REQUIRED {
        let mut reasons: Vec<String> = match *name {
            "fee_topology_diversity_index" | "fee_topology_diversity_index_v2" => sybil
                .fee_topology_diversity_v2
                .as_ref()
                .map(|v| v.degraded_reasons.clone())
                .unwrap_or_default(),
            "dev_buyer_infrastructure_affinity" => sybil
                .dbia_evidence_v1
                .as_ref()
                .map(|v| v.degraded_reasons.clone())
                .unwrap_or_default(),
            "spend_fraction_divergence" => sybil
                .sfd_evidence_v1
                .as_ref()
                .map(|v| v.degraded_reasons.clone())
                .unwrap_or_default(),
            "demand_elasticity_score" | "demand_elasticity_score_v2" => sybil
                .demand_elasticity_v2
                .as_ref()
                .map(|v| v.degraded_reasons.clone())
                .unwrap_or_default(),
            "signer_cross_pool_velocity" | "cpv_other_pool_activity" => {
                sybil.cpv_evidence.degraded_reasons.clone()
            }
            "funding_source_concentration" => sybil
                .degraded_reasons
                .iter()
                .filter(|r| r.starts_with("FSC_"))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        let has_value = out.get(*name).is_some_and(Value::is_number);
        let mut status = if has_value {
            if reasons.is_empty() {
                "available"
            } else {
                "diagnostic"
            }
        } else {
            "input_unavailable"
        };
        if !has_value {
            match *name {
                "market_cap_sol" | "price_change_ratio" => {
                    if let Some(reason) = token.price_unavailable_reason {
                        reasons.push(reason.into());
                    } else if *name == "price_change_ratio" {
                        reasons.push(if token.first_price_sol.is_some_and(|p| p <= 0.0) {
                            "nonpositive_initial_price".into()
                        } else {
                            "initial_price_unavailable".into()
                        });
                    }
                }
                "bonding_progress_pct" if token.migration_ms.is_some() => {
                    status = "not_applicable";
                    reasons.push("pumpswap_has_no_bonding_curve".into());
                }
                "dev_paperhand_latency_ms"
                    if mfs.tx_intel_features.dev_wallet_known
                        && !mfs.tx_intel_features.dev_has_sold =>
                {
                    status = "censored";
                    reasons.push("no_dev_sell_by_cutoff".into());
                }
                "failed_tx_ratio"
                    if token.failed_tx_signatures.is_empty()
                        && token.successful_tx_signatures.is_empty() =>
                {
                    status = "insufficient_sample";
                    reasons.push("no_observed_swap_attempts".into());
                }
                "delta_jito_tip_intensity_31s_to_300s" if age < 300_000 => {
                    status = "not_yet_observable";
                    reasons.push("anchor_300s_not_reached".into());
                }
                "delta_jito_tip_intensity_1s_to_30s" | "delta_jito_tip_intensity_31s_to_300s" => {
                    status = "insufficient_sample";
                    reasons.push("jito_anchor_insufficient_observations".into());
                }
                "momentum" | "demand" | "alpha_joint" => {
                    status = if alpha_reason == Some("insufficient_sample") {
                        "insufficient_sample"
                    } else {
                        "input_unavailable"
                    };
                    reasons.push(alpha_reason.unwrap_or("alpha_not_measured").into());
                }
                "single_tx_price_impact_pct"
                | "single_sell_impact_pct"
                | "single_sell_impact_pct_observed" => {
                    status = "insufficient_sample";
                    reasons.push("no_verified_swap_price_pair".into());
                }
                "funding_source_concentration" => {
                    if let Some(fsc) = &sybil.funding_source_diagnostics {
                        reasons.extend(fsc.miss_reason_counts.iter().map(|r| r.reason.clone()));
                        if fsc.operational_unknown_buyer_count == 0 {
                            status = "insufficient_attribution";
                        }
                    }
                }
                _ => {}
            }
            if reasons.is_empty() {
                if let Some(fp) = fingerprint {
                    let prefix = match *name {
                        "jito_tip_intensity" => "JITO_",
                        "static_fee_profile_ratio" => "STATIC_FEE_",
                        "compute_unit_cluster_dominance" => "CU_CLUSTER_",
                        "flipper_presence_ratio" => "FLIPPER_",
                        "fixed_size_buy_ratio" | "fixed_size_buy_ratio_1e4" => "FIXED_SIZE_",
                        "avg_inner_ix_count_50tx" => "INNER_IX_",
                        "avg_cpi_depth_50tx" => "CPI_DEPTH_",
                        "early_top3_buy_volume_pct_3s" => "EARLY_TOP3_BUY_VOLUME_",
                        _ => "__no_match__",
                    };
                    reasons.extend(
                        fp.fingerprint_reason
                            .as_deref()
                            .unwrap_or("")
                            .split(',')
                            .filter(|r| r.starts_with(prefix))
                            .map(str::to_string),
                    );
                }
            }
            if reasons.iter().any(|r| {
                r.contains("INSUFFICIENT")
                    || r.contains("_MIN_")
                    || r.contains("NO_COMPARABLE_PAIRS")
                    || r == "EARLY_TOP3_BUY_VOLUME_ZERO"
            }) && status == "input_unavailable"
            {
                status = "insufficient_sample";
            }
            if *name == "dev_buyer_infrastructure_affinity"
                && !mfs.tx_intel_features.dev_wallet_known
            {
                status = "input_unavailable";
                reasons.retain(|r| r != "DBIA_NO_DEV_BUY");
                reasons.push("verified_developer_identity_unavailable".into());
            } else if reasons.iter().any(|r| r == "DBIA_NO_DEV_BUY") {
                status = "not_applicable";
            }
            if reasons.is_empty() && mfs.tx_intel_features.tx_count < 2 {
                status = "insufficient_sample";
                reasons.push("insufficient_observed_transactions".into());
            }
            if reasons.is_empty()
                && name.starts_with("dev_")
                && !mfs.tx_intel_features.dev_wallet_known
            {
                reasons.push("verified_developer_identity_unavailable".into());
            }
            if reasons.is_empty() {
                reasons.push("producer_did_not_supply_value".into());
            }
        }
        reasons.sort();
        reasons.dedup();
        result.insert((*name).into(), json!({"status":status,"reasons":reasons}));
    }
    result
}
