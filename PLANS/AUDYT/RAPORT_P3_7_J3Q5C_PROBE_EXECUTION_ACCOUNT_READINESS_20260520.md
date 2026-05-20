# RAPORT P3.7-J3 Probe Execution-Account Readiness

Date: 2026-05-20
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5c/decisions`

## Summary

```text
selected_probe_rows = 16
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 16
diagnosed_selected_probe_rows = 1
exact_decision_v3_join_rows = 16
missing_account_roles = {'none': 15, 'creator_vault': 1}
classifications = {'unknown': 15, 'execution_account_not_ready': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `4db1376021` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `be2a849b19` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `77862f2234` | `creator_vault` | `execution_account_not_ready` | `DF1ZZJhgvW4wLPuPrjvKTttV4odL8UaD4GxHSyNUGgUL` | `exact` | 0 | `execution_account_not_ready:creator_vault:DF1ZZJhgvW4wLPuPrjvKTttV4odL8UaD4GxHSyNUGgUL` |
| `a0d936d5f1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `e32bcb31a4` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `86d92583f5` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cceed765c8` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `ac3b03f470` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `861508c679` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `b3bf2c934d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `af6df73297` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `e84cc7f45c` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cd432df881` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cc2bcfd5e1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `bca52921d1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `88933e13a1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
