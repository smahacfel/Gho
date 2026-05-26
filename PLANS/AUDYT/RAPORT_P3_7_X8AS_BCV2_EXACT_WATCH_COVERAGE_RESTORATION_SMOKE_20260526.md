# RAPORT P3.7-X8AS BCV2 Exact-Watch Coverage Restoration Smoke

Data: 2026-05-26

## Cel

X8AS domyka code-level naprawe coverage dla route-compatible `observed_bcv2_pubkey` w Seer/gRPC:

- route-compatible observed BCV2 jest rejestrowany w dedykowanym `bcv2_accounts` lane,
- `PrimaryGlobal` exact `SubscribeRequest` zawiera `bcv2_accounts + generic_accounts`,
- BCV2 ma priorytet przed generic przy limicie `EXACT_ACCOUNT_PAYLOAD_CAP`,
- BCV2 insert wymusza osobny immediate resubscribe path,
- route-compatible observed BCV2 uruchamia bounded RPC hydration jako evidence-only,
- readiness nadal wymaga realnego AccountUpdate/AccountStateCore/MFS/DIAG albo RPC fetch evidence.

## Zakres

In-scope:

- `off-chain/components/seer/src/grpc_connection.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/lib.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`

Out-of-scope i niezmienione:

- Gatekeeper policy, scoring, thresholds
- TX builder, Helius Sender, live execution
- legacy/fallback route revival
- BCV2 handoff patch
- static BCV2 discriminator/memcmp filter
- runtime smoke R18/P2/live path

## Implementacja

### Seer gRPC exact-watch

- Dodano `bcv2_accounts` lane w `AccountRegistry`.
- Dodano `insert_bcv2`, `remove_bcv2`, `contains_bcv2`, `bcv2_resub_notify`.
- `PrimaryGlobal` exact branch wybiera `bcv2_accounts` przed `generic_accounts`.
- `curve_accounts` i `pool_accounts` pozostaja poza `PrimaryGlobal` exact branch.
- `curve_accounts` i `pool_accounts` nadal nie zmieniaja `PrimaryGlobal` request fingerprintu.
- `SUBSCRIBE_SENT` rozszerzono o `tracked_bcv2`, `bcv2_sent`, `bcv2_dropped`.
- Dodano markery:
  - `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED`
  - `BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED`
  - `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT`
  - `BCV2_ACCOUNT_UPDATE_RECEIVED`

### Parser route-compatible observed BCV2

- `BinaryParser` moze korzystac ze wspoldzielonego `AccountRegistry`.
- W `GeyserGrpc` parser dostaje aktywny registry z `GrpcConnection`.
- Po enrichment i fingerprinting trade, tylko `provenance_status=route_compatible` rejestruje BCV2 exact-watch.
- Non-route-compatible observed tx meta pozostaje hintem i nie zmienia registry/request shape.
- Dodano marker:
  - `BCV2_EXACT_WATCH_REGISTERED`

### RPC hydration evidence

- Dodano bounded queue dla BCV2 RPC hydration.
- Hydration uzywa `processed` commitment i timeoutu 750 ms.
- Wynik jest tylko evidence path, bez podnoszenia manifest-ready samym observed tx meta.
- Dodano markery:
  - `BCV2_RPC_HYDRATION_READY`
  - `BCV2_RPC_HYDRATION_MISSING`

### Audit

Rozszerzono `scripts/v3_p37_mfs_lifecycle_join_key_audit.py` o sekcje `bcv2_exact_watch_coverage` i markdown `BCV2 Exact Watch Coverage`.
Audit uwzglednia tez rotowane pliki `system.log.*` i `oracle.log.*`, bo X8AS zapisuje runtime markery do logow z sufiksem daty.

Nowe liczniki:

- `bcv2_exact_watch_registered_rows`
- `bcv2_exact_watch_in_subscribe_request_rows`
- `bcv2_exact_watch_subscribe_dropped_rows`
- `bcv2_resubscribe_sent_rows`
- `bcv2_rpc_hydration_ready_rows`
- `bcv2_rpc_hydration_missing_rows`
- `bcv2_account_update_received_rows`
- `bcv2_account_state_seen_rows`
- `bcv2_account_state_owner_rows`
- `bcv2_account_state_data_len_rows`

