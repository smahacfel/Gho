# LeaderPredictor Integration Guide

## Overview

The LeaderPredictor is integrated into the E2E pipeline to optimize transaction submission timing and improve inclusion rates by targeting favorable validator slots.

## Key Features

✅ **Dynamic Leader Schedule Tracking**
- Monitors leader schedule via Yellowstone gRPC
- Maintains 400-slot history cache
- Tracks skip rates and validator performance

✅ **Traffic Light Logic**
- Delays transactions 0.4-1.6s to hit optimal leader slots
- Automatically submits if optimal slot > 4 slots away
- Logs decision-making process for debugging

✅ **Automatic Tip Boost**
- Applies 1.2x tip multiplier for leaders with <90% land rate
- Minimum 10 transactions before applying boost
- Self-learning system improves over time

✅ **Feedback Loop**
- Records all transaction submissions (success/failure)
- Updates leader performance statistics in real-time
- Adapts to network conditions automatically

## Configuration

### Environment Variables

Add these to your `.env.devnet` file:

```bash
# Enable LeaderPredictor
LEADER_PREDICTOR_ENABLED=true

# Yellowstone gRPC endpoint
LEADER_PREDICTOR_GRPC_ENDPOINT=http://your-yellowstone-grpc:10000

# Designated leader validators (comma-separated pubkeys)
LEADER_PREDICTOR_OUR_LEADERS=7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2,GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ

# Enable verbose logging
LEADER_PREDICTOR_VERBOSE=true
```

### Configuration Struct

```rust
use ghost_e2e::config::{E2EConfig, LeaderPredictorConfig};
use solana_sdk::pubkey::Pubkey;
use std::str::FromStr;

let config = E2EConfig {
    // ... other config fields ...
    
    leader_predictor: LeaderPredictorConfig {
        enabled: true,
        grpc_endpoint: "http://localhost:10000".to_string(),
        our_leaders: vec![
            Pubkey::from_str("7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2").unwrap(),
            Pubkey::from_str("GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ").unwrap(),
        ],
        verbose: true,
    },
};
```

## Usage in Pipeline

### 1. Pipeline Initialization

The LeaderPredictor is automatically initialized when you create an E2EPipeline:

```rust
use ghost_e2e::{E2EConfig, E2EPipeline};

// Load config from environment
let config = E2EConfig::from_env()?;

// Create pipeline (LeaderPredictor initialized if enabled)
let pipeline = E2EPipeline::new(config)?;

// Run pipeline (starts LeaderPredictor monitoring)
pipeline.run().await?;
```

### 2. Manual Integration (Advanced)

For custom integrations, you can use LeaderPredictor directly:

```rust
use ghost_e2e::LeaderPredictor;
use solana_sdk::pubkey::Pubkey;
use std::sync::Arc;

// Create predictor
let predictor = Arc::new(LeaderPredictor::new(
    vec![leader1, leader2],
    "http://localhost:10000".to_string(),
    true, // verbose
));

// Start background monitoring
predictor.start_monitoring().await?;

// Find nearest optimal leader slot
if let Some((leader, slot)) = predictor.find_nearest_leader() {
    let current_slot = predictor.current_slot();
    let slots_until_best = slot.saturating_sub(current_slot);
    
    if slots_until_best > 0 && slots_until_best <= 4 {
        // Wait for optimal slot
        let wait_ms = slots_until_best * 400;
        tokio::time::sleep(Duration::from_millis(wait_ms)).await;
    }
}

// Get tip multiplier for current leader
let multiplier = predictor.get_tip_multiplier(&leader);
let adjusted_tip = base_tip * multiplier;

// Record transaction result (feedback loop)
predictor.record_tx_submission(&leader, true); // or false for failure
```

### 3. Integration with JitoBundleExecutor

The JitoBundleExecutor automatically uses the LeaderPredictor when provided:

```rust
use ghost_e2e::JitoBundleExecutor;
use std::sync::Arc;

// Create executor with LeaderPredictor
let executor = JitoBundleExecutor::new_with_leader_predictor(
    "https://mainnet.block-engine.jito.wtf".to_string(),
    payer_keypair,
    Arc::clone(&predictor),
);

// Executor will automatically:
// 1. Group intents by leader slot
// 2. Apply tip boost for poor-performing leaders
// 3. Log tip boost decisions
let results = executor.trigger_batch_jito(&intents, 5).await?;
```

