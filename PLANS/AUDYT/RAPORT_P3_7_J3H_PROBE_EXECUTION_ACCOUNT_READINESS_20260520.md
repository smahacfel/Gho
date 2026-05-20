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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r6/decisions`

## Summary

```text
selected_probe_rows = 5
diagnosed_selected_probe_rows = 5
exact_decision_v3_join_rows = 5
missing_account_roles = {'bonding_curve_v2': 5}
classifications = {'execution_account_not_ready': 5}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `bd19d6b410` | `bonding_curve_v2` | `execution_account_not_ready` | `F3hHfCY1DTTpapHU987qr9W3UxfDJz6B88BaYx5v4h7z` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:F3hHfCY1DTTpapHU987qr9W3UxfDJz6B88BaYx5v4h7z` |
| `d463245607` | `bonding_curve_v2` | `execution_account_not_ready` | `D1WhcgN7cb6sQJ1ZjCGsdauYZjLzBeRZ5cE8wcntGFSe` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:D1WhcgN7cb6sQJ1ZjCGsdauYZjLzBeRZ5cE8wcntGFSe` |
| `b99e003eb9` | `bonding_curve_v2` | `execution_account_not_ready` | `HY2vfVUdARHx1zhbiz8SmRqyaE9RdwRGF2nWLz26Ud5Q` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:HY2vfVUdARHx1zhbiz8SmRqyaE9RdwRGF2nWLz26Ud5Q` |
| `203f6906cc` | `bonding_curve_v2` | `execution_account_not_ready` | `9eAMNYHp8TpcaF326sceNjjG8qgZyVpU3KbF3VEqhesZ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9eAMNYHp8TpcaF326sceNjjG8qgZyVpU3KbF3VEqhesZ` |
| `3e6b9b5482` | `bonding_curve_v2` | `execution_account_not_ready` | `DYRFFF3h64yR4VLh86gteKpgAL34wzQfL74Bcj6oSCK6` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DYRFFF3h64yR4VLh86gteKpgAL34wzQfL74Bcj6oSCK6` |

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
