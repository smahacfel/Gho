# Trading Systems Reference

This reference contains the detailed phase-by-phase specification used by `trading-systems`.

## Operating assumptions

- most apparent opportunities are traps
- most signals decay once exposed to execution pressure
- latency matters, but selection quality matters more
- risk management is not a subsystem; it is the system boundary
- execution without reconciliation is incomplete
- a system that cannot explain its decisions cannot be trusted
- state drift is inevitable unless actively controlled
- stale data, duplicate delivery, and partial failure are normal conditions

## Phase 0 - System Boundary Definition

### 0.1 Scope
- markets/instruments in scope
- target horizon
- latency regime
- capital pool
- discretionary vs semi-automated vs autonomous operation

### 0.2 Constraints
- maximum drawdown
- per-trade risk
- maximum inventory
- max exposure per asset/sector/cohort
- slippage and fee assumptions
- compute/network budget
- data freshness budget
- retry budget

### 0.3 Information flow
- data available at decision time
- data that arrives too late
- cache vs live recompute boundary
- behavior under missing/stale/duplicate/inconsistent feeds

### 0.4 Handoff boundaries
- discovery -> `large-data-analytics`
- validation/falsification -> `statistical-research-engine`
- probability/calibration -> `statistics-probability`
- on-chain execution details -> `solana-pumpfun-architect`
- abstract decomposition -> `abstract-reasoning`

If boundaries cannot be defined, stop and request clarification.

## Phase 1 - Strategy and Signal Integration

### 1.1 Signal qualification
- out-of-sample usefulness
- temporal stability
- availability at decision time
- non-redundancy vs existing stack
- robustness to window/regime shifts
- cost-adjusted value (fees/slippage)

### 1.2 Signal transformation
Transform raw signal into:
- direction (buy/sell/hold/reduce/abort)
- size (units/notional/risk fraction)
- timing (immediate/conditional/delayed/abandoned)
- confidence (calibrated probability/score/rank)
- invalidation condition

### 1.3 Signal fusion
- explicit combine rule: weighted, voting, sequential, gated, hierarchical
- explicit conflict handling
- suppression of weak contradictory signals
- prevent dominance by one noisy feature

### 1.4 Signal freshness
Reject when:
- source stale
- supporting state materially changed
- freshness window expired
- assumptions invalid at execution time

## Phase 2 - Scoring Engine Design

### 2.1 Score definition
Every score declares:
- range
- interpretation
- monotonicity expectations
- calibration method
- degraded-input behavior

### 2.2 Score composition
- linear weighted
- logistic/sigmoid
- tree ensemble
- deterministic rule score
- hybrid with explicit gating

### 2.3 Score semantics
Score answers one question:
- "How strong is the case for action under current conditions?"

Avoid mixing edge quality, profitability, confidence, feasibility, and risk appetite in one opaque number.

### 2.4 Uncertainty propagation
Output:
- score
- uncertainty/confidence band
- regime class (if used)
- validity flag
- reason codes

## Phase 3 - Decision Engine

### 3.1 Pipeline
`Signal -> Validation -> Score -> Threshold Check -> Risk Check -> Execution Eligibility -> Order Construction -> Submission -> Confirmation -> Reconciliation`

### 3.2 Precedence
1. hard safety rules
2. risk limits
3. exposure limits
4. signal threshold
5. execution feasibility
6. optional optimizations

Later stage cannot override earlier hard rule unless policy explicitly allows.

### 3.3 Explicit states
- observed
- candidate
- validated
- sized
- approved
- submitted
- partially filled
- filled
- rejected
- expired
- canceled
- reconciled
- failed

Implicit transitions are forbidden.

### 3.4 Overrides (rare)
- emergency stop
- manual veto
- circuit breaker
- forced de-risking
- lockout after repeated failures

## Phase 4 - Position Sizing & Risk

### 4.1 Sizing method (choose explicitly)
- fixed fraction
- volatility-adjusted
- confidence-weighted
- dampened Kelly
- regime-adjusted

### 4.2 Risk aggregation
- per-trade
- per-asset
- portfolio total
- intraday loss
- peak-to-trough drawdown
- correlated exposure clusters

### 4.3 Limits
- soft limits: warn/reduce/confirm
- hard limits: reject/stop/flatten

### 4.4 Calibration stress cases
- high volatility
- low liquidity
- delayed execution
- partial fills
- correlated drawdown
- stale signal bursts

### 4.5 Invariants
- no single decision can silently exceed max exposure
- one bad feed cannot override safety
- one winning trade cannot hide cumulative loss

## Phase 5 - Execution Orchestration

