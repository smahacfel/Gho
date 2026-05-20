# RAPORT P3.7-J3G Probe Strict Execution Account Readiness

Date: 2026-05-20

Status:

```text
P3.7-J3G account readiness audit: PASS
R15-r5 runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r5/decisions`

## Summary

```text
selected_probe_rows = 5
diagnosed_selected_probe_rows = 5
exact_decision_v3_join_rows = 5
missing_account_roles = {'bonding_curve_v2': 4, 'creator_vault': 1}
classifications = {'override_present_but_account_missing_on_rpc': 5}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `1ffc9972a9` | `bonding_curve_v2` | `override_present_but_account_missing_on_rpc` | `7DKLnwxq2yGDpbLtgCLUA7whgAfsUwXUQBmvoZRM3dVC` | `exact` | 0 | `missing_required_account:bonding_curve_v2:7DKLnwxq2yGDpbLtgCLUA7whgAfsUwXUQBmvoZRM3dVC` |
| `3ef7ac995b` | `bonding_curve_v2` | `override_present_but_account_missing_on_rpc` | `7PHgw3gdNyH2pHxzcnw7At5xCShojUS3DGRqXPch1dc6` | `exact` | 0 | `missing_required_account:bonding_curve_v2:7PHgw3gdNyH2pHxzcnw7At5xCShojUS3DGRqXPch1dc6` |
| `1c7e2aade0` | `creator_vault` | `override_present_but_account_missing_on_rpc` | `Gpy9XNXft3Z9Uoo2xVxnQSiuf2KAX4K3GYEuF7kt4zcm` | `exact` | 0 | `missing_required_account:creator_vault:Gpy9XNXft3Z9Uoo2xVxnQSiuf2KAX4K3GYEuF7kt4zcm` |
| `3c682bbeee` | `bonding_curve_v2` | `override_present_but_account_missing_on_rpc` | `G7sVcSAsPmyC2gXcsXMKkGquYn3ytpvhBbFNddDv2Kji` | `exact` | 0 | `missing_required_account:bonding_curve_v2:G7sVcSAsPmyC2gXcsXMKkGquYn3ytpvhBbFNddDv2Kji` |
| `a4a2226016` | `bonding_curve_v2` | `override_present_but_account_missing_on_rpc` | `CALtyBCVyJQYmzgXm9RVqYLAJJAEj127LkBt8VVYqUbG` | `exact` | 0 | `missing_required_account:bonding_curve_v2:CALtyBCVyJQYmzgXm9RVqYLAJJAEj127LkBt8VVYqUbG` |

## Interpretation

The R15-r5 selected probes no longer fail on payer, user-volume, or generic
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
P3.7-J3H Probe Execution-Account Eligibility
```

J3H should add a decision-time-safe execution-account readiness criterion for
`bonding_curve_v2` and route-specific `creator_vault`, or add explicit
additive materialization of these account identities/readiness states. If the
accounts are known but absent on RPC at processed commitment, the row should be
classified as `probe_execution_accounts_not_ready` rather than dispatched.

R15-r6 should only run after a concrete eligibility/materialization fix. It
should not increase probe limits and should not weaken strict precheck.
