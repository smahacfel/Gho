# gRPC Connection Logging Examples

This document shows real logging output examples from the Yellowstone gRPC connection.

## Successful Connection Flow

```
2024-11-16T18:45:56.123Z INFO seer::grpc_connection Initializing gRPC connection manager
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Endpoint: http://localhost:10000
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Client ID: seer-devnet-001
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Auth: enabled
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Max reconnect attempts: 10
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Initial reconnect delay: 5s
2024-11-16T18:45:56.124Z INFO seer::grpc_connection   Max reconnect delay: 300s

2024-11-16T18:45:56.125Z INFO seer::grpc_connection STATE TRANSITION: DISCONNECTED -> CONNECTING | Initial connection attempt | Reconnects: 0

2024-11-16T18:45:56.126Z INFO seer::grpc_connection Attempting to connect to gRPC endpoint: http://localhost:10000
2024-11-16T18:45:56.127Z INFO seer::grpc_connection Adding authentication header
2024-11-16T18:45:56.128Z INFO seer::grpc_connection Authentication configured (note: may need custom transport configuration)

2024-11-16T18:45:56.345Z INFO seer::grpc_connection STATE: Establishing gRPC connection...
2024-11-16T18:45:56.456Z INFO seer::grpc_connection STATE: gRPC connection established, preparing subscription...
2024-11-16T18:45:56.457Z INFO seer::grpc_connection STATE: Subscribing to event stream...
2024-11-16T18:45:56.678Z INFO seer::grpc_connection STATE: Successfully subscribed to Yellowstone gRPC stream

2024-11-16T18:45:56.679Z INFO seer::grpc_connection STATE TRANSITION: CONNECTING -> CONNECTED | Connection established successfully | Reconnects: 0
2024-11-16T18:45:56.679Z INFO seer::grpc_connection Event stream health: HEALTHY - receiving events

2024-11-16T18:45:56.680Z INFO seer Event stream started for endpoint: http://localhost:10000
```

## Connection with Periodic Health Checks

```
2024-11-16T18:46:26.680Z INFO seer Event stream health check: HEALTHY | Events received: 245 | Last event: 1s ago | Total reconnects: 0
2024-11-16T18:46:26.680Z INFO seer::grpc_connection Connection uptime: 0h 0m 30s

2024-11-16T18:46:56.681Z INFO seer Event stream health check: HEALTHY | Events received: 523 | Last event: 0s ago | Total reconnects: 0
2024-11-16T18:46:56.681Z INFO seer::grpc_connection Connection uptime: 0h 1m 0s

2024-11-16T18:47:26.682Z INFO seer Event stream health check: HEALTHY | Events received: 812 | Last event: 2s ago | Total reconnects: 0
2024-11-16T18:47:26.682Z INFO seer::grpc_connection Connection uptime: 0h 1m 30s
```

## Reconnection After Network Failure (Exponential Backoff)

