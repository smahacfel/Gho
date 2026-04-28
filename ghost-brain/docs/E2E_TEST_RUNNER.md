# E2E Test Runner - Usage Guide

## Overview

The E2E Test Runner is a comprehensive testing tool that validates the complete Ghost pipeline from pool detection to on-chain execution. It supports multiple test scenarios and generates detailed reports.

## Test Scenarios

### Scenario A: Single Synthetic Pool
Quick validation test using a single simulated pool.
- **Duration**: ~1-2 seconds
- **Purpose**: Validate basic pipeline functionality
- **Use Case**: Quick smoke test, CI/CD validation

### Scenario B: Burst Test
Stress test with multiple synthetic pools in rapid succession.
- **Duration**: Configurable (default 60 seconds)
- **Purpose**: Load testing, performance validation
- **Use Case**: Validate system under high throughput

### Scenario E2E Full: Real Yellowstone Detection
Complete end-to-end test with real mempool detection.
- **Duration**: Configurable (default 5 minutes)
- **Purpose**: Production-like testing with real data
- **Use Case**: Pre-deployment validation, SLA verification

## Installation & Setup

### Prerequisites

1. **Rust toolchain** (1.72 or later)
2. **Solana CLI** (1.17.6 or later)
3. **Devnet keypair** with SOL balance
4. **Environment configuration**

### Setup Steps

```bash
# 1. Navigate to repository root
cd /path/to/ProjectSolanaGhost

# 2. Copy environment configuration
cp .env.devnet.example .env.devnet

# 3. Edit configuration (set RPC endpoints, keypair paths, etc.)
nano .env.devnet

# 4. Ensure keypair has SOL (for devnet)
solana airdrop 2 -k ~/.config/solana/devnet-ghost.json --url devnet

# 5. Build the test runner
cargo build --package ghost-e2e --bin e2e-test-runner --release
```

## Usage

### Basic Usage

```bash
# Run default scenario (E2E Full) with default settings
cargo run --package ghost-e2e --bin e2e-test-runner --release

# Run specific scenario
cargo run --package ghost-e2e --bin e2e-test-runner --release -- --scenario a
cargo run --package ghost-e2e --bin e2e-test-runner --release -- --scenario b
cargo run --package ghost-e2e --bin e2e-test-runner --release -- --scenario e2e-full
```

### Advanced Options

```bash
# E2E Full with custom wait time and pool limit
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --scenario e2e-full \
  --max-wait 600 \
  --max-pools 10

# Burst test with custom parameters
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --scenario b \
  --max-pools 20 \
  --duration 120

# Specify output file
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --output my-test-results.md

# Append to existing report
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --append

# Enable verbose logging
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --verbose
```

### Command Line Arguments

| Argument | Description | Default | Example |
|----------|-------------|---------|---------|
| `--scenario` | Test scenario to run (a, b, e2e-full) | `e2e-full` | `--scenario a` |
| `--max-wait` | Max wait time for pool detection (seconds) | `300` | `--max-wait 600` |
| `--max-pools` | Maximum pools to process | `5` | `--max-pools 10` |
| `--duration` | Duration for scenario B (seconds) | `60` | `--duration 120` |
| `--output` | Output report file path | `docs/testing/E2E_Results.md` | `--output report.md` |
| `--append` | Append to existing report | `false` | `--append` |
| `--verbose` | Enable verbose logging | `false` | `--verbose` |

## Configuration

### Environment Variables

The test runner uses the same configuration as the main E2E pipeline. Key variables:

```bash
# Solana Network
RPC_URL_DEVNET=https://api.devnet.solana.com
WEBSOCKET_URL_DEVNET=wss://api.devnet.solana.com
DIRECT_BUY_PROGRAM_ID=Ho1oGRam11111111111111111111111111111111111

# Keypairs
AUTHORITY_KEYPAIR_PATH=~/.config/solana/id.json
PAYER_KEYPAIR_PATH=~/.config/solana/id.json

# Seer Configuration
SEER_ENABLE_PUMPFUN=true
SEER_ENABLE_BONKFUN=true
SEER_CONNECTION_MODE=grpc
SEER_GRPC_ENDPOINT=http://grpc.mainnet.solana.com:10000

# Oracle Configuration
ORACLE_MIN_SCORE_THRESHOLD=70

# Trigger Configuration
TRIGGER_REDUNDANCY_FACTOR=3
TRIGGER_MAX_SPAN_SLOTS=4
TRIGGER_ENABLE_JITO=false
TRIGGER_DRY_RUN=true  # Set to false for live transactions

# Metrics Targets
METRICS_TARGET_LAND_RATE=95.0
METRICS_TARGET_INCLUSION_RATE=92.0
```

See `.env.devnet.example` for complete configuration options.

## Test Report

### Report Format

The test runner generates a markdown report with the following sections:

