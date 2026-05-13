## `rust-master/references.md`

```md
# Rust Master Reference

This file expands the `rust-master` skill. Read it only when deeper Rust runtime guidance is needed.

Use this reference for:

* async runtime architecture
* bounded concurrency design
* replay-safe state management
* hot-path optimization
* error taxonomy design
* unsafe Rust review
* concurrency testing
* runtime observability
* refactoring safety analysis

Do not load this file for small localized edits unless needed.

---

# 1. Operating Assumptions

For low-latency event-driven Rust systems:

* compilation does not prove semantic correctness
* async cancellation is a real control-flow event
* retry paths create state-management risk
* contention can destroy latency
* hidden allocations accumulate into tail latency
* duplicate events and stale inputs are normal
* runtime replay divergence is a correctness failure
* panic in production is usually a failure mode, not recovery
* `unsafe` is a liability unless rigorously justified
* a slightly slower obvious design is usually preferable to a fast fragile one

Rust should be used as:

* a correctness engine
* a contract language
* a concurrency discipline
* a safety boundary
* a performance tool

Correctness first.
Runtime stability second.
Measured optimization third.

---

# 2. Ownership and Runtime State

Ownership is not only a compiler concept. In runtime systems, ownership defines authority.

Every subsystem should define:

* what state it owns
* what state it may mutate
* what state it may only read
* who may persist state
* who may emit side effects
* what recovery responsibility it has

Good runtime ownership patterns:

* single writer per state domain
* immutable snapshots for evaluation
* bounded channels for ownership handoff
* append-only evidence for audit/replay
* narrow mutation APIs
* explicit state transition functions

Bad patterns:

* uncontrolled `Arc<Mutex<_>>`
* broad shared state handles
* side-effect mutation through read paths
* hidden caches acting as authority
* clone-first programming
* mutation during evaluation
* execution-time feature recomputation

Rule:

If it is unclear who owns a state field, the design is not ready.

---

# 3. Borrowing Discipline

Use borrowing to express access, not just to satisfy the compiler.

Rules:

* `&mut T` is a privilege, not a default
* prefer borrowing when ownership transfer is unnecessary
* use `clone()` only when cost and necessity are explicit
* avoid fighting the borrow checker with broad ownership
* avoid self-referential structures unless pinning is justified
* avoid returning references tied to unstable internal mutation
* avoid keeping mutable borrows across await points

Common failure modes:

* clone-first programming
* borrow-checker workaround that weakens the design
* broad mutable access where a narrow method would be safer
* shared ownership used to avoid clear ownership modeling
* interior mutability used without a strict invariant

Preferred fix:

Refactor ownership boundaries before reaching for shared mutation.

---

# 4. Replay and Determinism

Replay-safe systems must produce the same outcome from the same inputs.

Identical:

* input events
* event ordering
* config
* snapshots
* injected time
* deterministic random seeds

should produce identical:

* state transitions
* scores
* decisions
* execution intents
* reconciliation results

Rules:

* ordering assumptions must be explicit
* wall-clock reads must be isolated or injectable
* randomness must be seeded or removed
* nondeterministic iteration is forbidden where order matters
* hash-map iteration must not determine logic unless explicitly sorted
* replay-only behavior must not differ from live behavior unless labeled
* rounding and normalization rules must be stable

Replay divergence indicates one of:

* hidden time dependency
* nondeterministic ordering
* inconsistent input normalization
* mutable global state
* provider/state drift
* non-replayable external dependency
* hidden cache behavior

---

# 5. Async Runtime Discipline

Tokio and async Rust are appropriate for IO-heavy systems, but async does not make work automatically safe or non-blocking.

The runtime must explicitly handle:

* task ownership
* task cancellation
* shutdown
* backpressure
* queue capacity
* retry coordination
* timeout behavior
* error propagation
* panic supervision

Rules:

* async is for IO-bound concurrency
* CPU-heavy work should be isolated or offloaded
* blocking calls must not run inside async hot paths
* detached tasks require supervision
* spawned tasks must not silently drop errors
* cancellation is part of normal control flow
* select loops must preserve shutdown behavior
* retry loops must be bounded
* queue capacity must match real backpressure constraints

Avoid:

* unbounded `tokio::spawn`
* detached fire-and-forget tasks
* unbounded channels
* blocking sleep in async paths
* blocking file/network IO in async hot paths
* retries without cancellation
* long-held locks across `.await`

Preferred patterns:

* bounded channels
* explicit admission control
* task registries or supervisors
* structured shutdown signals
* timeout wrappers
* cancellation-safe state transitions

---

# 6. Bounded Concurrency

Concurrency is a state-management problem.

Good concurrency design defines:

* producers
* consumers
* ownership handoff
* backpressure
* queue capacity
* retry boundaries
* shutdown behavior
* failure propagation

Prefer:

* bounded channels over unbounded fan-out
* ownership transfer over locking
* isolated mutable actors
* immutable snapshots for readers
* narrow locks for unavoidable shared state
* explicit task lifecycle management

Avoid:

* lock-heavy architectures
* nested locks
* lock-order ambiguity
* unbounded fan-out
* queue growth without metrics
* uncontrolled retry amplification
* atomics without memory-model understanding
* `RwLock` by default

`RwLock` is not automatically faster than `Mutex`.

Use `RwLock` only when:

* reads dominate heavily
* write contention is low
* reader hold time is short
* fairness behavior is acceptable
* metrics confirm benefit

---

# 7. Hot-Path Engineering

Hot paths include:

* ingestion loops
* event normalization
* state update paths
* feature materialization
* evaluation loops
* execution intent construction
* retry scheduling
* reconciliation hot loops

Protect these aggressively.

Avoid:

* unnecessary allocation
* repeated string formatting
* avoidable `clone()`
* expensive parsing
* lock contention
* long critical sections
* blocking calls
* unbounded collections
* repeated serialization/deserialization
* hidden recomputation

Prefer:

* borrowed views
* preallocated buffers where justified
* stack-local temporary data
* compact structs
* cache-friendly layouts
* bounded collections
* append-only logs/evidence
* cheap counters and metrics
* deterministic batching

Optimization rules:

* measure before optimizing
* profile before rewriting
* preserve correctness first
* preserve replay semantics first
* preserve ordering guarantees first
* document assumptions behind micro-optimizations

Latency optimization that weakens correctness is a regression.

---

# 8. Error Handling Discipline

Errors must preserve operational meaning.

Rules:

* no `unwrap()` or `expect()` in production paths
* no panic-driven control flow
* no flattening all errors into strings too early
* no hidden retry loops that erase original failure
* every recoverable error must be classified
* every unrecoverable invariant violation must be visible

Error taxonomy:

* parse error
* validation error
* domain error
* runtime state error
* IO error
* transport/provider error
* timeout
* cancellation
* concurrency failure
* resource exhaustion
* invariant violation
* reconciliation mismatch
* unrecoverable internal failure

Library-style code should prefer typed error enums.

Application/runtime code may wrap errors with context when it improves observability.

Retries should preserve:

* original error class
* attempt count
* last failure
* retry reason
* abandon reason
* related state snapshot or decision id

---

# 9. Panic Policy

Panic is acceptable only for:

* tests
* impossible invariant violations
* unrecoverable startup configuration errors
* explicit fail-fast paths where continuing would corrupt state

Panic is not acceptable for:

* expected invalid input
* provider failures
* timeout
* parse failures
* stale data
* missing optional state
* retry exhaustion
* normal domain rejection

If panic is unavoidable:

* message must be actionable
* invariant must be documented
* state corruption risk must be avoided
* tests should cover the invariant when possible

---

# 10. Unsafe Rust Discipline

Unsafe is allowed only when safe Rust is insufficient and the benefit is real.

Before using unsafe, answer:

* why is safe Rust insufficient?
* what exact invariant makes this sound?
* who maintains that invariant?
* how is the invariant tested?
* what happens under panic/cancellation?
* how is aliasing prevented?
* how is lifetime validity preserved?
* how is layout/alignment guaranteed?
* how does this affect replay/runtime correctness?

Rules:

* keep unsafe blocks minimal
* document every unsafe block with `# Safety`
* avoid `static mut`
* avoid raw pointer manipulation unless necessary
* avoid unsafe to bypass ownership design
* isolate unsafe behind safe APIs
* test with Miri/sanitizers where applicable
* ensure unsafe does not create hidden runtime state

