# Seer - Real-time InitializePool Detector

Seer is the real-time pool detection component of the Ghost trading system. It monitors the Solana blockchain via Geyser/WebSocket connections to detect `InitializePool` events from Pump.fun and Bonk.fun AMM programs.

## Features

- **Real-time Detection**: Connects to Solana Geyser/WebSocket or PumpPortal for low-latency event streaming
- **Multiple Source Modes**: Supports gRPC (Yellowstone), Geyser WebSocket, Helius WebSocket, and PumpPortal WebSocket
- **Binary Parsing**: Analyzes raw transaction data to identify InitializePool instructions
- **Multi-AMM Support**: Detects pools from Pump.fun and Bonk.fun
- **Configurable Filters**: Filter by AMM type, quote mint, and liquidity thresholds
- **High Performance**: Target latency < 10ms, Land Rate ≥ 95%
- **Prometheus Metrics**: Comprehensive monitoring and SLA tracking
- **Auto-Reconnection**: Robust connection management with exponential backoff
- **PumpPortal Integration**: Direct real-time Pump.fun data ingestion via PumpPortal WebSocket

## Source Modes

Seer supports multiple event sources:

1. **GeyserGrpc** (Recommended): Yellowstone gRPC for production/HFT with mempool filtering
2. **GeyserWebSocket**: Legacy Geyser plugin via WebSocket
3. **HeliusWebSocket**: Standard Helius/Solana RPC WebSocket for testing
4. **PumpPortalWs** (New): PumpPortal WebSocket for direct Pump.fun data ingestion

See [PumpPortal WebSocket Mode](./PUMPPORTAL_WEBSOCKET_MODE.md) for details on the new mode.

## Architecture

```
Geyser WebSocket → Binary Parser → Filters → CandidatePool → Oracle
```

## Quick Start

### As Standalone Binary

```bash
# Build
cargo build --release

# Run with default config
cargo run --release

# Run with custom endpoint
SEER_GEYSER_ENDPOINT="wss://api.mainnet-beta.solana.com" cargo run --release
```

### Pump.fun Collector (NDJSON)

The `pumpfun_collector` binary emits NDJSON records to stdout and optionally appends to a file.
This is a scaffold for future tasks: it currently emits example records only and does not connect to Helius WSS/HTTP yet. `COMMITMENT` is parsed but not used at this stage.
Set `HELIUS_WSS_URL` (wss) and `HELIUS_HTTP_URL` (https) before running. Optional settings:

- `OUTPUT_PATH` (append NDJSON to a file)
- `WINDOW_SECS` (default 360)
- `COMMITMENT` (default confirmed)
- `MAX_RPC_CONCURRENCY` (default 4)
- `MAX_ACTIVE_MINTS` (default 10)
- `RPC_RPS_LIMIT` (default 9)

```bash
HELIUS_WSS_URL="wss://api.mainnet-beta.solana.com" \
HELIUS_HTTP_URL="https://api.mainnet-beta.solana.com" \
cargo run -p seer --bin pumpfun_collector
```

### As Library

```rust
use seer::{Seer, config::SeerConfig};
use tokio::sync::mpsc;

#[tokio::main]
async fn main() {
    let config = SeerConfig::default();
    let (tx, mut rx) = mpsc::channel(100);
    
    let seer = Seer::new(config, tx);
    
    // Run in background
    tokio::spawn(async move {
        seer.run().await.expect("Seer failed");
    });
    
    // Process candidates
    while let Some(candidate) = rx.recv().await {
        println!("New pool detected: {:?}", candidate);
    }
}
```

## Configuration

### Connection Modes

Seer supports three connection modes:
- **gRPC Mode** (Recommended): High-performance Yellowstone gRPC for real-time events
- **WebSocket Mode** (Legacy): Traditional WebSocket/Geyser connection
- **Helius WebSocket Mode** (Free Tier): Standard Solana RPC WebSocket with enhanced monitoring

See [GRPC_CONFIGURATION.md](./GRPC_CONFIGURATION.md) for detailed gRPC setup guide.
See [HELIUS_ADAPTER_IMPROVEMENTS.md](./HELIUS_ADAPTER_IMPROVEMENTS.md) for Helius adapter features and monitoring.

### Environment Variables

#### Connection Settings
- `SEER_CONNECTION_MODE`: Connection mode (`grpc` or `websocket`, default: `grpc`)
- `SEER_GRPC_ENDPOINT`: Yellowstone gRPC endpoint (default: `http://grpc.mainnet.solana.com:10000`)
- `SEER_GEYSER_ENDPOINT`: Geyser WebSocket endpoint (default: `wss://api.mainnet-beta.solana.com`)
- `SEER_RPC_ENDPOINT`: RPC endpoint for queries (default: `https://api.mainnet-beta.solana.com`)

