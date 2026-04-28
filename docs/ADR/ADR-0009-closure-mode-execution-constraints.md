# ADR-0009: Closure-Mode Execution Constraints for Pipeline Remediation

**Date:** 2026-03-20  
**Status:** Proposed  
**Author:** Ghost Father  

## Context

The pipeline remediation effort has reached the stage where the main risk is no longer missing capability, but uncontrolled interpretation during implementation.

The system already has a strong canonical decision core, but still suffers from:

- multiple state write paths that require explicit precedence enforcement
- WAL and recovery behavior that must become decision-order aware, not append-order driven
- semantic drift between raw-chain and synthetic event sources
- risk of adding more features before the current architecture is fully closed and hardened

A looser implementation plan would leave too much room for scope creep, caller-level precedence logic, and partial fixes that look complete in documentation but are not enforced in runtime behavior.

## Decision

The remediation program will execute in **closure mode** under the following constraints:

1. No new feature work, source modes, or non-essential execution-path enhancements may be added while this plan is in progress.
2. `ShadowLedger` precedence must be enforced in storage/arbitration code, not only in comments, docs, or caller logic.
3. Recovery-critical WAL replay must be based on explicit replay-order metadata aligned with decision-order, not append-order or wall-clock order.
4. Synthetic and raw-chain events must be normalized through one explicit semantic contract before downstream interpretation.
5. Legacy paths must be explicitly classified and prevented from producing unauthorized side effects.
6. A phase cannot be considered complete unless code, telemetry, tests, and documentation are updated together.

These constraints are concretely expressed in:

- `/root/Gho/PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`

## Architectural Impact

This decision affects implementation sequencing across:

- `ghost-core` — precedence enforcement and replay ordering
- `ghost-launcher` — startup restore ordering, legacy-path guards, runtime semantics
- `seer` — source normalization boundary and event meaning propagation
- `ghost-brain` — correct separation between pre-commit soft truth and canonical state

The architectural impact is primarily one of **constraint hardening**: turning implicit assumptions into explicit, enforced rules.

## Risk Assessment

**Rate:** High

Why high:

- allowing caller-level precedence logic would preserve silent state corruption risk
- allowing append-order replay would create a false sense of deterministic recovery
- allowing scope creep during remediation would re-open ambiguity before it is closed
- allowing legacy paths to remain only “documented” but not guarded would preserve duplicate side-effect risk

## Consequences

### Positive

- Reduces room for implementer interpretation
- Forces correctness-critical rules into code instead of prose
- Makes recovery and write authority testable and observable
- Prevents feature creep from undermining closure work

### Negative

- Slows down opportunistic feature additions during the remediation window
- Requires stricter sequencing and may temporarily block unrelated “small improvements”
- Increases the amount of up-front rigor before implementation work begins

## Alternatives Considered

1. **Keep the plan broad and let implementers decide details**
   - Rejected because this system is at the stage where ambiguity is itself the main defect.

2. **Allow feature work in parallel if it seems unrelated**
   - Rejected because new state paths and event semantics frequently create hidden coupling.

3. **Document precedence and replay behavior without enforcing it in code**
   - Rejected because this would preserve the gap between design intent and runtime behavior.

## Validation Steps

The closure-mode decision is valid only if the following remain true during implementation:

1. No new writer paths to `ShadowLedger` are introduced.
2. Precedence decisions are observable through code paths, logs, and metrics.
3. Recovery tests prove decision-order aware replay behavior.
4. Legacy side-effect suppression is covered by tests and runtime guards.
5. Each completed phase updates code, telemetry, tests, and documentation together.
