# Oracle Brain Decision Logging System

Complete implementation of comprehensive decision logging for the Oracle Brain scoring system as specified in the architecture document.

## Overview

This system implements full traceability of Oracle Brain scoring decisions, including:
- **Initial Score** (T < 2s): Fast decision for buy/skip based on ultra-fast modules
- **Follow-up Scores** (T > 2s): Periodic re-evaluation at 1s, 5s, 30s, 60s intervals
- **Corrections**: Explicit tracking of all score adjustments with reasons
- **JSONL Logs**: Per-candidate decision logs in machine-readable format

## Architecture

```
┌──────────────────────────────────────────────────────────────┐
│                    Initial Scoring (T < 2s)                   │
│  ┌──────────┐  ┌──────────┐  ┌──────────┐  ┌──────────┐    │
│  │   QASS   │  │   SSMI   │  │   MPCF   │  │   SOBP   │    │
│  └────┬─────┘  └────┬─────┘  └────┬─────┘  └────┬─────┘    │
│       └──────────────┴─────────────┴──────────────┘          │
│                          │                                     │
│                          v                                     │
│                  ┌──────────────┐                            │
│                  │  initialScore │ --> BUY/SKIP Decision     │
│                  └───────┬──────┘                            │
└──────────────────────────┼───────────────────────────────────┘
                           │
                           v
┌──────────────────────────┼───────────────────────────────────┐
│              Follow-up Scoring (T > 2s)                       │
│                          │                                     │
│   ┌─────────────────────┴────────────────────────┐          │
│   │                                                │          │
│   v                                                v          │
│  t=1s                                            t=5s         │
│  Quick check                                     MCI check    │
│  QASS update                                     QEDD check   │
│                                                               │
│   v                                                v          │
│  t=30s                                          t=60s         │
│  Full QEDD analysis                             Final check  │
│  Chaos Engine sims                              GeneMapper   │
│                                                               │
│   └─────────────────────┬────────────────────────┘          │
│                          v                                     │
│                  ┌──────────────┐                            │
│                  │ followupScore │ --> HOLD/SELL/SCALE_OUT   │
│                  └───────┬──────┘                            │
└──────────────────────────┼───────────────────────────────────┘
                           │
                           v
                    ┌──────────────┐
                    │ DecisionLogger│ --> JSONL files
                    └──────────────┘
```

## Components

### 1. Decision Logger (`decision_logger.rs`)

Async JSONL writer that logs all decisions per candidate:

```rust
use ghost_brain::oracle::{
    DecisionLogger, DecisionLoggerConfig, OracleDecisionLog,
    InitialComponents, DecisionType
};

// Create logger
let config = DecisionLoggerConfig {
    log_dir: "datasets/decisions".into(),
    channel_buffer_size: 1000,
    enabled: true,
};
let logger = Arc::new(DecisionLogger::new(config));

// Create decision log
let components = InitialComponents {
    base_shadow: 60,
    qass_score: 78.5,
    qedd_survival_30s: Some(0.71),
    mci: Some(0.74),
    chaos_loss_prob: Some(0.12),
    gene_match_score: Some(0.03),
    extras: HashMap::new(),
};

let log = OracleDecisionLog::new(
    "pool_123".to_string(),
    62,
    DecisionType::Buy,
    components,
);

// Log decision (fire-and-forget)
logger.log(log).await;
```

### 2. Follow-up Scoring (`followup_scoring.rs`)

Manages periodic re-evaluation of candidates:

```rust
use ghost_brain::oracle::{
    FollowupScoringManager, FollowupConfig, FollowupContext
};

// Create manager
let config = FollowupConfig {
    enabled: true,
    intervals_ms: vec![1000, 5000, 30000, 60000],
    mci_drop_threshold: 0.50,
    qedd_lambda_spike_threshold: 2.0,
    exit_threshold: 40,
    ..Default::default()
};

let manager = FollowupScoringManager::new(config, logger);

// Spawn follow-up task
let context = FollowupContext {
    candidate_id: "pool_123".to_string(),
    initial_score: 62,
    initial_components,
    start_time: Instant::now(),
    config: config.clone(),
};

manager.spawn_followup_task(context);
```

