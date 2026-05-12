---
name: ghost-execution
description: "Ghost Brain PREDATOR architecture, Gatekeeper V2/V2.5 decision pipeline, PDD/DOW/TAS/APS module design, pump.fun selective sniper execution, SSOT contracts, JSONL decision logging, and all Ghost-specific system decisions. Use when working on any Ghost component, decision flow, config, or architectural trade-off."
allowed-tools: "Read, Edit, Grep, Bash"
---

# Ghost Execution — Project-Specific Architecture & Orchestration

Use this skill when the task involves:
- designing, modifying, or debugging any Ghost Brain component
- working on Gatekeeper V2, V2.5, or future versions
- implementing or tuning PDD, DOW, TAS, or APS modules
- working on the decision pipeline, confidence scoring, or verdict logic
- working on Seer (Yellowstone gRPC ingestion layer)
- working on ShadowLedger, WAL, or canonical state management
- working on DecisionLogger, JSONL schema, or SSOT contracts
- working on HyperPrediction Oracle or post-Gatekeeper systems
- making architectural decisions about Ghost's data flow or component boundaries
- diagnosing why pools are incorrectly accepted or rejected
- tuning entry drift limits, confidence thresholds, or window timing
- implementing new GatekeeperMode variants or pipeline extensions

## Identity and Operating Model

Ghost Brain PREDATOR is a Rust-based autonomous selective sniper targeting pump.fun pool launches on Solana. It is **not** an HFT system. It is **not** a MEV bot. It is a precision filtering and entry system operating within a hard 10-second observation window from pool creation.

The agent must internalize the core design philosophy:

> Ghost does not predict the future. Ghost eliminates obvious traps and enters early enough that profit is real.

This distinction is **non-negotiable**. Every architectural decision must be evaluated against it.

The system operates as a two-mesh sieve:
- **Coarse mesh (PDD):** eliminates obvious pump & dump traps — facts, not estimates. Hard veto.
- **Fine mesh (TAS + APS + V2 core):** passes only pools with upward trajectory and adaptive market fit.

## System Architecture

### Component Map

```
Yellowstone gRPC Stream
        │
        ▼
     [Seer]                  — gRPC Yellowstone ingestion, event normalization
        │
        ▼
  [GatekeeperBuffer]         — transaction ingestion, tracking, feature accumulation
        │
        ▼
  [Gatekeeper V2.5]          — 4-module decision pipeline (PDD → Core → TAS → APS → DOW)
        │
        ├── REJECT_*          — classified rejection with reason code
        │
        └── BUY verdict       — Early (2-5s) / Normal (5-7s) / Extended (7-10s)
                │
                ▼
   [HyperPrediction Oracle]   — post-Gatekeeper prediction system (independent)
                │
                ▼
   [DecisionLogger]           — JSONL schema v16, SSOT audit trail
                │
                ▼
   [ShadowLedger / WAL]       — canonical state, position tracking, recovery
```

### Component Responsibilities

**Seer**
- Sole ingestion point for Yellowstone gRPC stream
- Normalizes raw events into canonical internal format
- No RPC calls on the ingestion path

**GatekeeperBuffer**
- Ingests transactions via `ingest_transaction_tracking_only()`
- Accumulates features for the observation window
- Feeds `MaterializedFeatureSet` as SSOT

**Gatekeeper V2.5**
- Four-module decision pipeline (see Decision Pipeline section)
- Operates in mode `v25` (new), `long` (legacy), or `standard` (legacy)
- Emits typed verdict with confidence, reason codes, and timing metadata

**HyperPrediction Oracle**
- Operates independently post-Gatekeeper
- Does NOT have feedback into Gatekeeper decision (V2.5 constraint — V3 target)

**DecisionLogger**
- JSONL format, schema v16
- Every decision must be fully auditable from logs alone
- New fields additive only — old fields never removed

**ShadowLedger / WAL**
- Canonical state for all active and closed positions
- Write-ahead log for crash recovery
- New enum variants added; existing serialization unchanged

