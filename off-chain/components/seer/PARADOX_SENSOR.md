# 🔮 Paradox Sensor (EchoScanner) - Network Telemetry Analysis

## Overview

The **Paradox Sensor** is a passive network telemetry analyzer that detects pre-transactional anomalies from HFT (High-Frequency Trading) activity by monitoring WebSocket/gRPC packet metadata.

Unlike traditional transaction parsers, the Paradox Sensor analyzes **temporal and volumetric patterns** rather than transaction content, acting as a "sonar" that detects market tension through network side-channels.

## Architecture

```
┌──────────────────┐
│  WebSocket/gRPC  │
│   Data Stream    │
└────────┬─────────┘
         │ packet arrival
         ▼
┌──────────────────┐
│  record_pulse()  │  ← Hot Path (O(1))
│   Ring Buffer    │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Analysis Loop    │  ← Background Worker
│  (every 50ms)    │     (Tokio Task)
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ Statistical      │
│ Calculations:    │
│ • Inter-Arrival  │
│ • Jitter (σ)     │
│ • Packet Density │
│ • Tension Score  │
└────────┬─────────┘
         │
         ▼
┌──────────────────┐
│ watch::channel   │ ← Real-time state
│  ParadoxState    │   broadcast
└──────────────────┘
```

## Core Concepts

### 1. Network Pulse
Every incoming packet is recorded with:
- **Timestamp**: Monotonic `Instant` (nanosecond precision)
- **Size**: Payload size in bytes

### 2. Statistical Metrics

#### Jitter (σ)
Standard deviation of inter-arrival times (IAT):
```rust
jitter = sqrt(Σ(iat - mean_iat)² / n)
```
- **Low jitter** + High density = Synchronized bot activity
- **High jitter** + High density = Network congestion/DDOS

#### Packet Density
Packets per second within the analysis window (500ms):
```rust
density = packet_count * (1000ms / window_size_ms)
```

#### Tension Score (0-100)
Heuristic formula combining density and jitter:
```rust
tension_raw = (density^1.1) / (jitter + 1.0)
tension = (tension_raw / 50.0).min(100.0)
```

**Interpretation:**
- **< 50**: Normal market activity
- **50-80**: Elevated activity (watch closely)
- **> 80**: 🔴 **ANOMALY** - Likely HFT bot swarm detected

### 3. Paradox State
Published every 50ms via `tokio::sync::watch` channel:
```rust
pub struct ParadoxState {
    pub tension: f64,          // 0.0 - 100.0
    pub jitter_ms: f64,        // milliseconds
    pub density_bps: f64,      // packets per second
    pub anomaly_detected: bool // true if tension > 80
}
```

## Integration Guide

### 1. Initialization (in Seer)

The Paradox Sensor is automatically initialized when creating a `Seer` instance:

```rust
use seer::{Seer, config::SeerConfig};
use tokio::sync::mpsc;

let config = SeerConfig::default();
let (candidate_tx, candidate_rx) = mpsc::channel(100);

// Sensor is created and attached to WebSocket internally
let seer = Seer::new(config, candidate_tx);

// Get the state receiver to monitor real-time telemetry
let paradox_rx = seer.paradox_state_receiver().unwrap();
```

### 2. Monitoring in Trigger

```rust
use seer::paradox_sensor::ParadoxState;

// Subscribe to Paradox state changes
let mut paradox_rx = seer.paradox_state_receiver().unwrap();

tokio::spawn(async move {
    while paradox_rx.changed().await.is_ok() {
        let state = *paradox_rx.borrow();
        
        if state.anomaly_detected {
            warn!("🔴 HFT ANOMALY: Tension={:.2}%, Jitter={:.2}ms, Density={:.0}pps",
                state.tension, state.jitter_ms, state.density_bps);
            
            // React: Increase tip, pause trading, or adjust strategy
            adjust_trading_strategy(&state);
        }
    }
});
```

### 3. Adaptive Tip Adjustment

```rust
fn calculate_adaptive_tip(base_tip: f64, paradox_state: &ParadoxState) -> f64 {
    if paradox_state.anomaly_detected {
        // High market tension = more competition = higher tip needed
        let tension_multiplier = 1.0 + (paradox_state.tension / 100.0);
        base_tip * tension_multiplier
    } else {
        base_tip
    }
}

// Example:
// base_tip = 0.001 SOL
// tension = 85%
// adjusted_tip = 0.001 * 1.85 = 0.00185 SOL
```

### 4. Defensive Strategy Pattern

