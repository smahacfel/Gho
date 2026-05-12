## `trading-systems/references.md`

# Trading Systems Reference

This file expands the `trading-systems` skill. Read it only when deeper architecture, scoring, risk, execution, replay, validation, or system-integrity guidance is needed.

Use this reference for:

* selective trading architecture
* decision pipeline design
* scoring and policy integration
* risk and sizing design
* execution orchestration
* reconciliation design
* replay/recovery planning
* validation and stress testing
* observability and decision journals

Do not load this file for small localized edits unless needed.

---

# 1. Operating Assumptions

For selective low-latency trading systems:

* most apparent opportunities are traps
* most signals decay once exposed to execution pressure
* latency matters, but selection quality matters more
* risk management is not a subsystem; it is the system boundary
* execution without reconciliation is incomplete
* a system that cannot explain decisions cannot be trusted
* state drift is inevitable unless actively controlled
* stale data, duplicate delivery, and partial failure are normal
* replay divergence is a correctness failure
* optimistic local state is not execution truth

The system must prefer:

* explicit state ownership
* immutable decision snapshots
* deterministic transitions
* bounded retries
* explicit invalidation
* reconciliation over optimism
* reason-code-compatible decisions

---

# 2. System Boundary Definition

Before changing architecture, define scope.

## Scope

Define:

* market/instrument scope
* target horizon
* latency regime
* capital pool
* execution mode
* autonomous vs semi-automated operation
* observation-window shape
* terminal decision semantics

## Constraints

Define:

* maximum drawdown
* per-trade risk
* maximum inventory
* maximum simultaneous exposure
* max exposure per asset/cohort/regime
* slippage and fee assumptions
* compute/network budget
* data freshness budget
* retry budget
* execution validity window

## Information Flow

Define:

* data available at decision time
* data that arrives too late
* cache vs live recompute boundary
* stale-data behavior
* duplicate-delivery behavior
* inconsistent-feed behavior
* fallback-state policy
* replay reconstruction path

If boundaries cannot be defined, stop and request clarification.

---

# 3. SSOT and Snapshot Discipline

A decision system must define:

* canonical source of truth
* state writer authority
* feature ownership
* materialization boundary
* snapshot identity/version
* freshness semantics
* replay reconstruction rule

Rules:

* decisions operate on immutable snapshots
* scoring must not mutate runtime state
* execution must not recompute authoritative features
* mixed live-state reads during scoring are forbidden
* dual-authority feature derivation is forbidden
* post-verdict updates must not rewrite historical decisions

Preferred model:

