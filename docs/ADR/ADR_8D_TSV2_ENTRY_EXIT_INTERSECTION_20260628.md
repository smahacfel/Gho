# ADR-8D: TSV2 entry+exit intersection offline proof

Status: MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED
Typ: ADR-8D / offline research evidence
Data: 2026-06-28
Autor/Agent: Codex
Zakres: PR-TSV2-EIX-A0
Poziom ryzyka: MEDIUM

## 1. Decision

PR-TSV2-EIX-A0 was implemented as an offline-only proof script and report set. No runtime path was changed.

Final verdict: `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`

## 2. Scope

- R49: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- R50: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

## 3. Evidence

| scope | artifact | exists | size_bytes | path |
| --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | shadow_lifecycle | True | 280671496 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | probe_shadow_lifecycle | True | 27343702 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | shadow_exit_replay_v1 | True | 11742516 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | gatekeeper_v2_decisions | False | 0 |  |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_mask_summary | True | 2110106 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_cost_sensitivity | True | 666003 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | a2_stability | True | 4373516 | reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | shadow_lifecycle | True | 212628141 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_lifecycle.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | probe_shadow_lifecycle | True | 38523937 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/probe_shadow_lifecycle.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | shadow_exit_replay_v1 | True | 9226688 | /mnt/HC_Volume_105935807/logs/shadow_run/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/shadow_exit_replay_v1.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | gatekeeper_v2_decisions | True | 1370423178 | /mnt/HC_Volume_105935807/logs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/decisions/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/v2.5/v25_shadow/ba77fb17e35a73ba5efe8f4640339922d4f1ccaf3735ea18931f8b0e65f11970/gatekeeper_v2_decisions.jsonl |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_mask_summary | True | 2146365 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_summary_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_cost_sensitivity | True | 654251 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_cost_sensitivity_a2.csv |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | a2_stability | True | 4450505 | reports/selector/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1/time_stop_v2_mask_stability_a2.csv |

## 4. Fixed Inputs

- Entry cohorts: `S1_F5, C1, C2, C3, C4`
- Exit masks: `M4_CONFIRM_2_WINDOWS, M5_DELAY_4000MS_CONFIRM, M6_DELAY_8000MS_CONFIRM, M7_CLASS_RESTRICTED`
- Exit cells: `(7500, -6000, 60000); (10000, -6000, 60000); (7500, -6000, 120000); (10000, -6000, 120000)`
- New masks: `false`
- New thresholds: `false`
- R50 retuning: `false`

## 5. Result

- `fixed_rules_tested_count = 80`
- `entry_exit_evaluable_rows = 0`
- `passing_fixed_rules_count = 0`
- `best_fixed_rule = none`
- `runtime_approval = false`
- `shadow_close_only_approval = false`
- `raw_jsonl_committed = false`

Blocking reason: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1:missing_gatekeeper_v2_decisions_jsonl`

Reason: R49 pre-entry feature surface unavailable.

EIX hypothesis not falsified numerically. It is blocked by missing decision-time pre-entry evidence.

## 6. Consequences

The current local evidence set cannot prove a fixed ORG-A0 entry cohort plus TSV2 exit mask/cell intersection across R49 and R50, because R49 pre-entry decision/materialized feature evidence is missing. The proof remains offline-only and gives no basis for runtime change, `shadow_close_only`, active close, Gatekeeper policy change, selector runtime change, alpha hook, XGBoost, or TX/Jito/live path change.

No R51 is approved from this result.

POST-EIX contingency: recover original R49 pre-entry decision/materialized feature evidence and rerun the same fixed EIX script, or close EIX as `MISSING_EVIDENCE / INCONCLUSIVE_RESEARCH / DATA_BLOCKED`. Do not add masks, thresholds, R50 retuning, runtime changes, active close, or `shadow_close_only`.