Unsafe must not be used as a shortcut around design problems.

---

# 11. Type-Driven Runtime Design

Use types to prevent invalid runtime states.

Good uses:

* enums for mutually exclusive lifecycle states
* newtypes for units and identifiers
* typed IDs for sessions, pools, snapshots, decisions
* typed errors for failure classification
* explicit config structs for thresholds and limits
* `Result` for fallible operations
* `Option` only when absence is a valid state

Avoid:

* raw `String` for domain identifiers when a type is warranted
* `bool` flags that encode multiple states
* large unstructured config maps
* ambiguous numeric units
* optional fields that hide invalid construction
* generic traits that obscure behavior

A good type makes invalid states difficult or impossible.

---

# 12. Runtime Architecture Boundaries

Separate:

* ingestion
* normalization
* parsing
* state mutation
* feature materialization
* immutable evaluation
* decision policy
* execution orchestration
* reconciliation
* persistence
* observability

Avoid:

* parsing inside decision logic
* execution logic mutating scoring state
* evaluation paths doing live reads
* persistence hidden inside hot-path logic
* broad cross-module mutable handles
* monolithic runtime functions
* hidden side effects

Good module boundaries reveal:

* who owns data
* who may mutate state
* what can fail
* what is deterministic
* what is best-effort
* what is hot path
* what is replay-critical

