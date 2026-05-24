# RAPORT P3.7-E3 - Legacy Buy Authority/Readiness Convergence

Data: 2026-05-24

## Werdykt

P3.7-E3 code-level: PASS

Runtime E3S: NOT VALIDATED

Zakres byl waski: legacy_buy curve authority/readiness convergence. Nie zmieniano Gatekeepera, progow, L2, Phase B, P2/live ani coordination-risk.

## Co zostalo domkniete

- Active path nie promuje juz route_builder curve do ready tylko dlatego, ze RPC widzi konto.
- Route-builder curve moze zostac podniesione do `authoritative_cross_checked` tylko przy exact match z autorytatywnym zrodlem materializacji.
- Probe-side skip path wykonuje account-set/RPC readiness diagnostics przed zapisaniem route resolution, wiec `authoritative_account_state` nie zostaje juz bez `rpc_load_status`.
- `legacy_buy_curve_authority_readiness_status` jest propagowane przez probe/active diagnostics, transport, entry rows i audit.
- `bonding_curve_v2` z primary `routed_exact_sol_in` nie zatruwa legacy_buy fallback missing-role/readiness classification.

## Nowe statusy

`legacy_buy_curve_authority_readiness_status` moze raportowac m.in.:

- `authoritative_and_load_ready`
- `authoritative_but_not_load_checked`
- `authoritative_but_missing_on_rpc`
- `load_ready_but_authority_unverified`
- `derived_mismatch_authoritative_source`
- `authority_missing`
- `unknown`

## Nowe liczniki audytu

- `legacy_buy_curve_authority_readiness_status_counts`
- `legacy_buy_curve_authoritative_and_load_ready_rows`
- `legacy_buy_curve_load_ready_but_authority_unverified_rows`
- `legacy_buy_curve_authoritative_but_not_checked_rows`
- `legacy_buy_curve_derived_matches_account_state_rows`
- `legacy_buy_curve_derived_mismatch_account_state_rows`
- `legacy_buy_route_ready_after_reconciliation_rows`
- `legacy_buy_route_still_not_ready_after_reconciliation_rows`

Analogiczne liczniki sa raportowane dla active shadow z prefiksem `active_shadow_`.

## Walidacja

Uruchomione:

```text
cargo check -p ghost-launcher
cargo test -p ghost-launcher p37_legacy_buy --lib
cargo test -p ghost-launcher p37_route_resolver --lib
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts.test_v3_p37_mfs_lifecycle_join_key_audit.P37MfsLifecycleJoinKeyAuditTests.test_e3_legacy_buy_authority_readiness_counters_are_reported scripts.test_v3_p37_mfs_lifecycle_join_key_audit.P37MfsLifecycleJoinKeyAuditTests.test_l1r16_route_resolver_counters_are_reported
cargo fmt --check -p ghost-launcher
git diff --check
```

Wynik: PASS. Rust komendy nadal emituja istniejace warningi repo, bez nowych bledow kompilacji/testow.

## Następny krok

P3.7-E3S - Legacy Buy Authority/Readiness Smoke.

Parametry zgodnie z decyzja:

```text
shadow_only
execution_mode = shadow
max_probe_dispatches_per_run = 15
max_concurrent = 1
runtime 20-30 min albo do cap
```

E3S ma rozstrzygnac, czy pojawia sie `legacy_buy_curve_authoritative_and_load_ready_rows > 0` oraz czy legacy_buy fallback daje successful entry bez post-simulation AccountNotFound.