```
2024-11-16T18:50:15.234Z ERROR seer::grpc_connection gRPC stream error: connection reset by peer
2024-11-16T18:50:15.235Z ERROR seer Event stream health: UNHEALTHY - stream error occurred
2024-11-16T18:50:15.235Z WARN seer gRPC stream ended
2024-11-16T18:50:15.235Z WARN seer Event stream statistics: 1245 events received

2024-11-16T18:50:15.236Z INFO seer Starting Seer event processing loop

2024-11-16T18:50:15.237Z INFO seer::grpc_connection STATE TRANSITION: CONNECTED -> RECONNECTING | Connection failed: Stream error (attempt 1/10) | Reconnects: 0
2024-11-16T18:50:15.237Z INFO seer::grpc_connection Connection uptime: 0h 3m 18s
2024-11-16T18:50:15.237Z WARN seer::grpc_connection Reconnect attempt 1/10 failed: Stream error. Waiting 5 seconds (exponential backoff)...

[5 second delay]

2024-11-16T18:50:20.238Z INFO seer::grpc_connection Attempting to connect to gRPC endpoint: http://localhost:10000
2024-11-16T18:50:20.450Z ERROR seer::grpc_connection Failed to establish gRPC connection: connection refused

2024-11-16T18:50:20.451Z INFO seer::grpc_connection STATE TRANSITION: RECONNECTING -> RECONNECTING | Connection failed: Failed to connect (attempt 2/10) | Reconnects: 1
2024-11-16T18:50:20.451Z INFO seer::grpc_connection Connection uptime: 0h 3m 23s
2024-11-16T18:50:20.451Z WARN seer::grpc_connection Reconnect attempt 2/10 failed: Failed to connect. Waiting 10 seconds (exponential backoff)...

[10 second delay]

2024-11-16T18:50:30.452Z INFO seer::grpc_connection Attempting to connect to gRPC endpoint: http://localhost:10000
2024-11-16T18:50:30.678Z ERROR seer::grpc_connection Failed to establish gRPC connection: connection refused

2024-11-16T18:50:30.679Z INFO seer::grpc_connection STATE TRANSITION: RECONNECTING -> RECONNECTING | Connection failed: Failed to connect (attempt 3/10) | Reconnects: 2
2024-11-16T18:50:30.679Z INFO seer::grpc_connection Connection uptime: 0h 3m 33s
2024-11-16T18:50:30.679Z WARN seer::grpc_connection Reconnect attempt 3/10 failed: Failed to connect. Waiting 20 seconds (exponential backoff)...

[20 second delay]

2024-11-16T18:50:50.680Z INFO seer::grpc_connection Attempting to connect to gRPC endpoint: http://localhost:10000
2024-11-16T18:50:50.890Z INFO seer::grpc_connection STATE: Establishing gRPC connection...
2024-11-16T18:50:51.012Z INFO seer::grpc_connection STATE: Successfully subscribed to Yellowstone gRPC stream

2024-11-16T18:50:51.013Z INFO seer::grpc_connection STATE TRANSITION: RECONNECTING -> CONNECTED | Connection established successfully | Reconnects: 2
2024-11-16T18:50:51.013Z INFO seer::grpc_connection Event stream health: HEALTHY - receiving events
2024-11-16T18:50:51.014Z INFO seer Event stream started for endpoint: http://localhost:10000
```

## Maximum Reconnection Attempts Exceeded

```
2024-11-16T19:00:00.000Z INFO seer::grpc_connection STATE TRANSITION: RECONNECTING -> RECONNECTING | Connection failed: Failed to connect (attempt 10/10) | Reconnects: 9
2024-11-16T19:00:00.000Z INFO seer::grpc_connection Connection uptime: 0h 15m 45s
2024-11-16T19:00:00.000Z WARN seer::grpc_connection Reconnect attempt 10/10 failed: Failed to connect. This was the final attempt.

2024-11-16T19:00:00.001Z INFO seer::grpc_connection STATE TRANSITION: RECONNECTING -> FAILED | Max reconnect attempts (10) exceeded | Reconnects: 9
2024-11-16T19:00:00.001Z ERROR seer::grpc_connection Failed to connect after 10 attempts: connection refused

2024-11-16T19:00:00.002Z ERROR seer Seer error: gRPC connection error: Failed to connect after 10 attempts. Restarting in 10 seconds...
```

## Verbose Mode Logging

When `SEER_VERBOSE=true` is set:

```
2024-11-16T18:45:56.680Z DEBUG seer::grpc_connection Subscription request: SubscribeRequest { accounts: {"amm_pools": SubscribeRequestFilterAccounts { ... }}, ... }
2024-11-16T18:45:57.123Z TRACE seer Received gRPC update (total: 1)
2024-11-16T18:45:57.234Z TRACE seer Received gRPC update (total: 2)
2024-11-16T18:45:57.345Z DEBUG seer::binary_parser Received transaction update
2024-11-16T18:45:57.346Z DEBUG seer::binary_parser Discriminator match for PumpFun
2024-11-16T18:45:57.347Z INFO seer Detected new pool: ABC123... on PumpFun (latency: 3.21ms)
```

