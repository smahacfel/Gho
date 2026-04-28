# Confidence Model Documentation

## Overview

The Confidence Model is a formal system for quantifying the reliability and trustworthiness of Oracle Brain scoring decisions. It provides a confidence score in the range [0, 1] that represents how certain the system is about its predictions based on the quality and coherence of signals from all analytical modules.

## Mathematical Foundation

### Core Formula

The overall confidence C ∈ [0, 1] is computed as a weighted sum of module contributions:

```
C = Σ(w_i · c_i) / Σ(w_i)

where:
  C ∈ [0, 1]    - Overall confidence score
  w_i ∈ ℝ⁺      - Weight for module i
  c_i ∈ [0, 1]  - Normalized contribution from module i
```

### Confidence Interpretation

| Confidence Range | Interpretation | Position Sizing |
|-----------------|----------------|-----------------|
| C > 0.8 | High Confidence | Full position, normal sizing |
| 0.5 < C ≤ 0.8 | Medium Confidence | Reduced position (50-80%) |
| C ≤ 0.5 | Low Confidence | Skip or minimal position (<30%) |

## Module Contribution Table

Each module contributes to overall confidence based on signal quality, strength, noise level, and data completeness.

### 1. SOBP (Slot-Over-Slot Buying Pressure)

- **Weight**: 12.0
- **Formula**: `c_sobp = (1.0 - sobp_drop) · min(1.0, sobp_current / sobp_ma)`
- **Contribution Logic**:
  - High when buying pressure is stable (low drop)
  - High when current SOBP is at or above moving average
  - Captures momentum and pressure sustainability
- **Noise Sources**:
  - Slot boundary artifacts
  - Network latency variations
  - Block producer scheduling irregularities
- **Data Quality Indicators**:
  - ✅ Good: sobp_drop < 0.2, current/ma ≈ 1.0-1.5
  - ⚠️ Medium: sobp_drop 0.2-0.5, current/ma 0.5-2.0
  - ❌ Poor: sobp_drop > 0.5, current/ma < 0.5 or > 2.0

### 2. MPCF (Micro-Payload Cognitive Fingerprint)

- **Weight**: 10.0
- **Formula**: `c_mpcf = mpcf_entropy`
- **Contribution Logic**:
  - High entropy indicates organic, non-bot behavior
  - Directly uses normalized entropy value
  - Measures transaction pattern diversity
- **Noise Sources**:
  - Transaction batching effects
  - MEV bundle interference
  - Wallet software defaults
- **Data Quality Indicators**:
  - ✅ Good: entropy > 0.7 (organic patterns)
  - ⚠️ Medium: entropy 0.4-0.7 (mixed patterns)
  - ❌ Poor: entropy < 0.4 (bot-like patterns)

### 3. IWIM (Inter-Wallet Interaction Matrix)

- **Weight**: 8.0
- **Formula**: `c_iwim = network_coherence · (1.0 - bot_score)`
- **Contribution Logic**:
  - High when wallet network shows organic interconnections
  - Low when bot activity is detected
  - Captures graph-level behavior patterns
- **Noise Sources**:
  - Sybil wallet networks
  - Wash trading patterns
  - Legitimate automated market makers
- **Data Quality Indicators**:
  - ✅ Good: coherence > 0.7, bot_score < 0.2
  - ⚠️ Medium: coherence 0.4-0.7, bot_score 0.2-0.5
  - ❌ Poor: coherence < 0.4, bot_score > 0.5

### 4. SSMI (Sub-Slot Microentropy Index)

- **Weight**: 9.0
- **Formula**: `c_ssmi = ssmi_entropy`
- **Contribution Logic**:
  - High entropy indicates healthy microstructure
  - Measures fine-grained transaction timing diversity
  - Complements MPCF at smaller time scales
- **Noise Sources**:
  - Slot timing jitter
  - Concurrent transaction batching
  - Network propagation delays
- **Data Quality Indicators**:
  - ✅ Good: entropy > 0.65 (diverse microstructure)
  - ⚠️ Medium: entropy 0.4-0.65 (moderate diversity)
  - ❌ Poor: entropy < 0.4 (synchronized patterns)

### 5. QASS (Quantum Amplitude Scoring System)

- **Weight**: 15.0 (highest weight - most critical)
- **Formula**: `c_qass = min(1.0, qass_score / 100.0) · (1.0 - qass_volatility)`
- **Contribution Logic**:
  - High when QASS score is strong (near 100)
  - Reduced by score volatility/instability
  - Captures overall opportunity quality
