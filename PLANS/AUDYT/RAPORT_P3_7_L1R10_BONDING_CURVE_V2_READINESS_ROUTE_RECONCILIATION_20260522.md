# RAPORT P3.7-L1R10 BondingCurveV2 Readiness / Route Source Reconciliation

Date: 2026-05-22T21:21:34.014506+00:00
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck`

## Status

```text
P3.7-L1R10 status = not_ready_diagnosed
active_shadow_bcv2_not_ready_rows = 2
probe_bcv2_not_ready_rows = 3
classifications = {'builder_pubkey_not_seen_in_diag': 5}
recommended_next_stage = bonding_curve_v2_route_source_or_account_coverage_repair
L2 ablation / collection / Phase B / P2 / live / tuning = HOLD / NO-GO
```

## Evidence Summary

```text
diag_account_update_total = 2389
diag_seen_exact_pubkey_rows = 0
diag_seen_other_curve_pubkey_rows = 5
mfs_contains_bonding_curve_v2_key_rows = 0
mfs_contains_builder_bcv2_pubkey_rows = 0
rpc_current_checked = True
rpc_current_status_counts = {'error': 5}
rpc_current_error_counts = {'HTTP Error 403: Forbidden': 5}
classification_reasons = {'builder_bcv2_pubkey_not_seen_in_diag': 5, 'diag_seen_other_curve_pubkey_for_same_mint': 5, 'mfs_missing_bonding_curve_v2_field': 5, 'mfs_missing_builder_bcv2_pubkey': 5}
```

## Interpretation

The builder-provided `bonding_curve_v2` identities are present in active/probe
failure artifacts, but the analyzed DIAG relay did not observe the exact
`bonding_curve_v2` pubkey for any reconciled row.

When DIAG evidence exists for the same mint, it is for a different
`bonding_curve` pubkey. That points at a route/account-source coverage issue:
local curve updates prove the legacy bonding curve path, not the exact
`bonding_curve_v2` account that the builder inserts into simulation metas.

This is not a threshold or L2 policy problem. The next repair must decide
whether `bonding_curve_v2` should be materialized/covered explicitly, whether
the route builder is selecting the wrong source, or whether this route should
be sampled only after `bonding_curve_v2` simulation-load readiness is proven.

## Sample Rows

```text
plane = active_shadow
artifact_sources = ['buys', 'entry', 'lifecycle']
ab_record_id = 3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw:1779479327251:1779479329251:BUY
base_mint = 8NZF1E4gXWVjJ93xK1f3Uhc5AxYJNrCw61FygPGFpump
pool_id = 3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw
builder_bonding_curve_v2_pubkey = gjYKQBf1jMqUvnULVNtn6DnASGvQ1bRkMRJzBACcNgk
classification = builder_pubkey_not_seen_in_diag
classification_reasons = ['builder_bcv2_pubkey_not_seen_in_diag', 'diag_seen_other_curve_pubkey_for_same_mint', 'mfs_missing_bonding_curve_v2_field', 'mfs_missing_builder_bcv2_pubkey']
diag_seen_exact_pubkey = False
diag_seen_other_curve_pubkey_for_mint = True
diag_other_curve_pubkeys_for_mint = ['3zVcDEafuV5Bo8HbrSWPjSwikmrW8gvemNRVWD4zRsgw']
mfs_contains_bonding_curve_v2_key = False
mfs_contains_builder_bcv2_pubkey = False
rpc_current_status = error
```

```text
plane = active_shadow
artifact_sources = ['buys', 'entry', 'lifecycle']
ab_record_id = 8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx:1779479366413:1779479368413:BUY
base_mint = AD9DC7itU5R9oBLGYfDwdPZtZHgfPJnZtyNEitRqpump
pool_id = 8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx
builder_bonding_curve_v2_pubkey = 9XSnsH74CwZPW4AnrCBQitBzsrEN5Qc8nGY3u71wcYsX
classification = builder_pubkey_not_seen_in_diag
classification_reasons = ['builder_bcv2_pubkey_not_seen_in_diag', 'diag_seen_other_curve_pubkey_for_same_mint', 'mfs_missing_bonding_curve_v2_field', 'mfs_missing_builder_bcv2_pubkey']
diag_seen_exact_pubkey = False
diag_seen_other_curve_pubkey_for_mint = True
diag_other_curve_pubkeys_for_mint = ['8nvY4GH6kA1EmjWCvuPaczPYzkDxZcRZEuV8pEwYNdjx']
mfs_contains_bonding_curve_v2_key = False
mfs_contains_builder_bcv2_pubkey = False
rpc_current_status = error
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = 7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f:1779479275477:1779479277477:TIMEOUT
base_mint = 5MmDSpj3vmPkwEctKuxWY3m98qMu2XMxXtiDvXZepump
pool_id = 7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f
builder_bonding_curve_v2_pubkey = 9FG8c1Y9JNAqzMRVLy8gUuAwZsGy8EX69rNWixDRKzW
classification = builder_pubkey_not_seen_in_diag
classification_reasons = ['builder_bcv2_pubkey_not_seen_in_diag', 'diag_seen_other_curve_pubkey_for_same_mint', 'mfs_missing_bonding_curve_v2_field', 'mfs_missing_builder_bcv2_pubkey']
diag_seen_exact_pubkey = False
diag_seen_other_curve_pubkey_for_mint = True
diag_other_curve_pubkeys_for_mint = ['7cGhtsshrWLSicPBR4f84AdjAJd9mcrjB46ZxisEsL5f']
mfs_contains_bonding_curve_v2_key = False
mfs_contains_builder_bcv2_pubkey = False
rpc_current_status = error
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX:1779479311452:1779479313452:TIMEOUT
base_mint = 5hR8kSK4L3QBbeHDt1K4zFb4yC3NBwkEpYPXENYBpump
pool_id = FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX
builder_bonding_curve_v2_pubkey = Bja6zgQ3dnJDKJjmM2acgnSnDRBaCuccDF7AHokay4rM
classification = builder_pubkey_not_seen_in_diag
classification_reasons = ['builder_bcv2_pubkey_not_seen_in_diag', 'diag_seen_other_curve_pubkey_for_same_mint', 'mfs_missing_bonding_curve_v2_field', 'mfs_missing_builder_bcv2_pubkey']
diag_seen_exact_pubkey = False
diag_seen_other_curve_pubkey_for_mint = True
diag_other_curve_pubkeys_for_mint = ['FYwcJLWdg1HD8nv6gejfppPXkAtktS4aGsHbqbjJ7ZQX']
mfs_contains_bonding_curve_v2_key = False
mfs_contains_builder_bcv2_pubkey = False
rpc_current_status = error
```

```text
plane = probe
artifact_sources = ['probe_skip']
ab_record_id = 9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP:1779479332131:1779479334131:TIMEOUT
base_mint = 4L281xWoT52AEG3i9fGJ6T6yRV4gVW6ci1Pc8NYdvjCd
pool_id = 9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP
builder_bonding_curve_v2_pubkey = 28fmwfwvmTCcqAcm1jhmrrAvKrSBAxs5jKUbrWWX8cqP
classification = builder_pubkey_not_seen_in_diag
classification_reasons = ['builder_bcv2_pubkey_not_seen_in_diag', 'diag_seen_other_curve_pubkey_for_same_mint', 'mfs_missing_bonding_curve_v2_field', 'mfs_missing_builder_bcv2_pubkey']
diag_seen_exact_pubkey = False
diag_seen_other_curve_pubkey_for_mint = True
diag_other_curve_pubkeys_for_mint = ['9m5Fk1VynnD6i6YvPwLBi4sxW5yJ9Xk7maVykkoHRMUP']
mfs_contains_bonding_curve_v2_key = False
mfs_contains_builder_bcv2_pubkey = False
rpc_current_status = error
```
