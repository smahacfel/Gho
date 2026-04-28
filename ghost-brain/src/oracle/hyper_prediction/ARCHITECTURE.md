# HyperPrediction Oracle Architecture

## Overview

The HyperPrediction Oracle provides fast (<2s) token evaluation combining multiple analysis modules. It replaces SimpleOracle with an advanced prediction system that integrates:
- Shadow Ledger RAM simulations
- SSMI (Sub-Slot Microentropy Index)
- MPCF (Micro-Payload Cognitive Fingerprint)
- QASS (Quantum Amplitude Superposition Scoring)
- SCR/ULVF/POVC from HyperOracle
- Enhanced scoring with contextual analysis

## Module Breakdown

### verdict.rs (Types)
Decision-related types for the Oracle:
- `OracleDecision`: Buy/Skip/Watch decision enum
- `RiskLevel`: Low/Medium/High/VeryHigh risk classification
- `RiskThresholds`: Configurable boundaries for risk assessment
- `FinalVerdict`: Aggregated verdict structure

### state.rs (Data)
Result structures and analysis phase tracking:
- `AnalysisPhase`: EarlyStage vs FullAnalysis mode tracking
- `QmanResult`: QMAN capital flow prediction results
- `HyperPredictionResult`: Complete evaluation result with all module outputs

### signals/ (Sensors)
Signal collection with explicit fallback tracking:
- **mod.rs**: `SignalCollector`, `SignalBundle`, `SignalResult`, `SignalSource`
- **ligma.rs**: Liquidity trap detection signals
- **qedd.rs**: Survival probability signals
- **cluster.rs**: Cabal/sybil detection signals
- **mci.rs**: Market coherence signals
- **paradox.rs**: HFT activity detection signals

Features:
- One file per signal type
- Explicit fallback tracking via `SignalSource` enum
- Centralized phase-conditional checks via `SignalCollector::run_if_mature()`

### scoring/ (Math)
Score calculation with transparent modifiers:
- **mod.rs**: Main `calculate_final_score()` orchestration
- **weights.rs**: `ScoringWeights` configuration
- **penalties.rs**: `apply_penalties()` - uncapped penalty application
- **boosters.rs**: `apply_boosters()` - uncapped boost application

Features:
- Uncapped internal calculation (can go negative or exceed 100)
- Risk level determined from RAW score before clamping
- Display score clamped to [0, 100] only for UI
- Configurable weights for all modifiers

### config.rs (Parameters)
All tunable constants loaded from TOML:
- `HyperPredictionConfig`: Central configuration struct
- SurvivorScore thresholds
- Cold start parameters
- MESA microstructure thresholds
- Scoring normalization factors
- Risk assessment boundaries

## Signal Flow

```text
EnhancedCandidate
     |
+----------------------------------------------------+
|              HyperPredictionOracle                 |
|                score_candidate()                   |
|                                                    |
|  Phase Detection (EarlyStage vs FullAnalysis)     |
|                      |                             |
|  +----------------------------------------------+  |
|  |              FAIL-FAST GATES                 |  |
|  |  - ClusterHunter (cabal_risk > 0.65)        |  |
|  |  - LIGMA (liquidity_trap_risk > threshold)   |  |
|  |  - LIGMA (psi_ligma < -0.5)                  |  |
|  +----------------------------------------------+  |
|                      |                             |
|  +----------------------------------------------+  |
|  |           PARALLEL ANALYSIS                  |  |
|  |  - Shadow Ledger wave                        |  |
|  |  - SSMI (Full Analysis only)                 |  |
|  |  - MPCF (both modes if tx_data present)      |  |
|  |  - IWIM (both modes, async in S1-S7)         |  |
|  |  - PRAECOG (both modes, static pool)         |  |
|  |  - SCR/ULVF/POVC (Full Analysis only)        |  |
|  |  - MESA (Full Analysis with pool+metrics)    |  |
|  |  - QMAN (both modes with wallet data)        |  |
|  +----------------------------------------------+  |
|                      |                             |
|              QASS Superposition                    |
|                      |                             |
|              QEDD/MCI Computation                  |
|                      |                             |
|  +----------------------------------------------+  |
|  |            VETO CHECKS                       |  |
|  |  - QEDD lambda > abort_threshold             |  |
|  |  - MCI < coherence_abort_threshold           |  |
|  +----------------------------------------------+  |
|                      |                             |
|            SurvivorScore Calculation               |
|                      |                             |
|            Early Exit (score < critical)           |
|                      |                             |
|  +----------------------------------------------+  |
|  |          scoring/calculate_final_score       |  |
|  |  1. Base from SurvivorScore                  |  |
|  |  2. QASS secondary modifier (+/-10 pts)      |  |
|  |  3. Fallback confidence multiplier           |  |
|  |  4. Penalties (uncapped, can go negative)    |  |
|  |  5. Boosters (uncapped, can exceed 100)      |  |
|  |  6. Risk from RAW score                      |  |
|  |  7. Clamp for display [0, 100]               |  |
|  +----------------------------------------------+  |
|                      |                             |
|           Generate Interpretation                  |
+----------------------------------------------------+
     |
HyperPredictionResult
```

