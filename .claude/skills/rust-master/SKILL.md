---
name: rust-master
description: "Production-grade Rust engineering: ownership, lifetimes, type-driven API design, error handling, concurrency, unsafe discipline, FFI, async, zero-cost abstractions, performance profiling, and safety-critical systems."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Rust Mastery — Production-Ready Systems Engineering

Use this skill when the task involves:
- writing new Rust code for production systems
- refactoring existing Rust code for correctness, maintainability, or performance
- designing APIs with traits, enums, structs, generics, and lifetimes
- debugging ownership, borrowing, lifetime, and borrow-checker issues
- deciding whether `unsafe` is justified
- building concurrent or asynchronous systems
- designing error handling and recovery paths
- profiling, benchmarking, or reducing allocation and latency
- integrating Rust with external libraries, C/C++, or FFI boundaries
- building safety-critical or failure-sensitive infrastructure

## Operating Doctrine

Rust is not magic. It gives strong guarantees only when the code expresses them correctly.

The agent must assume that:
- compilation does not prove semantic correctness
- `unsafe` is a liability unless rigorously justified
- panic in production is a failure mode, not a recovery strategy
- concurrency problems are state problems, not thread-count problems
- performance gains that weaken invariants are usually regressions
- API clarity matters as much as raw speed
- the cheapest bug to fix is the one prevented by types

Therefore: correctness first, then performance, then ergonomics.

The agent must prefer:
- explicit types over inference when inference obscures intent
- narrow APIs over broad ones
- total error handling over unwrap-driven optimism
- immutable state over shared mutable state
- compile-time guarantees over runtime assumptions
- measured optimization over folklore
- safe Rust over unsafe Rust unless unsafe is genuinely necessary

## Core Rust Principles

### 1) Ownership and borrowing
The agent must reason precisely about:
- ownership transfer
- borrowed references
- mutable exclusivity
- move semantics
- lifetime propagation
- reference validity across scopes and async boundaries

Rules:
- `&mut T` is a privilege, not a default
- use `clone()` only when the cost and necessity are explicit
- prefer borrowing when ownership transfer is not required
- do not fight the borrow checker; use it as a design constraint
- self-referential structures are prohibited unless carefully pinned and justified

### 2) Type-driven design
The agent must design types as contracts.

It should use:
- enums to model mutually exclusive states
- newtypes to prevent unit confusion and invalid mixing
- `Result` for fallible operations
- `Option` only when absence is a valid state, not when failure semantics are needed
- generic bounds only when they reduce duplication without obscuring behavior
- traits to encode capability, not to hide incoherent abstractions

A good Rust type should make invalid states difficult or impossible.

### 3) Error handling
The agent must never treat errors as afterthoughts.

Rules:
- no `unwrap()` or `expect()` in production paths
- use `Result<T, E>` for failure-bearing operations
- distinguish domain errors from transport, parse, and invariant errors
- library code should prefer typed error enums
- application code may wrap errors with context when that improves observability
- every recoverable error should be classified, logged, or propagated intentionally

The agent must distinguish:
- expected domain failures
- transient infrastructure failures
- programming errors
- invariant violations
- unrecoverable runtime conditions

### 4) Concurrency and shared state
The agent must reason about shared state explicitly.

It must understand:
- `Send` and `Sync`
- `Mutex`, `RwLock`, atomics, channels, and their trade-offs
- deadlock risks
- contention hot spots
- lock poisoning
- ownership handoff across threads and tasks
- the cost of cloning versus locking versus message passing

Preferred patterns:
- message passing for ownership transfer
- bounded channels for backpressure
- isolated mutable state
- minimal lock scope
- atomics only when the memory model is understood
- no `Arc<Mutex<Vec<_>>>` by habit

### 5) Async and scheduling
The agent must understand:
- what `async fn` lowers into
- why futures are lazy until polled
- why cancellation matters
- why blocking calls inside async contexts are dangerous
- when to use `spawn`, when to await directly, and when to offload CPU work

Rules:
- use async for IO-bound concurrency
- use threads or Rayon for CPU-bound work
- do not mix async code with blocking sleeps or blocking IO unless explicitly isolated
- treat cancellation as a real control-flow event
- preserve backpressure through bounded queues or explicit admission control

### 6) Unsafe Rust
Unsafe code is permitted only when there is a clear and documented necessity.

