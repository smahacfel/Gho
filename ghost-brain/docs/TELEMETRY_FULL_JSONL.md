# Full HyperPrediction JSONL Telemetry

## Overview

The telemetry system now records complete HyperPrediction scoring results in JSONL format, providing comprehensive visibility into all scoring subcomponents and their states.

## Key Features

### 1. **Complete Subcomponent Tracking**

Every scoring event captures all submodules:
- **QASS** (Quantum Amplitude Superposition Scoring)
- **SSMI** (Sub-Slot Microentropy Index)
- **MPCF** (Micro-Payload Cognitive Fingerprint)
- **IWIM** (Initial Wallet Intent Mapping)
- **SOBP** (Shadow Oracle Bonding Progress)
- **QOFSV** (Quantum Oracle Field Superposition Vector)
- **QEDD** (Quantum Entropy-Driven Decay)
- **MCI** (Market Coherence Index)
- **Gene Mapper** (Security analysis)
- **SCR** (Slot-Coherence Resonance)
- **ULVF** (Ultra-Early Liquidity Vector Field)
- **Chaos** (Monte Carlo simulations)
- **Resonance** (Bot detection)
- **Hunter Score** (External oracle)

### 2. **InsufficientData Status**

Fields with insufficient data are represented as `null` in the JSON output, clearly distinguishing between:
- **Available data**: Full subcomponent result object
- **InsufficientData**: `null` value

### 3. **Immediate Flush to Disk**

Unlike regular telemetry events (buffered and flushed every 10 events), scoring events are **immediately flushed to disk** after writing. This ensures:
- No data loss on crashes
- Real-time availability for monitoring
- Immediate commit semantics for critical decisions

### 4. **Raw Transaction Data**

Each scoring event includes raw transaction data that was analyzed, enabling:
- Post-hoc analysis of scoring decisions
- Training data for ML models
- Audit trail for regulatory compliance

## Output Format

### Example JSONL Record (Formatted)

```json
{
  "candidate_id": "pool_amm_abc123xyz",
  "timestamp": "2024-12-06T15:30:45.123Z",
  "txs": [
    {
      "signature": "sig_abc123",
      "slot": 245123456,
      "timestamp_ms": 1733500000000,
      "signer": "wallet_human_1",
      "is_buy": true,
      "volume_sol": 2.5
    },
    {
      "signature": "sig_def456",
      "slot": 245123457,
      "timestamp_ms": 1733500001000,
      "signer": "wallet_human_2",
      "is_buy": true,
      "volume_sol": 1.8
    }
  ],
  "qass": {
    "combined_score": 0.87,
    "active_waves": 6,
    "collapsed": false
  },
  "ssmi": {
    "ssmi_score": 0.72,
    "shannon_entropy": 5.8,
    "source_type": "Human",
    "bot_probability": 0.15
  },
  "mpcf": {
    "actor": "HumanMobile",
    "confidence": 0.89,
    "entropy": 6.2
  },
  "iwim": {
    "organic_score": 0.85,
    "sybil_score": 0.08,
    "rug_threat_score": 0.12,
    "confidence": 0.82
  },
  "sobp": {
    "progress_pct": 65,
    "price_ratio": 1.45
  },
  "qofsv": {
    "povc_cluster": 1,
    "cluster_name": "Organic Hype"
  },
  "qedd": {
    "lambda_now": 0.45,
    "survival_1s": 0.68,
    "survival_5s": 0.32
  },
  "mci": {
    "mci": 0.78,
    "dc": 0.85,
    "sc": 0.71
  },
  "gene_mapper": {
    "has_malicious_patterns": false,
    "suspicious_instructions": [],
    "risk_score": 0.05
  },
  "scr": {
    "score": 0.15
  },
  "ulvf": {
    "divergence": 0.08,
    "curl": 0.05
  },
  "chaos": {
    "crash_probability": 12.5,
    "pump_probability": 68.2,
    "median_roi": 145.0
  },
  "resonance": {
    "is_bot_pattern": false,
    "confidence": 0.91,
    "interval_regularity": 0.18
  },
  "hunter_score": 88,
  "final_score_initial": 85,
  "final_score_followup": null,
  "passed": true,
  "risk_level": "Low",
  "processing_time_us": 1800000,
  "base_score": 80,
  "interpretation": "Strong organic growth with minimal bot activity"
}
```

### Example with InsufficientData

```json
{
  "candidate_id": "pool_amm_timeout_xyz",
  "timestamp": "2024-12-06T15:31:15.456Z",
  "txs": [],
  "qass": null,
  "ssmi": null,
  "mpcf": null,
  "iwim": null,
  "sobp": null,
  "qofsv": null,
  "qedd": null,
  "mci": null,
  "gene_mapper": null,
  "scr": null,
  "ulvf": null,
  "chaos": null,
  "resonance": null,
  "hunter_score": null,
  "final_score_initial": 0,
  "final_score_followup": null,
  "passed": false,
  "risk_level": "VeryHigh",
  "processing_time_us": 2100000,
  "base_score": 0,
  "interpretation": "Pipeline timeout - candidate skipped"
}
```

