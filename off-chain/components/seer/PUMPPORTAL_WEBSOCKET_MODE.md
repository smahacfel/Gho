# PumpPortal WebSocket Mode

## Overview

PumpPortal WebSocket mode provides real-time Pump.fun token creation and trading data ingestion through PumpPortal's public WebSocket API (`wss://pumpportal.fun/api/data`). This mode serves as an alternative to gRPC/Geyser for accessing Pump.fun events with minimal latency.

## Features

- **Real-time Event Streaming**: Subscribe to new token creations and trades as they happen
- **Dynamic Subscription Management**: Automatically subscribes to trade events for newly detected tokens
- **In-Memory Statistics**: Tracks tx_count, volume (buy/sell split), and unique traders per mint
- **Exponential Backoff Reconnection**: Robust reconnection with exponential backoff (5s base → 300s max)
- **Capacity Management**: Configurable max active mints with LRU eviction
- **Batched Subscriptions**: Rate-limited subscription requests to avoid API throttling

## Configuration

### Via Ghost Launcher (Recommended)

When running Seer through the ghost-launcher, configure PumpPortal mode in the root `config.toml`:

```toml
[seer]
# Enable Seer component
enabled = true

# Set source_mode to PumpPortal WebSocket
source_mode = "pump_portal_ws"

# Connection mode (legacy, will be overridden by source_mode)
connection_mode = "grpc"

# Other Seer settings...
grpc_endpoint = "your-grpc-endpoint:443"
rpc_endpoint = "https://your-rpc-endpoint"
enable_pumpfun = true
enable_bonkfun = true
metrics_port = 9090
ipc_buffer_size = 100000

# PumpPortal-specific configuration
[seer.pumpportal]
ws_url = "wss://pumpportal.fun/api/data"
max_active_mints = 100
subscription_batch_size = 10
reconnect_base_delay_secs = 5
reconnect_max_delay_secs = 300
stats_window_secs = 900
```

**Start the launcher:**

```bash
# From repository root
cargo run --bin ghost-launcher --release
```

The launcher will read the config and start Seer in PumpPortal WebSocket mode with the configured parameters.

### Via Seer Binary (Standalone)

#### Environment Variables

#### Environment Variables

When running the Seer binary directly (without ghost-launcher), use environment variables:

```bash
# Set source mode to PumpPortal WebSocket (REQUIRED)
export SEER_SOURCE_MODE=pump_portal_ws

# PumpPortal WebSocket endpoint (default: wss://pumpportal.fun/api/data)
export PUMPPORTAL_WS_URL=wss://pumpportal.fun/api/data

# Maximum number of active mints to track simultaneously (default: 100)
export PUMPPORTAL_MAX_ACTIVE_MINTS=100

# Batch size for subscription requests (default: 10)
export PUMPPORTAL_SUBSCRIPTION_BATCH_SIZE=10

# Reconnection settings
export PUMPPORTAL_RECONNECT_BASE_DELAY_SECS=5
export PUMPPORTAL_RECONNECT_MAX_DELAY_SECS=300

# Time window for tracking stats per mint in seconds (default: 900 = 15 minutes)
export PUMPPORTAL_STATS_WINDOW_SECS=900

# Run seer binary
cd off-chain/components/seer
cargo run --bin seer --release
```

#### Legacy Environment Variables (Optional)

These are optional and apply to all modes:

```bash
# Other Seer configuration (applies to all modes)
export SEER_GEYSER_ENDPOINT=wss://api.mainnet-beta.solana.com
export SEER_GRPC_ENDPOINT=http://grpc.mainnet.solana.com:10000
export SEER_RPC_ENDPOINT=https://api.mainnet-beta.solana.com
export SEER_METRICS_PORT=9090
export SEER_IPC_BUFFER_SIZE=100000
```

## Usage Examples

### Example 1: Running via Ghost Launcher

1. **Edit root `config.toml`:**

```toml
[seer]
enabled = true
source_mode = "pump_portal_ws"  # This is the key setting
connection_mode = "grpc"
grpc_endpoint = "grpc.nln.clr3.org:443"
grpc_x_token = "your-api-key-here"
grpc_auth_header = "x-api-key"
rpc_endpoint = "https://rpc.nln.clr3.org"
enable_pumpfun = true
enable_bonkfun = true
metrics_port = 9090
ipc_buffer_size = 100000
ipc_backpressure_policy = "block"

[seer.pumpportal]
ws_url = "wss://pumpportal.fun/api/data"
max_active_mints = 100
subscription_batch_size = 10
reconnect_base_delay_secs = 5
reconnect_max_delay_secs = 300
stats_window_secs = 900
```

2. **Start the launcher:**

```bash
cargo run --bin ghost-launcher --release
```

3. **Expected logs:**

