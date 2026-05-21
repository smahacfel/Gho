# RAPORT P3.7-J3J Probe Execution-Account Readiness Coverage

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
bounded_wait_recommendation: not_justified_account_never_observed
recommended_next_stage: account_coverage_or_route_identity_investigation
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k4-r1/decisions`

## Summary

```text
selected_probe_rows = 12
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 12
diagnosed_selected_probe_rows = 1
exact_decision_v3_join_rows = 12
missing_account_roles = {'creator_vault': 1, 'none': 11}
classifications = {'execution_account_not_ready': 1, 'unknown': 11}
readiness_latency_classes = {'never_observed_in_run': 1}
wait_would_help_within_1500_ms = 0
recommended_next_stage = account_coverage_or_route_identity_investigation
```

## Readiness Latency

```text
audited_missing_account_rows = 1
observed_before_decision = 0
observed_between_decision_and_probe_selected = 0
observed_after_probe_selected = 0
never_observed_in_run = 1
ready_within_500_ms = 0
ready_within_1000_ms = 0
ready_within_1500_ms = 0
ready_within_3000_ms = 0
```

## Per-Probe Diagnosis

| probe | role | classification | latency class | ready after selected ms | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | ---: | --- | --- | ---: | --- |
| `da5fd88504` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `F1vEgURczDkBQ3a2po2sC56fuCaPGvoxsNgt3m534xuZ` | `exact` | 0 | `execution_account_not_ready:creator_vault:F1vEgURczDkBQ3a2po2sC56fuCaPGvoxsNgt3m534xuZ` |
| `3df75ea206` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f6dcb29546` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `094eef08e4` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `35e3d843c5` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `66a4538120` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `36bb623ccb` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f213513238` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `253b24c160` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9a5ff91aa7` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `232c9a168b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `29b72dd889` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |

## Interpretation

This report is an offline probe-readiness audit. It classifies selected
counterfactual probes and pre-scan skips by exact decision/V3 join status,
required-account role, and explicit precheck reason.

Rows classified as `unknown` in this report are selected probes that were
not stopped by execution-account precheck. They must be interpreted with
the paired probe transport/entry and simulation-error reports.

## Decision

Do not bypass required-account precheck. Do not use this report alone to
start collection.

If `execution_account_not_ready` dominates and no probe transport/entry rows
exist, the next step is account-readiness/materialization work. If transport
and entry rows exist, classify any simulation errors before scaling.

For J3J, bounded wait is justified only when missing execution accounts
are usually first observed after probe selection within the configured
wait window. If accounts are already observed before selection, the
problem is route/materialization coverage rather than runtime latency.
