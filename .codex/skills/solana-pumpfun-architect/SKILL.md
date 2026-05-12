---
name: solana-pumpfun-architect
description: "Low-latency Solana selective runtime engineering for event-driven pump.fun trading systems: deterministic execution orchestration, transaction lifecycle management, Yellowstone/Geyser ingestion, replay-safe runtime state handling, and production-grade selective sniper infrastructure."
allowed-tools: "Read, Edit, Grep, Bash"
---

# Solana + pump.fun Architect

Use this skill when the task involves:

* low-latency Solana trading runtimes
* pump.fun selective sniper infrastructure
* transaction orchestration and execution reliability
* Yellowstone/Geyser ingestion pipelines
* event-driven runtime systems
* replay-safe state management
* deterministic execution behavior
* Solana transaction lifecycle handling
* compute-budget and fee optimization
* stale-state and retry handling
* runtime-safe off-chain orchestration

Optimized for:

* selective execution systems
* bounded observation-window runtimes
* event-driven Solana architectures
* asynchronous off-chain runtimes
* replay-safe sniper infrastructure

Not optimized for:

* generic dApps
* frontend/web3 UI work
* NFT systems
* generalized Anchor tutorials
* HFT/MEV systems

---

# Quick Start

When activated:

> Preserve runtime determinism, canonical state ownership, replay safety, execution freshness, and low-latency execution integrity under adversarial Solana conditions.

Preferred runtime flow:


→ Yellowstone/Geyser
→ normalize events
→ update canonical state
→ materialize runtime snapshot
→ selective evaluation
→ execution eligibility
→ construct transaction
→ submit
→ confirm
→ reconcile
→ persist


For deeper Solana execution, Yellowstone, pump.fun lifecycle, transaction validity, or failure-mode guidance, read `references.md`.

---

# Core Runtime Doctrine

Assume:

* stale data is normal
* duplicate delivery is normal
* RPC inconsistency is normal
* retries will happen
* partial failure is inevitable
* execution windows are short
* inclusion probability is adversarial
* simulation is not execution truth

Therefore:

* correctness > speed
* determinism > cleverness
* reconciliation > optimistic execution
* signal quality > execution frequency
* runtime integrity > architectural elegance

---

# Canonical State & SSOT Discipline

Every runtime must define:

* canonical state authority
* snapshot materialization boundary
* feature ownership
* replay semantics
* freshness semantics

Rules:

* decisions must operate on immutable snapshots
* execution must never read partially-mutated state
* mixed runtime authorities are forbidden
* runtime mutation during evaluation is forbidden
* feature recomputation during execution is forbidden

Preferred model:


→ event stream
→ canonical runtime state
→ materialized snapshot
→ evaluation
→ terminal verdict
→ execution


Avoid:

* implicit state mutation
* dual ownership
* hidden cache authority
* live-state scoring during execution
* execution-time feature recomputation

---

# Observation Window Semantics

Selective runtimes must explicitly model:

* observation start
* accumulation phase
* evaluation trigger
* terminal deadline
* timeout behavior
* post-verdict handling

Preferred lifecycle:

```text
OBSERVED
→ ACCUMULATING
→ EVALUATING
→ APPROVED / REJECTED / TIMED_OUT
→ SUBMITTED
→ CONFIRMED
→ RECONCILED
```

Rules:

* stale observation windows must terminate
* decisions outside valid windows are invalid
* verdicts must be explicit
* lifecycle transitions must be deterministic and logged

---

# Event Ordering & Replay Rules

The runtime must explicitly handle:

* duplicate events
* out-of-order delivery
* replay reconstruction
* queue lag
* monotonicity assumptions
* late-arriving updates

Rules:

* replay must reconstruct identical decisions
* ordering guarantees must be explicit
* duplicate suppression must be deterministic
* event-time normalization must be explicit
* timestamp-domain mixing is unsafe unless normalized

Preserve:

* replay parity
* deterministic ordering
* bounded queue ownership
* append-only evidence chains

---

