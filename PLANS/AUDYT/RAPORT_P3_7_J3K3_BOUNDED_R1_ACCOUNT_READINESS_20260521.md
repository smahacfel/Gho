# RAPORT P3.7-J3J Probe Execution-Account Readiness Coverage

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1`

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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1/decisions`

## Summary

```text
selected_probe_rows = 26
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 26
diagnosed_selected_probe_rows = 6
exact_decision_v3_join_rows = 26
missing_account_roles = {'none': 20, 'creator_vault': 6}
classifications = {'unknown': 20, 'execution_account_not_ready': 6}
readiness_latency_classes = {'never_observed_in_run': 6}
wait_would_help_within_1500_ms = 0
recommended_next_stage = account_coverage_or_route_identity_investigation
```

## Readiness Latency

```text
audited_missing_account_rows = 6
observed_before_decision = 0
observed_between_decision_and_probe_selected = 0
observed_after_probe_selected = 0
never_observed_in_run = 6
ready_within_500_ms = 0
ready_within_1000_ms = 0
ready_within_1500_ms = 0
ready_within_3000_ms = 0
```

## Per-Probe Diagnosis

| probe | role | classification | latency class | ready after selected ms | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | ---: | --- | --- | ---: | --- |
| `83b82d0dc8` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `794e5fa7e9` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `10d66b7fed` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `7c04d49a4b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `90ddc06103` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `f7c1bab93a` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `9b8a76b458` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `44f622c693` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `ddd846fded` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `ac6b1a6174` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `a5ec8cb0dd` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `3a3b3c6e82` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c2097e177a` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `096abf3904` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `d2061296f0` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `07af39e2a3` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c9af6daab2` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` | `exact` | 0 | `execution_account_not_ready:creator_vault:6G2eB88pK6pu3J1dX3LXqv8K32CWjF4zwGMjsgtt3MZq` |
| `d0af35acbe` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `614568877e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `c65165a90b` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `4b28b203ad` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `a086393543` | `creator_vault` | `execution_account_not_ready` | `never_observed_in_run` |  | `6NGu3NvrcHUP3bpGLJFZ7mNbjVVwVwS8NfywQ4E3tSe8` | `exact` | 0 | `execution_account_not_ready:creator_vault:6NGu3NvrcHUP3bpGLJFZ7mNbjVVwVwS8NfywQ4E3tSe8` |
| `91c38ff679` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `2249e7c0c3` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `8821990a2d` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |
| `029ba2ce1e` | `none` | `unknown` | `never_observed_in_run` |  | `none` | `exact` | 0 | `none` |

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