## Gatekeeper V2.5 Decision Pipeline

### Module 0 — HARD FAILS CHECK (V2 legacy, unchanged)

Pre-filter on `MarketCapTooLow`, `SlowPool`, `ExtremeHhi`.
Failure → `REJECT_HARD_FAIL`

### Module 1 — PDD: Pump & Dump Detector (FIRST, HARD VETO)

PDD signals are **facts, not estimates**. PDD veto is absolute — no downstream module overrides it.

| Signal | Threshold | Detection Time | Certainty |
| :--- | :--- | :--- | :--- |
| Entry drift | > 5% vs initial pool price | immediate | high |
| Ramping pattern | 4 consecutive same-size buys | ~2–4s | high |
| Spike volume | volume rate 3s window vs rest | ~5–8s | medium |
| Whale concentration | top 3% holds > 60% | ~5–8s | medium |
| Reserve too small | below minimum health threshold | immediate | high |
| Flash crash risk | rapid price collapse pattern | ~3–6s | medium |

Failure → `REJECT_PUMP_AND_DUMP`

**Critical rule:** `max_price_change_ratio` must never be set to `9999.0` (effectively disabled). Entry drift is a hard, configured limit. Default: 5%. Shadow analysis threshold: 7%.

### Module 2 — CORE LAYER (V2 legacy, unchanged)

Core1 + Core2 + Core3 evaluation.
Failure → `REJECT_CORE_FAIL`

### Module 3 — SYBIL COMBO VETO + SOFT EXCESS (V2 legacy, unchanged)

Failure → `REJECT_SYBIL_SOFT_EXCESS` / `REJECT_SYBIL_INTERFERENCE` / `REJECT_SOFT_EXCESS`

### Module 4 — ALPHA GATE (V2 legacy, unchanged)

Failure → `REJECT_LOW_ALPHA`

### Module 5 — TAS: Trajectory Aware Scoring (SOFT MODULATOR)

TAS is **not a hard gate**. It modulates confidence, does not block independently — unless trajectory is extremely negative.

- Score < 0.30 → `REJECT_LOW_TRAJECTORY` (extreme negative only)
- Score >= 0.30 → modulates confidence ±25%
- TAS computed only when each segment has >= 3 TX; otherwise excluded from decision

TAS answers: is momentum **accelerating** (organic growth) or **decelerating** (fading pump)?

Point-in-time snapshots are insufficient. TAS requires trajectory analysis:
- HHI evolution across time segments
- Volume consistency (CV between segments)
- Inter-buy interval trend
- Spike vs uniform activity pattern

**Known limitation:** at < 3 TX per segment, TAS is too noisy to be reliable. Do not force computation on insufficient data.

### Module 6 — APS: Adaptive Prosperity (regime-aware thresholds)

APS replaces static Prosperity Filter branches with regime-adaptive thresholds.

Regime detection:
- `HighVol` — elevated market volatility
- `Normal` — baseline
- `LowVol` — suppressed activity

Default to `Normal` regime if sample < 30.

Failure → `REJECT_LOW_PROSPERITY`

### Module 7 — DECISION GATE: Dynamic Observation Window (DOW)

Three decision windows with distinct entry criteria:

| Window | Timing | Confidence Required | Additional Conditions |
| :--- | :--- | :--- | :--- |
| Early | 2–5s | >= 0.85 | All 6 phases pass, drift < 3%, momentum > 0.40 |
| Normal | 5–7s | >= 0.65 | Main path (~70% of BUYs) |
| Extended | 7–10s | >= 0.55 | PDD fully clean required |

**Golden window: 3–7s from pool creation.**

```
2–3s:   insufficient TX, high noise
3–7s:   ★ OPTIMAL ENTRY ★ — enough data, before typical dump window
8–15s:  typical dump window for scams
15s+:   too late — pump finished or dump in progress
```

Target: >= 85% of BUY decisions within golden window.
Timeout → `TIMEOUT` / `REJECT`

## What 10 Seconds Can and Cannot Do

