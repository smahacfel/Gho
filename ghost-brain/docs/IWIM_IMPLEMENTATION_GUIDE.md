# IWIM (Initial Wallet Intent Mapping) - Implementation Guide

## Overview

IWIM is an ultra-fast dev-wallet behavioral analysis system that operates within the critical 0-2 second window after token initialization. It detects creator intentions (SCAMMER vs BUILDER vs SYBIL-BOT) before any traditional rug signals appear.

**Performance Target**: <120μs per analysis
**Design Principles**: Zero-heap allocation, RPC-free, history-free

## Architecture

IWIM performs three-layer analysis:

```
┌────────────────────────────────────────────────────────────┐
│                    IWIM ANALYSIS PIPELINE                   │
└────────────────────────────────────────────────────────────┘
                            │
                            ▼
        ┌───────────────────────────────────────┐
        │  Lightning CTP                        │
        │  (Creator Temporal Pattern)           │
        │  • Burst/quiet detection              │
        │  • Authority chain analysis           │
        │  • Transaction density                │
        └───────────────┬───────────────────────┘
                        │
                        ▼
        ┌───────────────────────────────────────┐
        │  CMM                                  │
        │  (Creator Micro-Movement Model)       │
        │  • IAPP (token account spam)          │
        │  • AT (authority twitch)              │
        │  • CMS (creator micro-sweep)          │
        └───────────────┬───────────────────────┘
                        │
                        ▼
        ┌───────────────────────────────────────┐
        │  CDIS                                 │
        │  (Creator-Delta Intent Signature)     │
        │  • Aggregate scoring                  │
        │  • Behavioral fingerprinting          │
        │  • Weighted delta calculation         │
        └───────────────┬───────────────────────┘
                        │
                        ▼
        ┌───────────────────────────────────────┐
        │  IwimResult                           │
        │  • organic_score: f32                 │
        │  • sybil_score: f32                   │
        │  • rug_threat_score: f32              │
        │  • confidence: f32                    │
        └───────────────────────────────────────┘
```

## Component Details

### 1. Lightning CTP (Creator Temporal Pattern)

Analyzes creator timing behavior in the mint slot.

**Key Signals:**
- **Burst Detection**: ≥3 setup transactions in rapid succession
  - `CreateAccount`, `InitializeMint`, `InitializeMetadata`, `CreateTokenAccount`
  - Indicates semi-automatic rug-bot behavior
  
- **Quiet Detection**: 5-10 seconds of zero activity before mint
  - Indicates organic human creator
  - Absence suggests bot/script preparation
  
- **Authority Chain**: Multi-hop authority transfer (A→B→C→D)
  - Depth ≥3 indicates sybil network
  - Fresh wallets with minimal SOL = high suspicion

**Output**: `CtpSignal`
```rust
pub struct CtpSignal {
    pub burst_detected: bool,
    pub quiet_detected: bool,
    pub authority_chain_suspicious: bool,
    pub authority_chain_depth: usize,
    pub tx_density: f32,
    pub confidence: f32,
}
```

### 2. CMM (Creator Micro-Movement Model)

Analyzes creator wallet movements in first 2 seconds.

**Key Patterns:**

#### IAPP (Immediate SPL Account Pre-Provisioning)
- Creator opens multiple token accounts within 1 second
- **Threshold**: ≥2 accounts → 97% rug probability
- Scammers pre-provision accounts for dump coordination

#### AT (Authority Twitch)
- Authority change within 0.5-1.5s of pool initialization
- Indicates honeypot/anti-sell mechanism setup
- Typical of delayed-rug scripts

#### CMS (Creator Micro-Sweep)
- Premature claiming/swapping before organic buys
- Most compromising signal
- Creator moves tokens/SOL before market formation

**Output**: `CmmSignal`
```rust
pub struct CmmSignal {
    pub iapp_count: usize,
    pub authority_twitch: bool,
    pub creator_sweep: bool,
    pub first_auth_change_ms: Option<u64>,
    pub first_sweep_ms: Option<u64>,
    pub confidence: f32,
}
```

### 3. CDIS (Creator-Delta Intent Signature)

Aggregates all signals into composite behavioral score.

**Delta Tracking:**
- SOL balance delta (outflow = suspicious)
- Token account count delta
- Authority change count
- Behavioral fingerprint (64-bit hash)

**Weighted Formula:**
```
CDIS = 0.25 * SOL_delta
     + 0.20 * accounts_delta
     + 0.15 * auth_delta
     + 0.15 * IAPP
     + 0.15 * AT
     + 0.10 * CMS
```

**Output**: `CdisSignal`
```rust
pub struct CdisSignal {
    pub sol_delta: i64,
    pub accounts_delta: i32,
    pub auth_changes: usize,
    pub composite_score: f32,
    pub fingerprint: u64,
    pub confidence: f32,
}
```

## API Usage

### Basic Analysis

