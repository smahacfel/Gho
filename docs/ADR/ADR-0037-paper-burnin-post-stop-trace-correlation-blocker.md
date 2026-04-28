# ADR-0037: Paper burn-in post-stop remains blocked by trace correlation orphan shadow

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

The current paper-burnin run was explicitly stopped after BUY admission and paper lifecycle recovery had already been observed. A fresh formal post-stop execution of `scripts/shadow_run_report.py` was required to determine whether the closed session could now satisfy the PR-7 promotion gate.

## Decision

We terminated the active `ghost-launcher` process for the paper-burnin run and re-ran the formal report against the current artifacts.

The post-stop report for session `launcher-1774641626788` still returns `NO-GO`, but the remaining blocker has narrowed to trace correlation only.

Fresh post-stop report facts:
- `paper_lifecycle_complete = passed`
- `economics_not_fatal = passed`
- `paper_admitted = 4`
- `paper_completed = 4`
- `paper_closed = 4`
- `paper_inflight = 0`
- `shadow_success = 48`
- `total_net_pnl_sol = 0.000009601416101731039`
- `trace_correlation = failed`
- `missing_paper_for_shadow = ["EqviqeGKqS3tmoYr2LmMECF62G38kVY6XeL7C3Dmpump_J42MsySWuTchkmVKXbq38tc1NgK5ZdxdbiEVqD87iPb2_1774642169647"]`

We verified that the orphan candidate exists in:
- `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`
- `logs/shadow_run/paper-burnin-buys.jsonl`

and is absent from the paper event stream in `datasets/events/paper-burnin`.

## Architectural Impact

This confirms that the relaxed configuration fixed the previous zero-BUY / zero-paper-admission state. The system now completes paper lifecycles and closes positions cleanly.

However, rollout promotion remains blocked because the canonical trace-correlation contract still requires every successful shadow dispatch in scope to have a corresponding paper trace.

## Risk Assessment

**Rate:** Medium

The runtime is materially healthier than before, but the orphan shadow implies a remaining observability or admission-gap defect. Promoting to PR-7 while this invariant is broken would weaken the audit trail between gatekeeper decisions, shadow dispatch, and paper execution.

## Consequences

What becomes easier:
- We can now rule out restrictive filtering as the primary blocker.
- The session is fully closed; the report is no longer distorted by in-flight positions.

What becomes harder:
- Promotion cannot rely on economics or lifecycle success alone.
- The remaining defect now requires targeted forensic analysis of the shadow-to-paper handoff for one candidate.

## Alternatives Considered

### 1. Treat post-stop run as acceptable because paper lifecycle completed
Rejected because `trace_correlation` remains a formal rollout gate.

### 2. Ignore the single orphan shadow as acceptable noise
Rejected because PR-7 promotion requires exact traceability, not approximate operator confidence.

### 3. Freeze current evidence and perform narrow root-cause analysis on the orphan candidate
Accepted as the correct next step.

## Validation Steps

1. Stop the active paper-burnin `ghost-launcher` process.
2. Re-run the formal report with the rollout config and metrics artifacts.
3. Confirm `paper_inflight = 0` and `paper_lifecycle_complete = passed`.
4. Confirm the remaining failure is `trace_correlation` only.
5. Trace the orphan candidate `EqviqeGKqS3tmoYr2LmMECF62G38kVY6XeL7C3Dmpump_J42MsySWuTchkmVKXbq38tc1NgK5ZdxdbiEVqD87iPb2_1774642169647` across gatekeeper, shadow, and paper event paths before any further promotion decision.