```text
event stream
→ normalized canonical state
→ materialized feature snapshot
→ decision policy
→ terminal verdict
→ execution eligibility
````

A decision should answer:

```text
Given this exact snapshot and this exact config, what is the verdict?
```

Not:

```text
What does the live runtime happen to say right now?
```

---

# 4. Observation Window Model

Selective systems should model bounded observation explicitly.

Common windows:

* first N milliseconds
* first N seconds
* first N slots
* first N transactions
* early / normal / extended phases
* terminal deadline

Lifecycle:

```text
OBSERVED
→ ACCUMULATING
→ EVALUATING
→ APPROVED / REJECTED / TIMED_OUT
→ COMMITTED / SUBMITTED
→ CONFIRMED
→ RECONCILED
```

Rules:

* stale windows terminate
* timeout is a terminal outcome
* late-arriving events are classified
* evaluation triggers are deterministic
* post-verdict mutation cannot alter the historical verdict
* execution must use a fresh-enough snapshot

Failure modes:

* accidental window extension
* partial-state evaluation
* stale state execution
* wall-clock/event-time mismatch
* duplicate events distorting counters
* terminal verdict overwritten
* late metadata rewriting decision assumptions

---

# 5. Strategy and Signal Integration

Signals must be qualified before integration.

## Signal Qualification

Check:

* out-of-sample usefulness
* temporal stability
* decision-time availability
* non-redundancy with existing stack
* robustness to window/regime shifts
* cost-adjusted value after fees/slippage
* runtime feasibility
* reason-code compatibility

## Signal Transformation

Transform raw signal into:

* direction: buy / sell / hold / reduce / abort
* size: units / notional / risk fraction
* timing: immediate / conditional / delayed / abandoned
* confidence: calibrated probability / score / rank
* invalidation condition
* supported regimes
* degraded-input behavior

## Signal Fusion

Use explicit combine rules:

* weighted
* voting
* sequential
* gated
* hierarchical
* veto-based

Required:

* conflict handling
* suppression of weak contradictory signals
* protection against dominance by one noisy feature
* explicit precedence over risk and safety rules

## Signal Freshness

Reject when:

* source is stale
* supporting state changed materially
* freshness window expired
* assumptions invalidated before execution
* signal cannot be reconstructed under replay

---

# 6. Scoring Engine Design

Every score must declare:

* range
* interpretation
* monotonicity expectation
* calibration method
* supported regimes
* degraded-input behavior
* validity flag
* reason-code mapping

Score should answer one question:

```text
How strong is the case for action under current known conditions?
```

Avoid mixing into one opaque number:

* edge quality
* profitability
* confidence
* execution feasibility
* risk appetite
* position sizing

Score composition options:

* deterministic rule score
* linear weighted score
* logistic/sigmoid score
* tree-based model
* hybrid with explicit gating

For selective systems, prefer:

* interpretable scoring
* monotonic components where possible
* explicit vetoes for hard failure
* confidence/uncertainty propagation
* reason-code-compatible outputs

Avoid:

* giant opaque ensemble scores
* hard-to-audit nonlinear interactions
* uncalibrated confidence
* execution logic hidden in score computation

---

# 7. Decision Engine

Recommended pipeline:

```text
Signal
→ Validation
→ Score
→ Threshold Check
→ Risk Check
→ Execution Eligibility
→ Order Construction
→ Submission
→ Confirmation
→ Reconciliation
```

Decision precedence:

1. hard safety rules
2. risk limits
3. exposure limits
4. freshness/invalidation rules
5. signal threshold
6. execution feasibility
7. optional optimizations

Later stages cannot override earlier hard rules unless policy explicitly allows.

Explicit decision states:

* observed
* candidate
* validated
* scored
* approved
* rejected
* timed_out
* submitted
* confirmed
* reconciled
* failed

Implicit transitions are forbidden.

Overrides should be rare:

* emergency stop
* manual veto
* circuit breaker
* forced de-risking
* lockout after repeated failures

---

# 8. Risk and Position Sizing

Risk management is the system boundary.

## Sizing Method

Choose explicitly:

* fixed fraction
* fixed notional
* volatility-adjusted
* confidence-weighted
* regime-adjusted
* dampened Kelly

For high-noise selective systems, use conservative sizing unless validation is strong.

## Risk Aggregation

Track:

* per-trade risk
* per-token/asset exposure
* total portfolio exposure
* intraday loss
* peak-to-trough drawdown
* correlated exposure clusters
* open intent exposure
* pending execution exposure

## Limits

Soft limits:

* warn
* reduce
* require confirmation
* degrade mode

Hard limits:

* reject
* stop
* flatten
* lock out
* disable execution

## Invariants

* no single decision silently exceeds max exposure
* one bad feed cannot override safety
* one winning trade cannot hide cumulative loss
* retry loops cannot bypass exposure limits
* shadow success cannot be treated as live safety proof

---

# 9. Execution Orchestration

Execution intent must define:

* instrument/token
* side
* size
* maximum slippage
* time-in-force
* validity window
* invalidation condition
* expected execution path
* retry policy
* abandon conditions

Execution path:

```text
construct
→ pre-trade checks
→ authorize/sign
→ submit
→ observe accept/reject/unknown
→ wait confirmation
→ verify evidence chain
→ reconcile
```

Retry policy must define:

* max retries
* spacing
* retry conditions
* abandon conditions
* rebuild-from-fresh-state threshold
* idempotency key or equivalent identity
* duplicate suppression

Fallbacks must handle:

* timeout
* stale blockhash/expired context
* transport failure
* venue/provider reject
* insufficient balance
* insufficient compute/fee budget
* changed liquidity conditions
* partial/unknown outcome

Execution discipline:

* deterministic ordering where required
* idempotency
* stale intent invalidation
* confirmation over optimistic local state
* bounded retry behavior

---

# 10. Reconciliation and Post-Trade Integrity

Execution is incomplete until reconciled.

Reconcile expected vs actual:

* requested vs filled size
* intended vs realized average price
* estimated vs actual fees
* submit time vs confirm time
* expected vs observed state
* expected vs actual position
* expected vs actual exposure

Failure classes:

* signal failure
* freshness failure
* liquidity failure
* execution failure
* network/provider failure
* risk rejection
* reconciliation mismatch
* unknown/unclassified

After each attempt update:

* position state
* realized/unrealized P&L
* risk counters
* exposure totals
* retry metadata
* decision journal
* recovery metadata

Feedback loop targets:

* signal confidence
* threshold tuning
* risk calibration
* execution tuning
* feed reliability scoring
* stale-state rejection rate

Unknown is not success.

---

# 11. Replay and Recovery

System-level integrity requires:

* deterministic replay
* crash recovery
* idempotency
* orphan detection
* partial-state repair
* safe restart mode

Persist:

* positions
* pending intents/orders
* risk state
* feed markers
* decision journal
* recovery checkpoints
* execution attempts
* reconciliation outcomes

Startup recovery should:

1. load persisted state
2. reconstruct pending intents
3. check outstanding executions
4. reconcile observed reality
5. repair or quarantine inconsistent state
6. enter safe runtime mode
7. resume only after invariants hold

Circuit breakers:

* repeated reconciliation mismatch
* feed inconsistency
* excessive slippage
* consecutive execution failures
* drawdown breach
* invalid state transitions
* replay divergence
* stale-state accumulation

---

# 12. Validation and Testing

Backtesting minimums:

* out-of-sample validation
* walk-forward analysis
* realistic fees/slippage
* latency assumptions
* liquidity assumptions
* partial-fill modeling where relevant
* failed/rejected candidates included

Simulation/paper validation:

* simulate decisions
* compare expected vs observed behavior
* validate state transitions
* validate failure handling
* validate reconciliation
* validate reason codes

Stress tests:

* extreme volatility
* low liquidity
* sudden gaps
* delayed confirmations
* feed outages
* stale bursts
* duplicate events
* partial fills
* repeated rejections
* provider disagreement
* replay divergence
* retry storms

Validation bar:

System is validated only if it:

* survives realistic stress
* preserves risk limits
* reconciles correctly
* fails safely
* explains decisions
* maintains replay integrity

---

# 13. Observability and Decision Journal

Required metrics:

* hit rate
* selection precision
* slippage
* rejection rate
* stale rejection rate
* fill ratio
* confirmation latency
* decision latency
* decision-to-submit latency
* retry accumulation
* drawdown
* recovery time
* feed reliability
* reconciliation mismatch rate

Required logs:

* timestamp domain
* decision id
* signal id
* snapshot id/version
* score
* reason codes
* risk state
* execution state
* result state
* reconciliation outcome

Decision journal records:

* why decision was taken
* why rejected decisions were rejected
* supporting evidence
* active assumptions
* source snapshot
* execution attempt chain
* what later proved true/false

A system that cannot explain decisions cannot be trusted.

---

# 14. Failure Modes

Detect and name:

* signal latency overfitting
* risk-parameter overfitting
* survivorship bias
* infinite-liquidity assumption
* zero-slippage assumption
* fee omission
* stale signal execution
* decision/execution race
* retry double counting
* uncalibrated confidence
* execution drift from intent
* post-fill state mismatch
* hidden state mutation
* replay divergence
* stale snapshot usage
* runtime-infeasible feature
* invalid state transition
* circuit breaker bypass

If detected:

* pause unsafe rollout
* classify the failure
* preserve evidence
* recommend remediation

---

# 15. Uncertainty Policy

Never:

* present backtest as guarantee
* hide fees/slippage/latency assumptions
* claim safety without hard limits
* size without confidence model or explicit fixed-risk rule
* assume fill without evidence
* treat simulation as live inclusion
* hide unsupported regimes

Always:

* state market assumptions
* disclose simulation limits
* report worst-case with averages
* mark unverified claims as provisional
* separate signal confidence from execution feasibility
* separate risk appetite from signal strength

---

# 16. Output Expectations

Generated code/design should include:

* modular components
* explicit config for thresholds/limits/retries/circuit breakers
* explicit state transitions
* reason codes for accept/reject outcomes
* robust external-call error handling
* idempotent or safely repeatable paths
* no hidden state
* no hardcoded risk parameters
* no silent failure path
* test scaffolding for backtesting/stress scenarios
* replay/recovery considerations
* observability hooks

---

# 17. Review Checklist

Before finalizing non-trivial system work:

* system boundary defined
* risk boundary defined
* canonical state owner defined
* decision snapshot boundary defined
* freshness semantics defined
* observation window semantics defined
* scoring semantics defined
* HARD vs SOFT constraints separated
* risk and exposure limits preserved
* execution validity defined
* retry/idempotency behavior defined
* reconciliation path defined
* replay/recovery path considered
* observability sufficient
* failure modes named
* handoffs respected
* no unresolved authority conflicts
* no hidden state mutation

---

# 18. Final Principle

Selective trading systems win by rejecting more bad opportunities than they accept.

Latency matters, but only after:

* state is correct
* snapshot is valid
* risk is bounded
* execution is fresh
* reconciliation is possible
* decision is explainable

Selectivity beats speed.
Correctness beats complexity.
Replayability beats convenience.
Auditability beats cleverness.
Determinism beats intuition.
System integrity beats local optimization.