```rust
fn should_execute_trade(
    paradox_state: &ParadoxState,
    strategy: &TradingStrategy
) -> bool {
    match strategy {
        TradingStrategy::Aggressive => {
            // Always trade, just adjust tip
            true
        }
        TradingStrategy::Conservative => {
            // Avoid trading during high tension
            !paradox_state.anomaly_detected
        }
        TradingStrategy::Adaptive => {
            // Trade only if tension below threshold
            paradox_state.tension < 75.0
        }
    }
}
```

## Configuration

### Analysis Window
- **Default**: 500ms (configurable via `WINDOW_SIZE_MS`)
- Analyzes only recent packets within this window

### Update Frequency
- **Default**: 50ms (configurable via `ANALYSIS_INTERVAL_MS`)
- Publishes state 20 times per second

### Buffer Size
- **Default**: 2000 samples (configurable via `MAX_SAMPLES`)
- Protects against memory overflow

### Anomaly Threshold
- **Default**: 80% tension (configurable via `ANOMALY_TENSION_THRESHOLD`)
- Triggers `anomaly_detected` flag

## Performance Characteristics

### Hot Path (record_pulse)
- **Complexity**: O(1)
- **Lock Duration**: < 1μs (write lock on ring buffer)
- **Called**: On every WebSocket message arrival
- **Impact**: Negligible overhead on message processing

### Background Analysis
- **Complexity**: O(n) where n = samples in window
- **Frequency**: Every 50ms
- **Async**: Non-blocking (runs in separate Tokio task)

## Testing

Run unit tests:
```bash
cargo test -p seer paradox_sensor
```

Run demo:
```bash
cargo run --example paradox_sensor_demo
```

Expected output:
```
🔮 Paradox Sensor (EchoScanner) Demo
=====================================

✅ Paradox Sensor initialized
✅ Analysis loop started (refreshing every 50ms)

📊 Monitoring network telemetry...

📡 Phase 1: Normal market activity (low density, natural jitter)
[Sample 0020] 🟢 NORMAL | Tension:  12.45% | Jitter:   5.23ms | Density:    65 pps

📡 Phase 2: Increased activity (medium density)
[Sample 0040] 🟢 NORMAL | Tension:  34.78% | Jitter:   8.91ms | Density:   145 pps

📡 Phase 3: SIMULATING HFT BOT ATTACK (high density, low jitter)
⚠️  ANOMALY DETECTED!
    Market tension: 87.32%
    Jitter: 2.14ms
    Density: 456 packets/sec
    → Recommended action: Increase Jito tip or pause trading

[Sample 0060] 🔴 ALERT  | Tension:  87.32% | Jitter:   2.14ms | Density:   456 pps
```

## Calibration

The Paradox Formula is heuristic and may need calibration based on mainnet observations:

1. **Monitor baseline metrics** during normal market hours
2. **Record tension scores** during known bot attacks
3. **Adjust the normalization constant** (currently 50.0):
   ```rust
   let tension_normalized = (tension_raw / CALIBRATION_CONSTANT).min(100.0);
   ```
4. **Fine-tune the anomaly threshold** based on false positive rate

## Use Cases

### 1. Pre-Transaction Decision
Check market tension before building a transaction:
```rust
let state = paradox_rx.borrow();
if state.tension > 70.0 {
    // Increase tip or skip this opportunity
}
```

### 2. Real-Time Monitoring Dashboard
Display tension as a real-time gauge in the GUI

### 3. Backtest Analysis
Log tension scores alongside trade execution for post-mortem analysis

### 4. Risk Management
Implement circuit breakers based on sustained high tension

## Limitations

1. **Network-level only**: Does not analyze transaction content
2. **Heuristic-based**: Requires calibration for specific network conditions
3. **Window-dependent**: Short-term spikes may be missed in 500ms window
4. **False positives**: Legitimate high activity (e.g., major news) may trigger alerts

## Future Enhancements

- [ ] Machine learning model for pattern recognition
- [ ] Per-pool tension tracking
- [ ] Cross-correlation with on-chain metrics
- [ ] Historical tension database for trend analysis
- [ ] WebSocket message classification (create vs. trade vs. other)

## References

- Issue: "Stworzyć Paradox Sensor (EchoScanner)"
- Implementation: `off-chain/components/seer/src/paradox_sensor/`
- Examples: `off-chain/components/seer/examples/paradox_sensor_demo.rs`

---

**Author**: Ghost Trading System  
**Module**: Seer (off-chain/components/seer)  
**Status**: ✅ Implemented and tested
