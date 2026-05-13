---
name: rust-master
description: "Low-latency deterministic Rust runtime engineering for event-driven systems: async orchestration, bounded concurrency, replay-safe state management, ownership discipline, hot-path optimization, and production-grade reliability for selective execution runtimes."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Rust Master

Use this skill when the task involves:

* low-latency Rust runtimes
* async orchestration systems
* event-driven pipelines
* replay-safe state management
* bounded concurrency systems
* deterministic runtime behavior
* hot-path optimization
* ingestion and execution pipelines
* runtime-safe refactoring
* production-grade Rust infrastructure

Optimized for:

* Tokio-based runtimes
* selective execution systems
* stateful async systems
* low-latency event processing
* deterministic orchestration
* long-running runtime processes

Not optimized for:

* generic library design
* GUI applications
* academic Rust examples
* macro-heavy abstraction experiments
* broad framework architecture

---

# Quick Start

When activated:

> Preserve runtime determinism, explicit ownership boundaries, replay safety, bounded concurrency, and hot-path integrity. Prioritize correctness first, then runtime stability, then measured optimization.

Preferred runtime flow:

→ ingest
→ normalize
→ update canonical state
→ materialize immutable snapshot
→ evaluate
→ execute
→ reconcile
→ persist


For deeper guidance on unsafe, testing, concurrency validation, hot-path patterns, or observability, read `references.md`.

---

# Core Runtime Doctrine

Assume:

* stale data is normal
* duplicate delivery is normal
* retries are inevitable
* partial failure is expected
* async cancellation is real
* contention destroys latency
* hidden allocations accumulate
* replay divergence is a correctness failure

Therefore:

* correctness > throughput
* determinism > cleverness
* bounded concurrency > uncontrolled parallelism
* explicit ownership > shared mutable state
* replayability > convenience
* measured optimization > folklore

---

# Runtime Ownership Rules

Every subsystem must define:

* state owner
* mutation authority
* read-only consumers
* synchronization boundary
* persistence responsibility

Rules:

* hidden mutable state is forbidden
* shared mutable ownership must be minimized
* immutable snapshots are preferred for read paths
* runtime mutation during evaluation is forbidden
* ownership transfer must be explicit

Avoid:

* uncontrolled `Arc<Mutex<_>>`
* implicit shared state
* mutation through side effects
* clone-first design
* hidden cache authority

Prefer:

* ownership handoff
* bounded channels
* immutable snapshots
* append-only evidence
* isolated mutable domains

---

# Replay & Determinism

Identical:

* event ordering
* snapshots
* config
* timing inputs

must produce identical:

* state transitions
* decisions
* execution paths
* reconciliation results

Rules:

* ordering assumptions must be explicit
* time dependencies must be injectable
* replay parity must be preserved
* nondeterministic iteration is forbidden where order matters
* randomness must be explicit and seeded

Replay divergence is a correctness problem.

---

# Async Runtime Discipline

The runtime must explicitly handle:

* cancellation
* bounded queues
* backpressure
* retry coordination
* task lifecycle supervision
* shutdown correctness

Rules:

* async is for IO-bound concurrency
* blocking work must be isolated
* detached tasks require supervision
* cancellation is part of control flow
* queue capacity must be intentional
* retries must be bounded and idempotent

Avoid:

* blocking in async contexts
* unbounded task spawning
* hidden runtime contention
* uncontrolled fan-out
* silent task failure

---

# Hot-Path Rules

Protect hot paths aggressively.

Avoid:

* widening synchronization scope
* unnecessary cloning
* hidden allocations
* expensive deserialization
* lock contention amplification
* cross-runtime blocking
* unnecessary heap traffic

Prefer:

* stack-local work where practical
* bounded allocation behavior
* cache-friendly layouts
* narrow lock scope
* immutable read paths

Latency optimization that weakens correctness is a regression.

---

# Error Handling Rules

Errors are part of runtime behavior.

Rules:

* `unwrap()` and `expect()` are forbidden in production paths
* every error path must be intentional
* error classification must be explicit
* retries must not erase diagnostics
* invariant violations must be visible
* panic-driven runtime control flow is forbidden

Classify errors into:

* parse error
* validation error
* runtime state error
* transport/provider error
* timeout
* cancellation
* concurrency failure
* invariant violation
* resource exhaustion
* reconciliation mismatch

---

# Unsafe Rust Rule

Unsafe is allowed only with explicit necessity.

Before using `unsafe`, answer:

* why safe Rust is insufficient
* what invariant guarantees soundness
* how the invariant is preserved
* how runtime/replay correctness is protected

Rules:

* unsafe blocks must be minimal
* every unsafe block requires `# Safety`
* unsafe must not bypass design problems
* unsafe boundaries must be testable

Prefer safe Rust unless measured constraints justify otherwise.

---

# Runtime Architecture Rules

Separate:

* ingestion
* normalization
* state mutation
* immutable evaluation
* execution orchestration
* reconciliation
* persistence
* observability

Avoid:

* monolithic runtime flows
* hidden mutation paths
* mixed ownership domains
* execution-time recomputation
* hidden side effects

---

## Temporal Integrity Rules

The agent must explicitly distinguish:
- event timestamp
- ingestion timestamp
- feature materialization timestamp
- decision timestamp
- execution timestamp
- reconciliation timestamp

A feature is valid only if it was fully observable at decision time.

The agent must reject:
- retroactively reconstructed features
- features depending on delayed indexing
- signals computed using post-decision state
- signals unavailable under live operational latency constraints

---

# FAST PATH RULE

If task is:

* localized
* single-module
* non-architectural
* non-runtime-critical

Then:

* avoid broad redesign
* preserve runtime invariants
* preserve latency-sensitive behavior
* modify minimally and safely

Do not trigger full-system decomposition unnecessarily.

---

# Handoff Boundaries

Delegate instead of solving:

* Solana runtime/execution semantics → `solana-pumpfun-architect`
* trading architecture → `trading-systems`
* statistical validation → `statistical-research-engine`
* signal mining → `large-data-analytics`
* abstract decomposition → `abstract-reasoning`

If unclear → stop and request clarification.

---

# Final Review Checklist

Before completion verify:

* ownership boundaries explicit
* mutation authority explicit
* replay semantics preserved
* deterministic ordering preserved
* async/blocking boundaries correct
* retries bounded and idempotent
* cancellation handled correctly
* no widened synchronization scope
* no hot-path regressions introduced
* observability sufficient
* failure paths intentional
* no hidden mutable state
* no unresolved runtime invariants

---

# Final Principle

Deterministic runtime behavior over abstraction elegance.
Replayability over convenience.
Correctness over throughput.
Bounded concurrency over uncontrolled parallelism.
Explicit ownership over hidden shared state.
Runtime integrity over clever optimization.