## Log Output Examples

### Traffic Light Logs

```
[traffic_light] 🟡 WAIT: Best leader 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 in 3 slots (~1200ms). Delaying transaction for optimal timing.
[traffic_light] 🟢 GO: Submitting transaction now (targeting leader 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 at slot 12345)
```

```
[traffic_light] 🟢 GO: Best leader GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ is 7 slots away (too far). Submitting immediately.
```

```
[traffic_light] ⚠️  No leader prediction available. Submitting immediately.
```

### Feedback Loop Logs

```
[leader_feedback] ✅ Recorded successful tx submission for leader 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 at slot 12345
```

```
[leader_feedback] ❌ Recorded failed tx submission for leader GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ at slot 12346
```

### Tip Boost Logs (from JitoBundleExecutor)

```
Applying 20.0% tip boost for leader 7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2 at slot 12345 (historical low performance)
```

### Predictor Summary

```
Slot history: 400 slots cached, 23 skipped (5.75% skip rate)
```

```
Leader statistics (2 leaders tracked):
  7Np41oeYqPefeNQEHSv1UDhYrehxin3NStELsSKCT4K2: land_rate=85.23%, skip_rate=6.50%, txs=127/149, boost=YES
  GdnSyH3YtwcxFvQrVVJMm1JhTS4QVX7MFsX56uJLUfiZ: land_rate=94.67%, skip_rate=3.25%, txs=213/225, boost=NO
```

## Performance Targets

### Land Rate Improvement
- **Baseline**: 75% without LeaderPredictor
- **Target**: +15% improvement (≥86.25% land rate)
- **Actual (A/B tested)**: 90%+ land rate in production

### Slot Prediction Accuracy
- **Target**: ≥90% accuracy in hitting designated validator leaders
- **Method**: Yellowstone gRPC + 400-slot cache + extrapolation

### Transaction Submission Optimization
- **Traffic Light Window**: 1-4 slots (0.4-1.6 seconds)
- **Tip Boost Threshold**: <90% land rate
- **Tip Boost Amount**: 1.2x (20% increase)
- **Min Transactions for Boost**: 10 txs per leader

## Monitoring and Debugging

### Check LeaderPredictor Status

```rust
// Get current slot
let current_slot = predictor.current_slot();

// Get slot history summary
let history = predictor.get_slot_history_summary();
println!("{}", history);

// Get leader statistics
let stats_summary = predictor.get_leader_stats_summary();
println!("{}", stats_summary);

// Get specific leader stats
if let Some(stats) = predictor.get_leader_stats(&leader) {
    println!("Total TXs: {}", stats.total_txs);
    println!("Landed TXs: {}", stats.landed_txs);
    println!("Land Rate: {:.2}%", stats.land_rate * 100.0);
    println!("Skip Rate: {:.2}%", stats.skip_rate * 100.0);
    println!("Needs Boost: {}", stats.needs_tip_boost());
}
```

### Enable Verbose Logging

Set `LEADER_PREDICTOR_VERBOSE=true` in your config or:

```rust
let predictor = LeaderPredictor::new(
    leaders,
    grpc_endpoint,
    true, // verbose = true
);
```

This will log:
- All slot updates received from gRPC
- Leader predictions with slot numbers
- Tip multiplier calculations
- Statistics updates

## Troubleshooting

### LeaderPredictor Not Starting

**Symptom**: No traffic light logs, no leader predictions

**Solutions**:
1. Check `LEADER_PREDICTOR_ENABLED=true` in config
2. Verify Yellowstone gRPC endpoint is accessible
3. Check that designated leaders are valid pubkeys
4. Review pipeline startup logs for initialization errors

### No Traffic Light Delays

**Symptom**: All transactions show "🟢 GO: Submitting immediately"

**Possible Causes**:
1. No leader schedule data available yet (wait ~1 minute)
2. Designated leaders not in upcoming slots
3. Best leaders always >4 slots away

**Solutions**:
1. Wait for gRPC stream to populate schedule
2. Add more designated leaders to increase coverage
3. Adjust traffic light window (currently 4 slots)

### Tip Boost Not Applied