```
INFO Seer: Configuration loaded
INFO   Effective source mode: PumpPortalWs
INFO   PumpPortal WS URL: wss://pumpportal.fun/api/data
INFO   PumpPortal max active mints: 100
INFO   PumpPortal subscription batch size: 10
INFO Initializing PumpPortal WebSocket mode
INFO Connecting via PumpPortal WebSocket...
INFO Seer is now listening for InitializePool events...
```

### Example 2: Running Seer Binary Standalone

```bash
# Set required environment variables
export SEER_SOURCE_MODE=pump_portal_ws
export PUMPPORTAL_WS_URL=wss://pumpportal.fun/api/data
export PUMPPORTAL_MAX_ACTIVE_MINTS=100
export PUMPPORTAL_SUBSCRIPTION_BATCH_SIZE=10

# Optional: RPC endpoint for transaction fetching
export SEER_RPC_ENDPOINT=https://api.mainnet-beta.solana.com

# Run seer
cd off-chain/components/seer
cargo run --bin seer --release
```

**Expected logs:**

```
INFO Seer - Real-time InitializePool Detector
INFO Configuration loaded:
INFO   Source mode: Some(PumpPortalWs)
INFO   Effective source mode: PumpPortalWs
INFO PumpPortal WebSocket Mode:
INFO   WS URL: wss://pumpportal.fun/api/data
INFO   Max active mints: 100
INFO   Subscription batch size: 10
INFO   Reconnect base delay: 5s
INFO   Reconnect max delay: 300s
INFO   Stats window: 900s
INFO Starting Seer module
INFO Effective source mode: PumpPortalWs
INFO Initializing PumpPortal WebSocket mode
INFO Connecting via PumpPortal WebSocket...
INFO Seer is now listening for InitializePool events...
```

### Example 3: Using Programmatically

```rust
use seer::{Seer, SeerConfig};
use seer::config::SeerSourceMode;

#[tokio::main]
async fn main() {
    let mut config = SeerConfig::default();
    
    // Enable PumpPortal WebSocket mode
    config.source_mode = Some(SeerSourceMode::PumpPortalWs);
    
    // Configure PumpPortal settings
    config.pumpportal.ws_url = "wss://pumpportal.fun/api/data".to_string();
    config.pumpportal.max_active_mints = 100;
    
    let (candidate_tx, mut candidate_rx) = tokio::sync::mpsc::channel(100);
    let seer = Seer::new(config, candidate_tx);
    
    // Run Seer
    tokio::spawn(async move {
        seer.run().await.expect("Seer failed");
    });
    
    // Process candidates
    while let Some(candidate) = candidate_rx.recv().await {
        println!("New pool: {:?}", candidate);
    }
}
```

## Verifying Configuration

To verify that Seer is running in PumpPortal mode, check the logs for:

1. **Effective source mode:**
   ```
   INFO   Effective source mode: PumpPortalWs
   ```

2. **PumpPortal initialization:**
   ```
   INFO Initializing PumpPortal WebSocket mode
   ```

3. **Connection message:**
   ```
   INFO Connecting via PumpPortal WebSocket...
   ```

4. **No gRPC or Geyser messages:**
   - ❌ Should NOT see: "Connected to Geyser"
   - ❌ Should NOT see: "Connecting via Yellowstone gRPC"
   - ❌ Should NOT see: "Connecting via Geyser WebSocket"

## Event Flow

1. **Connection**: Seer establishes WebSocket connection to PumpPortal
2. **New Token Subscription**: Automatically subscribes to `subscribeNewToken` events
3. **Token Detection**: When a new token is detected:
   - Creates in-memory stats tracking for the mint
   - Adds mint to active tracking queue (LRU)
   - Queues trade subscription for the mint
4. **Trade Subscription**: Batches and sends `subscribeTokenTrade` requests
5. **Trade Events**: Updates in-memory stats (tx_count, volume, unique traders)
6. **Event Mapping**: Maps PumpPortal events to `GeyserEvent` for pipeline compatibility
7. **Cleanup**: Expires mints after stats window elapses

## Event Types

### New Token Event

PumpPortal sends new token events when a Pump.fun token is created:

```json
{
  "signature": "...",
  "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "bondingCurve": "8xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsV",
  "traderPublicKey": "9xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsW",
  "timestamp": 1699900000000,
  "virtualSolReserves": 30000000000,
  "virtualTokenReserves": 1073000000000000
}
```

### Trade Event

PumpPortal sends trade events for subscribed mints:

```json
{
  "signature": "...",
  "mint": "7xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsU",
  "bondingCurve": "8xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsV",
  "traderPublicKey": "9xKXtg2CW87d97TXJSDpbD5jBkheTqA83TZRuJosgAsW",
  "txType": "buy",
  "solAmount": 1000000,
  "tokenAmount": 5000000000,
  "timestamp": 1699900000000
}
```

