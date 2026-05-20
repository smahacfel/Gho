# RAPORT P3.7-J3H Probe Execution-Account Eligibility

Date: 2026-05-20

Status:

```text
P3.7-J3H account readiness audit: PASS
R15-r6 runtime smoke: NOT_READY_DIAGNOSED if no probe entries were generated
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8h/decisions`

## Summary

```text
selected_probe_rows = 8
pre_scan_precheck_skip_rows = 6
audited_probe_rows = 14
diagnosed_selected_probe_rows = 7
exact_decision_v3_join_rows = 14
missing_account_roles = {'bonding_curve_v2': 6, 'creator_vault': 1, 'none': 7}
classifications = {'execution_account_not_ready': 7, 'unknown': 1, 'missing_execution_route_identity': 6}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `948a8bc6b5` | `bonding_curve_v2` | `execution_account_not_ready` | `BhGWHnrKe3cpwY9NPHyfWqBLdaRpfDbqXnFvwbqeqEwp` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BhGWHnrKe3cpwY9NPHyfWqBLdaRpfDbqXnFvwbqeqEwp` |
| `f7344d017c` | `bonding_curve_v2` | `execution_account_not_ready` | `4bN9H1GSiko8tEhnzkd8HnhGt9QZm22rkYTumqhdeBJU` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4bN9H1GSiko8tEhnzkd8HnhGt9QZm22rkYTumqhdeBJU` |
| `b4f7ca25d9` | `bonding_curve_v2` | `execution_account_not_ready` | `BvQNek6paCxjuzPraQ3bq1XNKJy4zKYza4QxQb9AnLu8` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BvQNek6paCxjuzPraQ3bq1XNKJy4zKYza4QxQb9AnLu8` |
| `87cba0d1c1` | `bonding_curve_v2` | `execution_account_not_ready` | `9heG1sTqMNYNsWqhc88nLC65zffkXVBY3EFh5aP7Ba37` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9heG1sTqMNYNsWqhc88nLC65zffkXVBY3EFh5aP7Ba37` |
| `386f828345` | `bonding_curve_v2` | `execution_account_not_ready` | `94dS4qLpPTxUsFDtpw9L6ttY4tjczoJrHDKaC9VoyUHr` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:94dS4qLpPTxUsFDtpw9L6ttY4tjczoJrHDKaC9VoyUHr` |
| `91c92e99f2` | `creator_vault` | `execution_account_not_ready` | `HU4cTrdVp7U994UmKqviPJvgiqYcPStF6dT4kfcG6RLL` | `exact` | 0 | `execution_account_not_ready:creator_vault:HU4cTrdVp7U994UmKqviPJvgiqYcPStF6dT4kfcG6RLL` |
| `982eaa6845` | `bonding_curve_v2` | `execution_account_not_ready` | `2zW6cP41xszp7cRE1qVUX4xuuZrpK79W2gparVYoDkJY` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2zW6cP41xszp7cRE1qVUX4xuuZrpK79W2gparVYoDkJY` |
| `0dd2072176` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `012be4ec6b` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `446ffc5545` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `639c2772ba` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `bd87b73fdf` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `1b40abe8bc` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |
| `defacf8370` | `none` | `missing_execution_route_identity` | `none` | `exact` | 0 | `missing_execution_route_identity` |

## Interpretation

The R15-r6 selected probes no longer fail on payer, user-volume, or generic
`transaction_account` handling. They fail on strict routed execution accounts.

For all selected probes, the missing pubkey was present in the prepared
transaction account set and was checked by required-account precheck. The
selected decision rows had V3/MFS snapshots, curve data marked known/ready,
and clean account/curve evidence, but the snapshots do not materialize
`bonding_curve_v2` or `creator_vault` as explicit execution-account fields.

The current classification is therefore:

- runtime state: `override_present_but_account_missing_on_rpc`, because the
  prepared request had a concrete required pubkey and processed RPC/precheck
  did not find the account;
- dataset contract gap: the strict account identities are not explicit V3/MFS
  fields, so future probe eligibility cannot be audited from MFS alone.

## Decision

Do not bypass required-account precheck. Do not start collection.

Recommended next repair path:

```text
P3.7-J3I Probe Execution-Account Materialization Or Eligibility Narrowing
```

J3I should decide whether to add explicit decision-time-safe materialization
for `bonding_curve_v2` and route-specific `creator_vault`, or narrow probe
eligibility to rows where these strict execution accounts are already known
and ready. If the accounts are known but absent on RPC at processed
commitment, the row should remain classified as
`execution_account_not_ready` rather than dispatched.

R15-r6 should only run after a concrete eligibility/materialization fix. It
should not increase probe limits and should not weaken strict precheck.
