---
name: trading-systems
description: "Selective trading system architecture: decision engines, scoring models, order routing, risk management, position sizing, execution orchestration, and post-trade reconciliation for high-precision autonomous trading (not HFT, not MEV)."
allowed-tools: "Read, Edit, Grep, Bash, Python"
---

# Trading Systems - Selective Execution & Decision Integrity

Use this skill when the task involves:
- designing or modifying trading system architecture
- building decision engines or scoring models for trade selection
- implementing routing, execution, retry, and fallback policies
- defining position sizing, risk limits, drawdown controls, and circuit breakers
- orchestrating off-chain bots that interact with on-chain programs
- integrating validated signals into executable, auditable logic
- reconciling trade outcomes, partial fills, and failure states

## Quick Start

When this skill is activated, begin with:

> [Trading Systems] I will define system boundaries, integrate validated signals, design a scoring engine with uncertainty, enforce ordered decision rules, apply hard risk limits, orchestrate execution with retries and invalidation, and reconcile outcomes with explicit failure classification.

Then execute the phases in strict order (0-9): boundary -> signal integration -> scoring -> decisions -> risk -> execution -> reconciliation -> integrity -> validation -> observability.

## Operating Doctrine

- Selectivity over speed; correctness over throughput; survival over short-term gain.
- Risk management is the system boundary, not an optional module.
- Execution is incomplete until reconciliation closes the evidence chain.
- Stale data, duplicates, and partial failures are normal and must be handled explicitly.
- Every accepted or rejected action must have traceable reason codes.

## Non-Negotiable Rules

1. Never bypass position sizing, per-trade risk, or drawdown limits.
2. Never let score outputs bypass hard risk or exposure checks.
3. Never execute without invalidation/exit conditions.
4. Never assume liquidity, fill, or finality without evidence.
5. Never allow silent state mutation from execution paths.
6. Never treat backtests as forward guarantees.
7. Never hide failures; classify, reconcile, and feed back into tuning.

## Required Architecture Shape

Every design or implementation should explicitly separate:
- ingestion
- validation/freshness
- scoring (with uncertainty)
- decision policy
- risk and sizing
- execution orchestration
- reconciliation
- observability and decision journal

Avoid monolithic execution flows and hidden shared state.

## Handoff Boundaries

Hand off specialist logic instead of re-implementing it here:
- signal discovery or pattern mining -> `large-data-analytics`
- signal falsification / robust validation -> `statistical-research-engine`
- probability, calibration, statistical testing -> `statistics-probability`
- Solana on-chain design / tx construction / execution specifics -> `solana-pumpfun-architect`
- high-level decomposition of ambiguous architecture -> `abstract-reasoning`

If boundary, constraints, or ownership are unclear, stop and request clarification.

## Failure-Mode Discipline

Detect and explicitly name at least:
- stale signal execution
- race between decision and execution
- double counting from retries
- execution drift from intent
- uncalibrated confidence
- liquidity/slippage assumption errors
- post-fill state mismatch

When detected, stop unsafe rollout and recommend remediation.

## Required Final Review Checklist

Before finalizing any solution:
- [ ] system boundary and constraints defined
- [ ] signal qualification and freshness gates defined
- [ ] score interpretation + uncertainty defined
- [ ] ordered decision precedence enforced
- [ ] sizing and hard risk limits explicit
- [ ] execution retries/fallbacks/invalidation explicit
- [ ] reconciliation and failure classification defined
- [ ] idempotency and crash recovery covered
- [ ] observability and decision journal present
- [ ] assumptions and uncertainty disclosed
- [ ] specialist handoffs respected

## Detailed Reference

Use the full phase-by-phase specification in [reference.md](reference.md).
