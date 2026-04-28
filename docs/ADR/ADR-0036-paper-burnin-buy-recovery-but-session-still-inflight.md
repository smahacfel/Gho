# ADR-0036: Paper burn-in recovered BUY admission but current session remains in-flight

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

After the earlier PR-7 promotion check, the latest paper-burnin session returned `NO-GO` because the session produced only candidate-level events and never reached admitted paper lifecycle stages.

The operator then relaxed the rollout configuration to reduce gate strictness and requested a fresh runtime verification against the currently running `tmux` session `bot_A`.

## Decision

We re-validated the running paper-burnin instance using live runtime output, current decision artifacts, and a fresh execution of `scripts/shadow_run_report.py`.

Verified state for the current session:
- `bot_A` is running and ingesting live pool transactions in `shadow_only` / `paper` mode.
- `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` now contains frequent `BUY` verdicts with `decision_verdict_buy=true` and `shadow_execution_outcome="shadow_background_spawned"`.
- The current paper-burnin session now admits paper trades and emits lifecycle events (`EntrySubmitted`, `EntryFilled`, `PositionOpened`, `PositionClosed`, `ManagementOutcome`).
- Fresh formal report result for session `launcher-1774641626788` remains `NO-GO`, but the remaining failure is now limited to `paper_lifecycle_complete` because one position is still in flight.

Fresh report summary:
- `shadow_success = 26`
- `paper_admitted = 3`
- `paper_completed = 2`
- `paper_closed = 2`
- `paper_inflight = 1`
- `total_net_pnl_sol = 0.000006479459092643504`
- `economics_not_fatal = passed`
- `trace_correlation = passed`
- `no_live_side_effects = passed`
- `recovery_contract = passed`

## Architectural Impact

This confirms that the execution pipeline is no longer blocked at the gatekeeper-admission boundary under the relaxed configuration. The canonical path from gatekeeper BUY decision to shadow dispatch to paper lifecycle is functioning again.

However, PR-7 promotion is still not yet unlocked because the formal rollout contract evaluates the latest session scope, and that scope must be fully closed before `paper_lifecycle_complete` can pass.

## Risk Assessment

**Rate:** Medium

Regression risk is lower than in the prior `NO-GO` state because the system now demonstrates end-to-end admissions and completed paper trades. The remaining risk is operational rather than structural: reading a still-running session too early can produce a temporary `NO-GO` even when the system is behaving correctly.

## Consequences

What becomes easier:
- We now have evidence that the restrictive config was a genuine cause of the previous zero-BUY / zero-admission state.
- Current paper lane shows real recovery with admitted and closed positions.

What becomes harder:
- Operators must avoid interpreting an in-flight session as a final promotion artifact.
- Formal PR-7 readiness still requires a completed session slice rather than a mid-run snapshot.

## Alternatives Considered

### 1. Treat current state as immediate PR-7 readiness
Rejected because the formal report still returns `NO-GO` for the latest scope due to one in-flight position.

### 2. Ignore the current report and rely only on raw logs
Rejected because rollout promotion must remain evidence-driven and tied to the formal report gates.

### 3. Freeze a completed session window before rerunning the report
Accepted as the next correct operational step once the current in-flight position resolves.

## Validation Steps

1. Confirm `tmux` session `bot_A` is alive and continuously emitting runtime logs.
2. Verify new `BUY` decisions in `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`.
3. Confirm lifecycle events exist for the current session in `datasets/events/paper-burnin`.
4. Run:
   - `scripts/shadow_run_report.py --config /root/Gho/configs/rollout/paper-burnin.toml --metrics-text /root/Gho/logs/rollout/paper-burnin/metrics.prom --json`
5. Wait for the remaining in-flight paper position to close or explicitly freeze the session window, then rerun the report to obtain the final promotion verdict.
