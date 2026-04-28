# First attempt (60% win rate) vs Current

first_attempt = {
    "max_hhi": 0.145,
    "max_dev_volume_ratio": 0.22,
    "min_volume_gini": 0.56,
    "max_soft_score": 1,
    "min_market_cap_sol": 60.0,
    "min_bonding_progress_pct": 48.0,
    "max_avg_interval_ms": 320.0,
    "min_buy_ratio": 0.84,
    "min_tx_count": 9,
    "min_unique_signers": 8,
    "min_buy_count": 7,
}

# Current (from log sync)
current = {
    "max_hhi": 0.155,
    "max_dev_volume_ratio": 0.99,
    "min_volume_gini": 0.56,
    "max_soft_score": 255,
    "min_market_cap_sol": 50.0,
    "min_bonding_progress_pct": 48.0,
    "max_avg_interval_ms": 400.0,
    "min_buy_ratio": 0.84,
    "min_tx_count": 9,
    "min_unique_signers": 8,
    "min_buy_count": 7,
}

print("=== FIRST ATTEMPT vs CURRENT ===\n")

differences = []
same = []

for param in sorted(first_attempt.keys()):
    first_val = first_attempt[param]
    curr_val = current[param]
    
    if first_val == curr_val:
        same.append((param, first_val))
        print(f"✅ {param}: {first_val} = {curr_val}")
    else:
        differences.append((param, first_val, curr_val))
        print(f"❌ {param}: {first_val} → {curr_val} (CHANGED)")

print(f"\n{'='*60}")
print(f"SAME: {len(same)}")
print(f"CHANGED: {len(differences)}")
print(f"\nChanges (FIRST → CURRENT):")
for param, first, curr in differences:
    print(f"  {param}: {first} → {curr}")