1. **Header**: Test date, scenario name, pass/fail status
2. **Configuration**: All configuration parameters used
3. **Metrics**: Land Rate and Inclusion Rate vs targets
4. **Latency Breakdown**: Component-wise latency measurements
5. **Observations**: Per-pool observations and notes
6. **Conclusion**: Summary and recommendations

### Example Report

```markdown
---

## Test Run: 2024-01-15 14:30:00 UTC

**Scenario**: Scenario E2E Full: Yellowstone→Jito→DirectBuy→On-chain

**Status**: ✓ PASSED

### Configuration

| Parameter | Value |
|-----------|-------|
| RPC URL | `https://api.devnet.solana.com` |
| Redundancy Factor | N+3 |
| Jito Enabled | false |
| Dry Run | true |

### Metrics

| Metric | Target | Actual | Status |
|--------|--------|--------|--------|
| **Land Rate** | ≥ 95.0% | 96.50% | ✓ |
| **Inclusion Rate** | ≥ 92.0% | 93.20% | ✓ |

### Latency Breakdown

| Component | Latency (ms) | Notes |
|-----------|--------------|-------|
| Oracle Scoring | 45.23 ms | Time to score candidate |
| Trigger Send | 12.45 ms | Time to construct and send TX |
| Trigger Confirm | 1523.67 ms | Time from send to confirmation |
| **E2E Total** | **1581.35 ms** | Detection to confirmation |

### Conclusion

✓ **Test PASSED** - All SLA targets were met...

---
```

## Interpreting Results

### Success Criteria

A test **PASSES** if:
- ✓ Land Rate ≥ 95.0%
- ✓ Inclusion Rate ≥ 92.0%

### Common Issues

#### Low Land Rate
- **Symptom**: Land Rate < 95%
- **Causes**:
  - Seer parsing errors
  - Incorrect AMM program IDs
  - Network connectivity issues
- **Solutions**:
  - Check Seer logs for parsing errors
  - Verify WebSocket/gRPC endpoints
  - Enable verbose logging

#### Low Inclusion Rate
- **Symptom**: Inclusion Rate < 92%
- **Causes**:
  - Insufficient redundancy
  - Poor leader selection
  - Network congestion
- **Solutions**:
  - Increase `TRIGGER_REDUNDANCY_FACTOR`
  - Use dedicated RPC endpoints
  - Enable Jito bundle submission

#### High Latency
- **Symptom**: E2E latency > 3 seconds
- **Causes**:
  - Slow RPC endpoints
  - Oracle data fetching delays
  - Network issues
- **Solutions**:
  - Use premium RPC providers
  - Optimize Oracle scoring logic
  - Check network connectivity

## CI/CD Integration

### GitHub Actions Example

```yaml
name: E2E Tests

on:
  push:
    branches: [ main, develop ]
  pull_request:

jobs:
  e2e-test:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v3
      
      - name: Install Rust
        uses: actions-rs/toolchain@v1
        with:
          toolchain: stable
      
      - name: Setup environment
        run: |
          cp .env.devnet.example .env.devnet
          # Configure with CI-specific values
      
      - name: Run E2E Test
        run: |
          cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
            --scenario a \
            --output ci-test-report.md
      
      - name: Upload Report
        uses: actions/upload-artifact@v3
        with:
          name: e2e-test-report
          path: ci-test-report.md
```

## Best Practices

### Before Running Tests

1. ✓ Verify configuration in `.env.devnet`
2. ✓ Ensure keypairs have sufficient SOL balance
3. ✓ Test RPC/WebSocket endpoints are accessible
4. ✓ DirectBuyBuilder is deployed on devnet

### During Tests

1. Monitor logs for errors or warnings
2. Check network connectivity if timeouts occur
3. Use `--verbose` flag for detailed debugging

### After Tests

1. Review generated report thoroughly
2. Compare metrics against SLA targets
3. Document any anomalies or observations
4. Archive reports for historical comparison

## Troubleshooting

### Test Hangs or Timeouts

```bash
# Check if RPC endpoint is responsive
curl https://api.devnet.solana.com -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","id":1,"method":"getHealth"}'

# Verify WebSocket connectivity
wscat -c wss://api.devnet.solana.com

# Use shorter timeout for debugging
cargo run --package ghost-e2e --bin e2e-test-runner --release -- \
  --scenario e2e-full --max-wait 60
```

### Configuration Errors

```bash
# Verify environment is loaded
dotenv -f .env.devnet env | grep RPC_URL

# Check keypair exists and is valid
solana-keygen verify ~/.config/solana/devnet-ghost.json

# Validate DirectBuyBuilder ID
solana program show <DIRECT_BUY_PROGRAM_ID> --url devnet
```

### Build Errors

```bash
# Clean build artifacts
cargo clean

# Update dependencies
cargo update

# Rebuild from scratch
cargo build --package ghost-e2e --release
```

## Support

For issues or questions:
- Check existing GitHub issues
- Review logs with `--verbose` flag
- Consult main README.md
- Contact: support@ghostsolana.com

## License

See repository root LICENSE file.
