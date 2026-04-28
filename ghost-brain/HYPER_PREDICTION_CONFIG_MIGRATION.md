# HyperPrediction Configuration Migration Guide

## Overview

This document provides guidance on migrating from hardcoded constants to the new configuration system for the HyperPrediction Oracle. All scoring thresholds, penalties, and normalization factors are now configurable via `ghost_brain_config.toml`.

## What Changed

### Before (Hardcoded Constants)
```rust
const SURVIVOR_CRITICAL_THRESHOLD: u8 = 35;
const QASS_SECONDARY_MAX_ADJUSTMENT: i8 = 10;
const MESA_WASH_SEVERE_THRESHOLD: f32 = 0.85;
// ... 15+ other constants scattered throughout the code
```

### After (Configuration File)
```toml
[hyper_prediction]
survivor_critical_threshold = 35
qass_secondary_max_adjustment = 10
mesa_wash_severe_threshold = 0.85
# ... all parameters documented and configurable
```

## Configuration Structure

### File Location
```
ghost-brain/ghost_brain_config.toml
```

### New Configuration Section
```toml
[hyper_prediction]
# SurvivorScore Configuration
survivor_critical_threshold = 35
qass_secondary_max_adjustment = 10
qass_min_confidence_for_modifier = 0.6

# Cold Start Configuration
cold_start_max_adjustment = 0.3
cold_start_qedd_mci_weight = 10.0

# MESA Microstructure Thresholds
mesa_wash_severe_threshold = 0.85
mesa_wash_elevated_threshold = 0.70
mesa_bot_high_threshold = 0.90
mesa_bot_moderate_threshold = 0.75
mesa_organic_bonus_threshold = 0.75
mesa_organic_max_wash = 0.40
mesa_entropy_bonus_threshold = 0.80
mesa_entropy_max_wash = 0.50

# Scoring Normalization
min_volume_scale = 0.0001
relative_factor_cap = 2.0
burst_normalization = 2.0

[hyper_prediction.risk_thresholds]
very_high_confidence = 0.5
high_confidence = 0.7
medium_score = 60
```

## Parameter Reference

### SurvivorScore Configuration

#### `survivor_critical_threshold`
- **Type**: `u8` (0-100)
- **Default**: 35
- **Purpose**: Early exit threshold for tokens with very low SurvivorScore
- **Range**: [0, 100]
- **Tuning Guide**:
  - **Higher (40-50)**: More aggressive filtering, fewer false positives but more missed opportunities
  - **Lower (25-35)**: More permissive, catches more potential winners but with higher risk
  - **Recommended**: Keep at 35 (empirically validated survival threshold)

#### `qass_secondary_max_adjustment`
- **Type**: `i8` (signed)
- **Default**: 10
- **Purpose**: Maximum points QASS can add/subtract as secondary modifier
- **Range**: [0, 30]
- **Tuning Guide**:
  - **Higher (15-20)**: QASS has more influence (use if wave analysis is highly accurate)
  - **Lower (5-10)**: QASS as gentle nudge (current Phase 4.5 approach)
  - **Recommended**: 10 (balances SurvivorScore primacy with QASS value)

#### `qass_min_confidence_for_modifier`
- **Type**: `f32` (0.0-1.0)
- **Default**: 0.6
- **Purpose**: Minimum QASS confidence required to apply modifier
- **Range**: [0.0, 1.0]
- **Tuning Guide**:
  - **Higher (0.7-0.8)**: Only high-confidence QASS affects score (conservative)
  - **Lower (0.4-0.6)**: More QASS signals considered (riskier)
  - **Recommended**: 0.6 (filters out noisy low-confidence results)

### Cold Start Configuration

#### `cold_start_max_adjustment`
- **Type**: `f32` (0.0-1.0)
- **Default**: 0.3
- **Purpose**: Maximum percentage adjustment based on Chaos Engine pump probability
- **Range**: [0.0, 0.5]
- **Example**: `base_score=50, pump_prob=80% → adjustment=+9% → final=54.5`
- **Tuning Guide**:
  - **Higher (0.4-0.5)**: Aggressive early entry based on Monte Carlo simulations
  - **Lower (0.2-0.3)**: Conservative, wait for more transaction data
  - **Recommended**: 0.3 (30% adjustment balances early detection vs false positives)

#### `cold_start_qedd_mci_weight`
- **Type**: `f32` (1.0-20.0)
- **Default**: 10.0
- **Purpose**: Weight multiplier for first-candle volume in QEDD/MCI extrapolation
- **Range**: [1.0, 20.0]
- **Example**: `5 SOL burst in S1 → treated as 50 SOL accumulated volume`
- **Tuning Guide**:
  - **Higher (15-20)**: Trust early bursts more (pump.fun style)
  - **Lower (5-10)**: Require sustained volume accumulation
  - **Recommended**: 10.0 (empirically validated for pump.fun launches)