- **Noise Sources**:
  - Oracle MEV interference
  - Price manipulation attempts
  - Temporary liquidity imbalances
- **Data Quality Indicators**:
  - ✅ Good: score > 70, volatility < 0.2
  - ⚠️ Medium: score 40-70, volatility 0.2-0.5
  - ❌ Poor: score < 40, volatility > 0.5

### 6. QOFSV (Quantum Orderflow Shadow Vector)

- **Weight**: 11.0
- **Formula**: `c_qofsv = flow_magnitude · (1.0 - alignment_noise)`
- **Contribution Logic**:
  - High when flow is strong and direction is clear
  - Low when flow direction is noisy or contradictory
  - Captures momentum and directional clarity
- **Noise Sources**:
  - Order splitting strategies
  - Hidden liquidity pools
  - Cross-DEX arbitrage flows
- **Data Quality Indicators**:
  - ✅ Good: magnitude > 0.7, noise < 0.2
  - ⚠️ Medium: magnitude 0.4-0.7, noise 0.2-0.5
  - ❌ Poor: magnitude < 0.4, noise > 0.5

### 7. SCR (Slot-Coherence Resonance)

- **Weight**: 13.0
- **Formula**: `c_scr = 1.0 - scr_score` (inverse relationship)
- **Contribution Logic**:
  - High when bot activity is low (low SCR score)
  - Low when periodic/bot patterns detected (high SCR score)
  - Inverse relationship: less bots = higher confidence
- **Noise Sources**:
  - Network congestion patterns
  - Validator scheduling artifacts
  - Legitimate periodic trading strategies
- **Data Quality Indicators**:
  - ✅ Good: scr_score < 0.2 (minimal bot activity)
  - ⚠️ Medium: scr_score 0.2-0.5 (some bots detected)
  - ❌ Poor: scr_score > 0.5 (heavy bot activity)

### 8. FRB (Flow Resonance Broker)

- **Weight**: 7.0
- **Formula**: `c_frb = flow_coherence · (1.0 - resonance_noise)`
- **Contribution Logic**:
  - High when flow patterns are coherent and stable
  - Low when flow shows resonance noise (manipulation)
  - Detects artificial flow patterns
- **Noise Sources**:
  - Multi-pool arbitrage activity
  - Cross-DEX coordinated trading
  - Legitimate market making
- **Data Quality Indicators**:
  - ✅ Good: coherence > 0.7, noise < 0.2
  - ⚠️ Medium: coherence 0.4-0.7, noise 0.2-0.5
  - ❌ Poor: coherence < 0.4, noise > 0.5

### 9. QMAN (Quantum Market Anomaly Navigator)

- **Weight**: 14.0 (second highest - critical for risk)
- **Formula**: `c_qman = 1.0 - deviation_risk` (inverse relationship)
- **Contribution Logic**:
  - High when market behaves predictably (low deviation)
  - Low when anomalous behavior detected
  - Captures quantum state coherence
- **Noise Sources**:
  - Market regime transitions
  - Black swan events
  - Major news/announcements
- **Data Quality Indicators**:
  - ✅ Good: deviation < 0.2 (predictable behavior)
  - ⚠️ Medium: deviation 0.2-0.5 (moderate anomalies)
  - ❌ Poor: deviation > 0.5 (high uncertainty)

### 10. GeneMapper (Pattern Recognition)

- **Weight**: 10.0
- **Formula**: `c_gene = 1.0 - match_score` (inverse relationship)
- **Contribution Logic**:
  - High when no known scam patterns detected
  - Low when patterns match historical scams
  - Provides security layer
- **Noise Sources**:
  - False positive matches
  - Pattern drift over time
  - Legitimate tokens with similar patterns
- **Data Quality Indicators**:
  - ✅ Good: match < 0.1 (no scam patterns)
  - ⚠️ Medium: match 0.1-0.4 (weak similarities)
  - ❌ Poor: match > 0.4 (strong scam patterns)

### 11. ChaosEngine (Monte Carlo Simulation)

- **Weight**: 11.0
- **Formula**: `c_chaos = 1.0 - loss_probability` (inverse relationship)
- **Contribution Logic**:
  - High when simulations show low loss probability
  - Low when simulations predict high risk
  - Forward-looking risk assessment
- **Noise Sources**:
  - Model assumption violations
  - Limited scenario coverage
  - Rare event misestimation
- **Data Quality Indicators**:
  - ✅ Good: loss_prob < 0.2 (safe scenarios)
  - ⚠️ Medium: loss_prob 0.2-0.5 (moderate risk)
  - ❌ Poor: loss_prob > 0.5 (high risk)

## Weight Rationale

