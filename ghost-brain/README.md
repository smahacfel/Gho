# Ghost E2E Pipeline

End-to-end integration pipeline for the Ghost trading system on Solana Devnet.

## Overview

This pipeline connects all Ghost components from pool detection to transaction execution:

```
┌─────────────────────────────────────────────────────────────────┐
│                     GHOST E2E PIPELINE                           │
└─────────────────────────────────────────────────────────────────┘

   Seer (Pool Detection)
         │
         ├─ Detects InitializePool events from Pump.fun/Bonk.fun
         ├─ Binary parsing of mempool/Geyser stream
         └─ Forwards CandidatePool →
                                      │
                                      ▼
   Oracle (Candidate Scoring)
         │
         ├─ Scores candidates (0-100)
         ├─ Assesses risk level
         └─ Forwards ScoredCandidate →
                                      │
                                      ▼
   Features (Strategy Selection)
         │
         ├─ Selects trading strategy
         ├─ Calculates position size
         ├─ Generates SwapPlan
         └─ Forwards SwapPlan →
                                      │
                                      ▼
   DirectBuyBuilder (On-chain Intent)
         │
         ├─ Registers intent on-chain (PDA)
         ├─ Validates parameters
         └─ Returns transaction →
                                      │
                                      ▼
   Trigger (Transaction Sending)
         │
         ├─ Builds minimal transaction (~180B)
         ├─ Sends with N+3 redundancy
         ├─ Tracks confirmation
         └─ Updates metrics
                                      │
                                      ▼
                               Included on Solana
```

## Metrics & SLA

The pipeline tracks two critical metrics:

### Land Rate (Seer)
- **Definition**: Percentage of detected pools successfully parsed
- **Formula**: `(parsed_success / detected_total) × 100`
- **Target**: ≥ 95%
- **Measured per AMM**: Pump.fun and Bonk.fun separately

### Inclusion Rate (Trigger)
- **Definition**: Percentage of sent transactions confirmed on-chain
- **Formula**: `(confirmed / sent) × 100`
- **Target**: ≥ 92%
- **Impact**: N+3 redundancy strategy

### Latency Metrics
- **Seer Latency**: Time from event detection to CandidatePool creation
- **Oracle Latency**: Time to score a candidate
- **Trigger Send Latency**: Time to send transaction
- **Trigger Confirm Latency**: Time from send to confirmation
- **End-to-End Latency**: Total time from detection to confirmation

## Prerequisites

### 1. Solana CLI & Keypairs

```bash
# Install Solana CLI
sh -c "$(curl -sSfL https://release.solana.com/stable/install)"

# Set Solana to devnet
solana config set --url devnet

# Generate a keypair (or use existing)
solana-keygen new -o ~/.config/solana/devnet-ghost.json

# Airdrop SOL for testing
solana airdrop 2 -k ~/.config/solana/devnet-ghost.json
```

### 2. Deploy DirectBuyBuilder Program (if not already deployed)

```bash
# Build the program
anchor build

# Deploy to devnet
anchor deploy --provider.cluster devnet

# Note the program ID and update .env.devnet
```

### 3. Configuration

```bash
# Copy example configuration
cp .env.devnet.example .env.devnet

# Edit configuration
nano .env.devnet
```

**Required Configuration**:
- `RPC_URL_DEVNET`: Devnet RPC endpoint
- `WEBSOCKET_URL_DEVNET`: Devnet WebSocket endpoint  
- `DIRECT_BUY_PROGRAM_ID`: Deployed DirectBuyBuilder ID
- `AUTHORITY_KEYPAIR_PATH`: Path to authority keypair
- `PAYER_KEYPAIR_PATH`: Path to payer keypair (can be same as authority)

See [.env.devnet.example](../.env.devnet.example) for all options.

## Running the Pipeline

### Build

```bash
# From repository root
cargo build --package ghost-e2e --release
```

### Run

```bash
# Run with default logging
cargo run --package ghost-e2e --release

# Run with debug logging
RUST_LOG=ghost_e2e=debug,seer=debug,trigger=debug cargo run --package ghost-e2e --release

# Run in background (production)
nohup cargo run --package ghost-e2e --release > ghost-e2e.log 2>&1 &
```

### Monitoring

The pipeline logs important events and metrics:

