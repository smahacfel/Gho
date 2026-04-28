# Paradox Sensor Integration Guide

## Overview

This guide explains how to integrate the Paradox Sensor with Trigger (for dynamic Jito tips) and Guardian (for network stress monitoring).

## 1. Trigger Integration - Dynamic Jito Tips

### Location
`ghost-launcher/src/components/trigger/jito_tip.rs`

### Implementation

The Paradox Sensor adjusts Jito tips dynamically based on detected HFT bot activity:

```rust
use ghost_launcher::components::trigger::jito_tip::{
    calculate_paradox_adjusted_tip, calculate_safe_tip, TipGuardConfig
};

// Step 1: Get Paradox state from Seer
let paradox_state = paradox_rx.borrow();

// Step 2: Calculate base tip (your existing logic)
let base_tip = 0.01; // SOL

// Step 3: Apply Paradox adjustment
let paradox_adjusted = calculate_paradox_adjusted_tip(
    base_tip,
    paradox_state.tension,
    paradox_state.anomaly_detected
);

// Step 4: Apply TipGuard safety limits (REQUIRED!)
let config = TipGuardConfig::default();
let final_tip = calculate_safe_tip(
    paradox_adjusted,
    trade_value_sol,
    &config
);
```

### Formula

```text
If anomaly_detected:
    multiplier = 1.0 + (tension / 50.0)
    adjusted_tip = base_tip * multiplier
Else:
    adjusted_tip = base_tip (no change)
```

### Examples

| Tension | Anomaly | Base Tip | Multiplier | Adjusted | After TipGuard (max 0.04) |
|---------|---------|----------|------------|----------|---------------------------|
| 50%     | Yes     | 0.01 SOL | 2.0x       | 0.02 SOL | 0.02 SOL                  |
| 80%     | Yes     | 0.01 SOL | 2.6x       | 0.026 SOL| 0.026 SOL                 |
| 100%    | Yes     | 0.01 SOL | 3.0x       | 0.03 SOL | 0.03 SOL                  |
| 100%    | Yes     | 0.02 SOL | 3.0x       | 0.06 SOL | **0.04 SOL** (capped)     |
| 50%     | No      | 0.01 SOL | 1.0x       | 0.01 SOL | 0.01 SOL                  |

### Important Notes

1. **TipGuard Priority**: Paradox can increase tips, but TipGuard caps still apply:
   - Absolute max: 0.04 SOL (default)
   - Ratio max: 40% of trade value (default)

2. **Safe Mode Strategy**: If running in "Safe Mode", you may want to skip trading entirely when anomaly is detected instead of just increasing tip:

```rust
if strategy == Strategy::SafeMode && paradox_state.anomaly_detected {
    warn!("Safe Mode: Skipping trade due to HFT anomaly");
    return; // Don't trade during high tension
}
```

3. **Adaptive Strategy**: For adaptive strategies, use a threshold:

```rust
if paradox_state.tension > 75.0 {
    // High competition, maybe skip or use higher slippage
    slippage_bps *= 1.5;
}
```

## 2. Guardian Integration - Network Stress Monitoring

### Location
- `ghost-brain/src/guardian/types.rs` - Signal definition
- `ghost-brain/src/guardian/watchdog.rs` - Signal handler

### Implementation

Send Paradox Sensor state to Guardian for decision-making:

```rust
use ghost_brain::guardian::types::WatchdogSignal;
use std::time::{SystemTime, UNIX_EPOCH};

// Monitor Paradox state
tokio::spawn(async move {
    while paradox_rx.changed().await.is_ok() {
        let state = *paradox_rx.borrow();
        
        // Send network stress signal to Guardian
        let timestamp_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as u64;
        
        let signal = WatchdogSignal::NetworkStress {
            tension: state.tension,
            jitter_ms: state.jitter_ms,
            density_bps: state.density_bps,
            anomaly_detected: state.anomaly_detected,
            timestamp_ms,
        };
        
        // Send to Guardian watchdog
        let _ = watchdog_tx.send(signal).await;
    }
});
```

### Guardian Logic

The Guardian accumulates risk from network stress:

