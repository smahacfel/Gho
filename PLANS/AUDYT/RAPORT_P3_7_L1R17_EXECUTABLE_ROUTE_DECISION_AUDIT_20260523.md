# RAPORT P3.7-L1R17 / J3S Executable Route Decision Audit

Data: 2026-05-23

## Werdykt

P3.7-L1R17: CODE/AUDIT PASS.

R16-r13 route resolver pozostaje PASS-B / correct fail-closed, ale L1R17
doprecyzowuje dlaczego fallback nie odblokowal execution:

- active fallback `legacy_buy` nie ma core curve account source;
- probe fallback nadal wymaga nieegzekwowalnego primary route account set;
- fallback nie jest naprawialny jako prosty nonfatal/creatable-account fix;
- recommended next path:
  `route_class_exclusion_from_execution_label_universe`.

Execution unlock: NO.

L2 / collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO.

## Zakres Zmian

Runtime rows dostaly addytywna diagnostyke fallbacku:

```text
fallback_missing_roles
fallback_missing_pubkeys
fallback_account_sources
fallback_simulation_load_account_set
fallback_creatable_account_set
fallback_required_precheck_account_set
fallback_failure_class
```

Pola sa propagowane przez:

- probe selection/skip rows,
- probe transport/entry diagnostics,
- active shadow account diagnostics,
- active shadow entry/failure rows.

Join-key audit rozszerzono o:

```text
fallback_failure_class_counts
fallback_missing_role_counts
fallback_missing_pubkey_counts
fallback_account_source_counts
fallback_simulation_load_account_set_rows
fallback_creatable_account_set_rows
fallback_required_precheck_account_set_rows
fallback_repairable
recommended_next_path
executable_route_ready_rows
```

Audit potrafi tez rekonstruowac klasy fallbacku ze starszych R16-r13 rows,
ktore mialy tylko `fallback_route_not_ready_reason`.

## R16-r13 Evidence Po L1R17 Audit

Config:

`configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver.toml`

Raporty robocze:

- `/tmp/r16-r13-l1r17-join-key-audit.json`
- `/tmp/r16-r13-l1r17-join-key-audit.md`

Active shadow:

```text
active_shadow_route_fallback_attempted_rows = 9
active_shadow_route_fallback_success_rows = 0
active_shadow_route_fallback_failed_rows = 9
active_shadow_no_executable_route_account_set_rows = 9
active_shadow_executable_route_ready_rows = 0
active_shadow_fallback_failure_class_counts =
  {"fallback_missing_core_curve_account": 9}
active_shadow_fallback_missing_role_counts =
  {"bonding_curve": 9}
active_shadow_fallback_account_source_counts =
  {"legacy_buy_curve": 9}
active_shadow_fallback_repairable = false
active_shadow_recommended_next_path =
  route_class_exclusion_from_execution_label_universe
```

Probe:

```text
route_fallback_attempted_rows = 2
route_fallback_success_rows = 0
route_fallback_failed_rows = 2
no_executable_route_account_set_rows = 2
executable_route_ready_rows = 0
fallback_failure_class_counts =
  {"fallback_builder_account_source_unverified": 2}
fallback_missing_role_counts =
  {"bonding_curve_v2": 2}
fallback_account_source_counts =
  {"primary_route_account_set": 2}
fallback_repairable = false
recommended_next_path =
  route_class_exclusion_from_execution_label_universe
```

## Interpretacja

Fallback nie pada na payera, user ATA ani inny nonfatal/creatable account.

Active shadow fallback brakuje core `bonding_curve` dla `legacy_buy`.
Probe fallback jest jeszcze slabszy: wymaga tego samego nieegzekwowalnego
primary route account set (`bonding_curve_v2`).

To oznacza, ze obecna route class nie jest materialem do lifecycle-label
collection. Nie powinna byc liczona jako `buy_quality_bad` ani jako policy
outcome. To jest osobna klasa:

```text
execution_feasibility_reject / no_executable_route_account_set
```

## Decyzja

Nie robic kolejnego R16 smoke tylko po to, zeby jeszcze raz zobaczyc
`no_executable_route_account_set`.

Nastepny etap:

```text
P3.7-L1R18 - route class exclusion from execution-label universe
```

albo rownowazny targeted fix, jesli wlasciciel route dostarczy autorytatywne
core route account source dla `legacy_buy`.

Do czasu tego kroku:

- L2 ablation HOLD,
- collection HOLD,
- Phase B / P2 / live NO-GO,
- threshold tuning NO-GO.

## Walidacja

Uruchomione:

```text
cargo test -p ghost-launcher --lib p37_route_resolver -- --nocapture
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r13-executable-route-resolver.toml \
  --output-json /tmp/r16-r13-l1r17-join-key-audit.json \
  --output-md /tmp/r16-r13-l1r17-join-key-audit.md
```

Status:

- route resolver tests: PASS
- join-key audit tests: PASS
- R16-r13 L1R17 audit generation: PASS

