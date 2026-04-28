import json

log_str = '''{"log_schema_version":13,"timestamp":"2026-04-25T04:44:16.325047285+00:00","pool_id":"JDkRsLksL2GjMVCe9jV8VYJYvNDHkDZ2dCcYsRBzfSFR","join_key":"JDkRsLksL2GjMVCe9jV8VYJYvNDHkDZ2dCcYsRBzfSFR:ApzrVkH6heL4xjmbaFNDm8HgYMES5NUmppiYaVQpump:1777092247827","base_mint":"ApzrVkH6heL4xjmbaFNDm8HgYMES5NUmppiYaVQpump","first_seen_ts_ms":1777092247827,"first_seen_clock_source":"registered_wall","observation_start_ts_ms":1777092247827,"observation_end_ts_ms":1777092255827,"observation_window_ms":8000,"end_10s_ts_ms":1777092249827,"core_pass":true,"gatekeeper_version":"v2.2","dev_pubkey":"JRG7gugreDmPid8ZYUo4Ujwm6HkGfoXhoWFc33PjkhN","shadow_ready":true,"shadow_metadata_source":"local_task_state","shadow_trigger_present":true,"shadow_entry_mode":"shadow_only","shadow_trigger_eligible":true,"shadow_execution_outcome":"shadow_simulated","mode":"long","phases_passed":5,"min_phases_to_pass":1,"observation_duration_ms":8000,"finalize_lag_ms":3,"max_wait_time_ms":8000,"eval_count":1,"dust_filtered_count":2,"min_sol_threshold":0.001,"total_tx_evaluated":10,"unique_tx_evaluated":10,"min_tx_count":9,"unique_signers_evaluated":10,"min_unique_signers":8,"buy_count":10,"min_buy_count":7,"phase2_passed":true,"interval_cv":1.7545708608667776,"min_interval_cv":0.0,"max_interval_cv":9999.0,"burst_ratio":0.8,"max_burst_ratio":1.0,"avg_interval_ms":334.5,"min_avg_interval_ms":1.0,"max_avg_interval_ms":400.0,"timing_entropy":0.9404479886553263,"min_timing_entropy":0.01,"max_timing_entropy":9999.0,"min_dust_filtered_count":0,"phase3_passed":false,"unique_ratio":1.0,"min_unique_ratio":0.99,"max_unique_ratio":1.0,"hhi":0.10000000000000003,"max_hhi":0.155,"max_tx_per_signer_observed":1,"max_tx_per_signer":999999,"volume_gini":0.46340587862890403,"min_volume_gini":0.56,"max_volume_gini":11.0,"top3_volume_pct":0.6007062041287808,"max_top3_volume_pct":1.0,"same_ms_tx_ratio":0.3,"max_same_ms_tx_ratio":1.0,"phase4_passed":true,"buy_ratio":1.0,"min_buy_ratio":0.84,"max_buy_ratio":1.0,"avg_tx_sol":0.6348083815000001,"min_avg_tx_sol":0.01,"max_avg_tx_sol":9999.0,"volume_cv":0.8223679859908563,"min_volume_cv":0.01,"max_volume_cv":9999.0,"total_volume_sol":6.348083815000001,"min_total_volume_sol":1.0,"max_total_volume_sol":9999.0,"sol_buy_ratio":1.0,"min_sol_buy_ratio":0.0,"max_consecutive_buys_observed":10,"min_consecutive_buys":1,"phase5_passed":true,"dev_wallet_known":true,"dev_buy_total_sol":0.782222221,"max_dev_buy_sol":9999.0,"min_dev_buy_sol":0.0,"dev_tx_ratio":0.1,"max_dev_tx_ratio":0.99,"min_dev_tx_ratio":0.0,"dev_volume_ratio":0.12322178531286451,"max_dev_volume_ratio":0.99,"min_dev_volume_ratio":0.01,"dev_has_sold":false,"reject_on_dev_sell":false,"phase6_passed":true,"price_change_ratio":1.1288926529911767,"max_price_change_ratio":9999.0,"max_single_tx_price_impact_pct_observed":44.79115007012741,"max_single_tx_price_impact_pct":9999.0,"max_single_sell_impact_pct_observed":0.0,"max_single_sell_impact_pct":9999.0,"bonding_progress_pct":48.0,"min_bonding_progress_pct":48.0,"max_bonding_progress_pct":99.0,"curve_data_known":true,"curve_finality":"provisional","curve_finality_is_finalized":false,"bonding_progress_check_skipped":false,"current_market_cap_sol":53.328810108,"min_market_cap_sol":50.0,"curve_wait_ms":800,"curve_t0_event_ts_ms":1777092247799,"curve_t0_clock_source":"ingress_wall","curve_wait_elapsed_ms":8031,"curve_required_for_buy":true,"three_layer_enabled":true,"core1_passed":true,"core2_passed":true,"core3_passed":true,"dev_unknown":false,"soft_score":0,"soft_points":0,"max_soft_points":255,"effective_max_soft_points":255,"max_soft_score":255,"soft_flags":"CURVE_FINALITY_PROVISIONAL","legacy_soft_points":0,"legacy_soft_threshold":255,"legacy_soft_flags":"CURVE_FINALITY_PROVISIONAL","sybil_soft_points":2,"sybil_soft_threshold":6,"total_soft_points":2,"sybil_soft_flags":"low_sfd","sybil_lead_signal":"LOW_SFD","sybil_interference_layer_enabled":true,"sybil_combo_veto_enabled":false,"decision_reason":"BUY: soft_points=0/255 flags=[none] alpha_pass=true momentum=0.655 demand=0.358 joint=0.235 alpha_skip=none prosperity_pass=true mcap_floor_pass=true cpv_pass=true b1=false b2=true b3=false matched=[large_cap_buy_dominance]","decision_verdict_buy":true,"verdict_type":"BUY","alpha_gate_enabled":true,"alpha_pass":true,"alpha_actionable":true,"momentum":0.6547623032786885,"demand":0.35833333333333334,"alpha_joint":0.23462315867486336,"min_momentum":0.2,"min_demand":0.2,"min_alpha_joint":0.2,"min_alpha_sample":5,"prosperity_filter_enabled":true,"prosperity_pass":true,"prosperity_actionable":true,"prosperity_market_cap_floor_pass":true,"prosperity_cpv_pass":true,"prosperity_branch1_pass":false,"prosperity_branch2_pass":true,"prosperity_branch3_pass":false,"prosperity_matched_branches":["large_cap_buy_dominance"],"prosperity_min_market_cap_sol":35.0,"prosperity_max_signer_cross_pool_velocity":0.5,"prosperity_branch1_min_block0_sniped_supply_pct":0.28,"prosperity_branch1_max_sell_buy_ratio":0.16,"prosperity_branch2_min_market_cap_sol":50.0,"prosperity_branch2_min_early_slot_volume_dominance_buy":0.9,"prosperity_branch3_max_hhi":0.0416,"prosperity_branch3_min_fee_topology_diversity_index":0.0909,"iwim_enabled":false,"block0_sniped_supply_pct":0.1339079199101016,"flip_ratio_10s":0.09090909090909091,"cu_price_p90_1s":4000000.0,"cu_price_p90_10s":4000000.0,"priority_fee_surge_slope":0.0,"buyer_pre_balance_cv":0.5698538753473597,"avg_inner_ix_count_50tx":19.666666666666668,"min_avg_inner_ix_count_50tx":0.01,"max_avg_inner_ix_count_50tx":9999.0,"avg_cpi_depth_50tx":2.25,"sell_buy_ratio":0.09090909090909091,"min_sell_buy_ratio":0.0,"max_sell_buy_ratio":0.4,"compute_unit_cluster_dominance":0.25,"min_compute_unit_cluster_dominance":0.0,"max_compute_unit_cluster_dominance":0.99,"static_fee_profile_ratio":0.2,"min_static_fee_profile_ratio":0.0,"max_static_fee_profile_ratio":0.99,"fixed_size_buy_ratio":0.18181818181818182,"min_fixed_size_buy_ratio":0.0001,"fixed_size_buy_ratio_1e4":0.18181818181818182,"flipper_presence_ratio":0.0,"jito_tip_intensity":0.08333333333333333,"min_jito_tip_intensity":0.0,"max_jito_tip_intensity":1.0,"early_slot_volume_dominance_buy":0.9661170287001878,"max_early_slot_volume_dominance_buy":0.99,"whale_reversal_ratio_top3":0.0,"whale_reversal_ratio_top1":0.0,"dev_sold_within_3s":false,"dev_sold_within_5s":false,"fingerprint_degraded":false,"fee_topology_diversity_index":0.4,"min_fee_topology_diversity_index":0.06,"dev_buyer_infrastructure_affinity":0.20555555555555557,"max_dev_buyer_infrastructure_affinity":0.6,"spend_fraction_divergence":0.041060494281572865,"min_spend_fraction_divergence":0.06,"demand_elasticity_score":0.3333333333333333,"min_demand_elasticity_score":0.05,"signer_cross_pool_velocity":0.3,"max_signer_cross_pool_velocity":0.5,"max_funding_source_concentration":0.6,"funding_source_diagnostics":{"buyer_sample_count":7,"known_source_count":0,"unknown_buyer_count":7,"structural_unknown_buyer_count":0,"operational_unknown_buyer_count":0,"indeterminate_unknown_buyer_count":7,"miss_reason_counts":[{"reason":"FSC_NO_RETAINED_RECIPIENT_HISTORY","class":"indeterminate","count":7}]},"sybil_metric_degraded_reasons":["FSC_INSUFFICIENT_KNOWN_SOURCES"],"ab_window_ms":2000,"ab_t0_event_ts_ms":1777092247799,"ab_t_end_event_ts_ms":1777092249799,"ab_window_complete":true,"ab_window_close_reason":"END_REACHED","ab_tx_count_window":11,"ab_unique_signers_window":10,"ab_fail_count_window":0,"ab_window_origin":"NewPoolDetected","ab_record_id":"JDkRsLksL2GjMVCe9jV8VYJYvNDHkDZ2dCcYsRBzfSFR:1777092247799:1777092249799:BUY","vectors_max_len":200}'''