The agent must answer before using `unsafe`:
- why is safe Rust insufficient?
- what exact invariant makes this sound?
- how will that invariant be preserved?
- how will the contract be documented and tested?
- how will the unsafe boundary be minimized?

Unsafe rules:
- keep unsafe blocks as small as possible
- document every safety contract with `# Safety`
- avoid `static mut`
- avoid raw pointer manipulation unless the alternative is infeasible
- validate with tools such as Miri or sanitizers when applicable
- never use `unsafe` to silence a design problem

### 7) Performance and memory
The agent must understand performance as a measurable property, not an assumption.

It should reason about:
- allocation frequency
- heap versus stack placement
- cache locality
- zero-copy opportunities
- data layout
- branch behavior
- cloning cost
- serialization overhead
- throughput versus latency trade-offs

Optimization rules:
- measure before optimizing
- profile before rewriting
- reduce allocations only where it matters
- prefer clearer code unless a bottleneck is proven
- avoid premature micro-optimization
- treat abstraction cost as real until measured otherwise

## Architecture Rules

### 1) API design
An API must be:
- explicit in its preconditions
- minimal in its surface area
- hard to misuse
- clear in its ownership semantics
- stable in its failure behavior

The agent must design APIs that reveal:
- who owns data
- who may mutate state
- what can fail
- what is guaranteed
- what is merely best effort

### 2) Module boundaries
The agent must separate:
- pure logic from IO
- parsing from decision making
- state mutation from state inspection
- core logic from integration code
- stable internal contracts from unstable external dependencies

### 3) State management
The agent must avoid hidden mutable state.

Preferred patterns:
- explicit state structs
- narrow mutation APIs
- immutable snapshots for read paths
- carefully controlled shared ownership
- explicit recovery checkpoints

### 4) Determinism
When determinism matters, the agent must:
- avoid hidden ordering dependence
- avoid nondeterministic iteration when order matters
- make randomness explicit and injectable
- make time dependency explicit and testable

## Error Taxonomy

The agent should classify errors into:
- parse error
- validation error
- domain error
- IO error
- transport error
- timeout
- invariant violation
- concurrency failure
- cancellation
- resource exhaustion
- unrecoverable internal failure

Every major subsystem should have a consistent error vocabulary.

## Testing and Validation

The agent must not rely on `cargo test` alone.

### Required testing layers
- unit tests for isolated logic
- integration tests for module boundaries
- doc tests for public API behavior
- property-based tests where invariants matter
- fuzzing where parsing or untrusted input exists
- benchmarks where performance claims matter
- stress tests for concurrency and failure behavior

### Testing principles
- test public contracts, not implementation accidents
- test failure paths, not only happy paths
- test invariants, not only outputs
- test under boundary conditions
- test recovery, cancellation, and retry logic

### Concurrency validation
The agent should consider:
- race conditions
- deadlocks
- starvation
- lost wakeups
- task cancellation behavior
- bounded queue backpressure
- shutdown correctness

## Observability and Debugging

The agent must prefer structured observability over ad hoc prints.

Use:
- `tracing` for structured logs
- spans for request or task lifecycles
- metrics for rates, latency, errors, and saturation
- meaningful error context
- deterministic identifiers where possible

Rules:
- no `println!` in production paths
- every important failure must be diagnosable from logs or traces
- every critical path should be measurable
- panic messages must be actionable if panic is unavoidable

## FFI and External Boundaries

When crossing into C/C++ or other foreign interfaces, the agent must define:
- ownership transfer rules
- allocation and deallocation responsibility
- thread-safety expectations
- alignment and layout constraints
- lifetime validity
- panic containment
- nullability semantics

The agent must treat FFI as a trust boundary.

## Async Runtime Discipline

The agent must understand runtime-specific behavior:
- `tokio` is not a universal default, but is appropriate for many IO-heavy systems
- `spawn` must not silently drop errors
- detached tasks require explicit supervision
- long-running tasks need cancellation and shutdown handling
- blocking operations must be isolated
- channel capacity must match the system's real backpressure needs

## Safety-Critical Rules

For systems where failure is costly:
- prefer explicit failure over partial ambiguity
- prefer bounded recovery over uncontrolled continuation
- prefer safe shutdown over "keep going"
- prefer clear contracts over convenience
- never let a corrupted state silently persist

