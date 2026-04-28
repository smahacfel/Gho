# IWIM Test Corpus & Property-Based Tests

## Overview

This document describes the comprehensive test corpus and property-based tests for the IWIM (Initial Wallet Intent Mapping) module. The tests validate creator behavioral classification, threat pattern detection, and performance requirements for the critical 0-2s window after token launch.

## Test Corpus Generators

The corpus module provides synthetic shadow-ledger snapshots representing realistic dev-wallet transaction sequences for three major threat categories:

### Scenario Types Covered

#### 1. Organic Developer Patterns

**`generate_organic_clean()`** - Clean token launch
- Minimal setup transactions (3-4 txs)
- No pre-mint burst activity
- No authority changes after launch
- No premature token movements
- **Expected IWIM scores:**
  - Organic: 0.7-0.9
  - Rug threat: 0.1-0.3
  - Sybil: 0.1-0.3

**`generate_organic_with_helper()`** - Single helper wallet
- Setup involves 1 helper account creation
- Legitimate authority transfer to multisig (after 5s)
- No IAPP, AT, or CMS patterns
- **Expected IWIM scores:**
  - Organic: 0.6-0.8
  - Rug threat: 0.2-0.4
  - Sybil: 0.2-0.4

**`generate_organic_methodical()`** - Methodical preparation
- Multiple setup steps with natural spacing
- Metadata updates and refinements
- Patient pre-launch preparation
- **Expected IWIM scores:**
  - Organic: 0.75-0.95
  - Rug threat: 0.05-0.2
  - Sybil: 0.1-0.25

#### 2. Rug Chain Patterns

**`generate_rug_high_iapp()`** - IAPP (Initial Account Pre-Positioning)
- 3-5 CreateTokenAccount transactions within 1000ms after pool init
- Classic pre-positioned rug setup
- **Per IWIM spec:** IAPP ≥ 2 → 97% rug probability
- **Expected IWIM scores:**
  - Organic: 0.05-0.2
  - Rug threat: 0.95-0.99
  - Sybil: 0.3-0.6

**`generate_rug_authority_twitch()`** - AT (Authority Twitch)
- Authority change within 500-1500ms after pool init
- Quick authority modification (honeypot setup)
- **Expected IWIM scores:**
  - Organic: 0.1-0.3
  - Rug threat: 0.6-0.85
  - Sybil: 0.2-0.4

**`generate_rug_creator_sweep()`** - CMS (Creator Micro-Sweep)
- Premature token transfer/swap within 2s of pool init
- Creator dumping before organic market formation
- **Expected IWIM scores:**
  - Organic: 0.05-0.2
  - Rug threat: 0.75-0.95
  - Sybil: 0.2-0.4

**`generate_rug_combo_iapp_cms()`** - Combo: IAPP + CMS
- High IAPP count + creator sweep
- Maximum rug probability
- **Expected IWIM scores:**
  - Organic: 0.0-0.1
  - Rug threat: 0.97-0.99
  - Sybil: 0.4-0.7

**`generate_rug_combo_at_cms()`** - Combo: AT + CMS
- Authority twitch + creator sweep
- Honeypot + immediate dump
- **Expected IWIM scores:**
  - Organic: 0.0-0.15
  - Rug threat: 0.85-0.98
  - Sybil: 0.3-0.5

#### 3. Sybil Network Patterns

**`generate_sybil_burst()`** - Burst pattern
- 5+ account creations in <500ms
- Highly automated setup
- Bot-like transaction density
- **Expected IWIM scores:**
  - Organic: 0.1-0.3
  - Rug threat: 0.3-0.6
  - Sybil: 0.7-0.95

**`generate_sybil_authority_chain()`** - Authority chain
- Multiple authority changes (A→B→C→D chain)
- Over-prepared wallet infrastructure
- 3+ authority hops
- **Expected IWIM scores:**
  - Organic: 0.05-0.2
  - Rug threat: 0.4-0.7
  - Sybil: 0.75-0.95