Zachowane liczniki X8/readiness:

- `working_builder_bcv2_account_state_seen_rows`
- `working_builder_manifest_ready_rows`
- `successful_probe_entry_rows`
- `active_shadow_successful_entry_rows`

## Walidacja

Wyniki lokalne:

- `cargo check -p seer`: PASS
- `cargo test -p seer grpc_connection -- --nocapture`: PASS, 78 passed
- `cargo test -p seer binary_parser -- --nocapture`: PASS, 102 passed
- `python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v`: PASS, 36 passed
- `python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py`: PASS
- `cargo fmt --check`: PASS
- `git diff --check`: PASS

Uwagi do walidacji:

- `cargo check` i testy nadal emituja istniejace ostrzezenia z `ghost-core`/`seer`.
- W `binary_parser` naprawiono test fixture `make_ftdi_buy_event`, ktory uzywal `Pubkey::new_unique()` jako signera. Ten pubkey moze byc off-curve, a runtime context celowo filtruje off-curve ownerow. Fixture uzywa teraz realnego `Keypair::new().pubkey()`.

## X8AS Runtime Smoke

Konfiguracja smoke:

- namespace: `shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke`
- config lokalny: `configs/rollout/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke.local.toml`
- execution mode: `Shadow`
- entry mode: `shadow_only`
- builder mode: `working_builder_parity`
- R18/P2/live/Sender path: nieuruchomione

Artefakty runtime:

- preflight: `logs/rollout/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/x8as_preflight.log`
- runtime console: `logs/rollout/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/x8as_runtime_console.log`
- runtime status: `logs/rollout/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/x8as_runtime_status.env`
- audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/v3_p37_x8as_join_key_audit.json`
- audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x8as-bcv2-exact-watch-coverage-restoration-smoke/v3_p37_x8as_join_key_audit.md`

Runtime window:

- start UTC: `2026-05-26T06:33:39Z`
- stop UTC: `2026-05-26T06:41:39Z`
- exit status: `2`
- stop reason: watchdog fatal, gRPC stalled for `359166ms` with transport progress also stale for `359166ms`, above `300000ms`

Preflight: PASS.

Runtime zakonczyl sie przed planowanym limitem 30 minut z powodu provider/transport stall. Run nadal daje dowod exact-watch coverage, bo BCV2 registration, subscribe inclusion i forced resubscribe wystapily przed watchdog exit.

## Runtime Evidence

BCV2 exact-watch marker rows from the X8AS audit:

- `bcv2_exact_watch_registered_rows`: `2304`
- `bcv2_exact_watch_in_subscribe_request_rows`: `1508`
- `bcv2_exact_watch_subscribe_dropped_rows`: `0`
- `bcv2_resubscribe_sent_rows`: `1504`
- `bcv2_rpc_hydration_ready_rows`: `0`
- `bcv2_rpc_hydration_missing_rows`: `366`
- `bcv2_account_update_received_rows`: `64`
- `bcv2_account_state_seen_rows`: `0`
- `bcv2_account_state_owner_rows`: `0`
- `bcv2_account_state_data_len_rows`: `0`

Interpretacja:

- `BCV2_EXACT_WATCH_REGISTERED` jest dodatni.
- `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED` jest dodatni, wiec BCV2 weszlo do aktywnego `PrimaryGlobal` exact request surface.
- `BCV2_EXACT_WATCH_RESUBSCRIBE_SENT` jest dodatni, wiec osobny BCV2 immediate forced resubscribe path zadzialal.
- `BCV2_EXACT_WATCH_SUBSCRIBE_DROPPED` pozostaje `0`, wiec smoke nie pokazal utraty BCV2 przez cap.
- `BCV2_RPC_HYDRATION_READY` pozostaje `0`, a `BCV2_RPC_HYDRATION_MISSING` jest dodatni.
- `BCV2_ACCOUNT_UPDATE_RECEIVED` jest dodatni jako globalny exact-watch marker, ale nie wolno go mieszac z working-builder readiness evidence.

