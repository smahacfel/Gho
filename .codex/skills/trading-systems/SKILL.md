---
name: trading-systems
description: "Deterministic selective trading runtime design for low-latency autonomous systems: scoring, decision policy, execution orchestration, risk enforcement, reconciliation, and replay-safe state management."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Trading Systems

Use this skill when the task involves:

* selective trading runtime architecture
* decision engines or scoring systems
* execution orchestration and retries
* risk systems and position sizing
* stateful event-driven trading pipelines
* reconciliation and post-trade integrity
* replay-safe runtime behavior
* bounded observation-window systems

Optimized for:

* selective execution systems
* event-driven runtimes
* asynchronous low-latency systems
* deterministic autonomous trading systems

Not optimized for:

* HFT market making
* MEV searchers
* discretionary GUI workflows
* generalized OMS systems

---

# Quick Start

When activated:

> Preserve deterministic runtime behavior, explicit state ownership, immutable decision snapshots, risk-first execution discipline, and replay-safe execution semantics.

Preferred runtime flow:

```text
ingest
→ normalize
→ update state
→ materialize snapshot
→ evaluate
→ risk gate
→ execute
→ confirm
→ reconcile
→ persist
````

For deeper architecture, risk, scoring, execution, replay, validation, or observability guidance, read `references.md`.

---

# Core Doctrine

* Selectivity over speed.
* Correctness over throughput.
* Risk management is the system boundary.
* Execution is incomplete until reconciliation finishes.
* Stale data, duplicates, and partial failures are normal conditions.
* Decisions must produce explicit reason codes.
* Runtime integrity is more important than elegant abstractions.
* Determinism is preferred over heuristic ambiguity.

---

# SSOT & Snapshot Discipline

Every system must define:

* canonical source of truth
* ownership of every feature/state field
* immutable decision snapshot boundary
* freshness semantics

Rules:

* decisions operate on immutable snapshots
* no feature recomputation during evaluation
* no mixed live-state reads during scoring
* no dual-authority feature derivation
* runtime mutation during evaluation is forbidden

Avoid:

* hidden recomputation
* mixed authorities
* implicit state mutation
* runtime-dependent scoring behavior

---

# Observation Window Semantics

Selective systems must explicitly model:

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
→ COMMITTED
→ RECONCILED
```

Rules:

* decisions outside valid observation windows are invalid
* stale windows must terminate
* terminal verdicts must be explicit
* lifecycle transitions must be deterministic and logged

---

# Event-Time & Freshness Rules

Always separate:

* wall-clock time
* event time
* chain time
* confirmation time
* processing time

Rules:

* stale-state execution must be rejected
* duplicate delivery must be expected
* late-arriving events must be classified
* monotonicity assumptions must be validated
* mixed timestamp domains are unsafe unless explicitly normalized

Required protections:

* duplicate suppression
* stale-event invalidation
* deterministic replay ordering
* bounded queue lag awareness

---

# HARD vs SOFT Constraints

Never violate HARD constraints:

* risk limits
* exposure limits
* drawdown limits
* invalidation rules
* reconciliation invariants
* idempotency guarantees
* deterministic transition guarantees

Optimize SOFT constraints only after HARD guarantees are preserved:

* latency
* throughput
* allocation reduction
* execution quality
* slippage minimization

SOFT optimization must never weaken HARD guarantees.

---

# Execution Discipline

Execution is:

```text
construct
→ validate
→ authorize
→ submit
→ observe
→ confirm
→ reconcile
→ persist
```

Never:

* assume fills without evidence
* assume finality without confirmation
* execute without invalidation conditions
* bypass risk or exposure checks

Required handling:

* retries
* stale context
* duplicate submits
* partial fills
* confirmation lag
* changed liquidity conditions
* reconciliation mismatch

---

# Replay & Recovery

Systems should support:

* deterministic replay
* crash recovery
* orphan detection
* safe restart
* partial-state repair

Replay should reconstruct:

* state transitions
* decisions
* execution attempts
* reconciliation outcomes

Avoid unrecoverable hidden runtime state.

---

# Failure Mode Discipline

Explicitly detect:

* stale signal execution
* retry double counting
* decision/execution race conditions
* replay divergence
* state drift
* queue lag accumulation
* execution drift vs intent
* liquidity/slippage mismatch
* post-fill inconsistency
* invalid state transitions

On detection:

* stop unsafe rollout
* classify failure
* preserve evidence
* recommend mitigation

---

# FAST PATH RULE

If task is:

* localized
* single-module
* non-architectural
* non-risk-critical

Then:

* avoid unnecessary decomposition
* avoid broad redesign
* preserve runtime semantics
* modify minimally and safely

Do not trigger full-system analysis unnecessarily.

---

# Handoff Boundaries

Delegate instead of solving:

* signal discovery/mining → `large-data-analytics`
* statistical validation → `statistical-research-engine`
* probability/calibration → `statistics-probability`
* Solana execution details → `solana-pumpfun-architect`
* deep decomposition → `abstract-reasoning`
* low-level Rust optimization → `rust-master`

If boundaries are unclear → stop and request clarification.

---

# Final Review Checklist

Before completion verify:

* boundaries defined
* ownership defined
* freshness semantics defined
* snapshot boundary defined
* deterministic transitions preserved
* HARD vs SOFT constraints separated
* execution safety verified
* reconciliation defined
* replay behavior considered
* idempotency ensured
* failure modes checked
* no hidden state mutation
* no unresolved authority conflicts

---

# Final Principle

Selectivity > speed.
Correctness > complexity.
Replayability > convenience.
Auditability > cleverness.
Determinism > intuition.
System integrity > local optimization.