---

# 13. Testing and Validation

Do not rely on `cargo test` alone.

Required layers:

* unit tests for isolated logic
* integration tests for module boundaries
* replay tests for deterministic behavior
* stress tests for concurrency and queue behavior
* failure-path tests
* cancellation tests
* retry idempotency tests
* benchmarks for hot paths

Test cases should include:

* duplicate events
* stale inputs
* out-of-order events
* queue saturation
* cancellation during processing
* retry exhaustion
* shutdown during active work
* provider/transport failure
* reconciliation mismatch
* invalid state transitions

Testing principles:

* test public contracts, not implementation accidents
* test failure paths, not only happy paths
* test invariants, not only outputs
* test recovery behavior
* test deterministic replay
* test degraded conditions

---

# 14. Concurrency Validation

Concurrency validation should look for:

* race conditions
* deadlocks
* starvation
* lost wakeups
* unbounded queue growth
* task leaks
* cancellation leaks
* lock-order inversion
* retry storms
* inconsistent shutdown
* hidden detached task failures

Useful techniques:

* stress tests
* deterministic schedulers where available
* loom-style modeling for critical concurrency
* bounded queue saturation tests
* cancellation injection
* timeout injection
* metrics-based runtime observation

Every long-running task should have:

* owner
* purpose
* shutdown path
* error reporting path
* cancellation behavior
* observability

---

# 15. Observability

Use structured observability, not ad hoc prints.

Preferred tools:

* `tracing`
* spans
* structured fields
* metrics
* deterministic identifiers
* latency histograms
* queue depth gauges
* retry counters
* error counters

Required visibility:

* task lifecycle
* queue lag
* retry accumulation
* cancellation behavior
* shutdown state
* hot-path latency
* state transition flow
* replay divergence
* reconciliation mismatch
* provider/transport failure

Rules:

* no `println!` in production paths
* critical paths must be measurable
* failures must be diagnosable
* logs must preserve useful identifiers
* high-volume logs must not destroy latency
* metrics must not allocate excessively in hot paths

---

# 16. Performance and Memory

Performance is measured, not assumed.

Reason about:

* allocation frequency
* heap vs stack placement
* cache locality
* branch behavior
* clone cost
* serialization overhead
* lock contention
* async scheduling overhead
* queue depth
* batching effects
* tail latency

Optimization hierarchy:

1. remove unnecessary work
2. reduce allocations
3. reduce contention
4. improve data layout
5. batch where safe
6. specialize only when measured
7. use unsafe only as last resort

Performance claims should include:

* benchmark or profile basis
* hot-path identification
* expected latency/throughput effect
* correctness risk
* replay/determinism risk

---

# 17. Common Rust Runtime Failure Modes

Detect and name:

* clone-first programming
* unwrap-driven development
* hidden shared mutable state
* borrow-checker workaround weakening design
* async blocking misuse
* detached task loss
* unbounded channel growth
* queue saturation without metrics
* synchronization widening
* lock held across await
* retry amplification
* cancellation leak
* hidden nondeterminism
* stale snapshot usage
* execution-time recomputation
* unsafe used as shortcut
* flattened errors destroying diagnostics
* test coverage ignoring failure paths
* public API exposing invalid states

If a failure mode appears:

* stop
* classify it
* preserve evidence
* correct the design
* add tests if appropriate

---

# 18. Review Checklist

Before finalizing a non-trivial Rust change:

* ownership boundaries are explicit
* mutation authority is explicit
* lifetimes are minimal and valid
* replay semantics are preserved
* deterministic ordering is preserved
* async/blocking boundaries are correct
* cancellation behavior is defined
* retries are bounded and idempotent
* synchronization scope did not widen unnecessarily
* hot-path behavior did not regress
* error paths are intentional
* unsafe is absent or justified
* observability is sufficient
* failure modes are considered
* tests cover success and failure paths
* benchmarks/profiles support performance claims
* handoffs to domain skills are respected

---

# 19. Final Principle

Rust is useful here because it can encode runtime contracts.

Use it to make systems:

* explicit
* bounded
* deterministic
* replayable
* observable
* safe under failure
* fast only after correctness is preserved

Deterministic runtime behavior over abstraction elegance.
Replayability over convenience.
Correctness over throughput.
Bounded concurrency over uncontrolled parallelism.
Explicit ownership over hidden shared state.
Runtime integrity over clever optimization.