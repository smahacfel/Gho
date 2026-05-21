# RAPORT P3.7-J3K2 Account Coverage / Route Identity Reconciliation

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k3-r1`

Status:

```text
J3K2 reconciliation: PASS
recommended_next_fix_path = execution_account_readiness_materialization
collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Summary

```text
audited_missing_account_rows = 6
exact_decision_v3_join_rows = 6
classifications = {'builder_required_account_not_in_mfs': 6}
recommended_fix_paths = {'execution_account_readiness_materialization': 6}
diag_seen_before_decision_rows = 0
prepared_request_not_built_rows = 6
```

## Interpretation

Q6-r2 already proved counterfactual probe transport/entry for ready rows.
This report explains the dominant skip class. If `missing_bonding_curve`
rows are seen in DIAG before decision but still skip before request build,
the blocker is route/materialization/override handoff rather than bounded
wait or RPC simulation itself.

## Reconciliation Rows

| probe | role | classification | detail | diag before decision | prepared status | fix |
| --- | --- | --- | --- | --- | --- | --- |
| `9b8a76b458` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `f7c1bab93a` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `ac6b1a6174` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `ddd846fded` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `c9af6daab2` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `a086393543` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |

## Decision

Do not run another blind timeout. Do not scale collection. The next fix
must target the dominant handoff class reported above.