This asymmetry is **fundamental** to Ghost's architecture. The agent must reason within it.

**10 seconds is sufficient for negative selection (trap detection):**
- Entry drift — fact, not estimate. Immediate, high certainty.
- Ramping — sequential pattern, low noise. 2–4s.
- Avg interval — primary survival signal (p=1.2e-90). 3–5s.
- Spike detection — requires window contrast. 5–8s.
- Whale concentration — noisy at low TX count. 5–8s.

**10 seconds is NOT sufficient for positive prediction:**
- Which pool will do 10x vs 2x — requires 30–60s+
- True community vs cabal — cabal simulates decentralization for 15–20s
- Graduation trajectory — minimum 30–60s needed
- HHI trajectory — extremely noisy at 5–10 TX per segment
- Volume consistency — single large TX generates false spike signal at small sample

**Architectural consequence:** Ghost eliminates obvious traps. Ghost does not predict winners. This is not a weakness — it is a deliberate design decision that determines everything downstream.

## Execution State Machine

Explicit states only. Implicit transitions are forbidden.

```
DETECTED
    → OBSERVING (GatekeeperBuffer accumulating)
        → HARD_FAIL_CHECK
            → REJECT_HARD_FAIL
        → PDD_CHECK
            → REJECT_PUMP_AND_DUMP
        → CORE_CHECK
            → REJECT_CORE_FAIL
        → SYBIL_CHECK
            → REJECT_SYBIL_* / REJECT_SOFT_EXCESS
        → ALPHA_CHECK
            → REJECT_LOW_ALPHA
        → TAS_MODULATION
            → REJECT_LOW_TRAJECTORY
        → APS_CHECK
            → REJECT_LOW_PROSPERITY
        → DECISION_GATE
            → EARLY_BUY (2–5s)
            → BUY (5–7s)
            → BUY (7–10s, extended)
            → TIMEOUT / REJECT
```

Every state transition logged with wall-clock timestamp and reason code.

## Verdict Types

The agent must use typed verdicts. Generic "transaction failed" is forbidden.

| Verdict | Source |
| :--- | :--- |
| `REJECT_HARD_FAIL` | Module 0 |
| `REJECT_PUMP_AND_DUMP` | PDD |
| `REJECT_CORE_FAIL` | Core Layer |
| `REJECT_SYBIL_INTERFERENCE` | Sybil Layer |
| `REJECT_SYBIL_SOFT_EXCESS` | Sybil + Soft Excess combo |
| `REJECT_SOFT_EXCESS` | Soft Excess |
| `REJECT_LOW_ALPHA` | Alpha Gate |
| `REJECT_LOW_TRAJECTORY` | TAS extreme negative |
| `REJECT_LOW_PROSPERITY` | APS |
| `EARLY_BUY` | DOW early window |
| `BUY` | DOW normal or extended window |
| `TIMEOUT` | Window expired without verdict |

## SSOT Contracts

These contracts must not be broken. Extensions are additive only.

| Contract | Rule |
| :--- | :--- |
| `MaterializedFeatureSet` | SSOT for all features. New fields optional only. Never remove existing fields. |
| `GatekeeperDecision` | New optional fields only. New enum variants added. Old variants unchanged. |
| `DecisionLogger` JSONL | Schema v16. New fields additive. Old fields preserved. |
| `GatekeeperV2Config` | Extended with sub-structs. All new fields `#[serde(default)]`. |
| Mode `long` / `standard` | Work unchanged. `v25` is the new path. |
| `ShadowLedger` / WAL | New enum variants added; existing serialization unchanged. |
| `GatekeeperBuyLog` | Old fields preserved, new fields added as optional. |
| `evaluate_curve_gate` | Untouched. |
| IWIM Veto Gate | Untouched. Operates after PDD check. |
| Sybil Interference | Untouched. Same pipeline position. |
| Existing tests | All must pass. New code in separate files. |

## File Map

