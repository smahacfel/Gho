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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8l.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8l/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8l/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8l/decisions`

## Summary

```text
selected_probe_rows = 20
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 20
diagnosed_selected_probe_rows = 19
exact_decision_v3_join_rows = 20
missing_account_roles = {'bonding_curve_v2': 18, 'creator_vault': 1, 'none': 1}
classifications = {'execution_account_not_ready': 19, 'unknown': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `461a05275f` | `bonding_curve_v2` | `execution_account_not_ready` | `7K934V54kCSYX6uwkmjnjSMQBqjYwSFgmrmg6WFuZkb4` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7K934V54kCSYX6uwkmjnjSMQBqjYwSFgmrmg6WFuZkb4` |
| `b9451db3e7` | `bonding_curve_v2` | `execution_account_not_ready` | `9KBK43Q7gE9EEaPg8sttxa9cgYtibLtsGXVBd4x3Gc7K` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9KBK43Q7gE9EEaPg8sttxa9cgYtibLtsGXVBd4x3Gc7K` |
| `2a9898ce14` | `bonding_curve_v2` | `execution_account_not_ready` | `7CLKmUokhZT32nuZ2kBbsDtK4ssRxLsRZvzAdhw1F1EZ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7CLKmUokhZT32nuZ2kBbsDtK4ssRxLsRZvzAdhw1F1EZ` |
| `343ffd6496` | `bonding_curve_v2` | `execution_account_not_ready` | `HcYccjqVe9fXSiLYHmXdVEPhnYDKQeSi5TAKZGoQ9P1T` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HcYccjqVe9fXSiLYHmXdVEPhnYDKQeSi5TAKZGoQ9P1T` |
| `4fc2281500` | `bonding_curve_v2` | `execution_account_not_ready` | `2X25kbMQnxof4afZYYU5SrRpACJSdXi7XSow8m5XQVCr` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2X25kbMQnxof4afZYYU5SrRpACJSdXi7XSow8m5XQVCr` |
| `a07df1b59f` | `bonding_curve_v2` | `execution_account_not_ready` | `AzDrp1TvxUSnM8Ve3tn6U7uqsrHS6WzvEPN51f9FeN4j` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AzDrp1TvxUSnM8Ve3tn6U7uqsrHS6WzvEPN51f9FeN4j` |
| `1ea1b358c2` | `bonding_curve_v2` | `execution_account_not_ready` | `BbenuvnQQVSywX9MfQ6GLjUzLS4nXD7QyrLGN8TN55vK` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BbenuvnQQVSywX9MfQ6GLjUzLS4nXD7QyrLGN8TN55vK` |
| `8caae9e052` | `bonding_curve_v2` | `execution_account_not_ready` | `EydUefoNCTJ8wvLgbDHvk5RRDRgDqpG1QNuyzvbY53xf` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:EydUefoNCTJ8wvLgbDHvk5RRDRgDqpG1QNuyzvbY53xf` |
| `2e13cffd75` | `bonding_curve_v2` | `execution_account_not_ready` | `4GXKt8YejyQJAzNd9ze6rPyKL2AesQkWjfLmLcaCCL4z` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4GXKt8YejyQJAzNd9ze6rPyKL2AesQkWjfLmLcaCCL4z` |
| `0399525556` | `bonding_curve_v2` | `execution_account_not_ready` | `Ckdvgwpd9tG3u5Jrpm3fpuHHtyrQWQJYNaPn3a3R5AMq` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Ckdvgwpd9tG3u5Jrpm3fpuHHtyrQWQJYNaPn3a3R5AMq` |
| `98b73043ff` | `bonding_curve_v2` | `execution_account_not_ready` | `E9SpYFGnV9fGkTSjSnpnRF7xE6iKA1HHmoSkmnPVhJoQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:E9SpYFGnV9fGkTSjSnpnRF7xE6iKA1HHmoSkmnPVhJoQ` |
| `b4ad18a881` | `bonding_curve_v2` | `execution_account_not_ready` | `7QH4NQouc7CJeL55f2gQxRLWLNyiUYLw6xRpUyu8aw5M` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7QH4NQouc7CJeL55f2gQxRLWLNyiUYLw6xRpUyu8aw5M` |
| `2a0670778a` | `bonding_curve_v2` | `execution_account_not_ready` | `B9dPkmqg4qrpz5z2Teq58cyhMxRhH6xR9JKrSQCRnWAh` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:B9dPkmqg4qrpz5z2Teq58cyhMxRhH6xR9JKrSQCRnWAh` |
| `7d3113cbff` | `bonding_curve_v2` | `execution_account_not_ready` | `3B47Kb4GX4zGkPUrXbo2o2yh12nqgQhX1rf52DWwVvDj` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3B47Kb4GX4zGkPUrXbo2o2yh12nqgQhX1rf52DWwVvDj` |
| `776a6c4bef` | `bonding_curve_v2` | `execution_account_not_ready` | `Esc3b7yQypbb4ojVjWkdoSmNcbCmC2YDBFVq1R3EzLaN` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Esc3b7yQypbb4ojVjWkdoSmNcbCmC2YDBFVq1R3EzLaN` |
| `4f6ae583bb` | `bonding_curve_v2` | `execution_account_not_ready` | `7mbVcJjmh516Tu5vcxP19oiUYbqAPkhWxgoU1FoktFbs` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7mbVcJjmh516Tu5vcxP19oiUYbqAPkhWxgoU1FoktFbs` |
| `12db24417b` | `bonding_curve_v2` | `execution_account_not_ready` | `Dq6BFWEFo1FBrUfu3Y1Lv4uviKjbsF5pLJDZNfyd1Zz8` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Dq6BFWEFo1FBrUfu3Y1Lv4uviKjbsF5pLJDZNfyd1Zz8` |
| `63702db837` | `creator_vault` | `execution_account_not_ready` | `9ewBoMrdG4KxAwPN69DWTVPpNUGaAu8TfBgCU7NJ2BRv` | `exact` | 0 | `execution_account_not_ready:creator_vault:9ewBoMrdG4KxAwPN69DWTVPpNUGaAu8TfBgCU7NJ2BRv` |
| `e7642d738b` | `bonding_curve_v2` | `execution_account_not_ready` | `izSNmYStFfZ5qNnCZLNZDAUvZYxk8PvG4qQNAaCM8Po` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:izSNmYStFfZ5qNnCZLNZDAUvZYxk8PvG4qQNAaCM8Po` |
| `8a4c8ccbfd` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
