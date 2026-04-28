# Scoring Module

This module contains the refactored scoring system for HyperPrediction Oracle.

## Overview

The scoring module provides a centralized, configurable, and **uncapped** scoring system that:
- Uses SurvivorScore as the primary base
- Applies penalties and boosters without artificial caps
- Determines risk levels from RAW scores
- Clamps scores to [0, 100] only for UI display

## Architecture

```
scoring/
├── mod.rs          # Main scoring orchestration
├── weights.rs      # Configurable weight structures
├── penalties.rs    # Penalty calculations (UNCAPPED)
├── boosters.rs     # Boost calculations (UNCAPPED)
└── README.md       # This file
```

## Key Concepts

### Uncapped Scoring

Unlike the previous system that capped scores at each step, the new system:

1. **Penalties can drive scores negative**
   ```rust
   let mut score = 30.0;
   score -= 25.0 * weights.wash_penalty_mult;  // MESA wash
   score -= 30.0 * weights.rug_penalty_mult;   // IWIM rug threat
   score -= 15.0 * weights.bot_penalty_mult;   // Bot detection
   // score = -40.0 (negative!)
   ```

2. **Boosters can exceed 100**
   ```rust
   let mut score = 95.0;
   score += 15.0 * weights.chaos_pump_boost_mult;     // Chaos pump
   score += 10.0 * weights.organic_boost_mult;        // IWIM organic
   score += 8.0 * weights.mesa_organic_boost_mult;    // MESA organic
   // score = 128.0 (exceeds 100!)
   ```

3. **Risk determined from RAW score**
   ```rust
   let risk = if boosted_score < 20.0 { VeryHigh }
              else if boosted_score < 40.0 { High }
              else if boosted_score < 60.0 { Medium }
              else { Low };
   ```

4. **Display score clamped for UI**
   ```rust
   let display_score = boosted_score.clamp(0.0, 100.0) as u8;
   ```

### Why Uncapping?

**Problem with Capped Scoring:**
```rust
// Example 1: Severe penalties saturate at 0
score = 50;
score = score.saturating_sub(25);  // 25
score = score.saturating_sub(30);  // 0 (saturated)
// Lost information: actual score should be -5

// Example 2: Strong boosts clamp at 100
score = 95;
score = score.saturating_add(15);  // 100 (clamped)
score = score.saturating_add(10);  // 100 (clamped)
// Lost information: actual score should be 120
```

**Solution with Uncapped Scoring:**
```rust
// Penalties preserved
score = 50.0;
score -= 25.0;  // 25.0
score -= 30.0;  // -5.0 (full range!)

// Boosts preserved
score = 95.0;
score += 15.0;  // 110.0
score += 10.0;  // 120.0 (full range!)

// Risk reflects true signal strength
risk = determine_risk_from_raw(score);  // Uses -5.0 or 120.0

// UI still gets 0-100
display = score.clamp(0.0, 100.0);  // 0 or 100
```

## Components

### ScoringWeights (`weights.rs`)

Centralized configuration for all scoring parameters:

```rust
pub struct ScoringWeights {
    // Signal weights
    pub ligma: f32,
    pub qedd: f32,
    pub survivor: f32,
    
    // Penalty multipliers
    pub wash_penalty_mult: f32,      // MESA wash trading
    pub bot_penalty_mult: f32,       // Bot detection
    pub rug_penalty_mult: f32,       // Rug threat
    
    // Boost multipliers
    pub organic_boost_mult: f32,     // Organic activity
    pub chaos_pump_boost_mult: f32,  // Pump probability
    
    // Normalization
    pub volume_scale: f64,
    pub liquidity_scale: f64,
    
    // Thresholds
    pub mesa_wash_severe_threshold: f32,
    pub survivor_critical_threshold: u8,
    // ... 40+ configurable parameters
}
```

### Penalties (`penalties.rs`)

All penalty calculations in one place:

```rust
pub fn apply_penalties(
    base_score: f32,
    // ... signals ...
    weights: &ScoringWeights,
    is_early_stage: bool,
) -> f32 {
    let mut score = base_score;
    
    // MESA penalties (full analysis only)
    if !is_early_stage {
        if mesa.wash_likeness > weights.mesa_wash_severe_threshold {
            score -= 25.0 * weights.wash_penalty_mult;
        }
    }
    
    // IWIM penalties (both modes)
    if iwim.rug_threat_score > 0.8 {
        score -= 30.0 * weights.rug_penalty_mult;
    }
    
    // ... all other penalties ...
    
    score  // Can be negative!
}
```

### Boosters (`boosters.rs`)

All boost calculations in one place:

