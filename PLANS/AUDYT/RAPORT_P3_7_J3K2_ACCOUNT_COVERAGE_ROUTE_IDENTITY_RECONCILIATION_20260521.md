# RAPORT P3.7-J3K2 Account Coverage / Route Identity Reconciliation

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-q6-r2`

Status:

```text
J3K2 reconciliation: PASS
recommended_next_fix_path = route_override_propagation
collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Summary

```text
audited_missing_account_rows = 396
exact_decision_v3_join_rows = 396
classifications = {'route_mismatch': 10, 'mfs_has_account_but_overrides_missing': 385, 'builder_required_account_not_in_mfs': 1}
recommended_fix_paths = {'route_identity_propagation': 10, 'route_override_propagation': 385, 'execution_account_readiness_materialization': 1}
diag_seen_before_decision_rows = 385
prepared_request_not_built_rows = 396
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
| `44f74252ac` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `663fa08774` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `46f2dfc99c` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `0fee4911df` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `ca29c86682` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d2f5dda6bb` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9d317dfbcc` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `24643f4c06` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4faa7f2fd8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9b642d2fe8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `cb22f1b9a3` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ac7b557935` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `f38064ff68` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `821335ac15` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c307b2b43b` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `322f7a7fdf` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `23ac7ffc67` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `471d9742b0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `b94246e4ff` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `479dc1af9a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `93cc2452d1` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `6be7c2f24f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `f5e882cc47` | `payer_pubkey` | `builder_required_account_not_in_mfs` | `strict_required_account_missing_without_diag_evidence` | `False` | `not_built_pre_route_precheck` | `execution_account_readiness_materialization` |
| `613daae8c6` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `471a4f9aa1` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ecad70718c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c5d7140731` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8dbc228e1d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `cd850f643c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c77fb2cf66` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4879e61e0c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8d32e229ef` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `2b009f2f47` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `083b5b24ae` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d59cd80e43` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3491f9fe42` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5888bc0777` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `67d3afafd4` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `bc12e04295` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a02c38880d` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `b3d4794f3a` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `5dbe4156d8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8e80b22267` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ab1330363c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ccbbecc816` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0c82639f40` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a5767cc963` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `11b6bf39bb` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `bd84dc92ee` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4c136142fb` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1de5e570c7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ad6a9c7b71` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c5afe92ca2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8f6cfcc0b3` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0e87f548f2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4e5edc404c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `527cf94ab6` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c485578387` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0a15bf50a5` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d324a2ae2d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `912b67eafe` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `06ac12166d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `32797ccb68` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5bc64276e6` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `16a6523dbb` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `77d977153a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c35dfa8710` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4e824a908c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ba9ace0a3c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1088cb2427` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8be81309be` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ce2dc71d6b` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d5ae8fe282` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c299648bfe` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3eb178d867` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `27940c6d2b` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d249f632a7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `bc4cf56342` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `80539f5e1a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4bc0285b68` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c1ca0ef817` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `dbdd3c989f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1303fac462` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9e42c01b84` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0bdcd718a5` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `343bc88d81` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `52aa21b06c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `167f3fce43` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `bccff954a7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `e9d71f38aa` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4fa15f9399` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0d86b38594` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `091f90d60e` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5982572e8f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d74fd55ab1` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `356d253bf2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `28980c6dc8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ab3196dc13` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `2068089104` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `e9367d7a63` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9a9e7a99f3` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `e2b230b869` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `68adc5eac9` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5f937f358c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1f1cbe2a27` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `300871ca28` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `7ee3638e34` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `341a8c4bf3` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `80dab06166` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `e91cda8223` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `739ef936c0` | `None` | `route_mismatch` | `buy_variant_or_route_identity_missing_before_request_build` | `False` | `not_built_pre_route_precheck` | `route_identity_propagation` |
| `24d16a2977` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1b85dbf1de` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `88dc35f7c7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `482386df3b` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `46994c49c4` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `6b7c802605` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `b12c0c8429` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8efd76e867` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `29fde34a11` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8a1898cf5a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8b971fbaf9` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `49f469cd61` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `fa0361c543` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `f8262ca3f9` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ab9d76baf7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5eb46d23a8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `6ce3a0a6b0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `90071b934a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d43994048d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `808ca554b7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `90f027bf20` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `54c3ac000f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5d0177c7ae` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0023c9b340` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `f9cba41d5d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `6d37625af2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `861e547929` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `bf404b82b0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a791ae8b88` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `7ef08ee6af` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ab053ec04a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ae9b58ecc2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a7e2573da5` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a0be88c473` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ee41ac2d68` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `cf5422b093` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ba2f0162eb` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `af52f7c9b4` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `7b49611ae2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `b9148ed53a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a48127a96d` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c3170d729f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3c5d973b33` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ef29f68cf4` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `201ccdb2c8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9701151279` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `035a35b1b8` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `f29da31901` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `2cc20feaf0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `4c7fa692aa` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8c29025582` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3b4d3fcd30` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `cf498b8f67` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c1237a21dd` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `da3fbb617c` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0695d86151` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `0bfed99e16` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a4e5bcb3c7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `12e2c4f8ae` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5d97a222ce` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `b632888416` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `c36d9dbdc6` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `592fd5a866` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `271b321406` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `8026439776` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `070d15bca0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ac0fe49888` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `5da9f0e92e` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3728b59784` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `67be686522` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `dd156111a7` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `00618e4243` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `6129bf02d0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ae3a693c77` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `ea4d6f21bd` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `35095a2e08` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `43f44bb271` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a3ea8474fe` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `1c37bdca32` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `71ca4ad394` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `46850a037a` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `df97557bdd` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `a0f0d87604` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `d90dc9477f` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `e23ee74431` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `3f82e87eae` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `07fac6e1e0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `94f9db36e2` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| `9592527fe0` | `bonding_curve` | `mfs_has_account_but_overrides_missing` | `diag_seen_before_decision_but_legacy_curve_not_materialized_or_handed_to_override` | `True` | `not_built_pre_route_precheck` | `route_override_propagation` |
| ... | ... | ... | ... | ... | ... | remaining rows: 196 |

## Decision

Do not run another blind timeout. Do not scale collection. The next fix
must target the dominant handoff class reported above.