| File | Change Type |
| :--- | :--- |
| `ghost-brain/ghost_brain_config.toml` | Add sections `[gatekeeper_v2.dow/tas/pdd/aps]`, set `mode = "v25"` |
| `ghost-brain/src/config/ghost_brain_config.rs` | Add fields `dow`, `tas`, `pdd`, `aps`; add `V25` to `GatekeeperMode` enum |
| `ghost-launcher/src/components/gatekeeper.rs` | Extend `GatekeeperAssessment`, `GatekeeperVerdictType`, `GatekeeperBuffer` |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | Add PDD and TAS layers, modify `evaluate_prosperity_filter()` |
| `ghost-launcher/src/oracle_runtime.rs` | Add `GatekeeperMode::V25` path with early evaluation |
| `ghost-brain/src/oracle/decision_logger.rs` | New JSONL fields, bump schema to v16 |
| `ghost-launcher/src/components/mod.rs` | Add `pub mod gatekeeper_pdd/trajectory/adaptive_prosperity` |

## Non-Negotiable Rules

1. **PDD is always first.** No module runs before PDD in the V2.5 pipeline. PDD veto is absolute.

2. **Entry drift is a hard configured limit.** `max_price_change_ratio = 9999.0` is a bug, not a feature. Default limit: 5%. Must be enforced.

3. **TAS is a soft modulator, not a hard gate.** TAS blocks only at score < 0.30. Otherwise it modulates confidence. Do not promote TAS to hard gate without explicit architectural decision.

4. **TAS requires >= 3 TX per segment.** If sample is insufficient, exclude TAS from decision. Do not compute on noise.

5. **Mode `long` and `standard` are untouched.** V2.5 runs exclusively under `mode = "v25"`. Legacy modes are fallback, not deprecated.

6. **All new config fields use `#[serde(default)]`.** Breaking existing deserialization is a critical regression.

7. **SSOT contracts are inviolable.** `MaterializedFeatureSet` is the single source of truth. No feature computed twice from different sources.

8. **Every verdict has a reason code.** `REJECT` without classification is forbidden.

9. **Extended window requires clean PDD.** BUY in 7–10s window is only valid when PDD passes completely. Do not relax this.

10. **HyperPrediction Oracle has no feedback into Gatekeeper.** These are separate systems. Coupling them is a V3 concern, not V2.5.

## Performance Targets

| Metric | V2 (current) | V2.5 (target) |
| :--- | :--- | :--- |
| Win rate | ~42% | >= 65% |
| Avg loss | -52.26% | < -15% |
| Avg profit | +50–100% | +50–110% |
| Time to decision (early) | N/A | 2–5s |
| Time to decision (avg) | 10s | 5–7s |
| BUYs in golden window (3–7s) | 0% | ~85% |
| Entry drift avg | +4.32% | < +2% |
| Worst loss | -79.87% | < -30% |

## Known Root Causes (V2 → V2.5)

| RC | Problem | V2.5 Fix |
| :--- | :--- | :--- |
| RC1 | Static single-point decision at 10s — best entry window missed | DOW — 3 dynamic windows, golden window targeting |
| RC2 | No pump & dump detection layer | PDD — hard veto on facts |
| RC3 | Entry drift blind spot (`max_price_change_ratio = 9999.0`) | PDD entry drift hard limit = 5% |
| RC4 | Prosperity Filter overfit on historical winners | APS — regime-adaptive thresholds |
| RC5 | No momentum trajectory analysis, only point-in-time snapshots | TAS — trajectory modulation |
| RC6 | No exit risk at entry, HyperPrediction Oracle decoupled | APS + TAS confidence integration |

## Risk Matrix

| Risk | Mitigation |
| :--- | :--- |
| PDD false positives (legitimate tokens rejected) | All PDD thresholds configurable; `mode = "long"` as fallback |
| TAS noise at small sample | TAS excluded if segment < 3 TX |
| Early entry in noise (2–5s) | Ultra-restrictive: 6/6 phases + drift < 3% + confidence >= 0.85 + momentum > 0.40 |
| Extended window entering dump zone (8–15s) | Extended only with full PDD pass + confidence >= 0.55 |
| Regime detection inaccurate at small sample | Default to Normal regime if sample < 30 |
| Conflict with existing tests | All new structures `#[serde(default)]`; new code in separate files |