```rust
use ghost_e2e::oracle::ultrafast::iwim::{iwim_analyze, IwimInput};

// Creator transaction sequence from first 0-2s window
let input = IwimInput {
    creator_pubkey: creator_wallet,
    init_slot: 12345,
    transactions: tx_sequence,
    time_window_ms: 2000,
    init_timestamp_ms: Some(1000000),
};

// Ultra-fast classification (<120μs)
let result = iwim_analyze(&input)?;

// Interpret results
match result {
    _ if result.rug_threat_score > 0.8 => {
        println!("HIGH RUG RISK: {:.2}", result.rug_threat_score);
        // Block trade / alert user
    }
    _ if result.organic_score > 0.7 => {
        println!("ORGANIC CREATOR: {:.2}", result.organic_score);
        // Proceed with trade
    }
    _ if result.sybil_score > 0.6 => {
        println!("SYBIL NETWORK: {:.2}", result.sybil_score);
        // Additional scrutiny
    }
    _ => {
        println!("INCONCLUSIVE (confidence: {:.2})", result.confidence);
    }
}
```

### Integration with Oracle Pipeline

```rust
use ghost_e2e::oracle::ultrafast::{iwim_analyze, mpcf_infer};

// Step 1: Analyze creator intent
let iwim_result = iwim_analyze(&creator_input)?;

// Step 2: Analyze buyer actors
let buyer_inferences: Vec<_> = buyer_txs
    .iter()
    .map(|tx| mpcf_infer(&tx.data))
    .collect();

// Step 3: Combine signals for QASS
if iwim_result.rug_threat_score > 0.8 {
    // High rug threat - skip even if buyers look organic
    return Decision::Skip;
}

if iwim_result.organic_score > 0.7 
    && buyer_inferences.iter().any(|b| b.is_human()) {
    // Organic creator + human buyers = strong buy signal
    return Decision::Buy;
}
```

## Test Corpus

IWIM includes test generators for common patterns:

### 1. Organic Creator
```rust
vec![
    vec![0x01; 100], // InitializeMint
    vec![0x02; 120], // InitializeMetadata
    vec![0x03; 150], // InitializePool
]
```
**Expected**: High `organic_score`, low `rug_threat_score`

### 2. Rug Pull (IAPP Pattern)
```rust
vec![
    vec![0x01; 100], // InitializeMint
    vec![0x02; 120], // InitializeMetadata
    vec![0x03; 150], // InitializePool
    vec![0x04; 80],  // CreateTokenAccount #1
    vec![0x05; 80],  // CreateTokenAccount #2
    vec![0x06; 80],  // CreateTokenAccount #3
]
```
**Expected**: `rug_threat_score ≥ 0.97` (per spec)

### 3. Authority Twitch
```rust
vec![
    vec![0x01; 100], // InitializeMint
    vec![0x02; 120], // InitializeMetadata
    vec![0x03; 150], // InitializePool
    vec![0x07; 90],  // AuthorityChange (within 500ms)
]
```
**Expected**: `rug_threat_score ≥ 0.70`

### 4. Creator Micro-Sweep
```rust
vec![
    vec![0x01; 100], // InitializeMint
    vec![0x02; 120], // InitializeMetadata
    vec![0x03; 150], // InitializePool
    vec![0x08; 110], // TokenTransfer (premature)
]
```
**Expected**: `rug_threat_score ≥ 0.85`

### 5. Sybil Burst
```rust
vec![
    vec![0x01; 80],  // CreateAccount
    vec![0x02; 80],  // CreateAccount
    vec![0x03; 80],  // CreateAccount
    vec![0x04; 100], // InitializeMint
    vec![0x05; 120], // InitializeMetadata
    vec![0x06; 150], // InitializePool
]
```
**Expected**: High `sybil_score`, `burst_detected = true`

## Performance Characteristics

### Target Metrics
- **Single Analysis**: <120μs
- **Batch (100 txs)**: <1ms total
- **Memory**: Zero heap allocation (stack-only)
- **Thread Safety**: Full Send + Sync support

### Benchmark Results
Run benchmarks with:
```bash
cargo bench -p ghost-e2e --bench oracle_bench -- iwim
```

Expected results:
- `iwim_organic_creator`: ~50-100μs
- `iwim_rug_iapp_pattern`: ~50-100μs
- `iwim_sybil_burst_pattern`: ~50-100μs
- `iwim_batch_100_mixed`: ~5-10ms (50-100μs average)

## Implementation Status

### ✅ Completed
- [x] Core module structure
- [x] Type definitions (IwimResult, CtpSignal, CmmSignal, CdisSignal)
- [x] API function (iwim_analyze)
- [x] Basic CTP/CMM/CDIS analysis logic
- [x] Score synthesis algorithm
- [x] 21 unit tests
- [x] Test corpus generators
- [x] Performance benchmarks
- [x] Thread safety
- [x] Zero-heap design

### 🚧 TODO (Marked in Code)
- [ ] **Transaction Parser**: Implement `classify_transaction()` to parse real Solana transaction bytes
  - Currently returns `TxType::Unknown` placeholder
  - Need to parse instruction discriminators
  - Identify Pump.fun/SPL/System program calls
  
