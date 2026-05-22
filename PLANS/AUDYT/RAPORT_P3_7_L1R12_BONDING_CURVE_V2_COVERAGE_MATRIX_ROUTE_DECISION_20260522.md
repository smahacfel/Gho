# RAPORT P3.7-L1R12 BondingCurveV2 Coverage Matrix / Route Decision

Date: 2026-05-22T22:25:51.433627+00:00
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck`

## Status

```text
L1R12 status = matrix_ready
matrix_rows = 5
active_shadow_rows = 2
probe_rows = 3
rpc_current_checked = True
rpc_get_account_status_counts = {'missing': 5}
matrix_classifications = {'builder_bcv2_missing_on_rpc': 5}
recommended_next_path = route_builder_source_repair_or_route_fallback
L2 / collection / Phase B / P2 / live / tuning = HOLD / NO-GO
```

## Coverage Matrix Summary

```text
diag_account_update_total = 2389
diag_seen_exact_builder_bcv2_rows = 0
diag_seen_other_curve_for_mint_rows = 5
mfs_contains_builder_bcv2_pubkey_rows = 0
tx_meta_account_16_matches_builder_bcv2_rows = 2
matrix_reasons = {'builder_bcv2_missing_on_rpc': 5, 'diag_did_not_see_exact_builder_bcv2': 5, 'diag_seen_other_curve_for_same_mint': 5, 'mfs_missing_bonding_curve_v2_identity': 5, 'mfs_missing_builder_bcv2_pubkey': 5, 'tx_meta_account_16_matches_builder_bcv2': 2}
```

## Interpretation

Every analyzed builder `bonding_curve_v2` pubkey is missing on the current
RPC preflight. DIAG saw another bonding curve for the same mint, but not
the exact builder account. This points away from simple AccountUpdate
coverage and toward route-builder source repair or a route fallback.

The next fix should answer why the route builder derives/selects these
`bonding_curve_v2` pubkeys and whether this route should be excluded until
a valid simulation-load account exists.

## Sample Rows

```text
plane = active_shadow
artifact_sources = ['buys', 'entry', 'lifecycle']
ab_record_id = 3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw:1779479327251:1779479329251:BUY
mint = 8NZF1E4gXWVjJ93xK1f3Uhc5AxYJNrCw61FygPGFpump
pool_id = 3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw
builder_bonding_curve_v2_pubkey = gjYKQBf1jMqUvnULVNtn6DnASGvQ1bRkMRJzBACcNgk
tx_meta_account_16_pubkey = gjYKQBf1jMqUvnULVNtn6DnASGvQ1bRkMRJzBACcNgk
tx_meta_account_16_matches_builder_bcv2 = True
diag_other_curve_pubkeys_for_mint = ['3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw']
rpc_get_account_status = missing
rpc_get_account_owner = None
rpc_get_account_data_len = None
matrix_classification = builder_bcv2_missing_on_rpc
matrix_reasons = ['tx_meta_account_16_matches_builder_bcv2', 'diag_seen_other_curve_for_same_mint', 'diag_did_not_see_exact_builder_bcv2', 'mfs_missing_bonding_curve_v2_identity', 'mfs_missing_builder_bcv2_pubkey', 'builder_bcv2_missing_on_rpc']
```

```text
plane = active_shadow
artifact_sources = ['buys', 'entry', 'lifecycle']
ab_record_id = 8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx:1779479366413:1779479368413:BUY
mint = AD9DC7itU5R9oBLGYfDwdPZtZHgfPJnZtyNEitRqpump
pool_id = 8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx
builder_bonding_curve_v2_pubkey = 9XSnsH74CwZPW4AnrCBQitBzsrEN5Qc8nGY3u71wcYsX
tx_meta_account_16_pubkey = 9XSnsH74CwZPW4AnrCBQitBzsrEN5Qc8nGY3u71wcYsX
tx_meta_account_16_matches_builder_bcv2 = True
diag_other_curve_pubkeys_for_mint = ['8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx']
rpc_get_account_status = missing
rpc_get_account_owner = None
rpc_get_account_data_len = None
matrix_classification = builder_bcv2_missing_on_rpc
matrix_reasons = ['tx_meta_account_16_matches_builder_bcv2', 'diag_seen_other_curve_for_same_mint', 'diag_did_not_see_exact_builder_bcv2', 'mfs_missing_bonding_curve_v2_identity', 'mfs_missing_builder_bcv2_pubkey', 'builder_bcv2_missing_on_rpc']
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = 7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f:1779479275477:1779479277477:TIMEOUT
mint = 5MmDSpj3vmPkwEctKuxWY3m98qMu2XMxXtiDvXZepump
pool_id = 7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f
builder_bonding_curve_v2_pubkey = 9FG8c1Y9JNAqzMRVLy8gUuAwZsGy8EX69rNWixDRKzW
tx_meta_account_16_pubkey = None
tx_meta_account_16_matches_builder_bcv2 = None
diag_other_curve_pubkeys_for_mint = ['7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f']
rpc_get_account_status = missing
rpc_get_account_owner = None
rpc_get_account_data_len = None
matrix_classification = builder_bcv2_missing_on_rpc
matrix_reasons = ['diag_seen_other_curve_for_same_mint', 'diag_did_not_see_exact_builder_bcv2', 'mfs_missing_bonding_curve_v2_identity', 'mfs_missing_builder_bcv2_pubkey', 'builder_bcv2_missing_on_rpc']
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX:1779479311452:1779479313452:TIMEOUT
mint = 5hR8kSK4L3QBbeHDt1K4zFb4yC3NBwkEpYPXENYBpump
pool_id = FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX
builder_bonding_curve_v2_pubkey = Bja6zgQ3dnJDKJjmM2acgnSnDRBaCuccDF7AHokay4rM
tx_meta_account_16_pubkey = None
tx_meta_account_16_matches_builder_bcv2 = None
diag_other_curve_pubkeys_for_mint = ['FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX']
rpc_get_account_status = missing
rpc_get_account_owner = None
rpc_get_account_data_len = None
matrix_classification = builder_bcv2_missing_on_rpc
matrix_reasons = ['diag_seen_other_curve_for_same_mint', 'diag_did_not_see_exact_builder_bcv2', 'mfs_missing_bonding_curve_v2_identity', 'mfs_missing_builder_bcv2_pubkey', 'builder_bcv2_missing_on_rpc']
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = 9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP:1779479332131:1779479334131:TIMEOUT
mint = 4L281xWoT52AEG3i9fGJ6T6yRV4gVW6ci1Pc8NYdvjCd
pool_id = 9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP
builder_bonding_curve_v2_pubkey = 28fmwfwvmTCcqAcm1jhmrrAvKrSBAxs5jKUbrWWX8cqP
tx_meta_account_16_pubkey = None
tx_meta_account_16_matches_builder_bcv2 = None
diag_other_curve_pubkeys_for_mint = ['9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP']
rpc_get_account_status = missing
rpc_get_account_owner = None
rpc_get_account_data_len = None
matrix_classification = builder_bcv2_missing_on_rpc
matrix_reasons = ['diag_seen_other_curve_for_same_mint', 'diag_did_not_see_exact_builder_bcv2', 'mfs_missing_bonding_curve_v2_identity', 'mfs_missing_builder_bcv2_pubkey', 'builder_bcv2_missing_on_rpc']
```