**`generate_sybil_coordinated()`** - Coordinated attack
- Burst + authority chain combo
- Maximum sybil indicators
- Sophisticated multi-wallet network
- **Expected IWIM scores:**
  - Organic: 0.0-0.15
  - Rug threat: 0.5-0.8
  - Sybil: 0.85-0.99

### Batch Generator

**`generate_corpus_batch()`** - Creates a realistic mix of 11 scenarios:
- 3 organic developers (clean, helper, methodical)
- 5 rug patterns (IAPP, AT, CMS, combo variants)
- 3 sybil networks (burst, chain, coordinated)

This simulates a realistic token launch environment with diverse creator behaviors.

## Running Tests

### All IWIM Tests

```bash
# Run all IWIM unit and property tests
cargo test -p ghost-e2e --lib ultrafast::iwim

# Run only corpus-based tests
cargo test -p ghost-e2e --lib ultrafast::iwim::tests::test_corpus

# Run only property tests
cargo test -p ghost-e2e --lib ultrafast::iwim::proptests
```

### Performance Benchmark (10k analyses)

```bash
# IMPORTANT: Run in release mode for accurate performance measurement
cargo test -p ghost-e2e --release bench_iwim_10k -- --ignored --nocapture

# Expected output:
# === IWIM 10k Benchmark ===
# Total analyses: 10000
# Average time per analysis: ~XX-XXX µs
# Target: <120 µs per analysis
# Status: ✓ PASS (release mode only)
```

### Property-Based Tests

The property-based tests use `proptest` to validate invariants across large input spaces:

1. **Score Bounds** - All scores always in [0.0, 1.0] range
2. **Performance** - Execution time always < 5ms safety margin
3. **Determinism** - Same input produces same output
4. **IAPP Threshold** - IAPP ≥ 2 always triggers 97% rug probability
5. **Confidence Floor** - Confidence never zero with valid data
6. **No Panics** - No panics on arbitrary transaction sequences

Run property tests with more cases:
```bash
# Default: 256 cases per property test
cargo test -p ghost-e2e --lib ultrafast::iwim::proptests

# More thorough: 10,000 cases per property test
PROPTEST_CASES=10000 cargo test -p ghost-e2e --lib ultrafast::iwim::proptests
```

## Test Coverage

### Unit Tests (30 existing + 11 new corpus tests = 41 total)

**Existing tests:**
- Basic validation tests (7)
- Signal default tests (3)
- Scoring logic tests (3)
- Simplified corpus tests (5)
- Advanced scenario tests (6)
- Performance tests (2)
- Utility tests (4)

**New corpus-specific tests:**
- `test_corpus_organic_clean` - Validates organic clean pattern detection
- `test_corpus_organic_with_helper` - Validates organic with helper wallet
- `test_corpus_rug_high_iapp` - Validates IAPP ≥ 2 → 97% rug threshold
- `test_corpus_rug_authority_twitch` - Validates AT pattern detection
- `test_corpus_rug_creator_sweep` - Validates CMS pattern detection
- `test_corpus_rug_combo_iapp_cms` - Validates combo rug patterns
- `test_corpus_sybil_burst` - Validates burst detection
- `test_corpus_sybil_authority_chain` - Validates authority chain detection
- `test_corpus_batch_processing` - Validates all 11 scenarios
- `test_corpus_scoring_determinism` - Validates deterministic behavior
- `test_corpus_performance_target_release` - Validates <120µs target

### Property-Based Tests (9 new)

**Core properties:**
- `prop_all_scores_in_valid_range` - Score bounds validation
- `prop_execution_time_reasonable` - Performance validation
- `prop_deterministic_behavior` - Determinism validation
- `prop_iapp_threshold_always_enforced` - IAPP spec enforcement
- `prop_confidence_never_zero_with_data` - Confidence floor
- `prop_no_panic_on_arbitrary_input` - Robustness validation

**Corpus-specific properties:**
- `prop_corpus_organic_always_low_rug` - Organic patterns validation
- `prop_corpus_rug_always_high_threat` - Rug patterns validation
- `prop_corpus_sybil_always_high_sybil` - Sybil patterns validation

