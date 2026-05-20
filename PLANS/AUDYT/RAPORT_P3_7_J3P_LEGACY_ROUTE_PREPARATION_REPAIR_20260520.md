# RAPORT P3.7-J3P Legacy Route Preparation Repair

Status: `CODE_LEVEL_REPAIR_PASS`

## Problem

R15-r8i nadal wymagał routed-only kont `bonding_curve_v2` i `creator_vault`
dla wybranych probe candidates, mimo że J3O rozdzielił layout kont legacy i
routed.

Przyczyna została zawężona do granicy przygotowania requestu:

```text
P3.7 probe resolver -> BuyAccountOverrides { buy_variant = LegacyBuy, legacy_buy_curve = Some(...) }
prepare_buy_request_with_tip_telemetry_and_amount_lamports -> sanitize_buy_variant_override()
LegacyBuy -> None
create_buy_build_profile -> default None to RoutedExactSolIn
precheck -> requires bonding_curve_v2 / creator_vault
```

## Naprawa

Dodano osobny sanitizer dla granicy przygotowania requestu:

```text
sanitize_buy_variant_override_for_prepared_request(...)
```

Semantyka:

- `RoutedExactSolIn` jest zachowany jak wcześniej.
- `LegacyBuy` jest zachowany tylko wtedy, gdy request ma
  `legacy_buy_curve` proof.
- `LegacyBuy` bez `legacy_buy_curve` nadal failuje closed i nie przechodzi jako
  nieweryfikowany legacy route.

Generic active sanitizer pozostaje konserwatywny; zmiana dotyczy zachowania
zweryfikowanej legacy route przez przygotowanie requestu.

## Status Gate

```text
code-level repair: PASS
next smoke: R15-r8j
collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

## Walidacja

```bash
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p trigger test_build_buy_ix -- --nocapture
python3 -m py_compile scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_probe_execution_account_readiness_report.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/components/trigger/component.rs ghost-launcher/src/oracle_runtime.rs off-chain/components/trigger/src/direct_buy_builder.rs
```

Wynik lokalny: PASS.

Targeted Rust:

- `p37_counterfactual_probe`: 8/8 PASS
- `p37_shadow_probe`: 32/32 PASS
- `trigger test_build_buy_ix`: 7/7 PASS
- targeted sanitizer/legacy request tests: PASS

Python:

- `py_compile`: PASS
- unittest readiness + join-key audit: 12/12 PASS

Formatting:

- `rustfmt --check`: PASS