## Analysis Phases

### EarlyStage (tx_count < 2)
Static analysis mode focused on:
- Chaos Engine Monte Carlo simulations
- Gene Mapper security analysis
- Shadow Ledger RAM simulations
- LIGMA liquidity trap detection (global guard)
- IWIM dev wallet intent (async, default trust until RPC responds)
- PRAECOG adversarial exploitability (static pool analysis)
- MPCF actor fingerprinting (if tx_data present - early warning)

Skips trend-based metrics that require transaction history:
- SSMI (timing patterns need history)
- SCR (FFT needs multiple samples)
- ULVF (momentum needs Δt)
- POVC (clustering needs behavior patterns)

### FullAnalysis (tx_count >= 2)
Complete analysis including all EarlyStage modules plus:
- SSMI (Sub-Slot Microentropy Index)
- SCR (bot detection via FFT)
- ULVF (liquidity vector field)
- POVC (cluster prediction)
- QEDD/MCI with full veto capability
- MESA (microstructure execution-shape)

## Configuration

All parameters are configurable via `ghost_brain_config.toml`:

```toml
[hyper_prediction]
survivor_critical_threshold = 35
qass_secondary_max_adjustment = 10
qass_min_confidence_for_modifier = 0.6
cold_start_max_adjustment = 0.3
cold_start_qedd_mci_weight = 10.0

# MESA thresholds
mesa_wash_severe_threshold = 0.85
mesa_wash_elevated_threshold = 0.70
mesa_bot_high_threshold = 0.90
mesa_bot_moderate_threshold = 0.75
mesa_organic_bonus_threshold = 0.75
mesa_organic_max_wash = 0.40

# Risk thresholds
[hyper_prediction.risk_thresholds]
very_high_confidence = 0.5
high_confidence = 0.7
medium_score = 60
```

## Usage Example

```rust
use ghost_brain::oracle::hyper_prediction::{
    HyperPredictionOracle,
    HyperPredictionResult,
    AnalysisPhase,
    RiskLevel,
};
use ghost_brain::config::GhostBrainConfig;

// Load configuration
let config = GhostBrainConfig::from_toml_file("ghost_brain_config.toml")?;

// Create oracle with 70% acceptance threshold
let oracle = HyperPredictionOracle::new_with_config(70, &config);

// Score a candidate
let result = oracle.score_candidate(
    &candidate,
    &pumpfun_cache,
    explicit_pool_state,
    tx_timestamps,
    tx_data,
    iwim_result,
    chaos_result,
    resonance_result,
    gene_safety_result,
    hunter_score,
    tx_metrics,
    cluster_result,
    paradox_state,
    tuned_weights,
)?;

// Check result
if result.passed {
    println!("BUY: {} (risk: {:?})", result.score, result.risk_level);
    println!("Phase: {:?}", result.analysis_phase);
} else {
    println!("SKIP: {}", result.interpretation);
}
```

## Performance Targets

- Decision time: T < 2 seconds from token creation
- EarlyStage processing: < 500ms
- FullAnalysis processing: < 1500ms
- LIGMA analysis: < 1ms
- QASS superposition: < 10ms

## Testing

See `ghost-brain/src/oracle/hyper_prediction/mod.rs` for comprehensive unit tests covering:
- Phase detection and activation
- Veto conditions and short-circuits
- Score calculation with various inputs
- Risk level determination
- Performance regression checks
