# Feature Worker Usage Example

This document provides examples of how to use the asynchronous feature worker in the H-5N1P3R system.

## Basic Usage

### 1. Initialize the Worker

```rust
use h_5n1p3r::features::{FeatureWorker, FeatureWorkerConfig, FeatureStore};
use h_5n1p3r::oracle::data_sources::OracleDataSources;
use h_5n1p3r::oracle::types::OracleConfig;
use std::sync::Arc;
use std::time::Duration;
use reqwest::Client;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // 1. Create OracleDataSources with RPC endpoints
    let rpc_endpoints = vec![
        "https://api.mainnet-beta.solana.com".to_string(),
        "https://solana-api.projectserum.com".to_string(),
    ];
    let http_client = Client::new();
    let oracle_config = OracleConfig::default();
    let data_sources = Arc::new(OracleDataSources::new(
        rpc_endpoints,
        http_client,
        oracle_config,
    ));

    // 2. Create FeatureStore with desired capacity and TTL
    let feature_store = Arc::new(FeatureStore::new(
        1000,  // max 1000 entries
        Duration::from_secs(300),  // 5-minute TTL
    ));

    // 3. Configure the worker
    let worker_config = FeatureWorkerConfig {
        max_concurrent_fetches: 10,
        fetch_timeout: Duration::from_secs(30),
        retry_attempts: 3,
        queue_capacity: 1000,
    };

    // 4. Create and start the worker
    let worker = FeatureWorker::new(
        data_sources,
        feature_store.clone(),
        worker_config,
    );
    let worker_handle = worker.start().await?;

    // Worker is now running in the background
    Ok(())
}
```

### 2. Submit Candidates for Processing

```rust
use h_5n1p3r::types::PremintCandidate;

// Create a candidate
let candidate = PremintCandidate {
    mint: "TokenMintAddress123...".to_string(),
    creator: "CreatorAddress123...".to_string(),
    program: "pump.fun".to_string(),
    slot: 123456789,
    timestamp: 1640995200,
    instruction_summary: Some("InitializePool".to_string()),
    is_jito_bundle: Some(false),
};

// Submit for processing (blocking if queue is full)
worker_handle.submit(candidate.clone()).await?;

// OR: Try to submit without blocking
match worker_handle.try_submit(candidate) {
    Ok(()) => println!("Candidate submitted successfully"),
    Err(e) => eprintln!("Queue is full: {}", e),
}
```

### 3. Retrieve Cached Features

```rust
// After the worker has processed a candidate, features are available in the store
let mint = "TokenMintAddress123...".to_string();

if let Some(features) = feature_store.get(&mint).await {
    println!("Found {} features in cache", features.len());
    
    // Access specific features
    if let Some(price) = features.get("price_current") {
        println!("Current price: {}", price);
    }
    if let Some(volume) = features.get("volume_24h") {
        println!("24h volume: {}", volume);
    }
} else {
    println!("Features not yet available or expired");
}
```

### 4. Graceful Shutdown

```rust
// When shutting down the system
worker_handle.shutdown().await?;
```

## Integration with Orchestrator

### Using SystemInitializer

```rust
use h_5n1p3r::orchestrator::{SystemInitializer, OrchestratorConfig};
use h_5n1p3r::features::FeatureStore;
use std::time::Duration;

// Load configuration
let config = OrchestratorConfig::from_toml_file("config.toml")?;

// Create shared data sources and feature store
let data_sources = Arc::new(OracleDataSources::new(
    vec![config.rpc_url.clone()],
    Client::new(),
    OracleConfig::default(),
));
let feature_store = Arc::new(FeatureStore::new(1000, Duration::from_secs(300)));

// Initialize the worker using the orchestrator config
let worker_config = config.feature_worker.to_worker_config();
let worker_handle = SystemInitializer::init_feature_worker(
    data_sources,
    feature_store.clone(),
    worker_config,
).await?;
```

## Configuration via TOML

Add the following to your `config.toml`:

```toml
[feature_worker]
max_concurrent_fetches = 10
fetch_timeout_secs = 30
retry_attempts = 3
queue_capacity = 1000
```

## Advanced Patterns

### Pattern 1: Fire-and-Forget Processing

The worker is designed for fire-and-forget operation. Submit candidates without waiting for completion:

```rust
// In your scoring/oracle code
for candidate in incoming_candidates {
    // Submit to worker (non-blocking)
    let _ = worker_handle.try_submit(candidate.clone());
    
    // Continue with scoring using existing cached data
    if let Some(features) = feature_store.get(&candidate.mint).await {
        // Use cached features for scoring
        let score = compute_score(&features);
    } else {
        // Use default scoring if features not yet available
        let score = compute_default_score(&candidate);
    }
}
```