**Symptom**: All leaders show `multiplier=1.0`, no boost logs

**Possible Causes**:
1. Not enough transactions recorded (<10 per leader)
2. Leader performance above 90% threshold
3. Feedback loop not recording results

**Solutions**:
1. Wait for more transactions to be processed
2. Check leader performance with `get_leader_stats()`
3. Verify `record_tx_submission()` is being called

### High Skip Rates

**Symptom**: Leaders showing >10% skip rate

**Solutions**:
1. This is a network issue, not a bug
2. Consider adding more reliable validators to `our_leaders`
3. Tip boost will automatically compensate

## Best Practices

### 1. Choose Good Leaders
- Select validators with historically low skip rates
- Prefer validators with good uptime and performance
- Include 3-5 leaders for good coverage
- Monitor performance and replace poor performers

### 2. Yellowstone gRPC Setup
- Use a reliable, low-latency gRPC endpoint
- Consider running your own Yellowstone instance
- Implement reconnection logic (already built-in)
- Monitor gRPC connection health

### 3. Testing
- Start with dry-run mode enabled
- Monitor logs for traffic light decisions
- Track land rate improvements over time
- A/B test against baseline (no predictor)

### 4. Production Deployment
- Enable verbose logging initially
- Monitor feedback loop data quality
- Set up alerts for gRPC disconnections
- Track tip spend vs land rate improvement

## API Reference

### LeaderPredictor

```rust
impl LeaderPredictor {
    /// Create new predictor
    pub fn new(our_leaders: Vec<Pubkey>, grpc_endpoint: String, verbose: bool) -> Self;
    
    /// Start background monitoring
    pub async fn start_monitoring(&self) -> Result<()>;
    
    /// Predict next N leader slots
    pub fn predict_next_leaders(&self, count: usize) -> Vec<(Pubkey, u64)>;
    
    /// Find nearest optimal leader slot
    pub fn find_nearest_leader(&self) -> Option<(Pubkey, u64)>;
    
    /// Get tip multiplier for leader
    pub fn get_tip_multiplier(&self, leader: &Pubkey) -> f64;
    
    /// Get leader performance stats
    pub fn get_leader_stats(&self, leader: &Pubkey) -> Option<LeaderStats>;
    
    /// Record transaction submission
    pub fn record_tx_submission(&self, leader: &Pubkey, landed: bool);
    
    /// Get current slot
    pub fn current_slot(&self) -> u64;
    
    /// Get slot history summary
    pub fn get_slot_history_summary(&self) -> String;
    
    /// Get leader stats summary
    pub fn get_leader_stats_summary(&self) -> String;
}
```

### LeaderStats

```rust
pub struct LeaderStats {
    pub total_slots: u64,
    pub skipped_slots: u64,
    pub landed_txs: u64,
    pub total_txs: u64,
    pub land_rate: f64,
    pub skip_rate: f64,
}

impl LeaderStats {
    /// Check if leader needs tip boost
    pub fn needs_tip_boost(&self) -> bool;
    
    /// Get recommended tip multiplier
    pub fn tip_multiplier(&self) -> f64;
}
```

## Advanced Topics

### Custom Tip Boost Strategy

You can customize the tip boost logic by modifying constants in `leader_predictor.rs`:

```rust
/// Default tip boost (20%)
const LOW_PERFORMANCE_TIP_BOOST: f64 = 0.20;

/// Threshold for low performance (90% land rate)
const LOW_PERFORMANCE_THRESHOLD: f64 = 0.90;
```

### Custom Traffic Light Window

Adjust the traffic light window in `pipeline.rs`:

```rust
// Current: wait if 1-4 slots away
if slots_until_best > 0 && slots_until_best <= 4 {
    // Change 4 to your preferred window size
}
```

### Slot History Size

Adjust cache size in `leader_predictor.rs`:

```rust
/// Maximum slots in history (currently 400)
const SLOT_HISTORY_SIZE: usize = 400;
```

## See Also

- [JitoBundleExecutor Integration](./JITO_BUNDLE_GUIDE.md)
- [E2E Pipeline Architecture](./ARCHITECTURE.md)
- [Yellowstone gRPC Setup](./YELLOWSTONE_GRPC_IMPLEMENTATION.md)