### MESA Microstructure Thresholds

#### Wash Trading Detection

**`mesa_wash_severe_threshold`** (Default: 0.85)
- **Triggers**: -25 pts penalty, VeryHigh risk escalation
- **Indicates**: Coordinated buy-sell cycling (85%+ wash likeness)
- **Pattern**: Alternating 1 SOL buys/sells from 2-3 wallets
- **Tuning**: Increase to 0.90 for stricter detection, decrease to 0.80 for earlier warning

**`mesa_wash_elevated_threshold`** (Default: 0.70)
- **Triggers**: -12 pts penalty, Medium→High risk escalation
- **Indicates**: Suspicious patterns (70%+ wash likeness)
- **Pattern**: High buy-sell symmetry, low unique wallets
- **Tuning**: Keep 0.15 gap below severe threshold for gradual penalty scaling

#### Bot Pattern Detection

**`mesa_bot_high_threshold`** (Default: 0.90)
- **Triggers**: -15 pts penalty, High risk escalation
- **Indicates**: Automated snipers (90%+ bot likeness)
- **Pattern**: Identical tx sizes, microsecond timing, low entropy
- **Tuning**: Increase to 0.95 if many legitimate tokens flagged as bots

**`mesa_bot_moderate_threshold`** (Default: 0.75)
- **Triggers**: -8 pts penalty
- **Indicates**: Significant sniper presence (75%+ bot likeness)
- **Pattern**: Some organic mixing with bot activity
- **Tuning**: Keep 0.15 gap below high threshold

#### Organic Activity Bonuses

**`mesa_organic_bonus_threshold`** (Default: 0.75)
- **Triggers**: +8 pts bonus (when wash < 0.40)
- **Indicates**: Genuine community interest (75%+ organic likeness)
- **Pattern**: Varied volumes, high unique wallets, high entropy
- **Tuning**: Increase to 0.80 for stricter organic criteria

**`mesa_organic_max_wash`** (Default: 0.40)
- **Purpose**: Maximum wash likeness allowed for organic bonus
- **Rationale**: Normal trading can show 30-40% wash patterns (profit-taking, rebalancing)
- **Tuning**: Increase to 0.50 if too restrictive

**`mesa_entropy_bonus_threshold`** (Default: 0.80)
- **Triggers**: +5 pts bonus (when wash < 0.50)
- **Indicates**: High unpredictability (80%+ entropy)
- **Pattern**: Genuine decentralized activity
- **Tuning**: Can be more permissive (0.75) or strict (0.85)

**`mesa_entropy_max_wash`** (Default: 0.50)
- **Purpose**: Maximum wash likeness allowed for entropy bonus
- **Rationale**: High entropy can coexist with some wash if patterns are unpredictable
- **Tuning**: More permissive than organic_max_wash (0.50 vs 0.40)

### Scoring Normalization

#### `min_volume_scale`
- **Type**: `f64` (scientific notation)
- **Default**: 0.0001 (1e-4)
- **Purpose**: Floor value for volume normalization (prevents division by zero)
- **Range**: [1e-5, 1e-3]
- **Tuning**: Rarely needs adjustment; pump.fun launches with < 0.0001 SOL are DOA

#### `relative_factor_cap`
- **Type**: `f64`
- **Default**: 2.0
- **Purpose**: Maximum multiplier for volume burst normalization
- **Range**: [1.5, 5.0]
- **Effect**: Caps extreme spikes to prevent single-transaction score manipulation
- **Tuning**:
  - **Lower (1.5-2.0)**: Conservative, dampens burst impact
  - **Higher (3.0-5.0)**: Allows larger bursts to influence score more

#### `burst_normalization`
- **Type**: `f64`
- **Default**: 2.0
- **Purpose**: Divides relative burst factor to control sensitivity
- **Range**: [1.5, 3.0]
- **Effect**: A 2x burst becomes 1.0x multiplier (neutral), 4x burst becomes 2x multiplier
- **Tuning**:
  - **Lower (1.5-2.0)**: More sensitive to bursts
  - **Higher (2.5-3.0)**: Dampens burst volatility further

### Risk Assessment Thresholds

#### `very_high_confidence`
- **Type**: `f32` (0.0-1.0)
- **Default**: 0.5
- **Purpose**: Confidence below this triggers VeryHigh risk
- **Meaning**: < 50% confidence = insufficient data quality
- **Typical Causes**: IWIM unavailable, multiple key signals missing, high fallback penalties
- **Tuning**: Rarely change; 0.5 is empirically validated threshold

#### `high_confidence`
- **Type**: `f32` (0.0-1.0)
- **Default**: 0.7
- **Purpose**: Confidence below this triggers High risk
- **Meaning**: 50-70% confidence = some data quality issues
- **Typical Causes**: Early-stage analysis (S1-S7 cycles), partial signal availability
- **Tuning**: Can lower to 0.6 for more permissive early-stage entries

