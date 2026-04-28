# ADR-0035: PR-7 blocked by latest paper burn-in NO-GO

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context
User requested a strict readiness audit against `PLANS/PLAN_WYKONAWCZY_PAPER_BURNIN_DO_MIKRO_LIVE_20260325.md`, explicitly skipping PR-5, to determine whether the repository may advance to PR-7 and, only if permitted, perform the first live run.

The audit had to avoid full workspace test compilation because of disk pressure. Therefore the readiness decision was based on source-level verification, rollout configuration verification, runbook verification, and formal artifact-based burn-in evidence.

Verified during this audit:
- Baseline stamp `/root/Gho/.ghost/baseline_accepted_revision` equals current repository HEAD `567bc6005b5907b116987339a9a82289759ceae9`.
- Recovery/durability markers are present in `logs/rollout/paper-burnin/system.log.2026-03-27`.
- No explicit live side effects, event-bus lag, or bulkhead safety violations were detected in the formal report scope.
- The latest session scope detected by `scripts/shadow_run_report.py` is `launcher-1774640502964`.
- That latest session contains only `Candidate` events with `REJECT`/`TIMEOUT` outcomes and no admitted paper lifecycle.

## Decision
PR-7 is blocked. No live run may be executed from this audit state.

The formal burn-in report generated from existing paper-burnin artifacts returned `NO-GO` for the latest detected session. The failing gates were:
- `paper_lifecycle_complete`
- `economics_not_fatal`

Observed latest-session facts:
- `shadow_rows=0`
- `shadow_success=0`
- `paper_admitted=0`
- `paper_completed=0`
- `paper_closed=0`
- `total_net_pnl_sol=0`

This means the current promotion evidence does not close PR-6 for the latest analyzed run window, so advancing to dual micro-live would violate the rollout contract.

## Architectural Impact
This decision preserves the rollout SSOT:
- PR-6 closure requires a formal paper burn-in `GO` verdict from artifacts, not just historical evidence that some earlier paper sessions completed.
- Promotion gating is currently anchored to the latest detected burn-in session scope from `datasets/events/paper-burnin`.
- Earlier successful paper lifecycle files do not override a latest-session `NO-GO` unless operators explicitly freeze and approve a different session slice as the acceptance scope.

The broader system impact is intentionally conservative: rollout promotion remains blocked until artifact scope, session slicing, and burn-in completion semantics are aligned with operator intent.

## Risk Assessment
**Rate:** High

If PR-7 were started despite this result, the system would enter live-side-effect territory without a closed PR-6 evidence chain. That would break the declared rollout sequence and make any live behavior non-compliant with the execution plan.

Regression risks affected:
- rollout governance and operator trust
- auditability of promotion decisions
- ability to attribute later live outcomes to a validated paper baseline

## Consequences
What becomes easier:
- Promotion decisions remain defensible and reproducible.
- Operators have a precise blocking condition: latest session has no admitted paper lifecycle and therefore cannot close PR-6.

What becomes harder:
- Historical successful paper runs cannot be used implicitly.
- If the intended acceptance scope was an earlier completed slice, operators must explicitly freeze that scope and regenerate the report for that accepted window.

## Alternatives Considered
### 1. Promote based on source inspection only
Rejected because the plan requires runtime closure evidence, not only code/config readiness.

### 2. Promote based on earlier successful paper lifecycle artifacts
Rejected because the formal report for the latest detected session is `NO-GO`, and silent scope shifting would be rollout-governance drift.

### 3. Ignore latest session because it appears candidate-only
Rejected because that would introduce implementer interpretation into a production promotion gate. If a different session should be authoritative, that must be explicitly defined and frozen.

## Validation Steps
1. Read baseline stamp:
   - `/root/Gho/.ghost/baseline_accepted_revision` -> `567bc6005b5907b116987339a9a82289759ceae9`
2. Read current git HEAD:
   - `git -C /root/Gho rev-parse HEAD` -> `567bc6005b5907b116987339a9a82289759ceae9`
3. Generate formal burn-in report from existing artifacts:
   - `/root/Gho/.venv/bin/python /root/Gho/scripts/shadow_run_report.py --config /root/Gho/configs/rollout/paper-burnin.toml --metrics-text /root/Gho/logs/rollout/paper-burnin/metrics.prom --json`
4. Confirm resulting verdict is `NO-GO`.
5. Confirm latest run scope forensic evidence:
   - `datasets/events/paper-burnin/exec_launcher-1774640502964_20260327_194142_0000.jsonl`
   - contains only `Candidate` events with `REJECT` / `TIMEOUT`
6. Do not start PR-7 / dual micro-live until a formal accepted paper burn-in session produces `GO` under explicitly defined scope.