#### gRPC Authentication
- `SEER_GRPC_CLIENT_ID`: Client identifier for this instance (optional)
- `SEER_GRPC_AUTH_TOKEN`: Authentication token for gRPC endpoint (optional)

#### Reconnection Configuration
- `SEER_MAX_RECONNECT_ATTEMPTS`: Maximum reconnection attempts (default: `10`)
- `SEER_RECONNECT_DELAY_SECS`: Initial reconnection delay (default: `5`)
- `SEER_MAX_RECONNECT_DELAY_SECS`: Maximum backoff delay cap (default: `300`)

#### Other Settings
- `SEER_VERBOSE`: Enable verbose logging (default: `false`)
- `SEER_METRICS_PORT`: Prometheus metrics port (default: `9090`)

### Programmatic Configuration

```rust
use seer::config::{SeerConfig, ConnectionMode, FilterConfig};

let config = SeerConfig {
    connection_mode: ConnectionMode::Grpc,
    grpc_endpoint: "http://localhost:10000".to_string(),
    grpc_client_id: Some("seer-001".to_string()),
    grpc_auth_token: Some("your-token".to_string()),
    geyser_endpoint: "wss://api.mainnet-beta.solana.com".to_string(),
    rpc_endpoint: "https://api.mainnet-beta.solana.com".to_string(),
    max_reconnect_attempts: 10,
    reconnect_delay_secs: 5,
    max_reconnect_delay_secs: 300,
    verbose: false,
    filter: FilterConfig {
        enable_pumpfun: true,
        enable_bonkfun: true,
        allowed_quote_mints: vec![
            "So11111111111111111111111111111111111111112".to_string(),  // SOL
        ],
        min_initial_liquidity_sol: Some(5.0),
    },
    channel_buffer_size: 1000,
    metrics_port: 9090,
};
```

## Metrics

Access Prometheus metrics at `http://localhost:9090/metrics`

### Key Metrics

- `seer_initialize_pool_detected_total`: Total pools detected
- `seer_initialize_pool_parsed_success_total`: Successfully parsed pools
- `seer_candidate_forwarded_to_oracle_total`: Candidates sent to Oracle
- `seer_latency_ms`: Processing latency histogram
- `seer_websocket_reconnections_total`: Connection health
- `seer_helius_land_rate_percent`: Helius adapter land rate (target ≥80%)
- `seer_helius_events_published_total`: Helius events successfully published
- `seer_helius_events_dropped_total`: Helius events dropped/filtered

### Land Rate Calculation

```promql
100 * (
    seer_initialize_pool_parsed_success_total{amm_program="pumpfun"}
    /
    seer_initialize_pool_detected_total{amm_program="pumpfun"}
)
```

**Target**: ≥ 95%

## Integration

### With Oracle

Seer forwards detected pools to Oracle for scoring:

```rust
// In Seer
let (candidate_tx, candidate_rx) = mpsc::channel(100);
let seer = Seer::new(config, candidate_tx);

// In Oracle
while let Some(candidate) = candidate_rx.recv().await {
    let scored = oracle.score_candidate(&candidate).await?;
    // Process scored candidate
}
```

### Data Flow

1. **Seer** detects `InitializePool` → produces `CandidatePool`
2. **Oracle** scores candidate → produces `ScoredCandidate`
3. **Features** evaluates strategy → produces `SwapPlan`
4. **DirectBuyBuilder Client** validates and executes → on-chain intent

## Development

### Running Tests

```bash
cargo test
```

### Running with Logs

```bash
RUST_LOG=seer=debug cargo run
```

### Building for Production

```bash
cargo build --release
```

## Performance Targets

- **Latency**: < 10ms from event to candidate
- **Land Rate**: ≥ 95% parse success
- **Throughput**: 100+ pools/hour during peak

## Supported AMMs

### Pump.fun
- Program ID: `6EF8rrecthR5Dkzon8Nwu78hRvfCKubJ14M5uBEwF6P`
- Quote: SOL

### Bonk.fun
- Program ID: `LanMV9sAd7wArD4vJFi2qDdfnVhFxYSUg6eADduJ3uj`
- Quote: BONK, USDC, SOL

## Documentation

- [Seer Overview](../../docs/SeerOverview.md) - Detailed architecture and design
- [Ghost Architecture](../../ARCHITECTURE.md) - Overall system architecture
- [AMM Integrations](../../Ghost_AMM_Integrations.md) - AMM-specific details

## License

See project root LICENSE file.