```text
If anomaly_detected && tension > 80.0:
    stress_risk_penalty = (tension - 80.0) / 100.0
    internal_risk_score += stress_risk_penalty
    
    If internal_risk_score > max_internal_risk_score (default 0.75):
        ABORT execution
```

### Risk Accumulation Examples

| Tension | Risk Penalty | Use Case |
|---------|--------------|----------|
| 80%     | 0.00         | Threshold (no penalty yet) |
| 85%     | 0.05         | Mild stress |
| 90%     | 0.10         | Moderate stress |
| 95%     | 0.15         | High stress |
| 100%    | 0.20         | Extreme stress |

**Note**: Risk accumulates with other factors (data integrity violations, internal task risks). If combined risk exceeds 0.75, Guardian aborts execution.

### Guardian Configuration

```rust
use ghost_brain::guardian::types::WatchdogConfig;

let config = WatchdogConfig {
    max_void_duration_ms: 2000,
    critical_failure_threshold: 3,
    retry_threshold: 5,
    min_qass_score: 0.5,
    max_internal_risk_score: 0.75, // Abort if exceeded
    enable_parallel_tasks: true,
};
```

To make Guardian more sensitive to network stress:
```rust
config.max_internal_risk_score = 0.60; // Lower threshold = more conservative
```

## 3. Complete Integration Example

### Full Flow in Main Application

```rust
use seer::{Seer, config::SeerConfig};
use ghost_brain::guardian::types::{WatchdogSignal, WatchdogConfig};
use ghost_launcher::components::trigger::jito_tip::{
    calculate_paradox_adjusted_tip, calculate_safe_tip, TipGuardConfig
};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // 1. Initialize Seer with Paradox Sensor
    let seer_config = SeerConfig::default();
    let (candidate_tx, mut candidate_rx) = mpsc::channel(100);
    let seer = Seer::new(seer_config, candidate_tx);
    
    // Get Paradox state receiver
    let paradox_rx = seer.paradox_state_receiver().unwrap();
    
    // 2. Setup Guardian channel
    let (guardian_tx, mut guardian_rx) = mpsc::channel(100);
    
    // 3. Spawn Paradox → Guardian bridge
    let paradox_for_guardian = paradox_rx.clone();
    let guardian_sender = guardian_tx.clone();
    tokio::spawn(async move {
        while paradox_for_guardian.changed().await.is_ok() {
            let state = *paradox_for_guardian.borrow();
            
            let signal = WatchdogSignal::NetworkStress {
                tension: state.tension,
                jitter_ms: state.jitter_ms,
                density_bps: state.density_bps,
                anomaly_detected: state.anomaly_detected,
                timestamp_ms: get_current_timestamp_ms(),
            };
            
            let _ = guardian_sender.send(signal).await;
        }
    });
    
    // 4. Trading loop with Paradox awareness
    let tip_config = TipGuardConfig::default();
    
    while let Some(candidate) = candidate_rx.recv().await {
        // Get current Paradox state
        let paradox_state = paradox_rx.borrow();
        
        // Check if we should trade (Safe Mode example)
        if paradox_state.anomaly_detected && paradox_state.tension > 90.0 {
            warn!("Skipping trade: Extreme network stress detected");
            continue;
        }
        
        // Calculate adaptive tip
        let base_tip = 0.01; // Your algorithm
        let trade_value = 0.1; // SOL
        
        let paradox_adjusted = calculate_paradox_adjusted_tip(
            base_tip,
            paradox_state.tension,
            paradox_state.anomaly_detected
        );
        
        let final_tip = calculate_safe_tip(
            paradox_adjusted,
            trade_value,
            &tip_config
        );
        
        info!("Executing trade with tip: {} SOL (paradox adjusted)", final_tip);
        
        // Execute trade with adjusted tip...
    }
    
    Ok(())
}

fn get_current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64
}
```

## 4. Testing

### Test Trigger Integration

```bash
cd ghost-launcher
cargo test jito_tip::tests::test_paradox
```

Expected tests:
- `test_paradox_adjusted_tip_no_anomaly` - No change when no anomaly
- `test_paradox_adjusted_tip_with_anomaly_tension_50` - 2.0x multiplier
- `test_paradox_adjusted_tip_with_anomaly_tension_80` - 2.6x multiplier
- `test_paradox_adjusted_tip_with_anomaly_tension_100` - 3.0x multiplier
- `test_paradox_adjustment_respects_tipguard` - TipGuard caps apply

