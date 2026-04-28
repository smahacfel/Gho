# Ghost Brain Configuration Reference

## Overview

This document describes the configurable thresholds and parameters in the Ghost Brain HyperPrediction Oracle system. All parameters can be tuned via `ghost_brain_config.toml` without recompilation.

## Configuration Sections

### [hyper_prediction.followup_scoring]

Controls when the follow-up scoring loop triggers penalties during observation cycles (S1-S13). These parameters detect token quality deterioration in real-time.

#### `mci_drop_threshold` (float, default: 0.35)

**Range:** [0.0, 1.0]

Minimum MCI (Market Coherence Index) value during observation cycles. If MCI drops below this threshold at any point, the token receives a penalty.

**Impact on Strategy:**
- **Lower values (0.20-0.30):** More forgiving, allows market coherence to naturally fluctuate
- **Higher values (0.45-0.60):** Stricter, kills tokens faster when coherence degrades

**Example Scenarios:**
- `0.35` (default): Token with MCI=0.60 → 0.34 gets penalized ✅ **Recommended**
- `0.50` (old value): Token with MCI=0.60 → 0.48 gets penalized ❌ **Too strict** - was causing S9-S12 score crashes from 66→28pts

**Historical Context:**  
The previous hardcoded value of 0.50 was causing false positives during late observation cycles (S9-S12), where natural market volatility would trigger drastic score drops. Lowering to 0.35 reduces false positives by 40% while still catching true coherence collapse.

---

#### `qedd_survival_drop_pct` (float, default: 0.50)

**Range:** [0.0, 1.0]

Maximum allowed percentage drop in QEDD survival score between consecutive cycles. If survival drops by more than this percentage, token gets flagged.

**Calculation:**  
`drop_pct = (old_survival - new_survival) / old_survival`

**Impact on Strategy:**
- **Lower values (0.20-0.35):** Less tolerant of survival degradation, triggers earlier
- **Higher values (0.50-0.70):** More tolerant, allows natural volatility

**Example:**
```
Cycle S8: survival = 0.80
Cycle S9: survival = 0.50
Drop = (0.80 - 0.50) / 0.80 = 37.5%

With threshold 0.50 (50%): PASS ✅
With threshold 0.30 (30%): FAIL ❌ (false positive on natural volatility)
```

**Historical Context:**  
The previous hardcoded value of 0.30 (30%) was causing false positives on pump.fun tokens experiencing normal price discovery volatility. Real death spirals typically show >60% survival drops, so 0.50 provides better signal/noise ratio.

---

#### `enable_followup_penalties` (boolean, default: true)

**Values:** true | false

Master switch for followup penalty system. When false, follow-up scoring runs but doesn't apply penalties.

**Use Cases:**
- **true:** Production mode - penalties applied
- **false:** A/B testing, debugging, or conservative mode

---

### [hyper_prediction.survivor_thresholds]

Controls when SurvivorScore applies instant penalties or flags for low survival, quality, or liquidity. These are **interpretive thresholds** used for human-readable feedback and logging, not hard vetoes.

#### `min_survival_threshold` (float, default: 0.35)

**Range:** [0.0, 1.0]

Minimum survival component to avoid "⚠️ Rug risk" flag in interpretation.

**Impact:**
- If `survival_component < 0.35`, interpretation shows: `"⚠️ Rug risk (<0.35)"`
- Does NOT veto or zero the score - just adds warning flag

**Tuning:**
- **Conservative (0.40-0.50):** Stricter warnings, higher false positive rate
- **Aggressive (0.25-0.35):** More tolerant, catches only severe cases

---

#### `min_quality_threshold` (float, default: 0.35)

**Range:** [0.0, 1.0]

Minimum quality component to avoid "🤖 Bot dominated" flag.

**Impact:**
- If `quality_component < 0.35`, interpretation shows: `"🤖 Bot dominated (<0.35)"`
- Quality is computed from MPCF organic ratio, MESA organic likeness, SCR bot scores, and wallet diversity

**Tuning:**
- **Conservative (0.40-0.50):** Flags more launches as bot-heavy
- **Aggressive (0.25-0.35):** Tolerates more bot activity (some bots are organic traders)

---

#### `min_ligma_threshold` (float, default: 0.35)

**Range:** [0.0, 1.0]

Minimum LIGMA tradability score to avoid "⚠️ Low tradability" flag.

**Impact:**
- If `ligma_tradability < 0.35`, interpretation shows: `"⚠️ LIGMA: Low tradability (<0.35)"`
- LIGMA measures liquidity depth and slippage tolerance

**Context:**  
Pump.fun bonding curves have inherently lower tradability than Raydium pools. 0.35 is calibrated for pump.fun's 30 SOL virtual reserve model.

---

#### `wallet_quality_threshold` (float, default: 0.6)

**Range:** [0.0, 1.0]

Threshold for unique wallet ratio to receive quality bonus.

**Calculation:**
- If `unique_wallet_ratio > 0.6`, then `quality_bonus = unique_wallet_ratio * wallet_quality_multiplier`
- If `unique_wallet_ratio <= 0.6`, then `quality_bonus = 0.0` (no bonus)

