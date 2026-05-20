# RAPORT P3.7-J3 Probe Execution-Account Readiness

Date: 2026-05-20
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b`

Status:

```text
P3.7-J3 execution-account readiness audit: PASS
runtime smoke status must be read from the paired smoke/join-key report
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r9-q5b/decisions`

## Summary

```text
selected_probe_rows = 50
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 50
diagnosed_selected_probe_rows = 3
exact_decision_v3_join_rows = 50
missing_account_roles = {'none': 47, 'creator_vault': 3}
classifications = {'unknown': 47, 'execution_account_not_ready': 3}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `ac7c12dcb8` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `44dd7ba22b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `008e0ffb26` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `23b7925037` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d3f92f483b` | `creator_vault` | `execution_account_not_ready` | `HmvCKj3v6M2iPqktVyUXnWVjn3RBpMWxuUry8MZW7dhA` | `exact` | 0 | `execution_account_not_ready:creator_vault:HmvCKj3v6M2iPqktVyUXnWVjn3RBpMWxuUry8MZW7dhA` |
| `6ccece6dd8` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `43853f58e7` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f7e7bbfff0` | `creator_vault` | `execution_account_not_ready` | `H1Wka2xyjZ5T7abQPku3JJKdoHPFoUocjX2u6q9sMi9J` | `exact` | 0 | `execution_account_not_ready:creator_vault:H1Wka2xyjZ5T7abQPku3JJKdoHPFoUocjX2u6q9sMi9J` |
| `75781ecbaa` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `e9a9545e11` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `7cec2690a3` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `2e00926150` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d6a1979156` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `a02c57cfd1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `b14cb0e28f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `da4b766767` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `039b05be9e` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `0e0d0e411e` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `74eaa7e8fa` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `5a5c1a89c0` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `5a67121ac4` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f708fcfd08` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `3ab803ce8e` | `creator_vault` | `execution_account_not_ready` | `FyXzt4va1DUDBzheCWUsXqp5DpERGGyDMFDyu5k5fivS` | `exact` | 0 | `execution_account_not_ready:creator_vault:FyXzt4va1DUDBzheCWUsXqp5DpERGGyDMFDyu5k5fivS` |
| `7389d32bc3` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `02e30780b7` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9ab15f937f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `8ba42f27cb` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `fd83c620b5` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `5e27fecff7` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `edaefd02b4` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `b5c7d1555b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cb78049dea` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `537188221b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f8f82b5d1f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9c01eb071d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f35e8952c0` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `791029e1b1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `84bbb0422f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `ac1923db28` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9ea15bdcbf` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `58e385dbe5` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `afd3ab73cc` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `0da5bd867c` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9a2d6a8cf1` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `442313feed` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `fc2aedd35d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `b730002d7d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `dece8b82c2` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `a14a318a2a` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9b8730f540` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