#### `medium_score`
- **Type**: `u8` (0-100)
- **Default**: 60
- **Purpose**: Score below this triggers Medium risk (when confidence OK)
- **Meaning**: < 60 score = fundamental weakness (low liquidity, no dev buy, etc.)
- **Historical Context**: Empirical "survival threshold" from backtesting
- **Tuning**: Rarely change; 60 is statistically validated survival boundary

## Migration Checklist

- [x] **Code Changes**: All hardcoded constants replaced with config fields
- [x] **Config File**: `ghost_brain_config.toml` updated with `[hyper_prediction]` section
- [x] **Documentation**: Each parameter documented with expected range and rationale
- [x] **Validation**: `HyperPredictionConfig::validate()` checks all ranges
- [x] **Backward Compatibility**: Defaults match previous hardcoded values

## Tuning Workflows

### Conservative Strategy (Minimize False Positives)
```toml
[hyper_prediction]
survivor_critical_threshold = 40  # Higher bar
qass_secondary_max_adjustment = 5  # Less QASS influence
mesa_wash_elevated_threshold = 0.65  # Earlier wash detection
mesa_bot_high_threshold = 0.85  # Stricter bot filtering
cold_start_max_adjustment = 0.2  # Less early-stage risk
```

### Aggressive Strategy (Maximize Coverage)
```toml
[hyper_prediction]
survivor_critical_threshold = 30  # Lower bar
qass_secondary_max_adjustment = 15  # More QASS influence
mesa_wash_elevated_threshold = 0.75  # More permissive
mesa_bot_high_threshold = 0.95  # Allow more bot activity
cold_start_max_adjustment = 0.4  # More early-stage risk
```

### Balanced Strategy (Default)
```toml
[hyper_prediction]
# Use default values - empirically validated for pump.fun
survivor_critical_threshold = 35
qass_secondary_max_adjustment = 10
mesa_wash_severe_threshold = 0.85
mesa_wash_elevated_threshold = 0.70
mesa_bot_high_threshold = 0.90
mesa_bot_moderate_threshold = 0.75
cold_start_max_adjustment = 0.3
```

## Testing Configuration Changes

### 1. Validation Testing
```rust
use ghost_brain::oracle::hyper_prediction::HyperPredictionConfig;

let config = HyperPredictionConfig {
    survivor_critical_threshold: 40,  // Test change
    ..Default::default()
};

// This will error if value is out of range
config.validate().expect("Config validation failed");
```

### 2. Integration Testing
```rust
use ghost_brain::config::GhostBrainConfig;
use ghost_brain::oracle::hyper_prediction::HyperPredictionOracle;

let config = GhostBrainConfig::from_toml_file("test_config.toml")?;
let oracle = HyperPredictionOracle::new_with_config(70, &config);

// Oracle will use your custom config values
```

### 3. A/B Testing
```bash
# Run with default config
./ghost-brain --config ghost_brain_config.toml

# Run with aggressive config
./ghost-brain --config ghost_brain_config_aggressive.toml

# Compare results and adjust
```

## Common Configuration Errors

### Error: `survivor_critical_threshold must be <= 100`
**Cause**: Set threshold above 100
**Fix**: Use values in [0, 100] range

### Error: `mesa_wash_elevated_threshold (0.90) must be <= mesa_wash_severe_threshold (0.80)`
**Cause**: Threshold ordering violated (elevated > severe)
**Fix**: Ensure elevated threshold is always less than severe threshold

### Error: `cold_start_max_adjustment must be in [0.0, 0.5]`
**Cause**: Set adjustment factor too high
**Fix**: Keep in [0.0, 0.5] range (max 50% adjustment)

### Error: `risk_thresholds.very_high_confidence (0.8) must be <= high_confidence (0.6)`
**Cause**: Confidence threshold ordering violated
**Fix**: Ensure very_high < high confidence thresholds

## Rollback Plan

If configuration changes cause unexpected behavior:

1. **Restore defaults**: Comment out custom `[hyper_prediction]` section
2. **Rebuild**: `cargo build --release`
3. **Verify**: Check that oracle uses hardcoded defaults
4. **Investigate**: Review logs to identify which parameter caused issues

## Support

For questions or issues with configuration:

1. Check parameter ranges in `config.rs` documentation
2. Review validation error messages (they indicate specific constraints)
3. Test changes incrementally (one parameter at a time)
4. Monitor SurvivorScore confidence and final scores for unexpected changes

## Future Enhancements

Potential additions to the configuration system:

- **Dynamic weight adjustment**: Auto-tune weights based on historical performance
- **Environment-specific configs**: Separate configs for dev/staging/prod
- **Real-time tuning**: Hot-reload configuration without restart
- **Parameter profiles**: Pre-defined sets (conservative/balanced/aggressive)
- **Validation CLI**: `ghost-brain validate-config` command
