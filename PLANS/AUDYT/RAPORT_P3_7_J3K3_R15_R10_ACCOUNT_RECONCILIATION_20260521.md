# RAPORT P3.7-J3K2 Account Coverage / Route Identity Reconciliation

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r10-j3k3`

Status:

```text
J3K2 reconciliation: PASS
recommended_next_fix_path = execution_account_readiness_materialization
collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Summary

```text
audited_missing_account_rows = 1
exact_decision_v3_join_rows = 1
classifications = {'builder_required_account_not_in_mfs': 1}
recommended_fix_paths = {'execution_account_readiness_materialization': 1}
diag_seen_before_decision_rows = 0
prepared_request_not_built_rows = 1
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
| `52e5fc01ec` | `creator_vault` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |

## Decision

Do not run another blind timeout. Do not scale collection. The next fix
must target the dominant handoff class reported above.