data = json.loads(log_str)

print("=== CORRECT LOG THRESHOLDS ===\n")
print(f"observation_window_ms: {data['observation_window_ms']}")
print(f"max_wait_time_ms: {data['max_wait_time_ms']}\n")

print("=== PHASE 1: Quantity ===")
print(f"min_tx_count: {data['min_tx_count']}")
print(f"min_unique_signers: {data['min_unique_signers']}")
print(f"min_buy_count: {data['min_buy_count']}\n")

print("=== PHASE 2: Velocity ===")
print(f"max_avg_interval_ms: {data['max_avg_interval_ms']}\n")

print("=== PHASE 3: Signer Diversity ===")
print(f"max_hhi: {data['max_hhi']}")
print(f"min_volume_gini: {data['min_volume_gini']}\n")

print("=== PHASE 4: Volume ===")
print(f"min_buy_ratio: {data['min_buy_ratio']}\n")

print("=== PHASE 5-6 ===")
print(f"max_dev_tx_ratio: {data['max_dev_tx_ratio']}")
print(f"max_dev_volume_ratio: {data['max_dev_volume_ratio']}")
print(f"min_dev_volume_ratio: {data['min_dev_volume_ratio']}")
print(f"min_market_cap_sol: {data['min_market_cap_sol']}\n")

