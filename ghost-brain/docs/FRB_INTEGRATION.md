# FRB Integration with QOFSV/QMAN/WHF

## Overview

This document describes the integration of **FRB (Fractal Resonance Bands)** with the Ghost quantum trading pipeline (QOFSV, QMAN, WHF). The integration provides multi-scale signal validation, false-positive reduction, and enhanced confidence scoring for trading decisions.

## Table of Contents

1. [Architecture](#architecture)
2. [Components](#components)
3. [Integration Points](#integration-points)
4. [Configuration & Tuning](#configuration--tuning)
5. [Monitoring & Metrics](#monitoring--metrics)
6. [Performance Characteristics](#performance-characteristics)
7. [Usage Examples](#usage-examples)
8. [Troubleshooting](#troubleshooting)

---

## Architecture

### Signal Flow

```
Transaction Stream
    ↓
BandExtractor (FRB Part 1)
    ↓
[Short Band (8-32 tx), Medium Band (32-128 tx), Long Band (128-512 tx)]
    ↓
ResonanceAnalyzer (FRB Part 2 & 3)
    ↓
[Resonance Score, Coherence Map, Signal Classification]
    ↓
FrbIntegrator
    ├─→ QOFSV Enhancement (coherence boost)
    ├─→ WHF Validation (wash/bot detection)
    └─→ QMAN Enhancement (confidence boost)
    ↓
Enhanced Trading Signal
```

### Multi-Signal Consensus

FRB acts as a **cross-validation layer** for other signals:

1. **QOFSV (Quantum Order-Flow State Vector)**
   - FRB provides amplitude boost based on multi-scale resonance
   - High resonance (0.7+) → 1.2-1.5x amplitude multiplier
   - Low resonance (<0.5) → No boost (1.0x)

2. **WHF (Harmonic Field Analysis)**
   - FRB validates wash trading and bot manipulation signals
   - Cross-checks band patterns against WHF field metrics
   - Flags contradictions as potential false positives

3. **QMAN (Quantum Capital Flow)**
   - FRB enhances capital flow prediction confidence
   - Multi-scale analysis confirms re-accumulation or distribution
   - Adds 10-20% confidence boost when patterns align

---

## Components

### 1. FrbIntegrator (`src/signals/frb_integrator.rs`)

Main integration engine that composes FRB signals with other modules.

**Key Methods:**

```rust
// Calculate QOFSV amplitude boost from FRB resonance
pub fn calculate_qofsv_boost(&self, frb_result: &FrbResult) -> QofsvEnhancement

// Validate WHF signal against FRB band patterns
pub fn validate_whf_signal(&self, frb_result: &FrbResult, whf_signal: &WhfSignal) -> WhfValidation

// Enhance QMAN signal with multi-scale confidence
pub fn enhance_qman_signal(&self, frb_result: &FrbResult, qman_signal: &QmanSignal) -> QmanEnhancement

// Full integration pipeline
pub fn integrate(
    &self,
    frb_result: FrbResult,
    whf_signal: Option<&WhfSignal>,
    qman_signal: Option<&QmanSignal>,
) -> FrbIntegrationResult
```

### 2. FrbResonanceConfig

Tunable configuration for production threshold adjustment.

**Default Thresholds:**

| Parameter | Default | Range | Description |
|-----------|---------|-------|-------------|
| `min_resonance_for_boost` | 0.5 | 0.0-1.0 | Minimum resonance for QOFSV boost |
| `max_coherence_boost` | 1.5 | 1.0-2.0 | Maximum amplitude multiplier |
| `bot_manipulation_threshold` | 0.7 | 0.0-1.0 | Resonance threshold for bot detection |
| `wash_trading_threshold` | 0.6 | 0.0-1.0 | Curl threshold for wash trading |
| `min_organic_buyers` | 5 | 1-100 | Minimum buyers for organic classification |
| `false_positive_tolerance` | 0.2 | 0.0-1.0 | Permissiveness for filtering (higher = more lenient) |

---

## Configuration & Tuning

### Initial Production Settings

**Conservative Profile** (Lower false positives, may miss some signals):
```rust
let config = FrbResonanceConfig {
    min_resonance_for_boost: 0.6,
    max_coherence_boost: 1.3,
    bot_manipulation_threshold: 0.75,
    wash_trading_threshold: 0.65,
    min_organic_buyers: 7,
    false_positive_tolerance: 0.15,
};
```

**Aggressive Profile** (More signals, higher false positive risk):
```rust
let config = FrbResonanceConfig {
    min_resonance_for_boost: 0.4,
    max_coherence_boost: 1.6,
    bot_manipulation_threshold: 0.65,
    wash_trading_threshold: 0.55,
    min_organic_buyers: 3,
    false_positive_tolerance: 0.25,
};
```

### Tuning Procedure

#### Step 1: Monitor Initial Metrics

Run in production for 24-48 hours with default settings and observe:

1. **False Positive Rate**
   ```
   FP_rate = frb_false_positive_suspected_total / frb_signals_generated_total
   ```
   - **Target:** <20% (0.2)
   - **Acceptable:** 10-30%
   - **Problem:** >40%

2. **Signal Count**
   - Too few signals → Lower thresholds
   - Too many signals → Raise thresholds

3. **Integration Confidence Distribution**
   - Low confidence (<0.4) → Consider raising boost factors
   - Very high confidence (>0.9) constantly → May be over-boosting

#### Step 2: Adjust Based on Metrics

**If False Positive Rate > 30%:**
```rust
// Tighten filters
config.min_resonance_for_boost += 0.1;
config.bot_manipulation_threshold += 0.05;
config.min_organic_buyers += 2;
```

**If Missing Good Signals (low catch rate):**
```rust
// Loosen filters
config.min_resonance_for_boost -= 0.1;
config.false_positive_tolerance += 0.05;
config.min_organic_buyers -= 1;
```

**Key Grafana Queries:**

```promql
# False positive rate over time
rate(frb_false_positive_suspected_total[1h]) / rate(frb_signals_generated_total[1h])

# QOFSV boost distribution
histogram_quantile(0.95, frb_qofsv_coherence_boost)

# WHF validation success rate
frb_whf_validation_success_total / (frb_whf_validation_success_total + frb_whf_validation_failure_total)

# Average resonance score
rate(frb_resonance_score_sum[5m]) / rate(frb_resonance_score_count[5m])
```

---

## Monitoring & Metrics

### Prometheus Metrics Categories

1. **Performance Metrics**
   - `frb_band_extraction_duration_us` - Extraction latency (P50, P95, P99)
   - `frb_e2e_pipeline_duration_us` - End-to-end latency

2. **Signal Quality Metrics**
   - `frb_resonance_score` - Histogram (0.0-1.0)
   - `frb_coherence_short_medium` - Short-medium band coherence
   - `frb_trend_likelihood` - Trend continuation probability

3. **Integration Metrics**
   - `frb_qofsv_coherence_boost` - QOFSV amplitude multiplier distribution
   - `frb_whf_validation_success_total` - Successful WHF validations
   - `frb_qman_confidence_boost` - QMAN confidence enhancement

4. **Anomaly Detection**
   - `frb_bot_manipulation_detected_total` - Bot activity count
   - `frb_wash_trading_detected_total` - Wash trading detection count
   - `frb_false_positive_suspected_total` - Suspected false positives

---

## Performance Characteristics

### Benchmarks

**Component Latencies:**

| Operation | P50 | P95 | P99 | Target |
|-----------|-----|-----|-----|--------|
| Transaction Addition | 1-2μs | 3-5μs | 5-10μs | <10μs |
| Band Extraction | 10-15μs | 20-30μs | 30-50μs | <50μs |
| Resonance Analysis | 5-10μs | 10-20μs | 15-30μs | <30μs |
| FRB Integration | 5-8μs | 10-15μs | 15-25μs | <25μs |
| **E2E Pipeline** | **25-40μs** | **40-70μs** | **60-100μs** | **<100μs** |

**Throughput:**

- **Transaction Processing:** 40,000-60,000 tx/s
- **Band Extraction:** 20,000-30,000 extractions/s
- **Full Integration:** 15,000-25,000 integrations/s

---

## Usage Examples

### Example 1: Basic Integration

```rust
use ghost_brain::signals::{
    BandExtractor, ResonanceAnalyzer, FrbIntegrator,
    BandTransaction,
};

// Initialize components
let mut extractor = BandExtractor::new();
let analyzer = ResonanceAnalyzer::new();
let integrator = FrbIntegrator::new();

// Process transactions...
for tx in transaction_stream {
    let band_tx = BandTransaction::new(
        tx.volume,
        tx.is_buy,
        tx.wallet,
        tx.timestamp_ms,
    );
    extractor.add_transaction(band_tx);
}

// Extract and analyze
let bands = extractor.extract_bands();
let frb_result = analyzer.analyze(bands);

// Integrate
let integration = integrator.integrate(frb_result, None, None);

println!("QOFSV Boost: {}x", integration.qofsv_enhancement.coherence_boost);
println!("Confidence: {:.2}", integration.integration_confidence);
```

### Example 2: With Metrics

```rust
use ghost_brain::metrics::{FrbMetrics, FrbMetricsReporter};
use std::time::Instant;

let metrics = FrbMetrics::new();
let reporter = FrbMetricsReporter::new(metrics.clone());

// Record transaction
reporter.record_transaction();

// Record band extraction with timing
let start = Instant::now();
let bands = extractor.extract_bands();
reporter.record_band_extraction(start.elapsed().as_micros() as f64);

// Record FRB result
let frb_result = analyzer.analyze(bands);
reporter.record_frb_result(&frb_result);

// Export metrics (e.g., to HTTP endpoint)
let prometheus_text = metrics.render();
```

---

## Running Tests and Benchmarks

### Stress Tests

Run comprehensive stress tests (100,000+ transactions):

```bash
cargo test --test frb_stress_test --release -- --nocapture
```

Individual stress tests:

```bash
# Massive transaction processing
cargo test --test frb_stress_test -- test_massive_transaction_processing --nocapture

# Zero-alloc hot-path validation
cargo test --test frb_stress_test -- test_zero_alloc_hot_path --nocapture

# Memory stability (long-running)
cargo test --test frb_stress_test -- test_memory_stability --nocapture
```

### Benchmarks

Run full benchmark suite:

```bash
cargo bench --bench frb_qofsv_hotpath
```

Individual benchmarks:

```bash
# E2E pipeline latency
cargo bench --bench frb_qofsv_hotpath -- e2e_tx_to_qofsv

# Band extraction performance
cargo bench --bench frb_qofsv_hotpath -- band_extraction

# Pipeline scaling with different transaction counts
cargo bench --bench frb_qofsv_hotpath -- pipeline_scaling
```

---

## Troubleshooting

### High Latency (>100μs P95)

**Diagnosis:**
```promql
histogram_quantile(0.95, rate(frb_e2e_pipeline_duration_us_bucket[5m]))
```

**Solutions:**
1. Check buffer size: `frb_buffer_size > 500` indicates possible memory pressure
2. Reduce window size: Use `WINDOW_1S` or `WINDOW_5S` instead of `WINDOW_60S`
3. Run benchmark: `cargo bench --bench frb_qofsv_hotpath`

### High False Positive Rate (>40%)

**Solutions:**
1. Tighten thresholds:
   ```rust
   config.min_resonance_for_boost = 0.6;  // from 0.5
   config.min_organic_buyers = 7;          // from 5
   ```
2. Review WHF integration: Ensure WHF signals are properly validated

### Low Signal Count

**Solutions:**
1. Lower thresholds:
   ```rust
   config.min_resonance_for_boost = 0.4;  // from 0.5
   config.false_positive_tolerance = 0.25; // from 0.2
   ```
2. Check input data: Verify transactions are flowing

---

## References

- **FRB Core:** [`src/signals/frb.rs`](../src/signals/frb.rs)
- **FRB Integrator:** [`src/signals/frb_integrator.rs`](../src/signals/frb_integrator.rs)
- **Metrics:** [`src/metrics/frb_metrics.rs`](../src/metrics/frb_metrics.rs)
- **Stress Tests:** [`tests/frb_stress_test.rs`](../tests/frb_stress_test.rs)
- **Benchmarks:** [`benches/frb_qofsv_hotpath.rs`](../benches/frb_qofsv_hotpath.rs)

---

**Last Updated:** 2025-12-06  
**Version:** 1.0.0  
**Authors:** Ghost Team