## Usage

### Basic Integration

```rust
use ghost_brain::telemetry::{TelemetryConfig, TelemetryRecorder};
use ghost_brain::oracle::hyper_prediction::HyperPredictionResult;
use std::path::PathBuf;
use std::sync::Arc;

// Create telemetry recorder
let telemetry_config = TelemetryConfig {
    log_path: PathBuf::from("logs/scoring.jsonl"),
    channel_buffer_size: 100,
    enabled: true,
};

let telemetry = Arc::new(TelemetryRecorder::new(telemetry_config).await?);

// After scoring a candidate...
if let Some(result) = oracle.score_candidate(&candidate).await? {
    // Extract transaction data
    let txs = extract_tx_data(&pool);
    
    // Log with immediate flush
    telemetry.log_hyper_prediction_scoring(
        pool.pool_amm_id.clone(),
        &result,
        txs,
    );
}
```

### Integration with OraclePipeline

```rust
use ghost_launcher::components::oracle_pipeline::OraclePipeline;

// Create pipeline with telemetry
let pipeline = OraclePipeline::with_telemetry(
    oracle_config,
    telemetry_recorder,
);

// Scoring automatically logs to telemetry
let result = pipeline.score_candidate(pool).await?;
```

## Query Examples

### View All Scoring Events

```bash
cat logs/scoring.jsonl | jq '.'
```

### Filter by Pass/Fail Status

```bash
# Only passed candidates
cat logs/scoring.jsonl | jq 'select(.passed == true)'

# Only failed candidates
cat logs/scoring.jsonl | jq 'select(.passed == false)'
```

### Extract Specific Fields

```bash
# Candidate ID, score, and risk level
cat logs/scoring.jsonl | jq '{id: .candidate_id, score: .final_score_initial, risk: .risk_level}'
```

### Find InsufficientData Cases

```bash
# Candidates with missing QASS data
cat logs/scoring.jsonl | jq 'select(.qass == null)'

# Count how many fields are null per record
cat logs/scoring.jsonl | jq 'to_entries | map(select(.value == null)) | length'
```

### Analyze Bot Activity

```bash
# High bot probability (SCR > 0.5)
cat logs/scoring.jsonl | jq 'select(.scr.score > 0.5)'

# POVC cluster distribution
cat logs/scoring.jsonl | jq '.qofsv.cluster_name' | sort | uniq -c
```

### Time-Based Analysis

```bash
# Scoring events in last hour
cat logs/scoring.jsonl | jq --arg ts "$(date -u -d '1 hour ago' +%Y-%m-%dT%H:%M:%S)" 'select(.timestamp > $ts)'

# Average processing time
cat logs/scoring.jsonl | jq '.processing_time_us' | awk '{s+=$1; c++} END {print s/c/1000000 " seconds"}'
```

## Testing

### Run Example Demo

```bash
cargo run --example hyper_prediction_telemetry_demo
```

This demonstrates three scenarios:
1. **Full data available**: All submodules succeeded
2. **Partial data**: Many InsufficientData (null) fields
3. **Timeout/failure**: Complete pipeline failure

### Run Unit Tests

```bash
cargo test -p ghost-brain test_log_hyper_prediction_scoring
```

## Performance Characteristics

- **Write latency**: <1ms (async, non-blocking)
- **Flush latency**: <5ms (immediate for scoring events)
- **Memory overhead**: ~100 events buffered in channel
- **Disk I/O**: One `write()` + one `flush()` per scoring event
- **File format**: JSONL (one JSON object per line)

## Monitoring Integration

The JSONL format is designed for easy integration with monitoring tools:

### Grafana Loki

```yaml
# promtail config
- job_name: ghost_scoring
  static_configs:
    - targets:
        - localhost
      labels:
        job: ghost_scoring
        __path__: /path/to/logs/scoring.jsonl
  pipeline_stages:
    - json:
        expressions:
          candidate_id: candidate_id
          score: final_score_initial
          passed: passed
          risk_level: risk_level
```

### Elasticsearch

```bash
# Ingest JSONL directly
cat logs/scoring.jsonl | while read line; do
  curl -XPOST 'http://localhost:9200/ghost_scoring/_doc' \
    -H 'Content-Type: application/json' \
    -d "$line"
done
```

### Custom Analytics

```python
import json
import pandas as pd

# Load into pandas DataFrame
records = []
with open('logs/scoring.jsonl', 'r') as f:
    for line in f:
        records.append(json.loads(line))

df = pd.DataFrame(records)

# Analyze pass rate by risk level
pass_rate = df.groupby('risk_level')['passed'].mean()
print(pass_rate)

# Distribution of InsufficientData
null_counts = df.isnull().sum()
print(null_counts)
```

## See Also

- [TelemetryRecorder API](../src/telemetry/recorder.rs)
- [HyperPredictionOracle](../src/oracle/hyper_prediction.rs)
- [OraclePipeline](../../ghost-launcher/src/components/oracle_pipeline.rs)
- [Example Demo](../examples/hyper_prediction_telemetry_demo.rs)
