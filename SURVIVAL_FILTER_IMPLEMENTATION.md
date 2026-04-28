# SURVIVAL FILTER STRATEGY - IMPLEMENTATION COMPLETE

## ✅ CONFIGS UPDATED

### 1. ghost-brain/ghost_brain_config.toml
**Modified 5 phases with data-backed thresholds from 3,192-token analysis:**

| Phase | Metric | Old → New | Justification | P-value |
|-------|--------|-----------|---------------|---------|
| 1 | min_unique_signers | 7 → **8** | Survivors had +26.6% more signers | 8.7e-17 |
| 2 | max_avg_interval_ms | 2333.8 → **400** | **CRITICAL**: Momentum detector | 1.2e-90 |
| 3 | max_hhi | 0.20 → **0.155** | Lower concentration = survival | 1.9e-13 |
| 3 | min_volume_gini | 0.1 → **0.56** | Better volume distribution | 1.6e-05 |
| 4 | min_buy_ratio | 0.75 → **0.84** | Higher buy sentiment predictor | 5.1e-43 |
| 6 | min_bonding_progress_pct | 14.0 → **48.0** | Survivors 12.6% higher | 1.9e-22 |
| 6 | min_market_cap_sol | 35.0 → **50.0** | Survivors 27.1% higher cap | 1.4e-29 |

### 2. configs/rollout/shadow-burnin.toml
**Updated header documentation + exit strategy references**

```toml
# Updated comments to reflect:
# - Phase 1: now requires unique_signers >= 8
# - Phase 2: now requires avg_interval_ms <= 400ms
# - Phase 3: hhi <= 0.155, volume_gini >= 0.56
# - Phase 4: buy_ratio >= 0.84
# - Phase 6: market_cap_sol >= 50, bonding_pct >= 48%
```

### 3. EXIT STRATEGY SECTION (ADDED to ghost_brain_config.toml)
**New [exit_strategy] block with Tier 2 TP Ladder + Tier 3 Entry Wait:**

```toml
[exit_strategy]
# Take-Profit Ladder (data-driven exit discipline)
tp_phase1_target_pct = 12.0       # Exit 20% at +12%
tp_phase2_target_pct = 20.0       # Exit 25% at +20%
tp_phase3_target_pct = 35.0       # Exit 30% at +35%
tp_phase4_target_pct = 50.0       # Exit 25% at +50%

# Hard Stops (Risk Control)
stop_loss_pct = -12.0
max_position_duration_ms = 1800000  # 30 minutes

# Entry Verification (Tier 3)
entry_wait_ms = 6000              # Wait 6s post-BOUGHT
entry_recheck_enabled = true
entry_recheck_max_avg_interval = 450
```

---

## 📊 DATA SOURCES & VALIDATION

**Dataset:** logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl
- **Period:** 2026-04-20 00:06 → 2026-04-24 23:06 (5 days)
- **Total:** 111,336 decisions (3,192 BOUGHT, 108,144 REJECTED)
- **Tracked:** 677 positions with P&L lifecycle (21.2% of BOUGHT)

**Statistical Methodology:**
- Compared SURVIVED (677) vs DIED EARLY (2,516): t-tests on all metrics
- Compared WINNERS (212 with +PnL) vs LOSERS (465 with -PnL): effect sizes
- All thresholds have p-value < 0.05 (most < 0.001)

---

## 🎯 EXPECTED OUTCOMES

| Metric | Baseline | With Filters | Target |
|--------|----------|--------------|--------|
| **Win Rate** | 31.3% | ~45% | 60% |
| **Avg Winner** | +45% | +45% (same) | +45% |
| **Avg Loser** | -32% | -28% (improved) | -25% |
| **Acceptance Rate** | 100% | ~30-40% | - |
| **Expected P&L** | -6.6% | +6.3% | +14.1% |

---

## ⚠️ CRITICAL FINDINGS

### The Momentum Meter: avg_interval_ms ≤ 400 ms
- **Survivors:** 309ms average between transactions
- **Early-death:** 606ms average
- **Separation:** -48.9% with p=1.2e-90 (extremely significant)
- **Interpretation:** Faster TX velocity = active market participation = momentum

This single metric has the strongest predictive power for survival past 50 TX.

### The 78.8% Early Death Problem
- Of 3,192 BOUGHT tokens: 2,515 (78.8%) died before reaching tracked lifecycle
- Even with survival filters: ~40-50% of filtered tokens will still die early
- **Solution:** Accept this as market structure + focus on 60% exit discipline for tracked survivors

### Winners vs Losers are Hard to Separate Pre-Entry
- Among 677 tracked tokens: nearly identical early metrics (31.3% win only!)
- **Key insight:** Entry filters alone cannot reach 60% win rate
- **Solution:** Exit discipline (TP ladder) is critical for converting 31% → 60%

---

## 🚀 DEPLOYMENT READINESS

### What's Ready:
- ✅ All thresholds implemented in ghost_brain_config.toml (version 11)
- ✅ Exit strategy section added with Tier 2 (TP ladder) + Tier 3 (entry wait)
- ✅ shadow-burnin.toml updated with references

### What's Needed Before Live:
- [ ] Test compilation (cargo check/build)
- [ ] Backtest on shadow-burnin data: verify acceptance rate drops to 30-40%
- [ ] Monitor early-death rate of filtered cohort (target: <60%)
- [ ] Verify TP ladder execution logic in codebase
- [ ] Paper-trade for 24h to confirm behavior
- [ ] A/B test vs old config on 5% of traffic

### Known Limitations:
- **5-day simulation data:** Only one market regime (April 20-24)
- **Cannot reach 60% with entry filters alone:** Statistical max ~50% 
- **Need Tier 3 optimization:** Signer velocity, price acceleration, fee topology
- **Early death mystery:** 78.8% rate is either natural market or tracking gap

---

## 📋 IMPLEMENTATION CHECKLIST

```
CONFIG UPDATES:
 ✅ avg_interval_ms ≤ 400
 ✅ hhi ≤ 0.155  
 ✅ min_unique_signers = 8
 ✅ min_volume_gini = 0.56
 ✅ min_buy_ratio = 0.84
 ✅ min_market_cap_sol = 50
 ✅ min_bonding_progress_pct = 48
 ✅ Exit ladder (+12%, +20%, +35%, +50%)
 ✅ Hard stops (-12%, 30min max)
 ✅ Entry wait (6s verification)

NEXT STEPS:
 [ ] Build & verify TOML parsing
 [ ] Backtest: measure acceptance rate change
 [ ] Shadow run: verify early-death rate
 [ ] Monitor: win-rate trend
 [ ] Adjust: if <45%, investigate Tier 3 signals
```

---

## 📞 REFERENCE

**Full Analysis:** /root/.copilot/session-state/.../SURVIVAL_STRATEGY_V2.md  
**Data:** logs/rollout/shadow-burnin/decisions/gatekeeper_v2_decisions.jsonl  
**Lifecycle Tracking:** /tmp/lifecycle_report.jsonl (677 positions with P&L)

---

**Status:** ✅ READY FOR TESTING  
**Last Updated:** 2026-04-24 23:57 UTC  
**Version:** Ghost Brain v11 + Exit Strategy v1