print("=== ALPHA GATE ===")
print(f"min_alpha_sample: {data['min_alpha_sample']}")
print(f"min_momentum: {data['min_momentum']}")
print(f"min_demand: {data['min_demand']}")
print(f"min_alpha_joint: {data['min_alpha_joint']}\n")

print("=== PROSPERITY FILTER ===")
print(f"prosperity_min_market_cap_sol: {data['prosperity_min_market_cap_sol']}")
print(f"prosperity_max_signer_cross_pool_velocity: {data['prosperity_max_signer_cross_pool_velocity']}")
print(f"prosperity_branch2_min_market_cap_sol: {data['prosperity_branch2_min_market_cap_sol']}")
print(f"prosperity_branch2_min_early_slot_volume_dominance_buy: {data['prosperity_branch2_min_early_slot_volume_dominance_buy']}")
print(f"prosperity_branch3_max_hhi: {data['prosperity_branch3_max_hhi']}")
print(f"prosperity_branch3_min_fee_topology_diversity_index: {data['prosperity_branch3_min_fee_topology_diversity_index']}\n")

print("=== OTHER PARAMS ===")
print(f"max_sell_buy_ratio: {data['max_sell_buy_ratio']}")
print(f"max_dev_buyer_infrastructure_affinity: {data['max_dev_buyer_infrastructure_affinity']}")
print(f"max_signer_cross_pool_velocity: {data['max_signer_cross_pool_velocity']}")
print(f"max_funding_source_concentration: {data['max_funding_source_concentration']}")
