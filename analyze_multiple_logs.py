import json

logs = [
    {"timestamp": "2026-04-25T01:03:59", "data": '{"max_hhi":0.155,"max_avg_interval_ms":400.0,"max_dev_volume_ratio":0.99,"min_volume_gini":0.56,"min_buy_ratio":0.84,"min_market_cap_sol":50.0,"min_bonding_progress_pct":48.0,"max_soft_score":255}'},
    {"timestamp": "2026-04-25T01:18:53", "data": '{"max_hhi":0.155,"max_avg_interval_ms":400.0,"max_dev_volume_ratio":0.99,"min_volume_gini":0.56,"min_buy_ratio":0.84,"min_market_cap_sol":50.0,"min_bonding_progress_pct":48.0,"max_soft_score":255}'},
]

first_attempt = {
    "max_hhi": 0.145,
    "max_avg_interval_ms": 320.0,
    "max_dev_volume_ratio": 0.22,
    "min_volume_gini": 0.56,
    "min_buy_ratio": 0.84,
    "min_market_cap_sol": 60.0,
    "min_bonding_progress_pct": 48.0,
    "max_soft_score": 1,
}

print("="*80)
print("ANALYZING MULTIPLE LOGS (2 entries) vs FIRST ATTEMPT\n")

for i, log_entry in enumerate(logs):
    data = json.loads(log_entry["data"])
    print(f"\n📊 LOG #{i+1} ({log_entry['timestamp']}):")
    print("-"*60)
    
    for param in sorted(first_attempt.keys()):
        log_val = data.get(param)
        first_val = first_attempt[param]
        
        if log_val == first_val:
            print(f"  ✅ {param}: {log_val} = {first_val}")
        else:
            print(f"  ❌ {param}: LOG={log_val}, FIRST={first_val}")

print("\n" + "="*80)
print("VERDICT:")
print("="*80)
print("""
Current logs show CONSISTENT values:
  - max_hhi: 0.155 (not 0.145)
  - max_avg_interval_ms: 400.0 (not 320.0)
  - max_dev_volume_ratio: 0.99 (not 0.22)
  - min_market_cap_sol: 50.0 (not 60.0)
  - max_soft_score: 255 (not 1)

MATCHED WITH FIRST ATTEMPT:
  ✅ min_volume_gini: 0.56
  ✅ min_buy_ratio: 0.84
  ✅ min_bonding_progress_pct: 48.0

CONCLUSION: 
Current (from logs) is MORE CORRECT - 3/8 match, but logs are CONSISTENT pattern
""")
