---
name: trading-systems
description: "Selective trading system architecture: decision engines, scoring models, order routing, risk management, position sizing, execution orchestration, and post-trade reconciliation for high-precision autonomous trading (not HFT, not MEV)."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Trading Systems - Selective Execution & Decision Integrity

Use this skill when the task involves:

* designing or modifying trading system architecture
* building decision engines or scoring models for trade selection
* implementing routing, execution, retry, and fallback policies
* defining position sizing, risk limits, drawdown controls, and circuit breakers
* orchestrating off-chain bots that interact with on-chain programs
* integrating validated signals into executable, auditable logic
* reconciling trade outcomes, partial fills, and failure states

---

# Quick Start

When activated:

> [Trading Systems] I will define system boundaries, integrate validated signals, design a scoring engine with uncertainty, enforce ordered decision rules, apply hard risk limits, orchestrate execution with retries and invalidation, and reconcile outcomes with explicit failure classification.

Then execute phases:
boundary → signals → scoring → decisions → risk → execution → reconciliation → integrity → validation → observability

---

# Operating Doctrine

* Selectivity over speed; correctness over throughput; survival over short-term gain.
* Risk management is the system boundary, not an optional module.
* Execution is incomplete until reconciliation closes the evidence chain.
* Stale data, duplicates, and partial failures are normal and must be explicitly handled.
* Every decision must produce traceable reason codes.
* Deterministic reasoning is preferred over heuristic ambiguity.

---

# HARD vs SOFT Constraints

## HARD constraints (never violated)

* position sizing limits
* drawdown limits
* invalidation / exit conditions
* execution safety rules
* idempotency guarantees

## SOFT constraints (optimization layer)

* performance
* latency
* execution quality
* slippage minimization

Never confuse soft constraints with hard constraints.

---

# STATE MODEL (required)

All systems must explicitly model state:

* INIT
* VALIDATED
* SCORED
* DECIDED
* EXECUTED
* FAILED
* RECONCILED

Rules:

* transitions must be deterministic
* transitions must be logged
* no hidden state mutation allowed

---

# NON-NEGOTIABLE RULES

1. Never bypass risk, sizing, or exposure controls.
2. Never allow score output to directly trigger execution without risk gating.
3. Never execute without invalidation conditions.
4. Never assume liquidity, fills, or finality without evidence.
5. Never allow silent state mutation.
6. Never treat backtests as forward truth.
7. Never hide failures — classify and propagate them.

---

# REQUIRED ARCHITECTURE SHAPE

System must always separate:

* ingestion
* validation / freshness
* scoring (with uncertainty)
* decision policy
* risk & sizing
* execution orchestration
* reconciliation
* observability / decision journal

Avoid:

* monolith execution flows
* shared implicit state
* hidden coupling between scoring and execution

---

# FAST PATH RULE (critical addition)

If task is:

* localized
* single-module
* non-architectural
* non-risk-related

Then:

* skip full pipeline reasoning
* use minimal safe execution path
* avoid unnecessary decomposition

---

# DETERMINISM REQUIREMENT

* identical inputs + state snapshot MUST produce identical decisions
* scoring must be reproducible
* no stochastic drift in decision layer unless explicitly parameterized

---

# HANDOFF BOUNDARIES

Delegate instead of solving:

* signal discovery / mining → `large-data-analytics`
* statistical validation → `statistical-research-engine`
* probability / calibration → `statistics-probability`
* Solana execution layer → `solana-pumpfun-architect`
* deep system decomposition → `abstract-reasoning`

If unclear → STOP and request clarification.

---

# FAILURE MODE DISCIPLINE

Explicitly detect:

* stale signal execution
* decision/execution race conditions
* retry double counting
* execution drift vs intent
* uncalibrated confidence
* liquidity/slippage mismatch
* post-fill state inconsistency

On detection:

* stop rollout
* classify failure
* propose mitigation

---

# CONVERGENCE RULES (NEW)

Stop reasoning when:

* additional decomposition does not change decision
* uncertainty is bounded and stable
* no new actionable constraints appear
* further analysis yields diminishing returns

Avoid infinite reasoning loops.

---

# ANTI–THEORY DRIFT (NEW)

* Do not prefer elegant models over operational reality
* If empirical constraints contradict reasoning → prioritize empirical truth
* Always reconcile abstraction with execution constraints

---

# REQUIRED FINAL REVIEW CHECKLIST

Before completion:

* [ ] system boundaries defined
* [ ] signal freshness validated
* [ ] scoring uncertainty defined
* [ ] decision precedence enforced
* [ ] HARD vs SOFT constraints separated
* [ ] state model respected
* [ ] execution safety verified
* [ ] reconciliation defined
* [ ] idempotency ensured
* [ ] failure modes checked
* [ ] determinism confirmed
* [ ] handoffs respected
* [ ] no open contradictions remain

---

# OUTPUT EXPECTATION

Must include:

* explicit reasoning only where needed
* structured decision path
* failure awareness
* uncertainty disclosure
* no hidden assumptions
* no implicit state transitions

---

# FINAL PRINCIPLE

Selectivity > speed
Correctness > complexity
Auditability > cleverness
Determinism > intuition
System integrity > local optimization

```
