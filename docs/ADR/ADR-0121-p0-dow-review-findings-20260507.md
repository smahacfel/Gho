# ADR-0121: P0 DOW review findings 2026-05-07

**Date:** 2026-05-07
**Status:** Accepted
**Author:** Ghost Father

## Context
P0 in `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md` defines the contract for DOW timing reliability: timer-fired Early/Normal/Extended checkpoints independent of TX flow, a real Extended checkpoint path, deadline-safe fallback semantics, a single serialized owner per pool/stage, explicit insufficient-data telemetry, and regression coverage for race safety and stage timing.

A read-only audit was requested for the implemented P0 work in `ghost-launcher`, with special focus on `gatekeeper_dow_timer.rs`, `oracle_runtime.rs`, `oracle_metrics.rs`, `gatekeeper_v25_regression.rs`, and DOW/deadline-related code in `gatekeeper.rs`.

## Decision
The reviewed P0 implementation does **not** fully satisfy the plan contract and should not be treated as closed.

The audit identified four material gaps:
1. `maybe_fire_shadow_checkpoint()` implements Normal/Extended as lower-bound deadlines (`>= 7000`, `>= 10000`) instead of distinct 5–7s / 7–10s windows, and it re-emits `InsufficientData` on every tick because `*_shadow_fired` is not latched for insufficient-data outcomes.
2. The timer path and deadline fallback path use different confidence formulas against the same DOW thresholds, so Extended outcomes are path-dependent.
3. `extended_require_pdd_clean` is configured but not consumed in either Extended decision path.
4. The regression tests partially encode the wrong behavior, so they allow or expect duplicates and weakened cardinality guarantees instead of enforcing the P0 DoD.

## Architectural Impact
This directly affects Gatekeeper V2.5 DOW semantics, shadow telemetry correctness, and auditability of Extended-stage decisions. It also weakens the SSOT expectation that timer path and deadline fallback represent the same decision model under different timing conditions.

The affected components are tightly coupled: `gatekeeper.rs` stage gating, `oracle_runtime.rs` per-pool timer orchestration, `oracle_metrics.rs` timer counters, and `gatekeeper_v25_regression.rs` acceptance coverage. Because P0 is on the critical path for later repair phases, these gaps can invalidate downstream conclusions drawn from shadow-burnin telemetry.

## Risk Assessment
**Rate: High**

Primary risks are misclassified DOW stage timing, inflated per-stage timer metrics, duplicate shadow records under quiet pools, and non-deterministic Extended outcomes depending on whether the timer or deadline fallback owns the decision. Test false positives increase regression risk because the current suite does not reliably detect these failures.

## Consequences
The current implementation provides useful scaffolding: a per-pool timer exists, Extended is no longer `unreachable!()`, and the runtime path is serialized through the same session owner. However, the behavioral contract remains materially open.

Operationally, this means P0 telemetry cannot yet be trusted as a clean measure of golden-window coverage, single-owner behavior, or timer-vs-deadline parity. Promotion decisions based on the current data would be unsound.

## Alternatives Considered
1. **Accept implementation as “good enough” because timer infrastructure exists.** Rejected because the plan contract is behavioral, not merely structural.
2. **Treat duplicate `InsufficientData` emissions as acceptable telemetry noise.** Rejected because P0 explicitly requires one serialized owner per pool/stage and explicit race-safe behavior.
3. **Treat timer-path and deadline-path confidence divergence as harmless implementation detail.** Rejected because both paths gate against the same configured thresholds and therefore must share semantics.

## Validation Steps
1. Rework stage-open predicates so Early/Normal/Extended are modeled as contract windows, not only terminal lower bounds.
2. Enforce exactly-once stage firing semantics per pool/stage while still preserving deadline fallback eligibility where required.
3. Unify Extended confidence computation across timer and deadline fallback paths.
4. Either honor `extended_require_pdd_clean` or remove the dead toggle from the contract surface.
5. Rewrite P0 regression tests so they assert exact stage timing, exact stage cardinality, explicit insufficient-data reason tagging, and race-safe parity between timer and deadline fallback.
6. Re-run `cargo test -p ghost-launcher --test gatekeeper_v25_regression -- --nocapture` after remediation.