Uwaga licznikowa: powyzsze sa rows markerow z `system_log + oracle_log`; oba logi moga zawierac te same wpisy runtime, wiec traktujemy je jako evidence rows, nie jako unique pubkey count.

Working-builder/readiness evidence:

- `decision_rows_total`: `36`
- `probe_selected_rows`: `21`
- `observed_bcv2_rows`: `20`
- `observed_bcv2_route_compatible_rows`: `20`
- `working_builder_buy_variant_counts`: `{"routed_exact_sol_in": 20}`
- `working_builder_manifest_contains_bcv2_rows`: `20`
- `working_builder_manifest_ready_rows`: `0`
- `working_builder_manifest_missing_required_rows`: `20`
- `working_builder_bcv2_account_state_lookup_performed_rows`: `20`
- `working_builder_bcv2_account_state_seen_rows`: `0`
- `working_builder_bcv2_account_state_owner_rows`: `0`
- `working_builder_bcv2_account_state_data_len_rows`: `0`
- `working_builder_bcv2_rpc_fetch_ready_rows`: `0`
- `working_builder_bcv2_rpc_fetch_missing_rows`: `20`
- `working_builder_bcv2_account_update_received_rows`: `0`
- `working_builder_bcv2_account_update_mapped_rows`: `0`
- `working_builder_bcv2_subscription_requested_rows`: `0`
- `successful_probe_entry_rows`: `0`
- `active_shadow_successful_entry_rows`: `0`

To potwierdza fail-closed readiness: observed tx meta nie odblokowalo manifest-ready bez realnego AccountState/MFS/DIAG/RPC-ready evidence.

`working_builder_bcv2_subscription_requested_rows=0` jest polem decision/probe diagnostics i nie jest dowodem przeciw aktywnemu subscribe. Dowod aktywnego subscribe w X8AS pochodzi z runtime markerow `BCV2_EXACT_WATCH_SUBSCRIBE_INCLUDED` oraz `SUBSCRIBE_SENT`.

## X8AS Smoke Decision

Werdykt code-level: PASS.

Werdykt X8AS exact-watch coverage: PASS-B z watchdog caveat.

Pelny PASS-A: NIEOSIAGNIETY.

R18 nadal NO-GO.

Uzasadnienie:

- PASS-A nie jest spelniony, bo `working_builder_manifest_ready_rows=0`, `successful_probe_entry_rows=0`, `active_shadow_successful_entry_rows=0`, `bcv2_rpc_hydration_ready_rows=0`, a runtime zakonczyl sie watchdogiem `exit=2`.
- PASS-B coverage jest spelniony, bo route-compatible BCV2 zostalo registered, included w aktywnym SubscribeRequest i wymusilo resubscribe, ale AccountUpdate/hydration nie dostarczyly working-builder ready evidence.
- To nie jest registry/fingerprint failure: aktywny request shape i BCV2 resubscribe sa udowodnione runtime markerami.
- Nastepny blocker jest po stronie provider/transport stability, timing/layout albo true missing. Dodatkowy caveat to watchdog fatal po gRPC stall.
- Ten smoke nie pokazal legacy/fallback/handoff/live pollution.

## Invarianty

Zachowane:

- observed tx meta pozostaje discovery hintem,
- manifest-ready nie jest podnoszony bez account/RPC evidence,
- shadow/live boundary nie zostal zmieniony,
- Gatekeeper/scoring/thresholds nie zostaly zmienione,
- builder/Sender/live execution nie zostaly zmienione,
- legacy/fallback route nie zostal reaktywowany,
- `curve_accounts` i `pool_accounts` nie trafiaja do `PrimaryGlobal` exact branch.

Residual risk:

- Jesli runtime pokaze `included` bez AccountUpdate/hydration ready, nastepny etap to provider/timing/layout audit albo true missing, nie builder/Gatekeeper.
- X8AS nie dodaje static BCV2 discriminator/memcmp, bo repo nadal nie ma potwierdzonego BCV2 discriminator.
