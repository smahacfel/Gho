## `solana-pumpfun-architect/references.md`

# Solana + pump.fun Architect Reference

This file expands the `solana-pumpfun-architect` skill. Read it only when deeper Solana runtime, pump.fun, transaction lifecycle, ingestion, or execution reliability guidance is needed.

Use this reference for:

* Solana transaction lifecycle reasoning
* Yellowstone/Geyser ingestion design
* event ordering and replay analysis
* pump.fun lifecycle modeling
* blockhash and TTL handling
* compute budget and priority fee policy
* account contention analysis
* stale-state and retry failure analysis
* reconciliation and execution audit design

Do not load this file for small localized edits unless needed.

---

# 1. Operating Assumptions

For low-latency Solana selective runtimes:

* Solana state is observed through imperfect feeds
* processed state may be useful but not final truth
* RPC providers may disagree
* simulation is not inclusion
* blockhash validity is short-lived
* account contention can destroy execution quality
* duplicate delivery is normal
* late delivery is normal
* retry behavior can amplify failures
* execution windows are short
* stale execution assumptions are dangerous
* replay divergence is a correctness failure

The runtime must prefer:

* explicit state ownership
* immutable decision snapshots
* deterministic ordering
* bounded retries
* explicit invalidation
* reconciliation over optimism

---

# 2. Solana Runtime Concepts

Always account for:

## Accounts

* account ownership
* writable account locks
* signer requirements
* lamports/rent
* executable accounts
* token accounts
* associated token accounts
* program-derived addresses

## Transactions

* recent blockhash
* versioned transactions
* address lookup tables
* instruction ordering
* signer set
* account metas
* compute budget instructions
* priority fee instructions

## Runtime Execution

* account locking
* compute unit consumption
* slot timing
* transaction expiration
* preflight/simulation limitations
* commitment levels
* confirmation latency

## Provider Reality

* RPC inconsistency
* WebSocket lag
* Yellowstone/Geyser stream lag
* provider-specific parsing behavior
* temporary transport failure
* processed vs confirmed/finalized mismatch

---

# 3. Yellowstone/Geyser Ingestion Discipline

Ingestion pipelines must handle:

* duplicate events
* late events
* out-of-order events
* account updates without expected transaction context
* transactions arriving before metadata
* dropped stream segments
* provider reconnects
* heartbeat gaps
* replay/resubscription behavior

Required properties:

* deterministic deduplication
* explicit event identity
* explicit timestamp source
* explicit slot source
* bounded queue behavior
* source-specific diagnostics
* replay compatibility

Useful event metadata:

* slot
* signature
* instruction index
* account pubkey
* write version if available
* event type
* source/provider
* receive timestamp
* normalized event timestamp
* parse status
* finality/commitment

Rules:

* never treat receive order as chain order unless explicitly justified
* never treat provider parse success as semantic truth without validation
* never silently repair event order
* never mix event-time and wall-clock time without explicit normalization

---

# 4. Event Ordering and Replay

Replay must be able to reconstruct:

* input event order
* normalized events
* state updates
* materialized snapshots
* decisions
* execution intents
* submission attempts
* confirmations
* reconciliation outcomes

Ordering keys should be explicit.

Possible ordering dimensions:

* slot
* transaction index
* instruction index
* signature
* account write version
* event timestamp
* receive sequence
* fallback monotonic counter

If a fallback ordering rule is used, it must be:

* deterministic
* documented
* observable
* replay-compatible

Replay divergence usually indicates:

* hidden wall-clock dependency
* nondeterministic iteration
* missing event identity
* ambiguous deduplication
* provider-dependent parsing
* implicit state mutation
* inconsistent timestamp normalization

---

# 5. Canonical State and Snapshot Discipline

Every runtime must define:

* canonical state owner
* write authority
* read authority
* materialization boundary
* fallback state policy
* replay reconstruction policy

Decision snapshots should be:

* immutable
* fully materialized before evaluation
* traceable to source events
* versioned or identifiable
* reproducible under replay

Avoid:

* scoring from partially updated state
* execution-time feature recomputation
* multiple canonical writers
* hidden cache authority
* fallback state silently overriding canonical state
* live-state reads during evaluation

A decision should answer:

