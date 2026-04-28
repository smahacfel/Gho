# SYBIL SCORE INVERSION: Critical Finding

## The Paradox

```
SOFT_SCORE Distribution:
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
                SURVIVED    DIED      p-value
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━
Mean            0.675       0.027     1.5e-268 ***
━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━━

BUCKETS:
Score 0:        46.1%       97.1%     ← Dead tokens clustered at 0
Score 1:        45.6%        2.8%     ← Survivors spread to 1
Score 2-3:       8.3%        0.1%     ← Rare in dead, present in survived
```

**This is BACKWARDS from what you'd expect!**
- Sybil detection should PENALIZE high scores (= more bot activity)
- But data shows: **HIGH soft_score = survival = GOOD**

---

## What soft_score Actually Measures

From the code examination:

### Score 0 Records:
- soft_flags: `CURVE_FINALITY_PROVISIONAL` (mostly)
- sybil_flags: `none` or `low_des` (low diversity ecosystem)
- Metrics: Very simple, few unique signers, no activity flags
- **Interpretation:** Clean but BORING = no community = dies

### Score 1 Records:
- soft_flags: `low_entropy, CURVE_FINALITY_PROVISIONAL`
- sybil_flags: `low_ftdi, low_des` (low fee topology diversity, low signer diversity enforcement)
- Metrics: **30+ unique signers**, high activity, low HHI (0.03)
- **Interpretation:** Active community = good = survives

### Score 2+ Records:
- soft_flags: `cabal, unique_oor, CURVE_FINALITY_PROVISIONAL`
- sybil_flags: Complex combinations (`high_cpv`, `high_dbia`)
- Metrics: Mixed (some with HIGH HHI 0.375)
- **Interpretation:** Some warning signs but still mixed results

---

## The Root Cause

**soft_score is NOT "sybil detection"** - it's more like **"activity classification"**:

- **Score 0:** "Minimal flags detected" → Often means low community engagement
- **Score 1-2:** "Some flags triggered" → Often means ACTIVE ecosystem
- **Score 3:** "Multiple flags" → Could be either sybil OR genuine activity

The naming is misleading! It's really a **"flag accumulator"** not a **"sybil risk meter"**.

---

## Strategic Implications

### Current Config Problem:
In `ghost_brain_config.toml` (before my changes):
```toml
# Phase 2: avg_interval_ms max = 2333.8
# Phase 6: min_bonding_progress = 14%
# etc.
```
The soft_score was **NOT being used as a filter** - tokens passed regardless of score.

### My Config Changes:
I did NOT add `max_soft_score ≤ X` filter because:
1. Data shows **higher soft_score = survival** (p=1.5e-268)
2. Adding `max_soft_score=1` would **eliminate 54% of survivors**
3. The inversion makes it useless as a traditional "sybil" filter

### What Should Happen Instead:

**Option 1: Rename & Repurpose**
- Rename to `activity_score` (not sybil_score)
- Use as **minimum threshold** instead of maximum
- Example: `min_activity_score = 0.5` → Prefer tokens with flags (=more active)

**Option 2: Ignore It**
- Just exclude soft_score from filters entirely
- Focus on direct metrics: `avg_interval_ms`, `hhi`, `unique_signers`, etc.
- These are more interpretable anyway

**Option 3: Invert the Threshold**
- Add `min_soft_score ≥ 0.5` (prefer flags, not penalize them)
- This would select for more active communities

---

## Recommendation

**DO NOT use soft_score as a hard filter.** Here's why:

1. **It's backwards:** High scores = good (not bad)
2. **It's misleading:** Named "sybil" but measures "activity"
3. **Better metrics exist:** `avg_interval_ms`, `hhi`, `unique_signers` are clearer

Instead, use soft_score as a **monitoring/scoring metric** only:
- Track it for understanding pool characteristics
- But don't use it to accept/reject in Phase 1-6 filters

---

## Data-Backed Conclusion

**The 2,361% inversion is not a bug - it's a feature.**

It tells us: **Tokens with activity flags tend to survive.**

This makes intuitive sense:
- Dead tokens often have NO flags = no community = boring
- Survivor tokens often have flags = active participants = momentum

The current gatekeeper treats flags as "warnings" (soft_score penalty).
But the data says flags are "indicators of engagement" (soft_score opportunity).

**My recommendation:** Keep current filtering thresholds but remove soft_score from critical decisions. Focus on: `avg_interval_ms` (momentum), `hhi` (concentration), `unique_signers` (community).

---

## Action Items

```
✅ DO NOT change current max_soft_score thresholds
✅ Do NOT add min_soft_score filters
✅ Keep soft_score for monitoring only
✅ Focus filters on: avg_interval_ms, hhi, buy_ratio, market_cap
```

The inverted sybil paradox actually **validates the current strategy** - 
it shows the gatekeeper is already accepting high-activity tokens (which happen to be survivors).

---

**Final Take:** The sybil inversion isn't a problem to solve; it's a **discovery that the system was already doing the right thing** by not over-penalizing activity.
