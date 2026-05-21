# RAPORT P3.7-J3K5 Creator-Vault Source Authority / Amount Guard

## Status

```text
J3K5 code-level implementation: PASS
R15 J3K5-r1 runtime smoke: MINIMAL PASS / DIAGNOSED
R15 J3K5-r2 runtime smoke: MINIMAL PASS / DIAGNOSED
Creator-vault authority diagnostics: RUNTIME OBSERVED
Amount guard diagnostics: CODE/TEST PASS, NOT OBSERVED IN R2
Collection / Phase B / P2 / live / tuning: HOLD / NO-GO
```

## Cel

J3K5 domyka diagnostykę dwóch klas błędów zobaczonych po pierwszych działających
probe transport/entry rows:

- `InstructionError(..., Custom(2006))` / `anchor_constraint_seeds` dla
  `creator_vault`;
- `InstructionError(..., Custom(6002))` / `too_much_sol_required`.

Zmiana nie poprawia requestu przez zgadywanie kont z logów symulacji. Logi
Anchor `Left/Right` są używane wyłącznie jako diagnostyka po fakcie.

## Zmiany

Runtime probe transport records dostały addytywne pola:

```text
creator_vault_authority_status
creator_vault_actual_pubkey
creator_vault_expected_pubkey
creator_vault_mismatch_reason
creator_identity_source
creator_identity_authoritative
amount_provided_lamports_if_available
amount_required_lamports_if_available
amount_shortfall_lamports_if_available
amount_guard_status
```

Semantyka:

- `creator_vault_source_not_authoritative` oznacza, że vault zbudowany przez
  request nie zgadza się z vaultem oczekiwanym przez program w logach Anchor.
- `creator_identity_authoritative=false` oznacza, że obecne źródło
  `creator_pubkey` nie jest wystarczającym dowodem dla tej route.
- `amount_required_exceeds_probe_amount` oznacza, że Pump.fun podał w logach
  kwotę wymaganą większą niż kwota probe.

Audit `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` raportuje teraz liczniki:

```text
creator_vault_authority_status_counts
creator_vault_mismatch_reason_counts
creator_identity_source_counts
amount_guard_status_counts
simulation_error_custom_code_counts
```

Dodano profil bounded smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1.toml
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2.toml
```

## Walidacja

Uruchomione lokalnie:

```text
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
```

Wynik:

```text
Python join-key audit tests: 10/10 PASS
Rust p37_shadow_probe tests: 40/40 PASS
Rust p37_counterfactual_probe tests: 8/8 PASS
```

## Runtime Smoke

Po implementacji uruchomiono bounded smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r1.toml
```

Wynik szczegółowy:

```text
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R1_SMOKE_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R1_JOIN_KEY_AUDIT_20260521.md
```

Najważniejszy wynik:

```text
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
simulation_error_custom_code_counts = {"custom_6002": 1}
amount_guard_status_counts = {"amount_guard_values_unavailable": 1}
creator_vault_authority_status_counts = {}
```

W smoke nie wystąpił `custom_2006`, więc creator-vault authority path pozostaje
code/test validated, ale bez nowego runtime row. `custom_6002` wystąpił i został
sklasyfikowany, ale logi tego row nie zawierały parseowalnych wartości
`Left/Right`.

Po tej obserwacji parser został poprawiony tak, aby obsługiwać inline Anchor
logs:

```text
Program log: Left: <value>
Program log: Right: <value>
```

Dodano testy pokrywające inline `Left/Right`.

## Runtime Smoke R2

Po poprawce parsera uruchomiono świeży bounded smoke:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j3k5-r2.toml
```

Wynik szczegółowy:

```text
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R2_SMOKE_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J3K5_BOUNDED_R2_JOIN_KEY_AUDIT_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J3K5_CREATOR_VAULT_AMOUNT_GUARD_R2_20260521.md
```

Najważniejszy wynik:

```text
v3_rows = 4
strict replay = full_replay_ok
probe_selection_rows = 19
probe_transport_rows = 10
probe_shadow_entry_rows = 9
probe_required_exact_decision_v3_join_coverage = 1.0
simulation_error_custom_code_counts = {"custom_2006": 2}
creator_vault_authority_status_counts = {"creator_vault_source_not_authoritative": 2}
creator_vault_mismatch_reason_counts = {"actual_expected_mismatch": 2}
amount_guard_status_counts = {}
active_buys_rows = 0
```

R2 nie zaobserwował `custom_6002`, więc amount guard parser pozostaje
code/test validated bez nowego runtime row po poprawce inline parsera. R2
zaobserwował natomiast `custom_2006` i potwierdził, że creator-vault mismatch
jest klasyfikowany jako `creator_vault_source_not_authoritative`.

## Decyzja

J3K5 jest zaliczone jako code-level PASS i bounded smoke MINIMAL PASS /
DIAGNOSED. Nie odblokowuje to jeszcze collection.

Następny gate:

```text
Q: naprawić creator-vault source authority / route identity dla custom_2006
```

Acceptance dla kolejnego smoke:

- strict replay OK;
- exact decision/V3 join 100%;
- active BUY rows pozostają 0 / niezmienione przez probe;
- creator-vault custom 2006 ma source-authority diagnosis;
- TooMuchSolRequired custom 6002 ma amount guard diagnosis, jeśli logi zawierają
  parseowalne wartości;
- brak P2/live/IWIM/threshold zmian.