### 5.1 Order intent
- instrument/token
- side
- size
- max slippage
- time-in-force
- validity window
- invalidation condition
- expected execution path

### 5.2 Path
1. construct
2. pre-trade checks
3. sign/authorize
4. submit
5. accept/reject observation
6. confirmation wait
7. evidence-chain verification
8. reconcile

### 5.3 Retry policy
- max retries
- spacing
- retry conditions
- abandon conditions
- rebuild-from-fresh-state threshold

### 5.4 Fallbacks
- timeout
- partial fill
- stale blockhash / expired context
- transport failure
- venue reject
- insufficient balance
- insufficient compute/fee budget
- changed liquidity conditions

### 5.5 Discipline
- deterministic ordering where required
- idempotency
- duplicate suppression
- stale intent invalidation
- confirmation over optimistic local state

## Phase 6 - Reconciliation & Post-Trade Integrity

### 6.1 Reconcile expected vs actual
- requested vs filled size
- intended vs realized average price
- estimated vs actual fees
- submit time vs confirm time
- expected vs observed state

### 6.2 Failure classes
- signal failure
- freshness failure
- liquidity failure
- execution failure
- network failure
- risk rejection
- reconciliation mismatch
- unknown/unclassified

### 6.3 State updates after each attempt
- position state
- realized/unrealized P&L
- risk counters
- exposure totals
- decision journal
- recovery metadata

### 6.4 Feedback loop targets
- signal confidence
- threshold tuning
- risk calibration
- execution tuning
- feed reliability scoring

## Phase 7 - System-Level Integrity

### 7.1 Idempotency
- duplicate intent suppression
- duplicate fill handling
- retry safety
- crash-safe re-entry without double counting

### 7.2 Persistence
- positions
- pending intents/orders
- risk state
- feed markers
- decision journal
- recovery checkpoints

### 7.3 Crash recovery
- startup reconciliation sequence
- replay procedure
- orphan order detection
- partial state repair
- safe restart mode

### 7.4 Health checks
- market data feed health
- RPC/transport health
- balance sufficiency
- clock drift / timestamp validity
- confirmation latency
- stale-state accumulation
- retry-failure accumulation

### 7.5 Circuit breakers
- repeated reconciliation mismatch
- feed inconsistency
- excessive slippage
- consecutive execution failures
- drawdown breach
- invalid state transitions

## Phase 8 - Validation & Testing

### 8.1 Backtesting minimums
- out-of-sample
- walk-forward
- realistic fees/slippage
- latency/liquidity assumptions
- partial-fill modeling where relevant

### 8.2 Simulation/paper
- simulate
- paper trade
- compare expected vs observed behavior
- validate transitions and failure handling

### 8.3 Stress tests
- extreme volatility
- low liquidity
- sudden gaps
- delayed confirmations
- feed outages
- stale bursts
- duplicate events
- partial fills
- repeated rejections

### 8.4 Validation bar
Validated only if system:
- survives realistic stress
- preserves risk limits
- reconciles correctly
- fails safely
- explains decisions

## Phase 9 - Observability & Decision Journal

### 9.1 Required logs
- timestamp
- decision id
- signal id
- score
- reason codes
- risk state
- execution state
- result state

### 9.2 Required metrics
- hit rate
- selection precision
- slippage
- rejection rate
- stale rejection rate
- fill ratio
- confirmation latency
- drawdown
- recovery time
- feed reliability

### 9.3 Decision journal
Record:
- why decision taken
- supporting evidence
- active assumptions
- what later proved true/false

## Failure modes to detect and name

- signal latency overfitting
- risk-parameter overfitting
- survivorship bias
- infinite-liquidity assumption
- zero-slippage assumption
- fee omission
- stale signal execution
- decision/execution race
- retry double counting
- uncalibrated confidence
- execution drift from intent
- post-fill state mismatch

If detected, pause unsafe rollout and recommend remediation.

## Uncertainty policy

Never:
- present backtest as guarantee
- hide fees/slippage/latency assumptions
- claim safety without hard limits
- size without confidence model
- assume fill without evidence

Always:
- state market assumptions
- disclose simulation limits
- report worst-case with averages
- mark unverified claims as provisional

## Output expectations

Generated code/design should include:
- modular components (ingestion, validation, scoring, decision, risk, execution, reconciliation, observability)
- explicit config for thresholds/limits/retries/circuit breakers
- reason codes for accept/reject outcomes
- robust external-call error handling
- idempotent or safely repeatable paths
- no hidden state
- no hardcoded risk parameters
- no silent failure path
- test scaffolding for backtesting and stress scenarios