```rust
pub fn apply_boosters(
    base_score: f32,
    // ... signals ...
    weights: &ScoringWeights,
    is_early_stage: bool,
) -> f32 {
    let mut score = base_score;
    
    // MESA boosts (full analysis only)
    if !is_early_stage {
        if mesa.organic_likeness > weights.mesa_organic_bonus_threshold {
            score += 8.0 * weights.mesa_organic_boost_mult;
        }
    }
    
    // Chaos boosts (both modes)
    if chaos.pump_probability > 60.0 {
        score += 15.0 * weights.chaos_pump_boost_mult;
    }
    
    // ... all other boosts ...
    
    score  // Can exceed 100!
}
```

### Main Scoring (`mod.rs`)

Orchestration function that ties everything together:

```rust
pub fn calculate_final_score(
    survivor_result: &Option<SurvivorScoreResult>,
    qass_result: &QASSResult,
    // ... all signals ...
    weights: &ScoringWeights,
    risk_thresholds: &RiskThresholds,
    fallback_tracker: &FallbackTracker,
    threshold: u8,
    base_score: u8,
    is_early_stage: bool,
) -> (u8, RiskLevel, bool) {
    // 1. Base from SurvivorScore
    let base = get_survivor_score(survivor_result, base_score);
    
    // 2. QASS modifier (±10 max)
    let with_qass = apply_qass_modifier(base, qass_result, weights);
    
    // 3. Fallback penalty
    let with_fallback = with_qass * fallback_tracker.confidence_multiplier();
    
    // 4. Penalties (UNCAPPED)
    let penalized = penalties::apply_penalties(
        with_fallback, /* signals */, weights, is_early_stage
    );
    
    // 5. Boosters (UNCAPPED)
    let boosted = boosters::apply_boosters(
        penalized, /* signals */, weights, is_early_stage
    );
    
    // 6. Risk from RAW score
    let risk = determine_risk(boosted);
    
    // 7. Clamp for display
    let display = boosted.clamp(0.0, 100.0) as u8;
    
    // 8. Pass/fail
    let passed = survivor_passed && display >= threshold;
    
    (display, risk, passed)
}
```

## Usage

```rust
use crate::oracle::hyper_prediction::scoring::{self, ScoringWeights};

// Create weights (or load from config)
let weights = ScoringWeights::default();

// Calculate final score
let (display_score, risk, passed) = scoring::calculate_final_score(
    &survivor_result,
    &qass_result,
    &ssmi_result,
    // ... all signals ...
    &weights,
    &risk_thresholds,
    &fallback_tracker,
    threshold,
    base_score,
    is_early_stage,
);

// display_score: 0-100 for UI
// risk: VeryHigh/High/Medium/Low (from RAW score)
// passed: true if meets threshold AND survivor passed
```

## Testing

Comprehensive test coverage ensures correctness:

```rust
#[test]
fn test_penalties_allow_negative_scores() {
    // Verify severe penalties can drive scores negative
}

#[test]
fn test_boosters_allow_scores_above_100() {
    // Verify strong boosts can exceed 100
}

#[test]
fn test_risk_determined_from_raw_score() {
    // Verify risk uses uncapped score
}

#[test]
fn test_early_stage_skips_trend_penalties() {
    // Verify mode-specific logic
}
```

## Configuration

Future: Load weights from `ghost_brain_config.toml`:

```toml
[scoring]
# Penalty multipliers (adjust severity)
wash_penalty_mult = 1.2      # Increase wash penalty by 20%
bot_penalty_mult = 0.8       # Decrease bot penalty by 20%
rug_penalty_mult = 1.5       # Increase rug penalty by 50%

# Boost multipliers
organic_boost_mult = 1.3
chaos_pump_boost_mult = 1.0

# Normalization
volume_scale = 0.0001
liquidity_scale = 1.0

# Thresholds
mesa_wash_severe_threshold = 0.85
survivor_critical_threshold = 35
```

## Migration from Old System

The old `combine_scores` function has been replaced but preserved as `combine_scores_legacy` for reference.

**Key differences:**
1. No more `.saturating_sub()` / `.saturating_add()` in penalties/boosters
2. No more intermediate `.clamp()` operations
3. Risk determined from RAW scores, not display scores
4. All constants moved to `ScoringWeights` struct

**External API unchanged:**
- `HyperPredictionResult.score` still returns 0-100
- Risk levels still use same enum
- All signal inputs remain the same

## Benefits

1. **No Information Loss**: Full signal range preserved
2. **Better Risk Assessment**: Risk reflects true signal strength
3. **Configurable**: All parameters adjustable without code changes
4. **Testable**: Clean separation makes testing easier
5. **Maintainable**: Single source of truth for all scoring logic

## Future Enhancements

- [ ] Load weights from config file
- [ ] A/B testing framework for weight tuning
- [ ] Historical scoring analysis tools
- [ ] Weight optimization via ML

## References

- Issue: `Criptocopenhaegen/ghost#X` (Extract scoring logic)
- Audit Report: `ANALYSIS_GHOST_BRAIN_SCORING.md`
- CHANGELOG: `CHANGELOG.md`