## Performance Targets

### Per-Analysis Processing Time

| Mode    | Target        | Measurement                                      |
|---------|---------------|--------------------------------------------------|
| Debug   | <500 µs       | Acceptable for development/testing              |
| Release | **<120 µs**   | **Production requirement - must meet this target** |

### 10k Batch Performance

| Batch Size | Release Mode Target | Debug Mode Acceptable |
|------------|---------------------|----------------------|
| 10,000 analyses | <1.2 seconds total | <5 seconds total |

### Validation

The `test_corpus_performance_target_release` test automatically validates performance:
- In **debug mode**: Allows up to 500 µs per analysis
- In **release mode**: Enforces <120 µs requirement

## TODO: Future Enhancements

The following improvements are documented in the code with TODO comments:

### 1. Real Transaction Parsing (Priority: HIGH)

```
Location: Various corpus tests
TODO: Replace heuristic pattern matching with full Solana transaction parsing
      using solana-sdk for precise instruction data extraction.
```

**Implementation approach:**
- Use `solana_sdk::transaction::VersionedTransaction` for deserialization
- Extract real instruction discriminators and data
- Parse account metadata for precise SOL delta calculation
- Extract real timestamps from transaction metadata

### 2. Shadow Ledger Integration (Priority: HIGH)

```
Location: test_corpus_* tests
TODO: Integrate with real shadow-ledger snapshots for realistic testing
      with actual transaction sequences from mainnet.
```

**Implementation approach:**
- Capture 100+ real rug patterns from mainnet
- Capture 100+ real organic launches
- Store as test fixtures (base64-encoded transactions)
- Regression test IWIM accuracy on real-world data

### 3. Advanced Burst Detection (Priority: MEDIUM)

```
Location: analyze_ctp()
TODO: Implement precise burst detection with real transaction timestamps
      instead of estimated timing.
```

**Current:** Estimates timing based on transaction position
**Target:** Use actual block timestamps for <100ms precision

### 4. Fingerprint Clustering Analysis (Priority: MEDIUM)

```
Location: generate_fingerprint()
TODO: Implement fingerprint clustering to detect coordinated sybil attacks
      across multiple token launches.
```

**Implementation approach:**
- Track fingerprints across launches in shadow-ledger
- Calculate Hamming distance between fingerprints
- Cluster similar fingerprints (distance <20%)
- Flag clusters with >5 similar fingerprints as sybil networks

### 5. Threshold Calibration (Priority: LOW)

```
Location: synthesize_scores()
TODO: Fine-tune scoring weights and thresholds based on real-world
      false positive/negative rates from mainnet data.
```

**Current thresholds:**
- IAPP ≥ 2 → 97% rug probability (fixed per spec)
- Burst ≥ 3 setup txs in 500ms (estimated)
- Authority chain ≥ 3 changes (estimated)

**Target:** Data-driven calibration with 95% precision/recall

### 6. Edge Pattern Collection (Priority: LOW)

```
Location: Throughout test corpus
TODO: Add edge cases and corner patterns discovered in production
```

**Planned edge patterns:**
- Slow-rug patterns (2-5s delay before dump)
- Multi-phase rug chains
- Legitimate DAO launches with complex authority structures
- Cross-token sybil correlation

## Usage Examples

### Basic Classification Test

```rust
use ghost_e2e::oracle::ultrafast::iwim::{iwim_analyze, IwimInput};
use ghost_e2e::oracle::ultrafast::iwim::corpus::*;

#[test]
fn classify_organic_creator() {
    let txs = generate_organic_clean();
    let input = IwimInput {
        creator_pubkey: [1u8; 32],
        init_slot: 50000,
        time_window_ms: 2000,
        transactions: txs,
        init_timestamp_ms: Some(1000000),
    };
    
    let result = iwim_analyze(&input).unwrap();
    
    println!("Organic score: {:.2}", result.organic_score);
    println!("Sybil score: {:.2}", result.sybil_score);
    println!("Rug threat: {:.2}", result.rug_threat_score);
    println!("Confidence: {:.2}", result.confidence);
    println!("Execution time: {}µs", result.execution_time_us);
}
```