## State Transition Summary

| From State | To State | Trigger | Example |
|------------|----------|---------|---------|
| DISCONNECTED | CONNECTING | Initial connection | Initial connection attempt |
| CONNECTING | CONNECTED | Successful subscription | Connection established successfully |
| CONNECTED | RECONNECTING | Stream error | Connection failed: Stream error |
| RECONNECTING | CONNECTED | Successful reconnection | Connection re-established |
| RECONNECTING | FAILED | Max attempts exceeded | Max reconnect attempts (10) exceeded |
| FAILED | CONNECTING | Manual restart | Restarting after failure |

## Backoff Progression Examples

### Fast Recovery (succeeds on attempt 2)
```
Attempt 1: Failed - Wait 5s
Attempt 2: Success! Total downtime: 5 seconds
```

### Moderate Recovery (succeeds on attempt 4)
```
Attempt 1: Failed - Wait 5s
Attempt 2: Failed - Wait 10s
Attempt 3: Failed - Wait 20s
Attempt 4: Success! Total downtime: 35 seconds
```

### Extended Outage (succeeds on attempt 8)
```
Attempt 1: Failed - Wait 5s
Attempt 2: Failed - Wait 10s
Attempt 3: Failed - Wait 20s
Attempt 4: Failed - Wait 40s
Attempt 5: Failed - Wait 80s
Attempt 6: Failed - Wait 160s
Attempt 7: Failed - Wait 300s (capped)
Attempt 8: Success! Total downtime: ~10 minutes
```

## Configuration Impact on Logging

### With Client ID
```
INFO seer::grpc_connection   Client ID: seer-prod-001
```

### With Authentication
```
INFO seer::grpc_connection   Auth: enabled
INFO seer::grpc_connection Adding authentication header
```

### Without Authentication
```
INFO seer::grpc_connection   Auth: disabled
```

## Metrics in Logs

The logs integrate with Prometheus metrics:
- `seer_websocket_reconnections{status="grpc_success"}` increments on connection success
- `seer_websocket_reconnections{status="grpc_failed"}` increments on each failure
- `seer_geyser_events_received{type="grpc_transaction"}` increments per transaction
- Event counts shown in health check logs

## Integration with Application Logs

```
2024-11-16T18:45:56.000Z INFO seer Seer - Real-time InitializePool Detector
2024-11-16T18:45:56.001Z INFO seer Version: 0.1.0
2024-11-16T18:45:56.002Z INFO seer Configuration loaded:
2024-11-16T18:45:56.003Z INFO seer   Connection mode: Grpc
2024-11-16T18:45:56.004Z INFO seer   Geyser endpoint: wss://api.mainnet-beta.solana.com
2024-11-16T18:45:56.005Z INFO seer   gRPC endpoint: http://localhost:10000
2024-11-16T18:45:56.006Z INFO seer   RPC endpoint: https://api.mainnet-beta.solana.com
2024-11-16T18:45:56.007Z INFO seer   Pump.fun enabled: true
2024-11-16T18:45:56.008Z INFO seer   Bonk.fun enabled: true

[gRPC connection logs appear here]

2024-11-16T18:45:56.680Z INFO seer Starting Seer module
2024-11-16T18:45:56.681Z INFO seer Connection mode: Grpc
2024-11-16T18:45:56.682Z INFO seer Pump.fun enabled: true
2024-11-16T18:45:56.683Z INFO seer Bonk.fun enabled: true
2024-11-16T18:45:56.684Z INFO seer Connecting via Yellowstone gRPC...
2024-11-16T18:45:56.685Z INFO seer Seer is now listening for InitializePool events...
```

## Log Filtering

To filter logs by component:

```bash
# All seer logs
RUST_LOG=seer=debug

# Only connection logs
RUST_LOG=seer::grpc_connection=debug

# Only state transitions (info level)
RUST_LOG=seer::grpc_connection=info

# Everything including traces
RUST_LOG=seer=trace
```