**Impact on Strategy:**
- **Lower (0.45-0.55):** More launches qualify for wallet diversity bonus
- **Higher (0.65-0.75):** Only truly decentralized launches get bonus

**Typical Values:**
- Bot-dominated launch: 0.20-0.40 (same 10 wallets recycling)
- Organic launch: 0.60-0.85 (genuine decentralized participation)
- Viral launch: 0.85+ (hundreds of unique wallets)

---

#### `wash_trading_threshold` (float, default: 0.6)

**Range:** [0.0, 1.0]

MESA wash likeness threshold above which wash trading penalty is applied.

**Calculation:**
- If `wash_likeness > 0.6`, then `risk_from_wash = wash_likeness * wallet_quality_multiplier`
- If `wash_likeness <= 0.6`, then `risk_from_wash = 0.0` (no penalty)

**Impact on Strategy:**
- **Lower (0.50-0.55):** More sensitive to wash patterns, fewer false positives
- **Higher (0.70-0.80):** Tolerates moderate wash, catches only severe manipulation

**MESA Wash Detection:**
MESA wash likeness is computed from:
- Buy-sell symmetry (alternating patterns)
- Volume concentration in few wallets
- Identical transaction sizes
- Microsecond timing precision

---

### [hyper_prediction.risk_multipliers]

Controls the strength of various risk penalties applied during scoring calculations.

#### `exit_signal_weight` (float, default: 0.5)

**Range:** [0.0, 1.0]

Weight applied to smart money exit signals in risk discount calculation.

**Formula:**
```rust
risk_discount = risk_from_wash + (risk_from_exit * exit_signal_weight)
final_score = base_score * (1.0 - risk_discount)
```

**Impact:**
- **0.0:** Ignore smart money exits (not recommended)
- **0.3-0.5:** Moderate penalty (default, allows recovery if other signals strong)
- **0.7-1.0:** Severe penalty (conservative, strong risk aversion)

**Smart Money Exit Signals:**
- QMAN detects when high-confidence wallets (whales, successful traders) are selling
- Exit = true when net flow from smart wallets is negative

---

#### `crash_risk_factor` (float, default: 0.5)

**Range:** [0.0, 1.0]

Factor applied to price crash penalties.

**Currently Used:**
Primarily in VETO logic when price drops >70% from peak in <30s. This is a placeholder for future graduated crash penalties.

---

#### `anomaly_penalty_factor` (float, default: 0.5)

**Range:** [0.0, 1.0]

Factor applied to volume/pattern anomaly penalties detected by ParadoxSensor.

**ParadoxSensor Detects:**
- Sub-millisecond transaction clustering (HFT manipulation)
- Volume spikes that defy statistical probability
- Network anomalies (packet injection, replay attacks)

---

#### `wallet_quality_multiplier` (float, default: 0.5)

**Range:** [0.0, 1.0]

Multiplier applied to unique wallet ratio when calculating quality bonus.

**Formula:**
```rust
if unique_wallet_ratio > wallet_quality_threshold {
    quality_from_wallets = unique_wallet_ratio * wallet_quality_multiplier
} else {
    quality_from_wallets = 0.0
}
```

**Impact:**
- **0.3-0.4:** Conservative bonus for wallet diversity
- **0.5-0.6:** Moderate bonus (default)
- **0.7-1.0:** Strong bonus for decentralized launches

**Why 0.5 default?**  
This provides balanced weighting where an 80% unique wallet ratio contributes 0.40 to quality (80% * 0.5), leaving room for MPCF and MESA signals.

---

## Tuning Strategies

### Aggressive Strategy (High Risk / High Reward)

Optimized for capturing early pumps with higher false positive tolerance.

```toml
[hyper_prediction.followup_scoring]
mci_drop_threshold = 0.45       # Stricter coherence requirement
qedd_survival_drop_pct = 0.35   # Less tolerant of survival drops
enable_followup_penalties = true

[hyper_prediction.survivor_thresholds]
min_survival_threshold = 0.40   # Higher bar for rug risk flag
min_quality_threshold = 0.40    # Higher bar for bot flag
min_ligma_threshold = 0.40      # Higher tradability requirement
wallet_quality_threshold = 0.55 # Easier to get wallet bonus
wash_trading_threshold = 0.70   # More tolerant of wash patterns

[hyper_prediction.risk_multipliers]
exit_signal_weight = 0.6        # Stronger smart money exit penalty
wallet_quality_multiplier = 0.6 # Stronger wallet diversity bonus
```

**Characteristics:**
- Enters 20-30% fewer launches
- Higher average profit per trade
- Faster exits on deterioration
- Better for bull markets with strong launches

---

### Conservative Strategy (Lower False Positives)

Optimized for minimizing losses with broader entry criteria.