## Mapping to GeyserEvent

PumpPortal events are mapped to the existing `GeyserEvent` enum for compatibility with the Seer pipeline:

### New Token → GeyserEvent::Transaction
- Creates a synthetic transaction event
- Includes pool initialization logs
- Sets `source = "pumpportal"`
- Sets `synthetic = true` (critical - ensures correct pipeline handling)
- `mpcf_payload_bytes = None` (PumpPortal doesn't provide raw instruction data)
- `mpcf_payload_missing_reason = ProviderDoesNotSupport`

### Trade → GeyserEvent::Transaction
- Creates a synthetic transaction event
- Includes trade-specific logs (Buy/Sell)
- Sets `source = "pumpportal"`
- Sets `synthetic = true` (critical - ensures correct pipeline handling)
- Updates in-memory statistics

**Why `synthetic = true`?** PumpPortal provides parsed event data without raw transaction bytes. Setting this flag ensures downstream components don't attempt to parse missing raw instruction data, preventing silent failures or false negatives.

## Statistics Tracking

For each active mint, Seer tracks:

- **tx_count**: Total number of transactions
- **buy_volume_lamports**: Total SOL spent on buys
- **sell_volume_lamports**: Total SOL received from sells
- **unique_traders**: Set of unique trader pubkeys
- **first_seen**: When the mint was first detected
- **last_trade_time**: Timestamp of most recent trade

These statistics are maintained in-memory and used for:
- Fast filtering and triggering decisions
- Real-time monitoring of token activity
- Automatic cleanup after the stats window expires

## Capacity Management

To prevent unbounded memory growth:

1. **Max Active Mints**: Configurable limit (default 100)
2. **LRU Eviction**: When at capacity, removes oldest mint
3. **Automatic Cleanup**: Expires mints after stats_window_secs
4. **Subscription Batching**: Batches trade subscriptions to avoid API spam

## Limitations

### No Raw Transaction Bytes

PumpPortal provides parsed event data but not raw transaction bytes. This means:

- `mpcf_payload_bytes` is always `None`
- `mpcf_payload_missing_reason` is set to `ProviderDoesNotSupport`
- MPCF entropy analysis falls back to heuristic-based classification

### No Slot Information

PumpPortal doesn't provide Solana slot information:

- `slot` field is set to `0`
- Cannot track block confirmations or reorganizations
- Timestamps are based on PumpPortal's processing time

### Single WebSocket Connection

According to PumpPortal documentation, avoid multiple parallel connections:

- Use one connection with multiple subscriptions
- Connection is shared across all mint subscriptions

## Monitoring & Metrics

PumpPortal mode emits standard Seer metrics:

- `seer_geyser_events_received{source="pumpportal"}`: Events received
- `seer_websocket_reconnections{status="pumpportal_*"}`: Reconnection attempts
- `seer_initialize_pool_detected{amm="pumpfun"}`: Pools detected
- `seer_initialize_pool_parsed_success{amm="pumpfun"}`: Successfully parsed pools

## Troubleshooting

### Connection Failures

```
ERROR: Failed to connect to PumpPortal: Connection failed
```

**Solution**: Check network connectivity and PumpPortal endpoint availability

### Subscription Spam

```
WARN: Failed to send trade subscriptions: rate limited
```

**Solution**: Increase `subscription_batch_size` or add delay between batches

### Memory Growth

```
WARN: Removed oldest mint to make room: [mint] (capacity: 100)
```

**Solution**: Increase `max_active_mints` or reduce `stats_window_secs`

### No Events Received

```
WARN: PumpPortal WebSocket stream ended unexpectedly
```

**Solution**: Check reconnection logs and ensure exponential backoff is working

## Comparison with Other Modes

| Feature | PumpPortalWs | GeyserGrpc | GeyserWebSocket |
|---------|--------------|------------|-----------------|
| Latency | Low | Lowest | Medium |
| Raw Bytes | ❌ | ✅ | ✅ |
| Slot Info | ❌ | ✅ | ✅ |
| Trade Events | ✅ | ✅ | ✅ |
| Setup Complexity | Low | High | Medium |
| Cost | Free | Paid | Paid/Free |
| Reliability | Medium | High | Medium |

## Future Enhancements

- [ ] Support for unsubscribing from inactive mints
- [ ] Configurable filters (min volume, min tx_count)
- [ ] Persistence of mint statistics across restarts
- [ ] Support for multiple simultaneous PumpPortal endpoints (if allowed)
- [ ] Integration with Raydium migration events

## References

- [PumpPortal Documentation](https://pumpportal.fun/docs)
- [Pump.fun Protocol](https://pump.fun)
- [Seer Architecture](./README.md)