### 3. Correction Reasons

All score adjustments are tracked with explicit reasons:

```rust
use ghost_brain::oracle::CorrectionReason;

// MCI drop below threshold
let correction = CorrectionReason::MciDrop {
    old_value: 0.74,
    new_value: 0.45,
    threshold: 0.50,
    impact: -15,
};

// QEDD λ spike
let correction = CorrectionReason::QeddLambdaSpike {
    old_lambda: 0.5,
    new_lambda: 3.2,
    threshold: 2.0,
    impact: -25,
};

// GeneMapper scam detection
let correction = CorrectionReason::GeneMapperHit {
    match_score: 0.82,
    pattern_id: "pump_dump_pattern_v2".to_string(),
    impact: -100,
};

// Guardian watchdog abort
let correction = CorrectionReason::GuardianAbort {
    reason: "Anomalous pattern".to_string(),
    signal_name: "chaos_engine_critical".to_string(),
    impact: -100,
};
```

## JSONL Log Format

Each candidate gets a JSONL file: `datasets/decisions/{candidate_id}/decision.jsonl`

```json
{
  "candidate_id": "pool_123",
  "timestamp": 1234567890,
  "initialScore": 62,
  "initial_decision": "BUY",
  "initial_components": {
    "base_shadow": 60,
    "qass_score": 78.5,
    "qedd_survival_30s": 0.71,
    "mci": 0.74,
    "chaos_loss_prob": 0.12,
    "gene_match_score": 0.03
  },
  "followupScores": [
    {
      "t_ms": 1000,
      "score": 58,
      "reason": "Small QASS fluctuation",
      "corrections": [
        {
          "type": "qass_score_drop",
          "old_score": 78.5,
          "new_score": 76.0,
          "drop_pct": 3.2,
          "impact": -2
        }
      ],
      "decision": "HOLD"
    },
    {
      "t_ms": 5000,
      "score": 45,
      "reason": "MCI drop + QEDD decline",
      "corrections": [
        {
          "type": "mci_drop",
          "old_value": 0.74,
          "new_value": 0.45,
          "threshold": 0.50,
          "impact": -15
        },
        {
          "type": "qedd_survival_drop",
          "old_survival": 0.71,
          "new_survival": 0.52,
          "horizon_s": 30,
          "impact": -8
        }
      ],
      "decision": "SELL"
    }
  ],
  "veto": null,
  "final_decision": "SELL",
  "total_corrections": 3,
  "completed_at": 1234567950
}
```

## Running Tests

### Unit Tests

```bash
# Test decision logger data structures
cargo test -p ghost-brain oracle::decision_logger

# Test follow-up scoring logic
cargo test -p ghost-brain oracle::followup_scoring
```

### Integration Tests

```bash
# Complete decision flow test
cargo test -p ghost-brain --test oracle_decision_logger_integration -- --nocapture

# Specific scenarios
cargo test -p ghost-brain --test oracle_decision_logger_integration test_veto_scenario
cargo test -p ghost-brain --test oracle_decision_logger_integration test_guardian_abort_scenario
```

### Dry Run Demo

```bash
# Run comprehensive dry run with 5 scenarios
cargo run --example oracle_decision_dry_run

# View generated logs
cat datasets/decisions/*/decision.jsonl | jq '.'
```

## Performance

- **Async writes**: Fire-and-forget pattern ensures minimal latency impact
- **Buffered I/O**: Channel-based buffering for batch writes
- **Per-candidate files**: Isolated logs prevent lock contention
- **Target latency**: < 40ns for scoring path (logging is async)

## Integration Points

### Current Status

✅ **Completed:**
- Decision logging data structures
- Async JSONL writer
- Follow-up scoring framework
- Comprehensive test suite
- Dry run demo

🚧 **In Progress:**
- Integration with HyperPredictionOracle
- Real-time QEDD/MCI/GeneMapper queries
- Snapshot Engine data consumption

