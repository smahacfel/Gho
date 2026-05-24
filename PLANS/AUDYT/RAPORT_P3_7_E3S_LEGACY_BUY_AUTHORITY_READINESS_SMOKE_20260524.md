# P3.7-E3S Legacy Buy Authority/Readiness Smoke

Data: 2026-05-24
HEAD: `6b7b42d`
Config lokalny: `configs/rollout/shadow-burnin-v3-p37-e3s-legacy-buy-authority-readiness-smoke.toml`
Namespace: `shadow-burnin-v3-p37-e3s-legacy-buy-authority-readiness-smoke`
Runtime: `timeout 20m`, shadow-only, promotion/P2/live off

## Werdykt

`E3S` nie odblokował execution.

Najważniejszy wynik jest rozdzielony:

- `legacy_buy_curve` convergence: **PASS**
- fallback/route executable account set: **FAIL / not executable**
- post-simulation `AccountNotFound`: **0**
- strict replay: **PASS**
- diagnostics/hash/join: **PASS**

To nie jest czyste `PASS-A`. Nie traktuję tego też jako pełne `PASS-B`, bo fallback `legacy_buy` nadal zawiera zanieczyszczenie primary route `bonding_curve_v2` w fallback account set, a równolegle raportuje braki ról, które powinny być obsłużone jako creatable/ephemeral, nie jako load-blocker.

## Run

Komenda:

```bash
timeout 20m env RUST_LOG=info ./target/release/ghost-launcher \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-e3s-legacy-buy-authority-readiness-smoke.toml
```

Proces zakończył się kodem `124`, czyli przez oczekiwany limit `timeout 20m`. Po runie nie został aktywny proces `ghost-launcher`.

Artefakty:

```text
buys.jsonl                         10 rows
shadow_entries.jsonl               10 rows
shadow_lifecycle.jsonl             10 rows
probe_selection.jsonl              15 rows
probe_skips.jsonl                  460 rows
seer_runtime_coverage_audit.jsonl  460 rows
```

## Replay / Diagnostics

`v3_full_replay_report.py --strict --json`:

```text
replay_status = full_replay_ok
total_rows = 460
v3_rows = 460
bad_rows = 0
```

`v3_p37_l1_reject_diagnostics.py --json`:

```text
diagnostic_quality.status = PASS
r16_artifact_identity_status = PASS
single_active_hash_status = PASS
decision_rows = 460
active_shadow_buys = 10
active_shadow_entries = 10
active_shadow_lifecycle = 10
probe_selection = 15
probe_transport = 0
probe_entries = 0
post_simulation_account_not_found_rows = 0
```

`v3_p37_mfs_lifecycle_join_key_audit.py`:

```text
probe_selection exact decision/V3 join = 15 / 15
probe_entry rows = 0
probe_lifecycle rows = 0
shadow_entry rows with ab_record_id = 10 / 10
shadow_lifecycle rows with ab_record_id = 10 / 10
```

## Active Path

Active BUY rows: `10`.

```text
legacy_buy_curve_authority_readiness_status:
  authoritative_and_load_ready = 10

legacy_buy_curve_authority_status:
  authoritative_account_state = 10

legacy_buy_curve_rpc_load_status:
  present_on_rpc_precheck = 10

legacy_buy_curve_rpc_load_ready:
  true = 10
```

To potwierdza, że E3 naprawił główny rozjazd z E2S dla samego `legacy_buy_curve`: ten sam curve jest teraz jednocześnie autorytatywny i load-ready.

Jednocześnie route nadal nie jest executable:

```text
legacy_buy_account_set_status:
  not_ready = 10

legacy_buy_route_ready:
  false = 10

legacy_buy_route_not_ready_reason:
  legacy_buy_simulation_load_not_ready = 10

fallback_route_attempted:
  true = 10

fallback_route_kind:
  legacy_buy = 10

fallback_route_ready:
  false = 10

fallback_failure_class:
  fallback_missing_user_ata_but_creatable = 10

route_resolution_status:
  no_executable_route_account_set = 10

selected_route_reason:
  no_route_candidate_passed_simulation_load_readiness = 10
```