The default weights reflect the relative importance of each module:

| Category | Modules | Total Weight | Rationale |
|----------|---------|--------------|-----------|
| Critical | QASS (15), QMAN (14) | 29 | Core scoring and anomaly detection |
| High Importance | SCR (13), SOBP (12), QOFSV (11), ChaosEngine (11) | 47 | Bot detection, momentum, risk |
| Medium Importance | MPCF (10), GeneMapper (10), SSMI (9), IWIM (8) | 37 | Pattern analysis and entropy |
| Support | FRB (7) | 7 | Flow coherence validation |
| **Total** | | **120** | |

## Confidence Degradation Factors

Confidence naturally degrades when:

1. **Data Quality Issues**
   - Missing or incomplete data from modules
   - Stale data exceeding freshness thresholds
   - Corrupted or invalid signal values

2. **High Noise Levels**
   - Excessive bot activity (high SCR, high IWIM bot score)
   - Wash trading patterns (high FRB noise)
   - Market manipulation attempts (high QOFSV noise)

3. **Signal Conflicts**
   - QASS bullish but QEDD showing high decay
   - SOBP rising but QMAN showing high deviation
   - Contradictory flow signals from different modules

4. **Uncertainty Conditions**
   - Low liquidity environments
   - Thin order books
   - High volatility periods
   - Market regime transitions

## Integration with Oracle Decision

The confidence score modulates the final Oracle decision:

```rust
// Example decision logic with confidence
if confidence > 0.8 && oracle_score > 70 {
    decision = BUY;
    position_size = FULL_SIZE;
} else if confidence > 0.5 && oracle_score > 60 {
    decision = BUY;
    position_size = REDUCED_SIZE * confidence;
} else {
    decision = SKIP;
}
```

## Calibration Requirements

For the confidence model to be reliable, it must be:

1. **Calibrated**: P(success | C=x) ≈ x for all x ∈ [0, 1]
   - Confidence 0.8 should mean ~80% success rate
   - Requires backtesting and parameter tuning

2. **Sharp**: Maximize separation between successful and failed trades
   - High confidence should strongly correlate with success
   - Low confidence should strongly correlate with failure

3. **Stable**: Confidence should not fluctuate wildly on small changes
   - Smooth response to input variations
   - Avoid cliff effects and discontinuities

## Usage Example

```rust
use ghost_brain::oracle::confidence_model::{ConfidenceModel, ConfidenceInputs};
use ghost_brain::signals::MarketSignals;

// Create model
let model = ConfidenceModel::default();

// Build inputs from signals
let signals = get_market_signals();
let inputs = ConfidenceModel::build_inputs_from_signals(
    &signals,
    qass_score,
    qass_volatility,
    scr_score,
    gene_mapper_score,
    chaos_loss_prob,
);

// Calculate confidence
let confidence_score = model.calculate_confidence(&inputs);

println!("Overall Confidence: {:.2}", confidence_score.overall);
println!("Data Quality: {:.2}", confidence_score.metadata.data_quality);
println!("Noise Level: {:.2}", confidence_score.metadata.noise_level);

// Use in decision making
if confidence_score.overall > 0.8 {
    // High confidence - full conviction
} else if confidence_score.overall > 0.5 {
    // Medium confidence - reduced position
} else {
    // Low confidence - skip or minimal position
}
```

## Telemetry Integration

Confidence scores are logged to:

1. **Decision Logs** (`OracleDecisionLog`)
   - `initial_confidence`: Confidence at T=0
   - `final_confidence`: Confidence after follow-ups
   - Per-followup confidence tracking

2. **Telemetry Metrics**
   - `confidence_distribution_histogram`: Distribution of confidence scores
   - `confidence_by_outcome`: Confidence vs. actual outcomes
   - `module_contribution_tracking`: Individual module contributions

3. **Debug Logs**
   - Full `ConfidenceScore` with all module contributions
   - Metadata about data quality and noise levels
   - Per-candidate confidence evolution over time

## Future Enhancements

1. **Adaptive Weights**: Learn optimal weights from historical performance
2. **Confidence Intervals**: Provide uncertainty bounds around confidence estimates
3. **Multi-Timeframe**: Separate confidence for different holding periods
4. **Risk-Adjusted Confidence**: Incorporate risk tolerance into confidence calculation
5. **Ensemble Methods**: Combine multiple confidence models for robustness

## See Also

- `confidence_model.rs` - Implementation
- `confidence_model_demo.rs` - Example usage
- `decision_logger.rs` - Integration with decision logging
- `scoring.rs` - Integration with candidate scoring