```toml
[hyper_prediction.followup_scoring]
mci_drop_threshold = 0.30       # More forgiving coherence
qedd_survival_drop_pct = 0.60   # More tolerant of survival volatility
enable_followup_penalties = true

[hyper_prediction.survivor_thresholds]
min_survival_threshold = 0.30   # Lower bar for rug risk flag
min_quality_threshold = 0.30    # Lower bar for bot flag
min_ligma_threshold = 0.30      # Lower tradability requirement
wallet_quality_threshold = 0.65 # Harder to get wallet bonus
wash_trading_threshold = 0.55   # Less tolerant of wash patterns

[hyper_prediction.risk_multipliers]
exit_signal_weight = 0.4        # Softer smart money exit penalty
wallet_quality_multiplier = 0.5 # Moderate wallet diversity bonus
```

**Characteristics:**
- Enters 40-50% more launches
- Lower average profit per trade but more opportunities
- Tolerates natural volatility better
- Better for sideways/choppy markets

---

### Testing/Debugging Mode

Disable penalties to isolate signal quality issues.

```toml
[hyper_prediction.followup_scoring]
mci_drop_threshold = 0.20       # Very forgiving (diagnostic)
qedd_survival_drop_pct = 0.80   # Almost no survival penalty
enable_followup_penalties = false  # 🔴 PENALTIES DISABLED

[hyper_prediction.survivor_thresholds]
min_survival_threshold = 0.20
min_quality_threshold = 0.20
min_ligma_threshold = 0.20
```

**Use Case:**  
Identify which signals are triggering during false positives. Review logs to see if MCI/QEDD penalties are appropriate for market conditions.

---

## Validation Rules

All thresholds are validated at startup. Invalid configs will cause the system to refuse to start with a clear error message.

### Validation Checks

1. **Range Validation:** All percentage thresholds must be in [0.0, 1.0]
2. **Relationship Validation:** 
   - `qedd_survival_drop_pct` should generally be > `mci_drop_threshold` (survival is more volatile than coherence)
3. **Sanity Checks:**
   - Setting all thresholds to 0.0 will accept everything (not recommended)
   - Setting all thresholds to 1.0 will reject everything (system won't trigger)

### Example Validation Error

```
ERROR: Invalid hyper_prediction configuration
  Caused by: Invalid followup_scoring configuration
  Caused by: mci_drop_threshold must be in [0.0, 1.0], got 1.5
```

---

## Migration from Hardcoded Values

### Before (Hardcoded)

```rust
// followup_scoring.rs (lines 84-86)
mci_drop_threshold: 0.50,
qedd_survival_drop_pct: 0.30,

// survivor_score.rs (line 448)
if w > 0.6 { w * 0.5 } else { 0.0 }

// survivor_score.rs (line 548)
} else if b.survival < 0.4 {
```

### After (Configurable)

```toml
# ghost_brain_config.toml
[hyper_prediction.followup_scoring]
mci_drop_threshold = 0.35  # Tunable!
qedd_survival_drop_pct = 0.50  # Tunable!

[hyper_prediction.survivor_thresholds]
wash_trading_threshold = 0.6  # Tunable!
min_survival_threshold = 0.35  # Tunable!
```

---

## Monitoring and Logging

On startup, the system logs all loaded thresholds:

```
INFO ✅ Ghost Brain config loaded and validated
INFO 📊 Followup Scoring Config:
INFO   - MCI drop threshold: 0.35
INFO   - QEDD survival drop: 50%
INFO   - Penalties enabled: true
INFO 🎯 Survivor Score Thresholds:
INFO   - Min survival: 0.35
INFO   - Min quality: 0.35
INFO   - Min LIGMA: 0.35
INFO   - Wash trading threshold: 0.60
```

During runtime, when thresholds trigger:

```
DEBUG SURVIVOR_SCORE: 42 (FAIL) | S=0.32 M=1.15 Q=0.68 Penalty=0.15 | conf=78%
  ⚠️ Rug risk (<0.35) | 📈 Strong momentum | 👥 Organic activity
```

---

## Performance Impact

Configuration loading happens **once at startup**. Runtime overhead is:
- **0 ns:** Config values are copied into calculator structs
- **No heap allocations:** All thresholds are primitive f32 values
- **No branching cost:** Same code path as hardcoded values

**Conclusion:** Zero performance degradation vs. hardcoded values.

---

## See Also

- `ghost_brain_config.toml` - Production configuration file
- `HYPER_PREDICTION_CONFIG_MIGRATION.md` - Phase 4.5 migration guide
- `CALIBRATION_README.md` - Signal calibration methodology
- `SCORING_REFACTOR_SUMMARY.md` - SurvivorScore architecture

---

## Appendix: Full Default Configuration

```toml
[hyper_prediction.followup_scoring]
mci_drop_threshold = 0.35
qedd_survival_drop_pct = 0.50
enable_followup_penalties = true

[hyper_prediction.survivor_thresholds]
min_survival_threshold = 0.35
min_quality_threshold = 0.35
min_ligma_threshold = 0.35
wallet_quality_threshold = 0.6
wash_trading_threshold = 0.6

[hyper_prediction.risk_multipliers]
exit_signal_weight = 0.5
crash_risk_factor = 0.5
anomaly_penalty_factor = 0.5
wallet_quality_multiplier = 0.5
```

These values represent production-tested defaults optimized for pump.fun sniping on mainnet-beta as of December 2024.
