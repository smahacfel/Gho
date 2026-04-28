# ADR-0008: Pipeline Authority, Event Semantics, and Recovery Remediation Plan

**Date:** 2026-03-20  
**Status:** Proposed  
**Author:** Ghost Father  

## Context

The current production pipeline has a strong canonical decision core, but surrounding contracts are not yet sufficiently formalized.

The most important architectural tensions are:

- `ShadowLedger` is fed from multiple write paths without a fully explicit authority hierarchy
- bootstrap semantics for `genesis_curve()` are duplicated across more than one runtime path
- multi-source ingest now mixes raw-chain and synthetic event origins, but downstream semantics are not yet normalized through a single contract
- shared WAL and disk snapshot APIs exist, but launcher startup does not yet perform full boot-time restore and replay orchestration
- legacy and canonical paths still coexist in ways that can confuse observability and, if left unguarded, side effects

A remediation plan is needed that preserves the current decision core while reducing ambiguity and hardening runtime recovery.

## Decision

A staged remediation plan will be adopted with the following order of execution:

1. Formalize `ShadowLedger` write authority and precedence before adding any new data paths or optimizations.
2. Establish single-writer bootstrap semantics for `genesis_curve()` and demote redundant bootstrap paths to no-op or explicit upgrade roles.
3. Define `AccountUpdate` as a repair/confidence-upgrade path, not an implicit canonical dependency for decision flow.
4. Introduce a unified semantic envelope for cross-source events so downstream logic can distinguish raw, adapted, and synthetic truth.
5. Wire boot-time recovery in launcher startup using disk snapshot restore plus WAL delta replay.
6. Unify freshness, finality, and `PendingCurve` policy under one explicit model and one config source of truth.
7. Separate legacy side effects from canonical runtime behavior with explicit guards, logging, and metrics.
8. Prove the contracts with precedence, recovery, cross-source, and restart tests.

The execution details are recorded in:

- `/root/Gho/PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`

## Architectural Impact

This decision affects all major runtime layers:

- `seer`: source routing, semantic normalization boundary, bootstrap initiation
- `ghost-launcher`: bridge behavior, event bus contracts, runtime startup ordering, legacy/canonical separation
- `ghost-core`: `ShadowLedger` authority model, precedence rules, recovery orchestration, disk snapshot + WAL replay
- `ghost-brain`: consumption semantics for pre-commit soft-truth versus canonical state

The main expected impact is not a new execution architecture, but a stricter contract around the one that already exists.

## Risk Assessment

**Rate:** High

Why high:

- precedence mistakes around `ShadowLedger` can produce silent state corruption or non-deterministic bootstrap behavior
- incomplete recovery orchestration can create a false sense of durability while still losing critical runtime state on restart
- semantic confusion between raw and synthetic event sources can bias heuristics, scoring, and operator interpretation
- legacy and canonical path overlap can create duplicate or misleading side effects

Regression risk is highest in:

- bootstrap and new-pool initialization
- recovery after process restart
- curve freshness/finality gating before Gatekeeper Phase 6
- observability and incident forensics

## Consequences

### Positive

- Makes write authority explicit instead of inferred from code archaeology
- Reduces the number of semantically active bootstrap paths
- Turns WAL/snapshot durability from partial infrastructure into actionable recovery behavior
- Gives downstream logic a reliable semantic contract for cross-source events
- Makes legacy paths safe to keep temporarily without pretending they are canonical

### Negative

- Requires coordinated changes across multiple crates and runtime boundaries
- Adds short-term implementation overhead for provenance, precedence metadata, and extra tests
- May temporarily expose design debt more clearly before the simplification work is complete

## Alternatives Considered

1. **Keep the current architecture and only improve documentation**
   - Rejected because the main issues are not only descriptive; they are contract and runtime-ordering problems.

2. **Rewrite the pipeline around a brand-new unified runtime**
   - Rejected because it would create unnecessary blast radius and delay critical hardening of the current production path.

3. **Prioritize performance/scaling first and defer authority/recovery cleanup**
   - Rejected because ambiguity in truth and restart semantics is more dangerous than current throughput limitations.

4. **Treat `AccountUpdate` as co-equal canonical truth with tx-first processing**
   - Rejected because it preserves ambiguity and makes tx-only mode semantically unstable.

## Validation Steps

The remediation will be considered valid only if all of the following are verified:

1. A complete matrix of `ShadowLedger` writers, their roles, and precedence exists and matches runtime behavior.
2. Duplicate bootstrap paths no longer create multiple semantically active `genesis_curve()` writes.
3. `tx_only` mode remains operational and explicitly supported.
4. Synthetic and raw event paths can be distinguished downstream via one normalized semantic contract.
5. Launcher startup restores state from disk snapshot and replays WAL deltas in the correct order.
6. `PendingCurve` has deterministic terminal behavior with metrics and config-driven policy.
7. Legacy paths are prevented from producing unauthorized side effects.
8. Integration and restart tests cover bootstrap, recovery, precedence, and cross-source semantic cases.
