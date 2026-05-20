# RAPORT P3.7-J3 Probe Execution-Account Readiness

Date: 2026-05-20
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8n/decisions`

## Summary

```text
selected_probe_rows = 17
pre_scan_precheck_skip_rows = 2
audited_probe_rows = 19
diagnosed_selected_probe_rows = 1
exact_decision_v3_join_rows = 19
missing_account_roles = {'creator_vault': 1, 'none': 18}
classifications = {'execution_account_not_ready': 1, 'unknown': 16, 'missing_execution_route_identity': 2}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `9a93a0bf19` | `creator_vault` | `execution_account_not_ready` | `Fc4EhGjXnm8EwBdzMztStk93CB1FoGNfaKZt19UsGTaU` | `exact` | 0 | `execution_account_not_ready:creator_vault:Fc4EhGjXnm8EwBdzMztStk93CB1FoGNfaKZt19UsGTaU` |
| `1434d77f46` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `2952383241` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `16cb856e0b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `75a656ffb1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `7a7958f36a` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `7106f4517b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `4d42315375` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f60e0e4df1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `3693632a2a` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f0a8bbd001` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `0ed2c92728` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `7c330ec0ba` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `606228920e` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f5dae00293` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `1959d471fc` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `59deca1e64` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `feb6638a95` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `a32864401a` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |

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