### Batch Processing Test

```rust
use ghost_e2e::oracle::ultrafast::iwim::{iwim_analyze, IwimInput};
use ghost_e2e::oracle::ultrafast::iwim::corpus::*;

#[test]
fn process_batch() {
    let corpus = generate_corpus_batch();
    
    for (scenario_name, txs) in corpus {
        let input = IwimInput {
            creator_pubkey: [1u8; 32],
            init_slot: 50000,
            time_window_ms: 2000,
            transactions: txs,
            init_timestamp_ms: Some(1000000),
        };
        
        let result = iwim_analyze(&input).unwrap();
        
        println!("{}: organic={:.2}, rug={:.2}, sybil={:.2}, confidence={:.2}",
            scenario_name,
            result.organic_score,
            result.rug_threat_score,
            result.sybil_score,
            result.confidence
        );
    }
}
```

### Performance Measurement

```rust
use ghost_e2e::oracle::ultrafast::iwim::{iwim_analyze, IwimInput};
use ghost_e2e::oracle::ultrafast::iwim::corpus::*;
use std::time::Instant;

#[test]
fn measure_performance() {
    let corpus = generate_corpus_batch();
    let iterations = 1000;
    
    let start = Instant::now();
    for _ in 0..iterations {
        for (_, txs) in &corpus {
            let input = IwimInput {
                creator_pubkey: [1u8; 32],
                init_slot: 50000,
                time_window_ms: 2000,
                transactions: txs.clone(),
                init_timestamp_ms: Some(1000000),
            };
            
            let _ = iwim_analyze(&input).unwrap();
        }
    }
    let elapsed = start.elapsed();
    
    let total_analyses = corpus.len() * iterations;
    let avg_micros = elapsed.as_micros() / total_analyses as u128;
    
    println!("Processed {} analyses in {:?}", total_analyses, elapsed);
    println!("Average: {}µs per analysis", avg_micros);
    println!("Target: <120µs");
}
```

## Integration with CI/CD

### GitHub Actions Workflow

Add to `.github/workflows/test.yml`:
```yaml
- name: Run IWIM Tests
  run: |
    cargo test -p ghost-e2e --lib ultrafast::iwim
    
- name: Run IWIM Performance Benchmark
  run: |
    cargo test -p ghost-e2e --release bench_iwim_10k -- --ignored --nocapture
```

### Performance Regression Detection

Monitor the benchmark output and fail CI if performance degrades:
```bash
#!/bin/bash
OUTPUT=$(cargo test -p ghost-e2e --release bench_iwim_10k -- --ignored --nocapture 2>&1)
AVG_MICROS=$(echo "$OUTPUT" | grep "Average time per analysis:" | awk '{print $5}')

if (( $(echo "$AVG_MICROS > 120" | bc -l) )); then
    echo "❌ Performance regression: ${AVG_MICROS}µs exceeds 120µs target"
    exit 1
else
    echo "✅ Performance target met: ${AVG_MICROS}µs < 120µs"
fi
```

## Contribution Guidelines

When adding new threat patterns or test cases:

1. **Add generator function** in `corpus` module following existing naming convention
2. **Add unit test** validating expected IWIM scores
3. **Add to corpus batch** if it represents a common pattern
4. **Update this documentation** with expected score ranges
5. **Run full test suite** to ensure no regressions

## References

- IWIM Implementation: `ghost-e2e/src/oracle/ultrafast/iwim.rs`
- IWIM Specification: `IWIM_FINAL_SUMMARY.md`
- IWIM Integration Guide: `ghost-e2e/IWIM_IMPLEMENTATION_GUIDE.md`
- Property Testing: https://github.com/proptest-rs/proptest
- MPCF Test Corpus: `ghost-e2e/MPCF_TEST_CORPUS.md` (reference pattern)
