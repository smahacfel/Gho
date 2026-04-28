# ADR-0017: Reconciliation of PR-7..PR-9 after Faza 4 durability closure

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

`PLAN_WYKONAWCZY.md` still defines FAZA III PR-7 (`launcher-commit-coordinator`), PR-8 (`delete-core-gatekeeper`) and PR-9 (`transport-and-log-pressure`) as the remaining execution sequence for Gatekeeper migration. Since that plan was written, the newer closure-mode roadmap in `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md` completed Faza 4 durability work:

- `ReplayOrderKey` exists in `ghost-core/src/wal.rs`
- launcher startup already performs snapshot restore and WAL delta replay in `ghost-launcher/src/main.rs`
- staged commit / pending live / rollback recovery is implemented in `ghost-launcher/src/wal_recovery.rs`
- recovery telemetry is emitted (`shadow_ledger_restore_duration_ms`, `wal_replay_duration_ms`, `runtime_recovery_mode`)

An audit was required to determine which parts of PR-7..PR-9 remain current and which assumptions are now stale.

## Decision

Treat PR-7 as **partially implemented but not rollout-complete**.

1. Keep PR-7 as an active step, but narrow it to the work that is still genuinely open:
   - final severing of runtime commit orchestration from legacy/core gatekeeper ownership,
   - removal of semantic dependence on `GatekeeperRegistry` from launcher runtime paths,
   - delivery of the operatorski compare-only commit-path rollout mode and divergence reporting,
   - switch-gate proving launcher coordinator can become the sole canonical writer.

2. Mark the following PR-7 assumptions as already satisfied by landed code and no longer future work:
   - deterministic startup restore flow,
   - strict WAL replay ordering via `ReplayOrderKey`,
   - snapshot restore + WAL delta replay,
   - recovery of staged commit / pending live state,
   - recovery telemetry.

3. Keep PR-8 as still required, but only after PR-7 rollout closure. `ghost-core/src/shadow_ledger/gatekeeper.rs` and `GatekeeperRegistry` are still present and still referenced (at minimum by launcher equivalence tests and core exports), so deletion is not yet valid.

4. Keep PR-9 ordered last. Transport/log-pressure work remains intentionally downstream of correctness, migration closure, and legacy gatekeeper removal.

## Architectural Impact

This decision reclassifies the remaining migration work:

- **No new durability scope** belongs in PR-7 anymore; that responsibility moved into completed Faza 4 work.
- **Launcher** is already the effective owner of major parts of commit state (`LauncherCommitCoordinator`, launcher-side `pending_live`, post-commit LivePipeline bootstrap), so PR-7 is now a closure/refinement phase rather than a greenfield migration.
- **Core** still exposes legacy gatekeeper machinery, which prevents PR-8 from being treated as done.
- **Rollout safety** is now the main missing architectural control: repo has equivalence tests, but does not yet expose the operatorski compare-only runtime path and switch-gate that the old plan explicitly requires.

## Risk Assessment

**Rate:** Medium

Main risks if the plan is not reconciled:

- engineers may reopen already-finished durability work inside PR-7,
- rollout could be declared complete based only on tests, without compare-only operator evidence,
- PR-8 deletion could be attempted before legacy dependencies are truly severed,
- PR-9 transport cleanup could start too early and hide unresolved ownership bugs.

## Consequences

### Positive

- execution scope becomes precise and closure-oriented,
- avoids duplicating completed recovery work,
- preserves strict ordering: PR-7 rollout closure -> PR-8 deletion -> PR-9 transport/log pressure,
- makes the missing compare-only rollout path an explicit blocker instead of an implied TODO.

### Negative

- `PLAN_WYKONAWCZY.md` now needs textual reconciliation to avoid stale instructions,
- some tests and comments still reference legacy `GatekeeperRegistry`, which can create confusion until PR-8 lands.

## Alternatives Considered

### 1. Treat PR-7 as fully current with no changes

Rejected because the repo already contains the durability/recovery stack that PR-7 used to assume as pending work.

### 2. Skip directly to PR-8 deletion

Rejected because compare-only commit-path rollout and final ownership severance are not yet operator-complete.

### 3. Fold PR-9 transport cleanup into PR-7

Rejected because that would violate closure-mode sequencing and reintroduce scope creep.

## Validation Steps

1. Verify `ghost-launcher/src/main.rs` performs snapshot restore and WAL replay before live ingest.
2. Verify `ghost-launcher/src/wal_recovery.rs` restores staged commits, committed history, pending live deltas, and rollback seeds.
3. Verify `ghost-core/src/wal.rs` defines `ReplayOrderKey` for recovery-critical records.
4. Verify launcher runtime paths already use `LauncherCommitCoordinator` (`ghost-launcher/src/components/gatekeeper.rs`, `ghost-launcher/src/components/gatekeeper_commit_loop.rs`, `ghost-launcher/src/oracle_runtime.rs`).
5. Verify no operatorski compare-only commit-path runtime exists yet; only equivalence tests are present.
6. Before PR-8, confirm zero runtime dependencies on `GatekeeperRegistry` and `shadow_ledger::gatekeeper` outside explicitly transitional tests.