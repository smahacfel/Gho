# RAPORT P3.7-L1R5 — BondingCurveV2 Simulation Account Contract Repair

Data: 2026-05-22
Status: code-level repair implemented

## Problem

R16-r5 zawęził `15/15` ślepych wcześniej `AccountNotFound` do jednego konta:

```text
simulation_error_account_role = bonding_curve_v2
simulation_error_account_source = route_builder
simulation_error_instruction_index = 3
simulation_error_account_index = 16
```

`DirectBuyBuilder` dodaje `bonding_curve_v2` jako account meta transakcji
symulacyjnej, ale probe precheck historycznie usuwał ten sam account z required
precheck. To pozwalało oznaczyć probe jako `ready`, mimo że RPC simulation
nadal musiała załadować `bonding_curve_v2` i kończyła się `AccountNotFound`.

## Zmiany

### Runtime

Zmieniono `ghost-launcher/src/components/trigger/component.rs`:

- usunięto/wyłączono probe-only bypass dla missing `bonding_curve_v2`;
- `bonding_curve_v2` z prepared buy instruction account index `16` jest teraz
  traktowany jako required przez `counterfactual_probe_required_account_roles()`;
- missing `bonding_curve_v2` powinien zatrzymać probe przed `simulate_buy` jako:

```text
probe_skipped
probe_skip_reason = execution_account_not_ready
precheck_failure_reason = execution_account_not_ready:bonding_curve_v2:<pubkey>
```

Nie zmieniono active policy, progów, IWIM, live/P2, probe amount, probe slippage
ani baseline configs.

### Audit

Rozszerzono `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` o liczniki:

```text
simulation_required_account_not_in_precheck_rows
simulation_account_meta_missing_on_rpc_rows
bonding_curve_v2_precheck_skipped_before_simulation_rows
bonding_curve_v2_account_not_found_after_simulation_rows
skip_execution_account_readiness_role_counts
```

Audit oznacza `bonding_curve_v2_account_not_found_after_simulation` jako
readiness blocker. Dzięki temu kolejny run nie może wyglądać na gotowy, jeśli
ten sam account nadal przechodzi do symulacji i pada na `AccountNotFound`.

### Config

Dodano świeży namespace smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r6-bcv2-contract.toml
```

Ten config zachowuje profil R16 standard-softPDD i zmienia tylko namespace/run
artefaktów. Nie jest to nowa policy.

## Testy

Wykonane:

```text
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo test -p ghost-launcher --lib p37_counterfactual_probe_required_accounts -- --nocapture
```

Wyniki:

```text
Python audit tests: PASS, 12/12
Rust targeted required-account tests: PASS, 5/5
```

Rust build wypisał istniejące ostrzeżenia workspace; nie są związane z L1R5.

## Runtime Gate

Następny gate:

```text
R16-r6 BondingCurveV2 Contract Smoke
```

Acceptance:

```text
strict replay = full_replay_ok
exact decision/V3 join = 100%
AccountNotFound unattributed = 0
AccountNotFound narrowed to bonding_curve_v2 = 0
or bonding_curve_v2 rows are precheck-skipped before simulate_buy
simulation_error_entry_rows are not lifecycle-eligible
active BUY / live / P2 untouched
```

Jeżeli po fixie probe rows będą głównie skipowane przez
`execution_account_not_ready:bonding_curve_v2:<pubkey>`, to jest poprawny wynik
diagnostyczny: system przestaje udawać, że account set jest gotowy do
symulacji.

## Decyzja

```text
P3.7-L1R5 code-level repair: IMPLEMENTED
L2 / ablation: HOLD
collection: HOLD
Phase B: HOLD
P2/live/threshold tuning: NO-GO
Next: R16-r6 bcv2 contract smoke
```