- [ ] **CTP Enhancement**: Full burst/quiet detection
  - Implement pre-mint quietness window tracking
  - Add real timestamp extraction from transactions
  - Improve authority chain traversal
  
- [ ] **CMM Enhancement**: Precise timing analysis
  - Implement 1-second window for IAPP counting
  - Sub-slot timing for AT detection
  - Real-time sweep detection
  
- [ ] **CDIS Enhancement**: Real delta tracking
  - Extract SOL balance changes from transaction data
  - Calculate actual account deltas
  - Implement sophisticated fingerprinting algorithm

### 📊 Future Enhancements
- [ ] Property-based testing with proptest
- [ ] Real mainnet transaction corpus
- [ ] Machine learning for score tuning
- [ ] Historical pattern database
- [ ] Advanced sybil clustering (Hamming distance)

## Integration Points

### Shadow Ledger
IWIM is designed to work alongside Shadow Ledger for zero-latency state access:
```rust
// IWIM analyzes creator → Shadow Ledger tracks market state
let iwim_result = iwim_analyze(&creator_input)?;
let market_snapshot = shadow_ledger.get_snapshots(&mint)?;

if iwim_result.rug_threat_score < 0.3 && market_snapshot.is_healthy() {
    // Safe to proceed
}
```

### MPCF Integration
IWIM (creator analysis) + MPCF (buyer analysis) = complete picture:
```rust
// Creator intent
let iwim = iwim_analyze(&creator_input)?;

// Buyer actors
let buyer_actors: Vec<_> = buyer_txs
    .iter()
    .map(|tx| mpcf_infer(&tx.data))
    .collect();

// Combined decision
let organic_creator = iwim.organic_score > 0.7;
let organic_buyers = buyer_actors.iter().filter(|a| a.is_human()).count() > 3;

if organic_creator && organic_buyers {
    // High confidence buy signal
}
```

### QASS/QOFSV Integration
IWIM results feed into quantum-style scoring:
```rust
use ghost_e2e::oracle::ultrafast::qass::QuantumAmplitudeScorer;

// Build IWIM wave
let iwim_wave = HeuristicWave {
    amplitude: iwim.organic_score as f64,
    phase: 0.0,
    frequency: 1.0,
    confidence: iwim.confidence as f64,
    name: "ψ_iwim".to_string(),
};

// Add to QASS
let mut scorer = QuantumAmplitudeScorer::new();
scorer.add_wave(iwim_wave);
// ... add other waves ...
let final_score = scorer.calculate_superposition();
```

## Design Philosophy

### Why Zero-History?
IWIM operates on **immediate behavioral patterns** only:
- No RPC calls = no latency
- No historical lookups = no database overhead
- Pure analysis of T+0 to T+2s window
- Enables sub-millisecond decision making

### Why Zero-Heap?
Hot-path performance requirements:
- Stack-only buffers (fixed-size arrays)
- No allocations in critical path
- Predictable memory footprint
- Cache-friendly data layout

### Why Intent-Based?
Traditional rug detection is reactive:
- Waits for volume drop
- Monitors holder count
- Tracks price action

IWIM is **proactive**:
- Detects intent in 0-2s window
- 99% of rugs show behavioral signals before execution
- Protects users before damage occurs

## Security Considerations

### False Positives
- Organic projects may occasionally trigger burst detection (multiple setup txs)
- Use confidence scores to filter uncertain cases
- Combine with MPCF buyer analysis for confirmation

### False Negatives
- Sophisticated rug-pulls may evade simple heuristics
- Always combine IWIM with other oracle signals
- Continuous improvement through pattern database

### Adversarial Resistance
- Behavioral signals are side effects of automation
- Hard to mask without manual interaction
- Multi-layer analysis (CTP+CMM+CDIS) increases evasion cost

## Metrics & Monitoring

Track IWIM performance in production:
```rust
// Execution time
if result.execution_time_us > 120 {
    metrics.increment("iwim.slow_analysis");
}

// Score distribution
metrics.histogram("iwim.organic_score", result.organic_score);
metrics.histogram("iwim.rug_threat_score", result.rug_threat_score);
metrics.histogram("iwim.sybil_score", result.sybil_score);

// Accuracy (if ground truth available)
if ground_truth.is_rug && result.rug_threat_score > 0.8 {
    metrics.increment("iwim.true_positive");
}
```

## References

- Specification: `HYPER PREDICTION.md` (IWIM section)
- MPCF Module: `ghost-e2e/src/oracle/ultrafast/mpcf.rs`
- Shadow Ledger: `ghost-core/src/shadow_ledger/`
- QASS: `ghost-e2e/src/oracle/ultrafast/qass.rs`

## Support

For issues or questions:
1. Check TODO comments in code for known limitations
2. Review test corpus for expected behavior
3. Run benchmarks to validate performance
4. Consult HYPER PREDICTION.md for theoretical background
