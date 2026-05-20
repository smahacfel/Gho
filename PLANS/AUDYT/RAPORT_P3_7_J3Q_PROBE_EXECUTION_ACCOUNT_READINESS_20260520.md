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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8k.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8k/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8k/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8k/decisions`

## Summary

```text
selected_probe_rows = 23
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 23
diagnosed_selected_probe_rows = 2
exact_decision_v3_join_rows = 23
missing_account_roles = {'none': 21, 'bonding_curve_v2': 2}
classifications = {'unknown': 21, 'execution_account_not_ready': 2}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `68d84ee241` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `6f650f421b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `af99bed3f4` | `bonding_curve_v2` | `execution_account_not_ready` | `4s3Rz95CVpTukkXRTheVUa9d3SFZcvZqfWSWDzAUV2JG` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4s3Rz95CVpTukkXRTheVUa9d3SFZcvZqfWSWDzAUV2JG` |
| `4a97b80cf7` | `bonding_curve_v2` | `execution_account_not_ready` | `8LPMBBbR2JWPK455w5fC8TDwqRJuBSFkVteFXJWx764i` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:8LPMBBbR2JWPK455w5fC8TDwqRJuBSFkVteFXJWx764i` |
| `bbf9112383` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `a0c717463b` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d72af4fb01` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `b65ebd6105` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `0e4b4bb0c3` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d9d18b4adb` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `8273ddfc85` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `06f610ab9e` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `03b6f1cb7d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `098c4aa4b7` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `a5e88b0637` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d409a5c353` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `a5d5324fa9` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `c211035d47` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `03a7edc6fd` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `0044d63772` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `5675ab8134` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `9e0ddecada` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `8292051f52` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
