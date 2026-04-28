# MPCF Test Corpus & Property-Based Tests

## Overview

This document describes the comprehensive test corpus and property-based tests for the MPCF (Micro-Payload Cognitive Fingerprint) module. The tests are designed to validate actor classification, entropy analysis, fingerprint uniqueness, and performance requirements.

## Fuzz Corpus Generators

The corpus module provides realistic transaction payload generators for all major actor types:

### Actor Types Covered

1. **Phantom Mobile** - `generate_phantom_mobile()`
   - High entropy (>5.0)
   - Large payload (800-1200 bytes)
   - Mobile SDK overhead and irregular spacing
   - Simulates organic mobile wallet users

2. **Phantom Desktop** - `generate_phantom_desktop()`
   - Moderate-high entropy (>4.5)
   - Medium payload (400-800 bytes)
   - Desktop SDK patterns with moderate spacing
   - Simulates organic desktop wallet users

3. **Sniper Scripts** - `generate_sniper_script()`
   - Low entropy (<4.0)
   - Small payload (200-400 bytes)
   - Tight instruction packing, minimal padding
   - Simulates automated sniping bots (gm, solsniper-like)

4. **MEV Arbitrage Bots** - `generate_mev_arb()`
   - Very low entropy (<3.5)
   - Minimal payload (150-300 bytes)
   - Extremely regular patterns
   - Simulates high-frequency MEV bots

5. **Liquidity Bots** - `generate_liquidity_bot()`
   - Moderate entropy (3.5-4.5)
   - Medium payload (300-500 bytes)
   - LP-specific instruction patterns
   - Simulates liquidity provision bots

6. **RPC Fillers** - `generate_rpc_filler()`
   - Low entropy (<4.0)
   - Standard payload (250-450 bytes)
   - RPC-generated consistent structure
   - Simulates automated market makers

7. **Sybil Bots** - `generate_sybil_bot(variant: u8)`
   - Low entropy (<4.0)
   - Consistent payload (200-350 bytes)
   - Nearly identical patterns across variants
   - Simulates coordinated multi-wallet attacks
   - Use different variants (0-255) to generate slight variations

### Batch Generator

**`generate_corpus_batch()`** - Creates a realistic mix of 15 transaction payloads:
- 4 human wallets (2 mobile, 2 desktop)
- 2 sniper scripts
- 2 MEV bots
- 1 liquidity bot
- 1 RPC filler
- 5 sybil bots (variants 0-4)

This simulates a realistic token launch environment with mixed actor types.

## Running Tests

### All MPCF Tests

```bash
# Run all MPCF unit and property tests
cargo test -p ghost-e2e --lib ultrafast::mpcf

# Run only corpus-based tests
cargo test -p ghost-e2e --lib ultrafast::mpcf::corpus

# Run only corpus property tests
cargo test -p ghost-e2e --lib ultrafast::mpcf::corpus_proptests
```

### Performance Benchmark (10k transactions)

```bash
# IMPORTANT: Run in release mode for accurate performance measurement
cargo test -p ghost-e2e --release bench_10k_performance -- --ignored --nocapture

# Expected output:
# === MPCF 10k Benchmark Results ===
# Total time: ~XXX ms
# Average time per tx: ~X-Y µs
# Target: <70 µs per tx
# Status: ✓ PASS
```

### Property-Based Tests

The property-based tests use `proptest` to validate invariants across large input spaces:

1. **Corpus Generators Validity** - All generators produce valid, classifiable payloads
2. **Human vs Bot Entropy Separation** - Human entropy consistently >1.0 higher than bot entropy
3. **Sybil Entropy Consistency** - Sybil variants have similar entropy (mass-produced pattern)
4. **Batch Processing Consistency** - Deterministic results across multiple runs
5. **Performance Linear Scaling** - Performance/tx remains constant regardless of batch size