### Test Guardian Integration

```bash
cd ghost-brain
cargo test guardian::types::tests::test_network_stress
```

Expected tests:
- `test_network_stress_signal` - Signal serialization
- `test_network_stress_signal_high_tension` - High tension scenario
- `test_all_watchdog_signal_variants_serialize` - All variants including NetworkStress

## 5. Monitoring & Debugging

### Log Messages

**Trigger** (when Paradox adjusts tip):
```
Paradox Sensor: Adjusting tip 2.6x due to market tension 80.00% (base: 0.01 SOL → adjusted: 0.026 SOL)
```

**Guardian** (when network stress detected):
```
⚠️ Paradox Sensor: High Network Stress detected - Tension: 87.32%, Jitter: 2.14ms, Density: 456pps at 1234567890ms
Network stress risk added: 0.07, total risk: 0.15/0.75
```

**Guardian** (when aborting due to accumulated risk):
```
ABORT: Total risk score 0.82 exceeds threshold 0.75 (network stress + other factors)
```

### Metrics

Add Prometheus metrics for monitoring:

```rust
// In your metrics module
lazy_static! {
    static ref PARADOX_TIP_ADJUSTMENTS: IntCounter = register_int_counter!(
        "paradox_tip_adjustments_total",
        "Total number of tip adjustments due to Paradox Sensor"
    ).unwrap();
    
    static ref PARADOX_GUARDIAN_ABORTS: IntCounter = register_int_counter!(
        "paradox_guardian_aborts_total",
        "Total number of Guardian aborts due to network stress"
    ).unwrap();
}

// Increment when using Paradox
if paradox_state.anomaly_detected {
    PARADOX_TIP_ADJUSTMENTS.inc();
}
```

## 6. Best Practices

1. **Always apply TipGuard** - Paradox increases tips, TipGuard caps them
2. **Use thresholds** - Don't trade on every tension spike, use meaningful thresholds (>75% or >80%)
3. **Combine signals** - Use Paradox with other indicators (QASS, QEDD, MCI) for better decisions
4. **Monitor logs** - Watch for false positives and calibrate thresholds
5. **Test on testnet** - Validate Paradox integration before mainnet
6. **Safe Mode option** - Implement a mode that skips trades during high tension

## 7. Troubleshooting

### Issue: Tips always at maximum (0.04 SOL)

**Cause**: Paradox multiplier too aggressive, hitting TipGuard cap

**Solution**: Lower base tip or adjust Paradox formula:
```rust
// Less aggressive multiplier
let paradox_multiplier = 1.0 + (tension / 100.0); // Max 2.0x instead of 3.0x
```

### Issue: Guardian aborting too frequently

**Cause**: `max_internal_risk_score` threshold too low

**Solution**: Increase threshold or adjust penalty formula:
```rust
// In WatchdogConfig
config.max_internal_risk_score = 0.85; // More tolerant

// Or adjust penalty (in watchdog.rs)
let stress_risk_penalty = (tension - 85.0) / 100.0; // Start penalty at 85% instead of 80%
```

### Issue: No Paradox adjustments happening

**Cause**: Anomaly threshold (80%) never reached

**Solution**: Check Paradox calibration or lower threshold:
```rust
// In paradox_sensor/mod.rs
const ANOMALY_TENSION_THRESHOLD: f64 = 70.0; // Lower threshold
```

---

## Summary

The Paradox Sensor integration provides two key benefits:

1. **Trigger**: Automatically increases Jito tips during HFT bot swarms to improve transaction inclusion
2. **Guardian**: Accumulates risk from network stress and aborts execution when conditions are too risky

Both integrations work together to make the Ghost trading system more adaptive and defensive against adverse network conditions.

**Key Formula**: `adjusted_tip = base_tip * (1.0 + tension/50.0)` when anomaly detected  
**Key Safety**: TipGuard caps (0.04 SOL absolute, 40% of trade) always apply  
**Key Threshold**: Guardian adds risk when tension > 80%, aborts when total risk > 0.75