```text
Given this exact snapshot and this exact config, what is the verdict?
````

Not:

```text
What does the current live system happen to say right now?
```

---

# 6. Observation Window Model

Selective pump.fun runtimes should explicitly model bounded observation.

Common windows:

* first N milliseconds
* first N seconds
* first N slots
* first N transactions
* early / normal / extended observation phases
* terminal deadline

Observation lifecycle:

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

* stale windows must terminate
* timeout is a terminal outcome
* late events after terminal verdict must be classified
* evaluation trigger must be deterministic
* post-verdict mutation must not change the historical decision
* execution must use a fresh-enough snapshot

Observation failure modes:

* window extended accidentally
* evaluation occurs with partial state
* late metadata rewrites earlier assumptions
* stale account state is used for execution
* wall-clock/event-time mismatch corrupts duration
* duplicate transactions distort counters
* terminal verdict is overwritten

---

# 7. Transaction Validity and Blockhash Lifecycle

Every transaction attempt must track:

* blockhash
* blockhash acquisition time
* slot reference
* validity TTL
* attempt number
* submission time
* confirmation deadline
* invalidation condition

Rules:

* expired blockhash invalidates the attempt
* stale liquidity/state invalidates the attempt
* stale decision snapshot may invalidate the attempt
* retry after invalidation must rebuild from fresh state
* retry must not reuse stale assumptions silently

Transaction attempt lifecycle:

```text
BUILD
→ PRECHECK
→ SIGN
→ SUBMIT
→ OBSERVE
→ CONFIRM / EXPIRE / REJECT / UNKNOWN
→ RECONCILE
```

Never treat submission as success.

---

# 8. Compute Budget and Priority Fee Policy

Execution policy must define:

* compute unit limit
* compute unit price
* priority fee strategy
* tip policy if applicable
* retry adjustment rules
* abandon conditions

Consider:

* congestion
* account contention
* expected execution complexity
* current slot/leader conditions if available
* strategy value
* stale-state risk
* retry budget

Rules:

* fee escalation must be bounded
* retry escalation must preserve idempotency
* priority fee optimization must not bypass risk checks
* compute budget changes should be observable
* failed compute assumptions should be classified

Failure classes:

* insufficient compute
* fee too low
* account lock contention
* stale blockhash
* provider submission failure
* runtime program error
* simulation-only success

---

# 9. Account Contention and Locking

Solana execution is affected by writable account locks.

Always consider:

* writable accounts
* shared hot accounts
* program accounts
* token accounts
* bonding curve accounts
* fee recipient accounts
* associated token accounts
* lookup table dependencies

Contention symptoms:

* inconsistent landing
* increased confirmation latency
* repeated submission failure
* sudden execution drift
* simulation success but no landing
* retry storms

Mitigation options:

* bounded retry
* priority fee adjustment
* defer/abandon
* rebuild transaction
* reduce unnecessary writable accounts
* simplify instruction path
* avoid duplicate submits

Never assume retries are harmless. Retrying into contention can worsen landing probability and increase stale-state risk.

---

# 10. pump.fun Lifecycle Model

Must distinguish:

* token creation
* pool detection
* bonding curve initialization
* first tradable state
* early trading burst
* bonding progress
* migration proximity
* post-migration state

Important distinctions:

* creation event is not tradability
* visibility is not execution eligibility
* early activity is noisy
* early buys can be artificial
* liquidity state can change quickly
* migration changes execution assumptions

Runtime should classify:

* observed token
* candidate token
* tradable token
* rejected token
* timed-out token
* committed token
* migrated token

pump.fun-specific failure modes:

* fake early demand
* dev-related manipulation
* artificial signer diversity
* clustered funding sources
* flipper-heavy early flow
* stale curve state
* migration boundary risk
* bonding curve reserve mismatch
* parser misclassification

---

# 11. Execution Immutability

Once execution begins:

* the approved decision must not be mutated
* scoring must not rewrite the decision
* feature updates must not retroactively change the verdict
* only reconciliation may evaluate realized outcome

If new information invalidates execution before submission:

```text
invalidate attempt
→ classify reason
→ decide rebuild / abandon
```

If new information arrives after submission:

```text
preserve original decision
→ reconcile outcome
→ record post-submit evidence
```

No mid-flight reinterpretation.

---

# 12. Retry and Idempotency

Retries must define:

* max attempts
* retry spacing
* retry conditions
* abandon conditions
* rebuild-from-fresh-state threshold
* duplicate suppression
* idempotency key or equivalent identity

Retry is allowed for:

* transient transport failure
* timeout without confirmed landing
* stale provider response
* explicitly recoverable submission error

Retry should be abandoned for:

* expired blockhash
* stale decision snapshot
* changed liquidity beyond tolerance
* insufficient balance
* invalid account state
* deterministic program failure
* exceeded execution window

Retry failure modes:

* duplicate execution
* retry double counting
* fee escalation runaway
* stale-state execution
* lost original error
* unbounded queue growth
* contention amplification

---

# 13. Simulation vs Inclusion

Simulation can help detect:

* program errors
* account mismatch
* compute failure
* insufficient funds
* invalid instruction data
* obvious slippage failure

Simulation cannot prove:

* inclusion
* finality
* future liquidity
* absence of contention
* future blockhash validity
* same provider state across time

Rules:

* simulation success is provisional
* simulation failure should be classified
* simulation state source must be known
* simulation must not mutate decision state
* simulation must not replace confirmation

---

# 14. Confirmation and Reconciliation

Execution is incomplete until reconciled.

Reconcile:

* intended transaction
* submitted transaction
* observed signature status
* account state after execution
* balance/token changes
* expected vs realized price
* expected vs actual fees
* expected vs observed reserves
* position state
* retry history

Confirmation outcomes:

* confirmed
* finalized
* failed
* expired
* dropped
* unknown
* conflicting provider evidence

Reconciliation failure modes:

* signature unknown
* provider disagreement
* account state mismatch
* balance mismatch
* partial side-effect
* duplicate execution
* position not reflected
* stale post-state

Unknown is not success.

---

# 15. Off-Chain Runtime Responsibilities

The off-chain runtime should own:

* stream connection lifecycle
* event normalization
* event routing
* pool/session lifecycle
* canonical state update routing
* duplicate suppression
* observation-window timing
* feature snapshot materialization
* execution eligibility
* transaction building
* submission attempts
* confirmation tracking
* reconciliation
* decision logging
* metrics

Each responsibility must have clear ownership.

Avoid:

* hidden background mutation
* untracked detached tasks
* unbounded event buffers
* execution from stale session state
* decision logging after state loss
* retry loops without ownership

---

# 16. Runtime Error Classification

All runtime failures should map to one or more of:

* data problem
* freshness problem
* timing problem
* blockhash problem
* account problem
* authority problem
* compute problem
* fee problem
* provider/network problem
* parser problem
* state ownership problem
* reconciliation problem
* invariant violation

Do not use generic “failed” buckets for production decisions.

Failure records should include:

* component
* operation
* pool/token/session id
* slot reference
* blockhash age if relevant
* snapshot id if relevant
* attempt number
* error class
* retry/abandon decision
* supporting evidence

---

# 17. Hot-Path Runtime Discipline

Hot paths include:

* stream receive
* event normalization
* deduplication
* account state update
* transaction routing
* feature materialization
* evaluation trigger
* execution intent construction

Avoid:

* blocking calls
* expensive clones
* excessive string allocation
* broad locks
* nested locks
* unbounded maps/queues
* heavy parsing in repeated loops
* logging that allocates heavily
* recomputing features unnecessarily

Prefer:

* bounded queues
* cheap event keys
* compact structs
* immutable snapshots
* append-only evidence
* narrow locks
* prevalidated metadata
* explicit backpressure

Hot-path optimization must not weaken correctness, replayability, or observability.

---

# 18. Observability

Required metrics:

* event lag
* queue lag
* account update lag
* decision latency
* decision-to-submit latency
* submit-to-confirm latency
* blockhash age
* retry accumulation
* stale execution rejection rate
* duplicate suppression count
* replay divergence
* reconciliation mismatch rate
* provider error rate

Required logs:

* slot reference
* blockhash age
* pool/token/session id
* snapshot id/version
* transaction lifecycle state
* reason codes
* retry attempts
* execution outcome
* reconciliation outcome

Rules:

* failures must be diagnosable
* high-volume logs must not destroy latency
* reason codes must survive retries
* metrics should not create hot-path allocation spikes

---

# 19. Code Review Checklist

Before finalizing non-trivial Solana runtime changes:

* canonical state ownership preserved
* materialized snapshot boundary preserved
* observation-window semantics respected
* event ordering assumptions explicit
* duplicate handling deterministic
* stale-state handling implemented
* blockhash lifecycle handled
* transaction validity window defined
* retries bounded and idempotent
* account contention considered
* compute/fee assumptions explicit
* simulation not treated as inclusion
* confirmation/reconciliation path defined
* execution immutability preserved
* error classes specific
* observability sufficient
* hot-path behavior not degraded
* no hidden state mutation
* no widened synchronization scope
* no execution-time feature recomputation

---

# 20. Final Principle

Solana selective execution systems operate under adversarial timing, incomplete state, and short validity windows.

Correct design means:

* know which state is authoritative
* know when the decision was made
* know whether execution is still valid
* know what was submitted
* know whether it landed
* know how reality differs from intent
* know how to replay the whole chain of evidence

Correctness dominates speed.
Replayability dominates convenience.
Reconciliation dominates optimism.
Deterministic runtime behavior dominates abstraction.
Signal quality dominates execution frequency.