# RAPORT P3.7-J3J Probe Execution-Account Readiness Coverage

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3`

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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3/decisions`

## Summary

```text
selected_probe_rows = 19
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 19
diagnosed_selected_probe_rows = 1
exact_decision_v3_join_rows = 19
missing_account_roles = {'none': 18, 'creator_vault': 1}
classifications = {'unknown': 18, 'execution_account_not_ready': 1}
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
| `2901afb89b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `9ceaf255e2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a66e7b0d80` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `1c7f1bb336` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6422ebb929` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `b7df8737aa` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `407c6e2f78` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6dc17e7931` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `4eba49a8e6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `8a1d62915c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `4c694fb2b1` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `6a3ba309bd` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `5506c67480` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d8dd2b3e84` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `0a3054b1e6` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `212cef76f2` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `52e5fc01ec` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `088c27854d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2dc20d156c` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |

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
