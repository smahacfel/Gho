# RAPORT P3.7-L1R9 Active Shadow BondingCurveV2 Precheck Contract

Data: 2026-05-22
Namespace smoke: `shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck`
Commit bazowy: `0ee7f59b1230e9f1234ac2a4b8efd1b79054b357`

## Werdykt

`P3.7-L1R9`: PASS diagnostyczny dla kontraktu active shadow precheck.

Active shadow `bonding_curve_v2` nie dochodzi już do `simulate_buy` jako runtime
`AccountNotFound`. Brak tego konta jest fail-closed przed symulacją:

```text
precheck_failure_reason = execution_account_not_ready:bonding_curve_v2:<pubkey>
simulation_error_category = active_shadow_precheck_failed
active_shadow_lifecycle_eligibility_status = not_lifecycle_eligible
```

Nie jest to PASS wykonania active shadow. Successful active shadow entries nadal
nie powstały w R16-r9, więc L2 ablation, collection, Phase B, P2/live i tuning
pozostają HOLD / NO-GO.

## Problem

Po L1R6 active shadow `AccountNotFound` był już atrybuowany do
`bonding_curve_v2`, ale ścieżka active shadow nadal próbowała wykonać symulację
z brakującym kontem w account metas. To łamało kontrakt:

```text
bonding_curve_v2 w transaction account metas
brak fail-closed prechecku
RPC simulation AccountNotFound
```

Jeżeli konto jest ładowane przez transakcję symulacyjną, to dla shadow execution
jest simulation-load required. Nie wolno traktować go jako optional tylko dlatego,
że nie jest częścią węższego historycznego required-role setu.

## Zmiany w kodzie

### Runtime

W `ghost-launcher/src/oracle_runtime.rs` dodano pre-simulation contract check dla
active shadow dispatch w trybie shadow-only:

- przed `dispatch_prepared_buy_shadow_only`;
- przed `dispatch_prepared_buy_with_shadow`, gdy entry mode to `ShadowOnly`;
- bez wpływu na live/P2.

Check wykorzystuje istniejące role/account readiness logic i zwraca syntetyczny
`TriggerDispatchReceipt` z błędem:

```text
execution_account_not_ready:<role>:<pubkey>
```

Taki wynik jest potem serializowany jako active shadow dispatch failure, nie jako
runtime simulation error.

### Diagnostyka eventów

W `ghost-launcher/src/events.rs` dodano addytywne pole:

```text
precheck_failure_reason
```

W active shadow diagnostics precheck failure:

- nie ustawia `simulation_error_kind = AccountNotFound`;
- ustawia `simulation_error_category = active_shadow_precheck_failed`;
- zachowuje role/pubkey/source brakującego konta;
- oznacza row jako `not_lifecycle_eligible`.

### Audit

W `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` dodano liczniki:

- `active_shadow_precheck_failed_rows`;
- `active_shadow_runtime_simulation_error_rows`;
- `active_shadow_simulation_required_account_not_in_precheck_count`;
- `active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows`;
- `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows`;
- `active_shadow_successful_entry_rows`;
- `active_shadow_lifecycle_eligible_rows`.

## Wynik R16-r9

Artefakty raportowe wygenerowano pod:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r9-active-shadow-bcv2-precheck/
```

### Replay

```text
status = ok
replay_status = full_replay_ok
total_rows = 79
v3_rows = 79
bad_rows = 0
```

### L1 reject diagnostics

```text
diagnostic_quality.status = PASS
active_shadow_account_not_found_rows = 0
```

### Active shadow dispatch diagnostics

```text
active_shadow_dispatch_failure_rows = 6
active_shadow_precheck_failed_rows = 6
active_shadow_runtime_simulation_error_rows = 0
active_shadow_bonding_curve_v2_precheck_skipped_before_simulation_rows = 6
active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows = 0
active_shadow_account_not_found_unattributed_rows = 0
active_shadow_successful_entry_rows = 0
active_shadow_lifecycle_eligible_rows = 0
active_shadow_lifecycle_eligible_failure_rows = 0
active_shadow_simulation_required_account_not_in_precheck_count = 0
active_shadow_precheck_status_counts = {"precheck_failed": 6}
active_shadow_simulation_error_category_counts = {"active_shadow_precheck_failed": 6}
active_shadow_account_set_match_counts = {"true": 6}
```

Interpretacja:

- L1R9 naprawia kontrakt active shadow precheck/simulation.
- Brakujące `bonding_curve_v2` nie przechodzi już jako runtime
  `AccountNotFound`.
- Dispatch failures nie są lifecycle-eligible.
- Active shadow execution nadal nie daje successful entries, bo konto wymagane
  do załadowania transakcji nie jest execution-ready.

## Testy i walidacja

Uruchomione testy:

```text
cargo test -p ghost-launcher --lib active_shadow_precheck_failure_is_not_runtime_account_not_found -- --nocapture
cargo test -p ghost-launcher --lib active_shadow_account_not_found -- --nocapture
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/v3_p37_l1_reject_diagnostics.py scripts/v3_p37_probe_execution_account_readiness_report.py scripts/v3_p37_shadow_lifecycle_labeler.py scripts/shadow_onchain_lifecycle_report.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs ghost-launcher/src/events.rs
git diff --check
```

Wynik: PASS.

## Ograniczenia

R16-r9 był krótkim smoke kontraktowym, nie policy/lifecycle quality runem.

Raport join-key pokazał `join_key_acceptance = degraded` / `join_quality =
mint_only`, ponieważ w tej próbce nie powstały probe transport/entry rows i run
nie jest przeznaczony do wnioskowania o jakości policy. To nie obala L1R9,
który sprawdzał kontrakt active shadow precheck dla `bonding_curve_v2`.

W logach pojawiły się również ostrzeżenia o brakujących reason code dla części
`v25_shadow` plane rows. To jest osobna higiena DecisionLogger/reportingu. Nie
zmienia werdyktu L1R9, ale nie wolno używać R16-r9 jako finalnego materiału do
L2 ablation.

## Aktualny stan

Naprawione:

- active shadow `AccountNotFound` nie jest ślepy;
- active shadow `bonding_curve_v2` runtime `AccountNotFound` został przeniesiony
  do pre-simulation precheck failure;
- failure rows zachowują identity/hash i nie są lifecycle-eligible.

Nadal blokuje:

- active shadow successful entry rows = 0;
- active execution jest blokowany przez `bonding_curve_v2` readiness/coverage;
- probe path w tej próbce nie wygenerował transport/entry;
- L2 policy ablation nadal nie ma czystego execution/lifecycle materiału.

## Następny krok

Nie przechodzić do L2, collection, Phase B ani P2/live.

Następny etap powinien rozstrzygnąć, dlaczego `bonding_curve_v2` wymagane przez
builder/route nie jest dostępne dla active shadow simulation-load w momencie
decyzji:

- account coverage / RPC visibility;
- route identity;
- builder account-source;
- execution-readiness universe.

Jeśli przyszły run pokaże successful active shadow entries i lifecycle labels z
utrzymanym diagnostic PASS, dopiero wtedy można wrócić do L2 ablation.
