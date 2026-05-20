# RAPORT P3.7-J3J R15-r8d Probe Execution-Account Wait Readiness

Date: 2026-05-20

Status:

```text
P3.7-J3J account wait audit: PASS
R15-r8d runtime smoke: NOT_READY_DIAGNOSED
Full / bounded collection: HOLD
Phase B / P2 / live / tuning: NO-GO
```

## Inputs

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8d/decisions`

## Summary

```text
selected_probe_rows = 25
diagnosed_selected_probe_rows = 21
exact_decision_v3_join_rows = 25
missing_account_roles = {'bonding_curve_v2': 19, 'creator_vault': 2, 'none': 4}
classifications = {'execution_account_not_ready': 21, 'unknown': 4}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `4fe5d8e03e` | `bonding_curve_v2` | `execution_account_not_ready` | `8rKbxpR7GPdNQkYhZRZSRy2jhYeNgq91izLs7LdTazc2` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8rKbxpR7GPdNQkYhZRZSRy2jhYeNgq91izLs7LdTazc2` |
| `2b1f0976cb` | `bonding_curve_v2` | `execution_account_not_ready` | `Fp4joViZ8jqERmwtjTtWSjhNukggFdbH7EobTJ3DxLve` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:Fp4joViZ8jqERmwtjTtWSjhNukggFdbH7EobTJ3DxLve` |
| `b3e496f5ca` | `bonding_curve_v2` | `execution_account_not_ready` | `XDyzn67wx9QuA3t4JUWXr79eQfdZ49Fw5GSiYeJWxCQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:XDyzn67wx9QuA3t4JUWXr79eQfdZ49Fw5GSiYeJWxCQ` |
| `a76d1b51f5` | `bonding_curve_v2` | `execution_account_not_ready` | `9WjonbYJxQBdDxRGT3yEFLQRXetVXtEhwFgWqkZoP6CQ` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9WjonbYJxQBdDxRGT3yEFLQRXetVXtEhwFgWqkZoP6CQ` |
| `e44bb25d5e` | `bonding_curve_v2` | `execution_account_not_ready` | `4FvmbzHfSKtUkraw1fXA2UpUQ4HCqbSAWjDfGQaBKpr4` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4FvmbzHfSKtUkraw1fXA2UpUQ4HCqbSAWjDfGQaBKpr4` |
| `c55a3cfae8` | `bonding_curve_v2` | `execution_account_not_ready` | `4eJ61aPq6PYLaPz7AYdyjjzKreHmKUCeKKu2cW4C98Wq` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4eJ61aPq6PYLaPz7AYdyjjzKreHmKUCeKKu2cW4C98Wq` |
| `214017cf19` | `bonding_curve_v2` | `execution_account_not_ready` | `9qqmUVbnhaByuCFvcLK5QWqnVa3Yn7McXQ2KNbmA8buy` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9qqmUVbnhaByuCFvcLK5QWqnVa3Yn7McXQ2KNbmA8buy` |
| `3c244a725c` | `bonding_curve_v2` | `execution_account_not_ready` | `5oPPVGpfUphBVUfJW464orUnYRiywxnBLgpAwnRQpxRu` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:5oPPVGpfUphBVUfJW464orUnYRiywxnBLgpAwnRQpxRu` |
| `9c676cafc4` | `bonding_curve_v2` | `execution_account_not_ready` | `CVkDAy7vJKd5whzVPdZtLEf3EXBqYxFMQbQrYGqDDCqU` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CVkDAy7vJKd5whzVPdZtLEf3EXBqYxFMQbQrYGqDDCqU` |
| `3411e75696` | `bonding_curve_v2` | `execution_account_not_ready` | `FDBMhrwLQ2XGjbCKAJ1FmsZVdjDz3tqVeW2r3taHpBGb` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FDBMhrwLQ2XGjbCKAJ1FmsZVdjDz3tqVeW2r3taHpBGb` |
| `6f1c9f9a08` | `bonding_curve_v2` | `execution_account_not_ready` | `BL5ruf66zHx4QcGwYgAWbY1xh8zf7ZiGkubeaEjhNPmV` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:BL5ruf66zHx4QcGwYgAWbY1xh8zf7ZiGkubeaEjhNPmV` |
| `9bd7bde04b` | `bonding_curve_v2` | `execution_account_not_ready` | `CMFBcHjJTkkzjoCe7xTktJDiTqYwEYE1kJUbUbbMD8ki` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CMFBcHjJTkkzjoCe7xTktJDiTqYwEYE1kJUbUbbMD8ki` |
| `d47981ebec` | `creator_vault` | `execution_account_not_ready` | `3juffhK5JRYxd8N7Zdnw23qBua2dqh6RbK7mALedLwMD` | `exact` | 0 | `execution_account_not_ready:creator_vault:3juffhK5JRYxd8N7Zdnw23qBua2dqh6RbK7mALedLwMD` |
| `3ce6764602` | `bonding_curve_v2` | `execution_account_not_ready` | `3nTQy8pwT272o9GV57tQsqoj26Hici4jzHGThztusxTq` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:3nTQy8pwT272o9GV57tQsqoj26Hici4jzHGThztusxTq` |
| `3dfaa8b228` | `bonding_curve_v2` | `execution_account_not_ready` | `CcJUbB1iVsnr3yeu3QMgt3Yi8YBiZePvE9fUEgYy5bwb` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CcJUbB1iVsnr3yeu3QMgt3Yi8YBiZePvE9fUEgYy5bwb` |
| `71ac6c4c53` | `bonding_curve_v2` | `execution_account_not_ready` | `2eiLhErYeAViDEtp5AxxzeAAMv6puRTSUrXDDsz1BcSw` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2eiLhErYeAViDEtp5AxxzeAAMv6puRTSUrXDDsz1BcSw` |
| `8b7d14e583` | `bonding_curve_v2` | `execution_account_not_ready` | `CgrALLKvQX2tuhyUnPgpoej5PD6MyTMhBSQMbCHM1WJ3` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CgrALLKvQX2tuhyUnPgpoej5PD6MyTMhBSQMbCHM1WJ3` |
| `ca4536bb5f` | `creator_vault` | `execution_account_not_ready` | `7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` | `exact` | 0 | `execution_account_not_ready:creator_vault:7SgfJrryT6JxrSTEh6KPVeHVvtziQRGoC523MpMhBpun` |
| `06ce87e89c` | `bonding_curve_v2` | `execution_account_not_ready` | `AcJvE4pMJYkTxB4BioV56e2R2YpnBKrPedzpL8HezvWn` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:AcJvE4pMJYkTxB4BioV56e2R2YpnBKrPedzpL8HezvWn` |
| `687b8546a9` | `bonding_curve_v2` | `execution_account_not_ready` | `6gLcmwPMnSbsJByg11VsRjiktp9T6aa6NnzZWjx9nu3W` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:6gLcmwPMnSbsJByg11VsRjiktp9T6aa6NnzZWjx9nu3W` |
| `e581181554` | `bonding_curve_v2` | `execution_account_not_ready` | `9j69dqosK9GF3HwUPhQiFbizGDyDj2k2GhXVXnGxz2ZC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:9j69dqosK9GF3HwUPhQiFbizGDyDj2k2GhXVXnGxz2ZC` |
| `cd91cc2dcd` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `efb1fe2704` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d57ce612d3` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `3f4bfa2f44` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

## Interpretation

The R15-r8d selected probes no longer fail on payer, user-volume, generic
`transaction_account` handling, or scan-plane concurrency. They fail on strict
routed execution accounts after the bounded execution-account wait window.

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
P3.7-J3K Execution Account Readiness Source Coverage
```

J3K should determine whether `bonding_curve_v2` and route-specific
`creator_vault` can be sourced from a decision-time-safe account coverage lane
or explicitly materialized as probe eligibility evidence. If these strict
accounts are known but absent on processed RPC after the bounded wait window,
the row should remain classified as `execution_account_not_ready` rather than
dispatched.

The next smoke should only run after a concrete account coverage or
materialization fix. It should not increase probe limits and should not weaken
strict precheck.