### Pattern 2: Cache-First Architecture

Always check the cache first, worker fills it asynchronously:

```rust
async fn get_features_for_scoring(
    candidate: &PremintCandidate,
    feature_store: &Arc<FeatureStore>,
    worker_handle: &FeatureWorkerHandle,
) -> Option<FeatureVector> {
    // Check cache first
    if let Some(features) = feature_store.get(&candidate.mint).await {
        return Some(features);
    }
    
    // Not in cache, submit to worker for future requests
    let _ = worker_handle.try_submit(candidate.clone());
    
    // Return None for now, will be available on next request
    None
}
```

### Pattern 3: Monitoring Worker Status

```rust
// Get cache statistics
let stats = feature_store.stats().await;
println!("Cache entries: {}", stats.entry_count);
println!("Cache size: {} bytes", stats.weighted_size);

// Check queue capacity
let available_capacity = worker_handle.queue_capacity();
println!("Queue available capacity: {}", available_capacity);
```

## Performance Tuning

### Concurrency Limits

Adjust `max_concurrent_fetches` based on:
- Available network bandwidth
- RPC endpoint rate limits
- System resources (CPU, memory)

**Recommendations:**
- Low traffic: 5-10 concurrent fetches
- Medium traffic: 10-20 concurrent fetches
- High traffic: 20-50 concurrent fetches (with multiple RPC endpoints)

### Cache TTL

Adjust TTL based on data freshness requirements:
- Real-time trading: 60-300 seconds (1-5 minutes)
- Analysis/backtesting: 600-3600 seconds (10-60 minutes)
- Historical data: 3600+ seconds (1+ hour)

### Queue Capacity

Size the queue based on burst traffic:
- `queue_capacity` should handle expected bursts
- If queue fills frequently, increase capacity or concurrent fetches
- Monitor queue capacity to detect bottlenecks

## Error Handling

The worker handles errors gracefully:

1. **Fetch failures**: Retried with exponential backoff
2. **Timeout**: Operations cancelled after `fetch_timeout`
3. **Invalid data**: Logged and skipped
4. **Queue full**: `try_submit` returns error, `submit` waits

## Metrics and Observability

The worker uses `tracing` for observability:

```rust
// Enable tracing in your application
use tracing_subscriber;

tracing_subscriber::fmt()
    .with_max_level(tracing::Level::DEBUG)
    .init();
```

Key log events:
- `Feature worker started` - Worker initialization
- `Cache hit for key: <mint>` - Feature found in cache
- `Successfully stored features for <mint>` - Features cached
- `All N attempts failed for <mint>` - Permanent failure
- `Feature worker stopped` - Worker shutdown

## Best Practices

1. **Single worker instance**: Create one worker per application
2. **Shared feature store**: Use the same FeatureStore across components
3. **Fail gracefully**: Don't block scoring on feature availability
4. **Monitor cache hit rate**: High hit rate = good performance
5. **Tune for your workload**: Adjust concurrency and TTL based on metrics
6. **Handle backpressure**: Check queue capacity before submission
7. **Graceful shutdown**: Always call `shutdown()` on exit

## Common Issues and Solutions

### Issue: Features not appearing in cache

**Causes:**
- RPC endpoint failures
- Network issues
- Invalid token addresses
- TTL expired

**Solutions:**
- Check RPC endpoint health
- Verify token addresses
- Increase retry attempts
- Adjust TTL

### Issue: High memory usage

**Causes:**
- Large queue capacity
- Too many cached entries
- Memory leak in data sources

**Solutions:**
- Reduce `queue_capacity`
- Reduce FeatureStore `max_capacity`
- Monitor cache size with `stats()`

### Issue: Slow feature extraction

**Causes:**
- Too few concurrent fetches
- Slow RPC endpoints
- Network latency

**Solutions:**
- Increase `max_concurrent_fetches`
- Add more RPC endpoints
- Use geographically closer endpoints
- Increase `fetch_timeout` for slow endpoints

## Testing

The worker includes comprehensive tests:

```bash
# Run all worker tests
cargo test --package h-5n1p3r features::worker

# Run specific test
cargo test --package h-5n1p3r test_cache_hit_skip

# Run with logging
RUST_LOG=debug cargo test --package h-5n1p3r features::worker -- --nocapture
```

## Further Reading

- [FeatureStore Documentation](./store.rs)
- [OracleDataSources Documentation](../oracle/data_sources.rs)
- [FeatureExtractionPipeline Documentation](./extractors.rs)
- [System Architecture](../../ARCHITECTURE.md)