Missing roles:

```text
legacy_buy_missing_roles:
  payer_pubkey = 10
  user_ata = 10
  user_volume_accumulator = 10

fallback_missing_roles:
  bonding_curve_v2 = 10
  payer_pubkey = 10
  user_ata = 10
  user_volume_accumulator = 10
```

Important: `fallback_creatable_account_set` zawiera `user_ata` i `user_volume_accumulator`, ale `fallback_missing_roles` nadal blokuje route na tych rolach. Dodatkowo `fallback_simulation_load_account_set` i `fallback_required_precheck_account_set` nadal zawierają primary-route `bonding_curve_v2`.

To oznacza, że po E3 problem nie jest już w authority/readiness `legacy_buy_curve`. Problem przesunął się do separacji fallback account set oraz klasyfikacji creatable/ephemeral accounts.

## Probe Path

Probe selection rows: `15`.
Probe transport rows: `0`.
Probe entry rows: `0`.

W `probe_skips.jsonl`:

```text
route_resolution_status:
  no_executable_route_account_set = 15
  None = 445

legacy_buy_curve_authority_readiness_status:
  authoritative_and_load_ready = 15
  None = 445

legacy_buy_route_ready:
  false = 15
  None = 445

lifecycle_label_eligibility:
  not_lifecycle_label_eligible = 15
  None = 445
```

Probe side również potwierdza convergence dla curve, ale nie daje transport/entry.

## Lifecycle / AccountNotFound

```text
simulation_outcome = failed: 10
lifecycle_label_eligibility = not_lifecycle_label_eligible: 10
active_shadow_lifecycle_eligibility_status = not_lifecycle_eligible: 10
simulation_error_category = active_shadow_precheck_failed: 10
post_simulation_account_not_found_rows = 0
successful_probe_entry_rows = 0
active_shadow_successful_entry_rows = 0
lifecycle_eligible_rows = 0
```

Precheck nadal fail-closed przed symulacją. Nie ma powrotu `AccountNotFound` po symulacji.

## Runtime Health Caveat

Log runtime zawiera 3 paniki workerów:

```text
thread 'tokio-rt-worker' panicked at ghost-brain/src/oracle/ultrafast/iwim.rs:573:16:
attempt to subtract with overflow
```

Proces nie zakończył się crashem przed `timeout`, a artefakty zostały zapisane, ale ten fakt trzeba traktować jako osobny runtime-health blocker przed jakimkolwiek dłuższym R18 lub szerszą walidacją. Nie wygląda to na bezpośrednią przyczynę braku `legacy_buy` execution, ale nie wolno tego zignorować.

## Decyzja

`E3S`:

```text
curve convergence: PASS
execution unlock: NO
fallback route success: 0
successful entries: 0
lifecycle eligible: 0
post-simulation AccountNotFound: 0
overall: FAIL/HOLD, not PASS-A
```

Nie uznaję jeszcze `legacy_buy` za martwe na podstawie E3S, bo E3 faktycznie rozwiązał authority/readiness dla core curve. Aktualny blocker jest węższy:

```text
fallback account set contamination / creatable account readiness contract
```

Następny techniczny krok, jeśli kontynuujemy `legacy_buy`, powinien być jeden i bardzo wąski:

```text
P3.7-E4 — legacy_buy fallback account-set separation + creatable/ephemeral precheck contract
```

Zakres E4:

1. `legacy_buy` fallback simulation/precheck set nie może zawierać primary-route `bonding_curve_v2`.
2. `user_ata` i `user_volume_accumulator` obecne w `creatable_account_set` nie mogą blokować route jako missing simulation-load accounts, jeśli builder faktycznie dodaje idempotent create.
3. `payer_pubkey` przy `payer_strategy = ephemeral` nie może być traktowany jak RPC-load account.
4. Route może przejść do `legacy_buy_route_ready=true` tylko po czystej separacji required-load vs creatable/ephemeral.

Jeśli E4 nie usunie tych trzech klas, wtedy `legacy_buy` należy zamknąć jako unsupported w obecnym route support.
