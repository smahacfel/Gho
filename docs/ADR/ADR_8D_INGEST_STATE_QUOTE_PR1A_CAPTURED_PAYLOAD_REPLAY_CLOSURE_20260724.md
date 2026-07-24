# ADR-8D: Ingest State Quote PR1A — captured payload hash i replay pending update

Status: `IMPLEMENTED / VERIFIED LOCALLY / REVIEW REQUIRED`

Typ: ADR-8D / review closure PR1A / addytywne kontrakty ingest

Data: `2026-07-24`

Repo: `/root/Gho_ingest`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Poprzednie ADR-y:

- `docs/ADR/ADR_8D_INGEST_STATE_QUOTE_PR1A_CONTRACTS_PROVIDER_PROVENANCE_20260724.md`;
- `docs/ADR/ADR_8D_INGEST_STATE_QUOTE_PR1A_PROVENANCE_CLAIMS_CLOSURE_20260724.md`.

Zakres: `PR 1 / Tura 1A` — końcowa korekta review; bez rozpoczęcia 1B.

## D0. Decyzja

`ObservationProvenanceV1::payload_hash_blake3` opisuje BLAKE3 captured
provider payload bytes przekazanych przez adapter do normalizacji. Nie
deklaruje nieposiadanych przez aktywny klient Yellowstone oryginalnych bajtów
ramki gRPC.

W Yellowstone klient najpierw otrzymuje zdekodowane `SubscribeUpdate`, a
adapter zapisuje jego dziecko `SubscribeUpdateTransaction` przez `prost` przed
oddaniem go do parsera. Dla `RawYellowstone` są to zatem prost-encoded bytes
tego zdekodowanego komunikatu. `source_family` i `schema_id` są wymaganymi
oznaczeniami tej reprezentacji. Envelope, kolejność/obecność nieznanych pól i
inne właściwości pierwotnej ramki gRPC nie są przez ten kontrakt obiecywane.

Pomocnik został nazwany
`payload_hash_for_captured_provider_payload`, aby API nie sugerowało dostępu do
wire bytes.

## D1. Problem

Poprzednia dokumentacja nazywała wejście hash „dokładnymi provider wire bytes”.
To byłoby prawdziwe tylko przy przechwytywaniu faktycznej ramki przed dekodowaniem.
Obecny adapter celowo tego nie robi; reserializacja zdekodowanej wiadomości jest
bezpieczną, deterministyczną reprezentacją adaptera, ale nie jest historycznym
payloadem transportowym.

W osobnym miejscu brakowało testu przejścia metadanych przez bufor
`PendingCurveUpdateSnapshot`: implementacja kopiowała pola, lecz test pokrywał
dotąd tylko `None`.

## D2. Granice i zachowanie

```text
decoded Yellowstone SubscribeUpdate
  -> prost-encoded SubscribeUpdateTransaction captured by adapter
  -> PumpEvent::Transaction::raw
  -> parser / normalization
```

Ten kontrakt nie przechwytuje ramek HTTP/2 lub gRPC i nie zmienia pętli odbioru,
kolejek, backpressure ani authority. Ewentualne prawdziwe wire capture wymaga
osobnego, jawnego projektu; nie jest potrzebne do PR1A.

Test replay udowadnia pełny przebieg:

```text
GeyserEvent::AccountUpdate(provider_id, provider_role, txn_signature)
  -> PendingCurveUpdateSnapshot
  -> register_curve_mapping
  -> replay_pending_curve_update_from_state
  -> SeerEvent::AccountUpdate IPC
```

Test asercyjnie sprawdza pola zarówno w snapshotcie bufora, jak i w wyniku IPC.

## D3. Inwarianty

- `payload_hash_blake3` nie jest używany do canonical state, deduplikacji ani
  decyzji Gatekeepera;
- `source_family` i `schema_id` definiują reprezentację hash bez mieszania
  raw protobuf i parsed NLN;
- `provider_id`, `provider_role` i `txn_signature` są tylko provenance;
- brak sygnatury pozostaje `None`;
- nie ma branchu po roli providera w `AccountStateCore`,
  `MaterializedFeatureSet`, canonical emission, shadow ani live execution;
- 1B (backpressure/gap behavior) pozostaje nierozpoczęte.

## D4. Dowód i CI

Wymagane przed publikacją finalnego Commit 1A:

```text
cargo fmt --all --check
cargo test -p ghost-core ingest_integrity -- --nocapture
cargo test -p seer --lib account_update_before_mapping_replays_provider_provenance_and_transaction_signature -- --nocapture
cargo test -p seer --lib raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc -- --nocapture
cargo test -p seer --lib account_update_preserves_provider_and_optional_transaction_signature -- --nocapture
cargo test -p seer --lib test_account_update_uses_curve_mapping -- --nocapture
cargo check -p ghost-brain --tests
timeout 900s cargo build --release --workspace
git diff --check
```

Znane czerwone workflowy GitHub Actions pozostają sklasyfikowane baseline:
`Restore Lifecycle Guard` i `Metric Contracts PR2C` kończą się `E0063` w
istniejących fixture'ach `PoolTransaction`/`TradeEvent`, które nie są częścią
PR1A. Nie są one przedstawiane jako zielone ani naprawiane szeroką zmianą
fixture'ów.

### Wynik z 2026-07-24

- `cargo fmt --all --check` — PASS;
- `cargo test -p ghost-core ingest_integrity -- --nocapture` — PASS, 8/8;
- `cargo test -p seer --lib account_update_before_mapping_replays_provider_provenance_and_transaction_signature -- --nocapture` — PASS, 1/1;
- `cargo test -p seer --lib raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc -- --nocapture` — PASS, 1/1;
- oba wskazane testy `AccountUpdate` — PASS, 1/1 każdy;
- `cargo check -p ghost-brain --tests` — PASS;
- `timeout 900s cargo build --release --workspace` — PASS, exit 0;
- `git diff --check` — PASS.

Build ma wyłącznie istniejące warnings/future-incompat dla `solana-client
v1.18.26`; nie ma errorów kompilacji PR1A.

## D5. Rollback

Rollbackiem jest rewert całego finalnego Commit 1A. Nie powstaje migracja
danych, nowy runtime authority ani niezależny stan do czyszczenia.
