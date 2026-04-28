# ADR-0018: Realignment of `PLAN_WYKONAWCZY.md` Phase III after Faza 4 closure

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

`PLAN_WYKONAWCZY.md` contained a Phase III narrative written before the durability/recovery closure captured in `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md` Faza 4. Since then, the repository gained concrete recovery and ordering capabilities:

- snapshot restore and WAL replay at launcher startup,
- `ReplayOrderKey` in WAL,
- staged commit / pending live / rollback recovery,
- recovery telemetry,
- launcher-side commit coordination and pending-live ownership,
- execution-layer BUY shadow-run (`shadow_only`, `live_and_shadow`) as a separate already-landed capability.

This made parts of the old Phase III plan stale or misleading, especially where PR-7 still implied that durability/recovery foundations or BUY-entry shadow-run were future work.

## Decision

Rewrite Phase III of `PLAN_WYKONAWCZY.md` to reflect the current repository state.

1. Reclassify PR-6 as residual extraction / transitional cleanup, not greenfield extraction.
2. Reclassify PR-7 as rollout closure for launcher commit coordination, centered on:
   - operatorski compare-only commit-path runtime,
   - divergence reporting,
   - switch gate,
   - final severing of production dependence on legacy gatekeeper runtime semantics.
3. Keep PR-8 as deletion-and-cleanup only, strictly after PR-7 rollout closure.
4. Keep PR-9 last.
5. Remove outdated claims that execution-layer BUY shadow-run is still backlog inside Phase III.
6. Explicitly document that `ghost-core/src/shadow_ledger/gatekeeper.rs` is not the current canonical runtime writer path, but still remains an active transitional dependency surface through exports, compatibility wiring, and legacy/equivalence tests.

## Architectural Impact

- The plan now matches actual code, reducing the risk of duplicate or regressive work.
- Phase III is narrowed to migration closure and legacy deletion, while durability/recovery responsibility stays in the already-completed closure-mode Faza 4 work.
- The repository gains a documented distinction between:
  - execution-layer BUY shadow-run, and
  - commit-path compare-only rollout for PR-7.

## Risk Assessment

**Rate:** Medium

Without this realignment:

- implementers could reopen already-finished Faza 4 scope,
- PR-7 could be treated as done based only on equivalence tests,
- PR-8 deletion could happen before production dependencies are truly removed,
- planning could continue to rely on outdated assumptions about `ghost-core` gatekeeper activity.

## Consequences

### Positive

- execution scope is now consistent with current code,
- stale backlog is removed from the active phase narrative,
- deletion safety for `ghost-core` gatekeeper is documented more precisely,
- next-step prioritization becomes clearer.

### Negative

- historical plan text no longer reads as a chronological build-up from scratch,
- some legacy test/document references still need future cleanup in PR-8.

## Alternatives Considered

### 1. Leave `PLAN_WYKONAWCZY.md` unchanged

Rejected because it no longer accurately described current repository reality.

### 2. Replace the whole plan with the closure-mode plan only

Rejected because `PLAN_WYKONAWCZY.md` still serves as a phase/PR execution document and remains useful after synchronization.

### 3. Delete the shadow-run annex entirely

Rejected because commit-path compare-only rollout remains an active blocker and still needs explicit execution guidance.

## Validation Steps

1. Verify the rewritten Phase III references only work still open after Faza 4.
2. Verify the plan distinguishes execution-layer BUY shadow-run from PR-7 compare-only commit-path rollout.
3. Verify the plan documents `ghost-core/src/shadow_ledger/gatekeeper.rs` as transitional-but-still-active rather than already safe to delete.
4. Use the rewritten plan as the authoritative execution baseline for choosing the next step after this change.