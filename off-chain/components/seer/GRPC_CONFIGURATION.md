# Yellowstone gRPC Configuration Guide

This document provides comprehensive guidance on configuring and using the Yellowstone gRPC connection for the Seer component.

## Overview

Seer supports two connection modes for receiving real-time blockchain events:
1. **WebSocket Mode** - Legacy mode using Solana's WebSocket API
2. **gRPC Mode** (Recommended) - High-performance mode using Yellowstone gRPC

## Configuration

### Connection Mode

Set the connection mode via environment variable:

```bash
# Use gRPC mode (recommended)
export SEER_CONNECTION_MODE=grpc

# Use WebSocket mode (legacy)
export SEER_CONNECTION_MODE=websocket
```

### gRPC Endpoint

Configure the Yellowstone gRPC endpoint:

```bash
# Example: Local gRPC server
export SEER_GRPC_ENDPOINT=http://localhost:10000

# Example: Triton (requires authentication)
export SEER_GRPC_ENDPOINT=https://api.triton.one

# Example: Helius (requires API key)
export SEER_GRPC_ENDPOINT=https://mainnet.helius-rpc.com
```

### Commitment / Mempool Mode

Control the commitment level (processed/mempool vs confirmed/finalized):

```bash
# Fastest: mempool/processed (default)
export SEER_COMMITMENT=processed

# Safer but slower:
export SEER_COMMITMENT=confirmed   # ~300-600ms behind mempool
export SEER_COMMITMENT=finalized   # >1s, not recommended for trading
```

If the provider rejects `processed/mempool`, Seer can automatically fall back to WebSocket by enabling:

```bash
export SEER_GRPC_WS_FALLBACK=true   # default
```

Latency expectations (typical):
- gRPC processed/mempool: 80–200ms from mint
- gRPC confirmed: 300–900ms from mint
- WebSocket fallback: 250–700ms depending on RPC indexing
- Helius WebSocket: 400–1200ms (free tier varies)

### Client Identification

Optionally identify your client instance:

```bash
export SEER_GRPC_CLIENT_ID=seer-production-001
```

This helps with:
- Debugging connection issues
- Server-side rate limiting and tracking
- Multi-instance deployments

### Authentication

For authenticated endpoints, provide an API key or token:

```bash
export SEER_GRPC_AUTH_TOKEN=your-api-key-here
```

**Note**: The token will be sent in the `Authorization: Bearer <token>` header. Ensure your gRPC provider supports this authentication method.

### Reconnection Configuration

Configure exponential backoff for connection failures:

```bash
# Initial delay between reconnection attempts (default: 5 seconds)
export SEER_RECONNECT_DELAY_SECS=5

# Maximum delay cap for exponential backoff (default: 300 seconds / 5 minutes)
export SEER_MAX_RECONNECT_DELAY_SECS=300

# Maximum number of reconnection attempts (default: 10)
export SEER_MAX_RECONNECT_ATTEMPTS=10
```

#### Exponential Backoff Behavior

The reconnection delay grows exponentially but is capped at the maximum:

| Attempt | Delay Calculation | Actual Delay |
|---------|-------------------|--------------|
| 1 | 5 * 2^0 | 5 seconds |
| 2 | 5 * 2^1 | 10 seconds |
| 3 | 5 * 2^2 | 20 seconds |
| 4 | 5 * 2^3 | 40 seconds |
| 5 | 5 * 2^4 | 80 seconds |
| 6 | 5 * 2^5 | 160 seconds |
| 7 | 5 * 2^6 | 300 seconds (capped) |
| 8+ | 5 * 2^(n-1) | 300 seconds (capped) |

### Verbose Logging

Enable detailed logging for debugging:

```bash
export SEER_VERBOSE=true
export RUST_LOG=seer=debug
```

## Complete Configuration Example

### For Development (Local gRPC Server)

```bash
export SEER_CONNECTION_MODE=grpc
export SEER_GRPC_ENDPOINT=http://localhost:10000
export SEER_VERBOSE=true
export RUST_LOG=seer=debug
```

### For Production (Authenticated Provider)

```bash
export SEER_CONNECTION_MODE=grpc
export SEER_GRPC_ENDPOINT=https://api.triton.one
export SEER_GRPC_CLIENT_ID=seer-mainnet-prod-01
export SEER_GRPC_AUTH_TOKEN=<your-api-key>
export SEER_MAX_RECONNECT_ATTEMPTS=10
export SEER_RECONNECT_DELAY_SECS=5
export SEER_MAX_RECONNECT_DELAY_SECS=300
export SEER_VERBOSE=false
export RUST_LOG=seer=info
```

## Using .env File

Create a `.env` file in the project root (copy from `.env.devnet.example`):

```env
# Connection Configuration
SEER_CONNECTION_MODE=grpc
SEER_GRPC_ENDPOINT=http://grpc.mainnet.solana.com:10000
SEER_GRPC_CLIENT_ID=seer-devnet-001
# SEER_GRPC_AUTH_TOKEN=your-api-key-here

# Reconnection Configuration
SEER_MAX_RECONNECT_ATTEMPTS=10
SEER_RECONNECT_DELAY_SECS=5
SEER_MAX_RECONNECT_DELAY_SECS=300

# Logging
SEER_VERBOSE=false
```