# Solana Runtime Awareness

Always account for:

* blockhash expiration
* slot timing
* writable account contention
* account locking
* compute budget constraints
* priority fee competition
* confirmation latency
* provider inconsistency
* simulation vs inclusion divergence

Never assume:

* successful simulation implies inclusion
* provider state is globally consistent
* retries are harmless
* liquidity remains stable during retries

---

# Execution Discipline

All execution paths must explicitly model:

* FILTERED
* SCORED
* VALIDATED
* SCHEDULED
* SUBMITTED
* CONFIRMED
* FAILED
* RECONCILED

Rules:

* transitions must be deterministic
* transitions must be logged
* retries must be idempotent
* execution must never silently mutate state
* stale execution assumptions must be invalidated

Once execution begins:

* scoring must not modify the decision
* signal reevaluation must not mutate execution state
* only reconciliation may evaluate realized outcome

No mid-flight reinterpretation allowed.

---

# Transaction Validity Rules

Every execution must define:

* blockhash lifetime
* slot reference
* validity TTL
* retry expiration boundary
* invalidation condition

If validity window expires:

```text
invalidate
→ rebuild from fresh state
```

Never reuse stale execution assumptions.

---

# Hot-Path Runtime Rules

Protect hot paths aggressively.

Avoid:

* widening synchronization scope
* unnecessary allocations
* expensive cloning
* blocking async paths
* hidden recomputation
* cross-runtime contention
* unbounded retry loops

Preserve:

* deterministic ordering
* bounded memory behavior
* replay safety
* queue locality
* explicit ownership boundaries

Prefer:

* append-only evidence
* immutable snapshots
* bounded queues
* idempotent retry paths
* minimal synchronization surfaces

---

# pump.fun Runtime Model

Must distinguish between:

* token creation
* visibility
* tradability
* economically executable state
* liquidity transition
* migration state

Rules:

* hype is not signal
* visibility is not execution eligibility
* early lifecycle is high-noise and adversarial
* selective filtering is mandatory

---

# Failure Mode Discipline

Explicitly detect:

* stale execution
* blockhash expiration
* retry amplification
* duplicate execution
* account contention
* RPC divergence
* replay mismatch
* state drift
* execution drift vs intent
* reconciliation mismatch
* queue lag accumulation
* invalid lifecycle transitions

On detection:

* stop unsafe execution
* classify failure
* preserve evidence
* recommend correction strategy

---

# Runtime Error Classification

All failures must map to:

* data problem
* freshness problem
* timing problem
* blockhash problem
* account contention problem
* authority problem
* compute/fee problem
* provider/network problem
* parsing problem
* reconciliation problem
* invariant violation

No generic “execution failed” buckets allowed.

---

# FAST PATH RULE

If task is:

* localized
* single-module
* non-architectural
* non-runtime-critical

Then:

* avoid unnecessary decomposition
* avoid broad refactors
* preserve runtime invariants
* preserve latency-sensitive behavior
* modify minimally and safely

Do not trigger full-system redesign unnecessarily.

---

# Handoff Boundaries

Delegate instead of solving:

* statistical validation → `statistical-research-engine`
* signal mining → `large-data-analytics`
* runtime architecture → `trading-systems`
* low-level Rust optimization → `rust-master`
* ambiguity decomposition → `abstract-reasoning`

If unclear → stop and request clarification.

---

# Final Review Checklist

Before completion verify:

* canonical state ownership preserved
* replay semantics preserved
* observation-window semantics respected
* deterministic transitions preserved
* duplicate handling implemented
* stale-state handling implemented
* blockhash lifecycle handled correctly
* retries bounded and idempotent
* execution freshness validated
* reconciliation defined
* no hidden state mutation
* no widened synchronization scope
* no hot-path regressions introduced

---

# Final Principle

Selective execution under adversarial low-latency conditions where:

* correctness dominates speed
* replayability dominates convenience
* reconciliation dominates optimism
* deterministic runtime behavior dominates abstraction
* signal quality dominates execution frequency