### Integration Example

```rust
// In HyperPredictionOracle::score_candidate()

// 1. Compute initial score
let initial_score = compute_initial_score(&candidate);
let components = build_initial_components(&candidate);

let log = OracleDecisionLog::new(
    candidate.pool_amm_id.to_string(),
    initial_score,
    DecisionType::Buy,
    components.clone(),
);

// 2. Log initial decision
decision_logger.log(log).await;

// 3. If BUY, spawn follow-up task
if initial_score >= threshold {
    let context = FollowupContext {
        candidate_id: candidate.pool_amm_id.to_string(),
        initial_score,
        initial_components: components,
        start_time: Instant::now(),
        config: followup_config,
    };
    
    followup_manager.spawn_followup_task(context);
}
```

## Configuration

### Decision Logger Config

```rust
DecisionLoggerConfig {
    log_dir: PathBuf::from("datasets/decisions"),
    channel_buffer_size: 1000,  // Adjust based on throughput
    enabled: true,
}
```

### Follow-up Config

```rust
FollowupConfig {
    enabled: true,
    intervals_ms: vec![1000, 5000, 30000, 60000],
    
    // Thresholds for corrections
    mci_drop_threshold: 0.50,
    qedd_lambda_spike_threshold: 2.0,
    qedd_survival_drop_pct: 0.30,
    chaos_loss_prob_threshold: 0.60,
    gene_match_threshold: 0.70,
    
    // Decision thresholds
    exit_threshold: 40,
    score_drop_pct_threshold: 0.30,
}
```

## Analysis Tools

### Query logs with jq

```bash
# Get all decisions
cat datasets/decisions/*/decision.jsonl | jq '.'

# Filter by decision type
cat datasets/decisions/*/decision.jsonl | jq 'select(.initial_decision == "BUY")'

# Find veto'd candidates
cat datasets/decisions/*/decision.jsonl | jq 'select(.veto != null)'

# Analyze corrections
cat datasets/decisions/*/decision.jsonl | jq '.followupScores[].corrections[]'

# Count decision types
cat datasets/decisions/*/decision.jsonl | jq '.final_decision' | sort | uniq -c
```

### Python analysis example

```python
import json
import glob

# Load all decision logs
decisions = []
for file in glob.glob('datasets/decisions/*/decision.jsonl'):
    with open(file) as f:
        for line in f:
            decisions.append(json.loads(line))

# Analyze by outcome
buy_count = sum(1 for d in decisions if d['initial_decision'] == 'BUY')
veto_count = sum(1 for d in decisions if d['veto'] is not None)

print(f"Total decisions: {len(decisions)}")
print(f"BUY decisions: {buy_count}")
print(f"Vetoed: {veto_count}")

# Correction frequency
correction_types = {}
for d in decisions:
    for followup in d['followupScores']:
        for correction in followup['corrections']:
            ctype = correction['type']
            correction_types[ctype] = correction_types.get(ctype, 0) + 1

print("\nCorrection frequency:")
for ctype, count in sorted(correction_types.items(), key=lambda x: x[1], reverse=True):
    print(f"  {ctype}: {count}")
```

## Next Steps

1. **Production Integration**
   - Hook into HyperPredictionOracle
   - Connect to real QEDD/MCI/GeneMapper modules
   - Integrate with Snapshot Engine

2. **Telemetry**
   - Add TelemetryEvent variants for decisions
   - Prometheus metrics for decision counts
   - Real-time monitoring dashboard

3. **Calibration**
   - Offline analysis of decision logs
   - Threshold optimization
   - A/B testing framework

4. **Performance**
   - Benchmark latency impact
   - Optimize buffer sizes
   - Compression for historical logs

## References

- Architecture: `STRUCTURE OF ORACLE BRAIN SYSTEM.md`
- Issue: Implement pełny logging decyzji Oracle Brain (initialScore, followupScore, korekty)
- Tests: `ghost-brain/tests/oracle_decision_logger_integration.rs`
- Example: `ghost-brain/examples/oracle_decision_dry_run.rs`
