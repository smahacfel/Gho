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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8i/decisions`

## Summary

```text
selected_probe_rows = 11
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 11
diagnosed_selected_probe_rows = 10
exact_decision_v3_join_rows = 11
missing_account_roles = {'creator_vault': 2, 'bonding_curve_v2': 8, 'none': 1}
classifications = {'execution_account_not_ready': 10, 'unknown': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `bf2a8a2226` | `creator_vault` | `execution_account_not_ready` | `9gt4SsbvumFHr5CgCFX9SQZPQhq2rroStxUvMXBmpAzh` | `exact` | 0 | `execution_account_not_ready:creator_vault:9gt4SsbvumFHr5CgCFX9SQZPQhq2rroStxUvMXBmpAzh` |
| `a48fe52b79` | `bonding_curve_v2` | `execution_account_not_ready` | `4QEAs8kV4ukHHLTzrieCgsGNXRcFcVV5Bo8EYeHYQbTR` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4QEAs8kV4ukHHLTzrieCgsGNXRcFcVV5Bo8EYeHYQbTR` |
| `33544e88eb` | `bonding_curve_v2` | `execution_account_not_ready` | `FBU72B89ugWbtKuSGfVx4a7hzWbNwyivYoucgumLeNb4` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:FBU72B89ugWbtKuSGfVx4a7hzWbNwyivYoucgumLeNb4` |
| `ec1fcfd3ce` | `bonding_curve_v2` | `execution_account_not_ready` | `7yMZ5jwTtyjKhHapwkgZcQ6WNhSonfdjq3f8zkBRBmtB` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7yMZ5jwTtyjKhHapwkgZcQ6WNhSonfdjq3f8zkBRBmtB` |
| `4a7950b731` | `bonding_curve_v2` | `execution_account_not_ready` | `4jxT15dBaHRcVDzxPGBJfxF5654Bd57toUsMTPFcEt4c` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:4jxT15dBaHRcVDzxPGBJfxF5654Bd57toUsMTPFcEt4c` |
| `7f0e463e4d` | `bonding_curve_v2` | `execution_account_not_ready` | `DpNMFsikh1Cus9EMKzXn7LBh8kMbTpokst83k7LH46ZC` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:DpNMFsikh1Cus9EMKzXn7LBh8kMbTpokst83k7LH46ZC` |
| `c1568cc3a9` | `bonding_curve_v2` | `execution_account_not_ready` | `25CmzVLtnxebivPZeLJLj3U5r3r2usdNCjPEJT5dZF1P` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:25CmzVLtnxebivPZeLJLj3U5r3r2usdNCjPEJT5dZF1P` |
| `fec5ceab2b` | `bonding_curve_v2` | `execution_account_not_ready` | `2MudUCyUDD7ozd89J5aFnryv81yfCcWTxodZiybPMSfs` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:2MudUCyUDD7ozd89J5aFnryv81yfCcWTxodZiybPMSfs` |
| `3e3f65da5d` | `creator_vault` | `execution_account_not_ready` | `Er1j95CFnmNcE5y5mERgFUpdcdugaVWUAVfofQT6wLK` | `exact` | 0 | `execution_account_not_ready:creator_vault:Er1j95CFnmNcE5y5mERgFUpdcdugaVWUAVfofQT6wLK` |
| `fdba58dd1d` | `bonding_curve_v2` | `execution_account_not_ready` | `CwJsvXPhu8xUT3wsn5NpnpeDXu3Eg6dzaTgYygsPnp9c` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:CwJsvXPhu8xUT3wsn5NpnpeDXu3Eg6dzaTgYygsPnp9c` |
| `a5c245d4e0` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
