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

## X8AS Smoke Decision

Code-level verdict: PASS.

Runtime X8AS verdict: PENDING.

Nie uruchamiano nowego runtime smoke, R18, P2/live ani Sender path. Ten raport nie claimuje `PASS-A` ani `PASS-B`.

Warunki runtime:

- `PASS-A`: BCV2 jest registered, included w aktywnym SubscribeRequest, hydration albo AccountUpdate daje real evidence, `working_builder_manifest_ready_rows > 0`, entries > 0, invarianty czyste.
- `PASS-B`: BCV2 jest included w SubscribeRequest, ale AccountUpdate/hydration nadal nie daje ready evidence. Blocker przechodzi wtedy na provider/timing/layout albo true missing, nie registry/fingerprint.
- `FAIL`: route-compatible observed BCV2 nie zmienia request shape, nie wymusza resubscribe, nie trafia do aktywnego SubscribeRequest, albo pojawia sie legacy/fallback/handoff pollution.

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