Load it before running:

```bash
source .env
cargo run --package seer --bin seer
```

## State Transition Logging

When running Seer with gRPC mode, you'll see comprehensive state transition logs:

```
STATE TRANSITION: DISCONNECTED -> CONNECTING | Initial connection attempt | Reconnects: 0
STATE: Establishing gRPC connection...
STATE: gRPC connection established, preparing subscription...
STATE: Subscribing to event stream...
STATE: Successfully subscribed to Yellowstone gRPC stream
STATE TRANSITION: CONNECTING -> CONNECTED | Connection established successfully | Reconnects: 0
Event stream health: HEALTHY - receiving events
```

On reconnection failures:

```
STATE TRANSITION: CONNECTED -> RECONNECTING | Connection failed: Stream error (attempt 1/10) | Reconnects: 0
Connection uptime: 0h 5m 23s
Reconnect attempt 1/10 failed: Stream error. Waiting 5 seconds (exponential backoff)...
```

## Health Monitoring

### Periodic Health Checks

Every 30 seconds, the connection logs health status:

```
Event stream health check: HEALTHY | Events received: 1234 | Last event: 2s ago | Total reconnects: 0
```

### Uptime Tracking

Each state transition includes uptime information:

```
Connection uptime: 2h 15m 42s
```

## Subscription Filters

The gRPC connection automatically applies filters for:

### Account Filters
- **Pump.fun Program**: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- **Bonk.fun Program**: `LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj`

### Transaction Filters
- Excludes vote transactions
- Excludes failed transactions
- Confirmed commitment level

### Slot Updates
- Tracks slot progression for sequencing

## Troubleshooting

### Connection Fails Immediately

**Problem**: Connection fails on first attempt.

**Solutions**:
1. Verify endpoint is correct and accessible
2. Check if authentication is required
3. Verify firewall/network settings
4. Try with verbose logging: `SEER_VERBOSE=true`

### Frequent Reconnections

**Problem**: Connection keeps dropping and reconnecting.

**Solutions**:
1. Check network stability
2. Verify the gRPC server is healthy
3. Increase `SEER_MAX_RECONNECT_DELAY_SECS` for more gradual backoff
4. Contact your gRPC provider about rate limits

### Authentication Errors

**Problem**: Getting 401/403 errors.

**Solutions**:
1. Verify `SEER_GRPC_AUTH_TOKEN` is correct
2. Check token hasn't expired
3. Ensure your subscription/plan is active
4. Verify the authentication method (Bearer token) is supported

### No Events Received

**Problem**: Connection successful but no events are coming through.

**Solutions**:
1. Verify the programs (Pump.fun/Bonk.fun) are active on the network
2. Check if filters are too restrictive
3. Monitor the health logs for event counts
4. Verify you're connected to the correct network (devnet/mainnet)

## Programmatic Configuration

For programmatic configuration, use the `SeerConfig` struct:

```rust
use seer::config::{SeerConfig, ConnectionMode};

let config = SeerConfig {
    connection_mode: ConnectionMode::Grpc,
    grpc_endpoint: "http://localhost:10000".to_string(),
    grpc_client_id: Some("my-client".to_string()),
    grpc_auth_token: Some("my-token".to_string()),
    max_reconnect_attempts: 10,
    reconnect_delay_secs: 5,
    max_reconnect_delay_secs: 300,
    verbose: true,
    ..Default::default()
};

let (tx, rx) = tokio::sync::mpsc::channel(1000);
let seer = seer::Seer::new(config, tx);
seer.run().await?;
```

## Performance Considerations

### gRPC vs WebSocket

| Feature | gRPC | WebSocket |
|---------|------|-----------|
| **Latency** | <3ms (95th percentile) | <5ms |
| **Filtering** | Server-side | Client-side |
| **Network Usage** | Low (filtered) | High (all events) |
| **Reliability** | High | Medium |
| **Setup Complexity** | Medium | Low |
| **Recommended For** | Production | Development |

### Best Practices

1. **Use gRPC in production** for better performance and reliability
2. **Set reasonable backoff limits** to avoid overwhelming failed endpoints
3. **Monitor health logs** to detect issues early
4. **Use client IDs** to track multiple instances
5. **Implement monitoring** on reconnection counts and uptime metrics

## Metrics

The gRPC connection exposes Prometheus metrics:

- `seer_websocket_reconnections{status="grpc_success"}` - Successful connections
- `seer_websocket_reconnections{status="grpc_failed"}` - Failed connection attempts
- `seer_geyser_events_received{type="grpc_transaction"}` - Transaction events received
- `seer_geyser_events_received{type="grpc_slot"}` - Slot updates received
- `seer_geyser_events_received{type="grpc_account"}` - Account updates received

Access metrics at: `http://localhost:9090/metrics`

## Support

For issues or questions:
1. Check the logs with `SEER_VERBOSE=true`
2. Review this documentation
3. Contact your gRPC provider for endpoint-specific issues
4. Open an issue in the project repository
