# RAPORT P3.7-J3K5 R15 Bounded R1 Smoke

## Status

```text
R15 bounded j3k5-r1 smoke: MINIMAL PASS / DIAGNOSED
J3K5 creator-vault source authority diagnostics: RUNTIME READY, no custom_2006 row observed
J3K5 amount guard diagnostics: RUNTIME READY, custom_6002 observed but Left/Right amount values unavailable
Collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Run

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1
```

Run został przerwany wcześnie po osiągnięciu targetu `probe_transport_rows >= 10`,
zamiast czekać na pełny timeout.

## Wyniki

```text
probe_selection_rows = 13
probe_skips_rows = 3
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_lifecycle_rows = 0
active_buys_rows = 0
```

Replay:

```text
v3_rows = 1
strict replay = full_replay_ok
bad_rows = 0
stale_against_config = false
```

Join-key audit:

```text
probe_readiness = ready_for_probe_transport_entry_join
probe_join_key_acceptance = pass
probe_join_quality = exact_probe_id_and_ab_record_id
probe_decision_join_acceptance = pass
probe_required_exact_decision_v3_join_coverage = 1.0
probe_transport_rows_with_ab_record_id = 10
probe_entry_rows_with_ab_record_id = 9
probe_transport_rows_with_probe_id = 10
probe_entry_rows_with_probe_id = 9
```

Materializacja entry:

```text
entry_materialized = 8
simulation_error = 1
transport_only_missing_token_quantity = 1
```

Reason counts:

```text
entry_row_present = 8
routed_exact_sol_in_entry_token_amount_raw_null = 1
simulation_slippage_or_price_mismatch:custom_6002 = 1
```

Buy variants:

```text
legacy_buy = 9
routed_exact_sol_in = 1
```

Token param roles:

```text
token_amount = 9
min_tokens_out = 1
```

## J3K5 Diagnostics

Creator-vault source authority:

```text
creator_vault_authority_status_counts = {}
creator_vault_mismatch_reason_counts = {}
creator_identity_source_counts = {}
```

W tym smoke nie wystąpił `custom_2006`, więc runtime nie miał okazji wypełnić
`creator_vault_source_not_authoritative`. Kod i audit są gotowe na tę klasę, ale
ten run jej nie zaobserwował.

Amount guard:

```text
simulation_error_custom_code_counts = {"custom_6002": 1}
amount_guard_status_counts = {"amount_guard_values_unavailable": 1}
```

`custom_6002` został sklasyfikowany jako
`simulation_slippage_or_price_mismatch`, ale logi tego konkretnego transport row
nie zawierały parseowalnych wartości `Left/Right`, więc pola
`amount_provided_lamports_if_available`,
`amount_required_lamports_if_available` i
`amount_shortfall_lamports_if_available` pozostały `null`.

## Decyzja

J3K5 smoke potwierdza:

- counterfactual probe selection/transport/entry nadal działa;
- exact decision/V3 join pozostaje 100%;
- active BUY nie został zmutowany;
- nowe pola diagnostyczne nie psują legacy rows ani join-key audit;
- `custom_6002` jest rozpoznany jako amount/slippage class, ale bez kwot z logów
  w tym konkretnym przebiegu.

To nie odblokowuje jeszcze collection. Przed zwiększeniem skali trzeba rozstrzygnąć,
czy brak wartości `Left/Right` dla `custom_6002` jest normalny dla części logów,
czy wymaga pobrania pełniejszych simulation logs z transport path.
