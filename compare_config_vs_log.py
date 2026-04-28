import re

# Log thresholds
log_thresholds = {
    "max_avg_inner_ix_count_50tx": 9999.0,
    "max_avg_interval_ms": 9999.0,
    "max_avg_tx_sol": 9999.0,
    "max_bonding_progress_pct": 99.0,
    "max_burst_ratio": 1.0,
    "max_buy_ratio": 1.0,
    "max_compute_unit_cluster_dominance": 0.99,
    "max_dev_buy_sol": 9999.0,
    "max_dev_buyer_infrastructure_affinity": 0.9,
    "max_dev_tx_ratio": 0.99,
    "max_dev_volume_ratio": 0.99,
    "max_early_slot_volume_dominance_buy": 0.99,
    "max_funding_source_concentration": 0.9,
    "max_hhi": 3.0,
    "max_interval_cv": 9999.0,
    "max_jito_tip_intensity": 1.0,
    "max_price_change_ratio": 9999.0,
    "max_same_ms_tx_ratio": 1.0,
    "max_sell_buy_ratio": 0.99,
    "max_signer_cross_pool_velocity": 0.9,
    "max_single_sell_impact_pct": 9999.0,
    "max_single_tx_price_impact_pct": 9999.0,
    "max_soft_points": 255,
    "max_soft_score": 255,
    "max_static_fee_profile_ratio": 0.99,
    "max_timing_entropy": 9999.0,
    "max_top3_volume_pct": 1.0,
    "max_total_volume_sol": 9999.0,
    "max_tx_per_signer": 999999,
    "max_unique_ratio": 1.0,
    "max_volume_cv": 9999.0,
    "max_volume_gini": 11.0,
    "max_wait_time_ms": 60000,
    "min_alpha_joint": 0.2,
    "min_alpha_sample": 3,
    "min_avg_inner_ix_count_50tx": 0.01,
    "min_avg_interval_ms": 1.0,
    "min_avg_tx_sol": 0.01,
    "min_bonding_progress_pct": 1.0,
    "min_buy_count": 2,
    "min_buy_ratio": 0.1,
    "min_compute_unit_cluster_dominance": 0.0,
    "min_consecutive_buys": 2,
    "min_demand": 0.2,
    "min_demand_elasticity_score": 0.05,
    "min_dev_buy_sol": 0.0,
    "min_dev_tx_ratio": 0.0,
    "min_dev_volume_ratio": 0.01,
    "min_dust_filtered_count": 0,
    "min_fee_topology_diversity_index": 0.06,
    "min_fixed_size_buy_ratio": 0.0001,
    "min_interval_cv": 0.0,
    "min_jito_tip_intensity": 0.0,
    "min_market_cap_sol": 1.0,
    "min_momentum": 0.2,
    "min_phases_to_pass": 1,
    "min_sell_buy_ratio": 0.0,
    "min_sol_buy_ratio": 0.0,
    "min_sol_threshold": 0.001,
    "min_spend_fraction_divergence": 0.06,
    "min_static_fee_profile_ratio": 0.0,
    "min_timing_entropy": 0.01,
    "min_total_volume_sol": 1.0,
    "min_tx_count": 3,
    "min_unique_ratio": 0.1,
    "min_unique_signers": 2,
    "min_volume_cv": 0.01,
    "min_volume_gini": 0.01,
}

# Read config file
with open('ghost-brain/ghost_brain_config.toml', 'r') as f:
    config_content = f.read()

print("MISMATCHES BETWEEN LOG AND CONFIG:\n")
mismatches = []

for param, log_val in sorted(log_thresholds.items()):
    # Find in config
    pattern = rf"^{param}\s*=\s*([\d.]+)"
    match = re.search(pattern, config_content, re.MULTILINE)
    
    if match:
        config_val = float(match.group(1))
        if config_val != log_val:
            mismatches.append((param, log_val, config_val))
            print(f"❌ {param}")
            print(f"   Log:    {log_val}")
            print(f"   Config: {config_val}")
            print()
    else:
        print(f"⚠️  {param} - NOT FOUND IN CONFIG")
        print()

if not mismatches:
    print("✅ ALL values match!")
else:
    print(f"\nTotal mismatches: {len(mismatches)}")
    print("\nGenerate fix commands:")
    for param, log_val, config_val in mismatches:
        print(f'grep -n "^{param}" ghost-brain/ghost_brain_config.toml')