Run property tests with more cases:
```bash
# Default: 256 cases per property test
cargo test -p ghost-e2e --lib ultrafast::mpcf::corpus_proptests

# More thorough: 10,000 cases per property test
PROPTEST_CASES=10000 cargo test -p ghost-e2e --lib ultrafast::mpcf::corpus_proptests
```

## Test Coverage

### Unit Tests (25 existing + 9 new corpus tests = 34 total)

**Corpus-specific tests:**
- `test_corpus_phantom_mobile_classification` - Validates mobile wallet detection
- `test_corpus_phantom_desktop_classification` - Validates desktop wallet detection
- `test_corpus_sniper_script_classification` - Validates sniper bot detection
- `test_corpus_mev_arb_classification` - Validates MEV bot detection
- `test_corpus_liquidity_bot_classification` - Validates liquidity bot detection
- `test_corpus_rpc_filler_classification` - Validates RPC filler detection
- `test_corpus_sybil_bot_fingerprint_similarity` - Validates sybil detection
- `test_corpus_batch_entropy_bounds` - Validates entropy within bounds [0.0, 8.1]
- `test_corpus_batch_fingerprint_uniqueness` - Validates fingerprint diversity
- `test_corpus_performance_target` - Validates <70 µs/tx in release mode

### Property-Based Tests (12 existing + 6 new = 18 total)

**New corpus property tests:**
- `prop_corpus_generators_valid` - All generators produce valid outputs
- `prop_human_higher_entropy_than_bots` - Entropy separation validation
- `prop_sybil_entropy_consistency` - Sybil pattern consistency
- `prop_batch_processing_consistency` - Deterministic behavior
- `prop_performance_linear_scaling` - O(n) performance scaling

## Performance Targets

### Per-Transaction Processing Time

| Mode    | Target        | Measurement                                      |
|---------|---------------|--------------------------------------------------|
| Debug   | <500 µs/tx    | Acceptable for development/testing              |
| Release | **<70 µs/tx** | **Production requirement - must meet this target** |

### 10k Batch Performance

| Batch Size | Release Mode Target | Debug Mode Acceptable |
|------------|---------------------|----------------------|
| 10,000 tx  | <700 ms total       | <5 seconds total     |

### Validation

The `test_corpus_performance_target` test automatically validates performance:
- In **debug mode**: Allows up to 500 µs/tx (not optimized)
- In **release mode**: Enforces <70 µs/tx requirement

## TODO: Future Enhancements

The following improvements are documented in the code with TODO comments:

### 1. Advanced Sybil Detection (Priority: HIGH)
```
Location: test_corpus_sybil_bot_fingerprint_similarity()
TODO: Implement advanced sybil detection based on fingerprint clustering
      Analyze hamming distance between fingerprints and flag when multiple
      transactions have suspiciously similar fingerprints.
```

**Implementation approach:**
- Calculate Hamming distance between all fingerprints in a batch
- Cluster fingerprints with distance <20% (tunable threshold)
- Flag clusters with >5 similar fingerprints as potential sybil networks
- Track fingerprint clusters over time for persistent sybil detection

### 2. Fingerprint Collision Detection (Priority: MEDIUM)
```
Location: test_corpus_batch_fingerprint_uniqueness()
TODO: Implement advanced collision detection and clustering analysis
      Track fingerprint collisions over time and identify patterns
      that indicate coordinated sybil attacks.
```

**Implementation approach:**
- Build historical database of fingerprints (ring buffer, 1M entries)
- Track collision rates per fingerprint
- Identify abnormal collision patterns (>0.1% collision rate)
- Cross-reference with actor type patterns

### 3. Tighter Sybil Thresholds (Priority: MEDIUM)
```
Location: prop_sybil_entropy_consistency()
TODO: Tighten this threshold after implementing advanced sybil detection
      Once we have fingerprint clustering, we can detect sybil networks
      more precisely.
```

**Current threshold:** Entropy variance <2.0 for sybil variants
**Target threshold:** Entropy variance <0.5 after clustering implementation