```
[INFO] Starting E2E Pipeline
[INFO] Authority: 7xKXtg2CW87...
[INFO] Payer: 7xKXtg2CW87...
[INFO] DirectBuyBuilder Program: Ho1oGRam11111...
[INFO] Starting Seer component
[INFO] Starting Oracle/Features component
[INFO] Starting DirectBuyBuilder/Trigger component
[INFO] E2E Pipeline is now running
[INFO] Detected new pool: 9xQeWvG816bUx9... on pumpfun (latency: 12.34ms)
[INFO] Oracle score: 85 (passed: true, risk: Medium)
[INFO] SwapPlan created: amount_in=7500000, min_amount_out=7125000000
[INFO] Intent initialized: signature=5FH...
[INFO] Land Rate - Pump.fun: 96.50%, Bonk.fun: 95.20%
[INFO] Inclusion Rate: 93.80%
```

## Metrics Export (Prometheus)

If `METRICS_ENABLE_PROMETHEUS=true`, metrics are exported on port 9090:

```bash
# Check metrics
curl http://localhost:9090/metrics

# Example metrics:
ghost_seer_pools_detected_total{amm_program="pumpfun"} 100
ghost_seer_pools_parsed_total{amm_program="pumpfun"} 97
ghost_land_rate_percent 97.0
ghost_trigger_txs_sent_total 50
ghost_trigger_txs_confirmed_total 46
ghost_inclusion_rate_percent 92.0
```

## Testing Scenarios

### Scenario 1: Single Pool Detection

1. Monitor logs for InitializePool detection
2. Verify Oracle scoring
3. Check SwapPlan generation
4. Confirm intent registration
5. Track transaction confirmation

### Scenario 2: High Volume Stress Test

1. Run during high activity periods
2. Monitor Land Rate under load
3. Track Inclusion Rate with multiple transactions
4. Measure latency distribution

### Scenario 3: SLA Validation

1. Run for extended period (1+ hours)
2. Calculate aggregate Land Rate
3. Calculate aggregate Inclusion Rate
4. Verify both meet SLA thresholds

## Troubleshooting

### Seer Not Detecting Pools

- Check WebSocket connection: `WEBSOCKET_URL_DEVNET` is correct
- Verify Pump.fun/Bonk.fun are enabled in config
- Check Geyser endpoint is responsive
- Enable verbose logging: `SEER_VERBOSE=true`

### Oracle Rejecting All Candidates

- Lower minimum score threshold: `ORACLE_MIN_SCORE_THRESHOLD`
- Check RPC endpoints are responsive
- Verify liquidity filters are not too strict

### Low Inclusion Rate

- Increase redundancy factor: `TRIGGER_REDUNDANCY_FACTOR`
- Check payer has sufficient SOL balance
- Verify RPC endpoint is not rate-limited
- Consider enabling Jito if on mainnet

### Configuration Errors

- Verify keypair files exist at specified paths
- Check DirectBuyBuilder ID is correct for devnet
- Ensure RPC/WebSocket URLs are for devnet

## Architecture

The E2E pipeline is organized into modules:

- **config.rs**: Configuration loading and validation
- **metrics.rs**: Prometheus metrics collection
- **oracle.rs**: Simplified Oracle interface for candidate scoring
- **strategy.rs**: Strategy selection and SwapPlan generation
- **pipeline.rs**: Main pipeline orchestration and component integration

## Development

### Running Tests

```bash
cargo test --package ghost-e2e
```

### Adding New Strategies

Edit `strategy.rs` and add new strategy logic to `StrategySelector::generate_swap_plan()`.

### Customizing Oracle Scoring

Edit `oracle.rs` and modify `SimpleOracle::calculate_simple_score()` or integrate with the full Oracle system.

## Production Deployment

For production use:

1. **Use dedicated RPC endpoints** (not public free tier)
2. **Enable Jito bundles** for MEV extraction
3. **Set up Prometheus + Grafana** for monitoring
4. **Configure alerts** for SLA violations
5. **Use separate authority and payer** keypairs
6. **Implement proper key management** (HSM, secrets manager)
7. **Set up log aggregation** (ELK, Datadog, etc.)
8. **Deploy redundant instances** for high availability

## License

See repository root LICENSE file.