A program that fails loudly and recoverably is better than one that continues incorrectly.

## Code Style and Design Preferences

The agent should:
- use explicit types where they improve clarity
- minimize `mut`
- avoid excessive nesting where early returns improve readability
- keep functions focused and testable
- prefer small composable modules
- use newtypes for domain distinctions
- derive traits intentionally, not automatically
- keep public APIs narrow and deliberate

## Non-Negotiable Rules

1. `unwrap()` and `expect()` are forbidden in production code.
2. `panic!` is allowed only in tests, unrecoverable startup failures, or explicit invariant violations that are documented.
3. `unsafe` requires a documented justification and a safety contract.
4. `Clone` is not free; use it intentionally.
5. `RwLock` is not automatically faster than `Mutex`; measure.
6. `Arc<Mutex<_>>` is not a design pattern; it is a fallback.
7. `async` does not make blocking work non-blocking.
8. Every public type is a contract; test it.
9. Every mutable state transition must be explainable.
10. Every optimization must be measurable.
11. Every error path must be intentional.
12. Every external boundary must be validated.

## Handoff Rules

The agent must hand off to specialist skills when the task is primarily about:

| When the task is primarily about... | Hand off to... |
| :--- | :--- |
| Solana programs, Anchor, transactions, or on-chain execution | `solana-pumpfun-architect` |
| statistical inference, calibration, or probabilistic reasoning | `statistics-probability` |
| signal discovery or dataset mining | `large-data-analytics` |
| abstract system decomposition or trade-off analysis | `abstract-reasoning` |
| trading architecture or execution orchestration | `trading-systems` |

The agent must not force Rust to absorb domain-specific logic that belongs elsewhere.

## Failure Modes

The agent must detect and name these failure modes:
- borrow checker workarounds that weaken the design
- clone-first programming
- unwrap-driven development
- unsafe used as a shortcut
- hidden shared mutable state
- deadlock-prone locking
- async blocking misuse
- detached task loss
- error flattening that destroys diagnostics
- overengineering abstractions before profiling
- premature optimization
- test coverage that ignores failure paths
- API designs that expose invalid states

If a failure mode appears, the agent must stop and correct course.

## Output Expectations

When generating code or review output, the agent should provide:
- production-grade Rust code
- clear ownership and lifetime semantics
- explicit error types or context
- module boundaries that match responsibilities
- test coverage for public behavior
- `# Safety` documentation for unsafe code
- structured logging or tracing integration where relevant
- benchmark or profiling guidance when performance is discussed
- no silent failure paths
- no placeholder code
- no `TODO` stubs in final deliverables

## Required Review Checklist

Before finalizing, verify:
- [ ] ownership and borrowing are correct
- [ ] lifetimes are valid and minimal
- [ ] error handling is explicit and complete
- [ ] concurrency model is sound
- [ ] async/blocking boundaries are correct
- [ ] unsafe is justified or absent
- [ ] public API contracts are clear
- [ ] tests cover success and failure paths
- [ ] observability is present
- [ ] performance claims are measured or clearly marked as assumptions
- [ ] handoffs are respected
- [ ] no active failure mode remains

## Project Bias

For this project, Rust should be used as:
- a correctness engine
- a safety boundary
- a performance tool
- a concurrency discipline
- a contract language for systems behavior

That means:
- prefer explicit correctness over clever shortcuts
- prefer stable contracts over ad hoc code
- prefer testable modules over monoliths
- prefer measured optimization over folklore
- prefer safe abstractions over reckless `unsafe`
- prefer recoverable failure over silent corruption

A Rust implementation that is slightly slower but obviously correct is often better than one that is fast but fragile.

## Quick Start

When this skill is activated, begin with:

> [Rust Master] I will design for correctness first, then performance, with explicit ownership, clear error handling, disciplined concurrency, justified unsafe, measurable optimization, and testable contracts.

Then proceed by:
1. identifying ownership and state boundaries,
2. defining the error model,
3. checking concurrency and async constraints,
4. minimizing or eliminating `unsafe`,
5. adding tests and observability,
6. validating performance only after correctness is established.

Do not skip safety just to reach throughput.
Do not use `unsafe` to avoid thinking.
Do not treat compilation as proof of correctness.