## Handoff Rules

| When the task is primarily about... | Hand off to... |
| :--- | :--- |
| Rust ownership, lifetimes, error types, concurrency, unsafe | `rust-master` |
| Solana account model, PDAs, token programs, transaction construction | `solana-pumpfun-architect` |
| Signal validation, separability testing, calibration, statistical robustness | `statistical-research-engine` |
| Pattern discovery in pool data, feature engineering from raw data | `large-data-analytics` |
| Architecture trade-off analysis, novel problem decomposition | `abstract-reasoning` |
| Position sizing, risk limits, reconciliation logic, system-level integrity | `trading-systems` |

Ghost-specific execution decisions, pipeline structure, and component contracts stay in this skill.

## Failure Modes

The agent must detect and name these Ghost-specific failure modes:

- `max_price_change_ratio` set to `9999.0` or any value that effectively disables entry drift check
- TAS promoted to hard gate without explicit architectural decision
- TAS computed on segments with < 3 TX
- PDD not running first in V2.5 pipeline
- New config fields missing `#[serde(default)]`
- `MaterializedFeatureSet` bypassed — feature computed from secondary source
- Verdict emitted without reason code
- Extended window BUY without full PDD pass
- HyperPrediction Oracle coupled into Gatekeeper decision loop
- SSOT contract broken — existing field removed or renamed
- Legacy modes `long` / `standard` modified
- Existing tests broken by new code
- Regime detection forced on sample < 30 without defaulting to Normal

If a failure mode is detected, the agent must stop, name it explicitly, and correct course before proceeding.

## Code Review Checklist

Before finalizing any Ghost component change, verify:

- [ ] PDD runs first in V2.5 pipeline, before all other modules
- [ ] Entry drift has a hard configured limit, not `9999.0`
- [ ] TAS is soft modulator only — hard block only at score < 0.30
- [ ] TAS excluded from decision when any segment has < 3 TX
- [ ] All new config fields have `#[serde(default)]`
- [ ] `MaterializedFeatureSet` is the single source of truth for all features
- [ ] Every verdict has a typed reason code — no generic REJECT
- [ ] Extended window BUY requires full PDD pass
- [ ] Legacy modes `long` and `standard` unchanged
- [ ] All existing tests pass
- [ ] New modules in separate files per file map
- [ ] JSONL schema bumped to v16 if new fields added
- [ ] Handoff respected — Ghost skill owns pipeline contracts, not domain logic

## Output Expectations

When generating Ghost-specific code or architectural decisions, the agent must produce:

- typed verdict enums with all variants from the verdict table
- explicit pipeline stage annotations in code structure
- `#[serde(default)]` on all new config fields
- reason codes on every decision path
- wall-clock timestamps on all state transitions
- no static threshold values hardcoded — all from config
- structured tracing with `tracing` crate, not `println!`
- no placeholder logic in pipeline-critical paths
- no coupling between HyperPrediction Oracle and Gatekeeper

## Quick Start

When this skill is activated, begin with:

> [Ghost Execution] I will reason within Ghost's architecture: selective sniper, not HFT. PDD as hard veto on facts. TAS as soft modulator on trajectory. DOW targeting the golden window (3–7s). SSOT contracts inviolable. Mode v25 is the new path; legacy modes untouched.

Then proceed by:
1. identifying which Ghost component and pipeline stage the task touches,
2. verifying PDD runs first and its veto is absolute,
3. confirming TAS is soft, not hard, and has sufficient TX per segment,
4. checking all new config fields use `#[serde(default)]`,
5. ensuring SSOT contracts are preserved,
6. handing off domain-specific sub-problems to specialist skills.

Do not predict the future — eliminate the obvious traps.
Do not bypass PDD.
Do not hardcode thresholds.
Do not break existing contracts.
Do not couple HyperPrediction Oracle into Gatekeeper.