### 4. Real Transaction Corpus (Priority: LOW)
```
TODO: Add corpus of real transaction bytes from mainnet for validation
      against production patterns.
```

**Implementation approach:**
- Capture 1,000 real transactions from each actor type
- Store as base64-encoded strings in test data files
- Add regression tests to ensure MPCF accurately classifies real data
- Update corpus generators to match real-world patterns more closely

## Usage Examples

### Basic Classification Test
```rust
use ghost_e2e::oracle::ultrafast::mpcf::mpcf_infer;
use ghost_e2e::oracle::ultrafast::mpcf::corpus::*;

#[test]
fn classify_phantom_mobile() {
    let payload = generate_phantom_mobile();
    let result = mpcf_infer(&payload);
    
    println!("Actor: {:?}", result.actor);
    println!("Confidence: {}", result.confidence);
    println!("Entropy: {}", result.entropy);
    println!("Fingerprint: {:x?}", result.fingerprint);
}
```

### Batch Processing Test
```rust
use ghost_e2e::oracle::ultrafast::mpcf::mpcf_infer;
use ghost_e2e::oracle::ultrafast::mpcf::corpus::*;

#[test]
fn process_batch() {
    let corpus = generate_corpus_batch();
    
    for (expected_type, payload) in corpus {
        let result = mpcf_infer(&payload);
        println!("{:?}: entropy={:.2}, confidence={:.2}", 
                 result.actor, result.entropy, result.confidence);
    }
}
```

### Performance Measurement
```rust
use ghost_e2e::oracle::ultrafast::mpcf::mpcf_infer;
use ghost_e2e::oracle::ultrafast::mpcf::corpus::*;
use std::time::Instant;

#[test]
fn measure_performance() {
    let corpus = generate_corpus_batch();
    let iterations = 1000;
    
    let start = Instant::now();
    for _ in 0..iterations {
        for (_, payload) in &corpus {
            let _ = mpcf_infer(payload);
        }
    }
    let elapsed = start.elapsed();
    
    let total_txs = corpus.len() * iterations;
    let avg_micros = elapsed.as_micros() / total_txs as u128;
    
    println!("Processed {} transactions in {:?}", total_txs, elapsed);
    println!("Average: {} µs/tx", avg_micros);
}
```

## Integration with CI/CD

### GitHub Actions Workflow

Add to `.github/workflows/test.yml`:
```yaml
- name: Run MPCF Tests
  run: |
    cargo test -p ghost-e2e --lib ultrafast::mpcf
    
- name: Run MPCF Performance Benchmark
  run: |
    cargo test -p ghost-e2e --release bench_10k_performance -- --ignored --nocapture
```

### Performance Regression Detection

Monitor the benchmark output and fail CI if performance degrades:
```bash
#!/bin/bash
OUTPUT=$(cargo test -p ghost-e2e --release bench_10k_performance -- --ignored --nocapture 2>&1)
AVG_MICROS=$(echo "$OUTPUT" | grep "Average time per tx:" | awk '{print $5}')

if (( $(echo "$AVG_MICROS > 70" | bc -l) )); then
    echo "❌ Performance regression: ${AVG_MICROS}µs/tx exceeds 70µs target"
    exit 1
else
    echo "✅ Performance target met: ${AVG_MICROS}µs/tx"
fi
```

## Contribution Guidelines

When adding new actor types or test cases:

1. **Add generator function** in `corpus` module following existing patterns
2. **Add classification test** validating expected actor type and entropy range
3. **Add to corpus batch** if it represents a common actor type
4. **Update this documentation** with the new actor type
5. **Run full test suite** to ensure no regressions

## References

- MPCF Implementation: `ghost-e2e/src/oracle/ultrafast/mpcf.rs`
- MPCF Benchmarks: `ghost-e2e/benches/oracle_bench.rs`
- Property Testing: https://github.com/proptest-rs/proptest
- MPCF Design Doc: `HYPER PREDICTION.md` (lines 465-680)
