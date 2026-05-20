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

- config: `/root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8j.toml`
- probe_selection: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8j/probe_selection.jsonl`
- probe_skips: `/root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8j/probe_skips.jsonl`
- decision_root: `/root/Gho/logs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r8j/decisions`

## Summary

```text
selected_probe_rows = 21
pre_scan_precheck_skip_rows = 0
audited_probe_rows = 21
diagnosed_selected_probe_rows = 1
exact_decision_v3_join_rows = 21
missing_account_roles = {'none': 20, 'bonding_curve_v2': 1}
classifications = {'unknown': 20, 'execution_account_not_ready': 1}
```

## Per-Probe Diagnosis

| probe | role | classification | pubkey | decision join | account updates | reason |
| --- | --- | --- | --- | --- | ---: | --- |
| `60a87b9b37` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `76fa374efc` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `52773f1405` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `1319b3a28c` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cd0c101ebd` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `bdc0fc06c5` | `bonding_curve_v2` | `execution_account_not_ready` | `7bdxaZhYPMMteD2PrFq1d6F81r9CxCDBnnW2kRyZRSNx` | `exact` | 0 | `execution_account_not_ready:bonding_curve_v2:7bdxaZhYPMMteD2PrFq1d6F81r9CxCDBnnW2kRyZRSNx` |
| `5fbf6cc20c` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `cbe7d97060` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `87b8328fc5` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `05bb418c6f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `ed5debbe5f` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `f830b0b669` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d44849a343` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `e04c0b2d54` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `d3e35ea32a` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `5da74b3516` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `4966d87606` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `1e83df51f4` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `449390160d` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `450be89743` | `none` | `unknown` | `none` | `exact` | 0 | `none` |
| `6722f40878` | `none` | `unknown` | `none` | `exact` | 0 | `none` |

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
