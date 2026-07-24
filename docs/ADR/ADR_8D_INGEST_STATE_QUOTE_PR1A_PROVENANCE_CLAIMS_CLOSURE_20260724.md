# ADR-8D: Ingest State Quote PR1A — domknięcie provenance transakcji i kontraktu claims

Status: `IMPLEMENTED / VERIFIED LOCALLY / REVIEW REQUIRED`

Typ: ADR-8D / review closure PR1A / addytywne kontrakty ingest

Data: `2026-07-24`

Repo: `/root/Gho_ingest`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Poprzedni ADR:
`docs/ADR/ADR_8D_INGEST_STATE_QUOTE_PR1A_CONTRACTS_PROVIDER_PROVENANCE_20260724.md`

Zakres: `PR 1 / Tura 1A` — wyłącznie domknięcie uwag review; bez 1B.

## D0. Decyzja

PR1A otrzymuje trzy addytywne korekty kontraktu:

1. provenance transakcyjny (`provider_id`, `provider_role`) jest zachowany od
   `PumpEvent::Transaction` przez `GeyserEvent`, `BinaryParser`,
   `TradeEvent`/`InitializePoolEvent` oraz `SeerEvent` IPC;
2. wcześniejszy, zbyt rygorystyczny `PumpMutationSemanticPayloadV1` zostaje
   zastąpiony przez `PumpMutationClaimsV1`, którego pola opisujące dane
   providera są opcjonalne;
3. nazwa rodziny dowodu, szerokość ordinala i semantyka hash są jednoznaczne:
   `ObservationSourceFamilyV1`, `u32` i `payload_hash_blake3`.

Te pola są obserwacją i audytowalną provenance. Nie wybierają primary,
nie deduplikują, nie zmieniają `AccountStateCore`, canonical emission,
`MaterializedFeatureSet`, Gatekeepera ani shadow/live behavior.

## D1. Problem

Po pierwszym review PR1A transport zachowywał provider provenance do
`GeyserEvent::Transaction`, ale parser wygaszał go przed `TradeEvent`.
Rozdzielenie primary od secondary, witness provenance i późniejsza
reconcyliacja nie mogą być poprawne, jeśli semantyczna obserwacja traci źródło
przed granicą IPC.

Ten sam review wykazał, że payload ze wszystkimi polami wymaganymi wymuszałby
na parsed NLN fałszywe defaulty albo odrzucenie niepełnej obserwacji. To
zacierałoby różnicę między `Unknown` i konkretnym, sprzecznym claimem.

## D2. Kontrakt danych

`PumpMutationClaimsV1` używa `Option` dla `curve`, `mint`, `route_variant`,
`side`, `success`, `token_amount_units` i wszystkich pól opcjonalnych.
`None` znaczy, że provider pola nie znał albo go nie dostarczył. Konflikt
powstaje wyłącznie między dwoma konkretnymi, nierównymi wartościami. PR1A nie
tworzy rygorystycznego validated fact; taka promocja wymaga późniejszej,
transaction-local primary raw validation.

`RawPumpMutationLocatorV1::semantic_event_ordinal` i
`CanonicalPumpOrderKeyV1::semantic_event_ordinal` są `u32`, zgodnie z
istniejącym `TradeEvent::event_ordinal`; nie wprowadzają węższego limitu
`u16`.

`ObservationSourceFamilyV1` to rodzina dowodu (`RawYellowstone` albo
`ParsedNln`), nie transportowe `SourceKind`. `payload_hash_blake3` jest BLAKE3
captured provider payload bytes przekazanych adapterowi do normalizacji, a
`source_family` wraz z `schema_id` definiują ich reprezentację. Yellowstone
przekazuje prost-encoded zdekodowany `SubscribeUpdateTransaction`; nie jest to
oryginalna ramka gRPC i nie obiecuje zachowania envelope ani nieznanych pól
wire. Hash jest kluczem audytu jednej obserwacji, a nie hashem zgodności
semantycznej raw protobuf z parsed JSON.

## D3. Granica propagacji

```text
raw Yellowstone PumpEvent::Transaction
  -> GeyserEvent::Transaction
  -> BinaryParser
  -> TradeEvent / InitializePoolEvent
  -> SeerEvent::{Trade, PoolDetected} IPC
  -> launcher odbiera identyczny typed event przed adapterem runtime
```

`InitializePoolEvent` kopiuje metadata do `CandidatePool`, więc zachowuje je
również `SeerEvent::PoolDetected`. `TradeEvent` zachowuje je w
`SeerEvent::Trade`.

Celowo nie rozszerzono `PoolTransaction`: jest on istniejącym wejściem
canonical runtime i dodanie tam nowych provider fields wymagałoby szerokiej
migracji niezwiązanych konstruktorów oraz ryzykowałoby zmianę authority.
Przyszły Observation Ledger może czytać provenance z granicy IPC bez takiego
przełączenia runtime.

## D4. Kompatybilność i formalne bramki

Nowe pola `TradeEvent`, `InitializePoolEvent` i `CandidatePool` są
`Option` z `#[serde(default, skip_serializing_if = "Option::is_none")]`.
Stare JSON bez provider metadata nadal się deserializują, a rekord bez
metadata nie otrzymuje dodatkowych kluczy podczas serializacji.

Receipt baseline `88aa1b7` pozostaje uczciwy co do czerwonych testów baseline.
Brakujący non-CI harness/corpus ma status `DEFERRED HARD GATE`, nie waiver:

- przed pierwszą zmianą zachowania transportu w 1B identyczny workload musi
  zostać uruchomiony na clean worktree rodzica 1A i na diffie 1B, z throughput,
  p99 receive-to-normalize, RSS i queue behavior;
- przed 1C/1D musi istnieć zamrożony corpus dla account duplicates, provider
  conflicts i raw/NLN reconciliation.

## D5. Inwarianty

- `tx_index = Some(0)` pozostaje prawidłowy;
- `txn_signature = None` pozostaje `None`;
- jedna signature może zawierać wiele poprawnych mutacji;
- żaden nowy typ nie steruje policy ani state authority;
- raw-first nie czeka na NLN, a NLN-first nie jest dopuszczony do runtime;
- nie dodano blocking send, gap ledger ani arbitrażu — to nadal zakres 1B+.

## D6. Weryfikacja wymagana dla closure

Wymagane są:

```text
cargo fmt --all --check
cargo test -p ghost-core ingest_integrity -- --nocapture
cargo test -p seer --lib account_update_preserves_provider_and_optional_transaction_signature -- --nocapture
cargo test -p seer --lib test_account_update_uses_curve_mapping -- --nocapture
cargo test -p seer --lib raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc -- --nocapture
timeout 900s cargo build --release --workspace
git diff --check
```

Dodatkowe testy closure pokrywają stary JSON `TradeEvent`,
`InitializePoolEvent -> CandidatePool -> IPC`, fail-closed provider role oraz
`EntryAnchor` provenance.

### Wynik closure z 2026-07-24

Wszystkie wymienione wyżej bramki przeszły na aktualnym diffie. W szczególności:

- `cargo fmt --all --check` i `git diff --check` — PASS;
- `cargo test -p ghost-core ingest_integrity -- --nocapture` — 8/8 PASS,
  w tym odczyt poprzednich nazw pól provenance;
- oba testy `AccountUpdate` wskazane powyżej — 1/1 PASS każdy;
- `raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc` — 1/1 PASS;
- grupa `provider_metadata` — 5/5 PASS;
- `connect_geyser_fails_closed_on_invalid_provider_role_contract` — 1/1 PASS;
- `cargo check -p ghost-brain --tests` oraz oba sprawdzenia
  `seer_connection_mode_test` — PASS;
- `timeout 900s cargo build --release --workspace` — PASS, exit 0,
  `Finished release profile [optimized] target(s) in 9m 42s` na finalnym diffie.

### Addendum końcowego review z 2026-07-24

Końcowa korekta PR1A precyzuje, że `payload_hash_blake3` jest BLAKE3 captured
provider payload bytes przekazanych adapterowi do normalizacji. Dla Yellowstone
są to prost-encoded bytes zdekodowanego `SubscribeUpdateTransaction`, a nie
oryginalna ramka gRPC. Dodano też test
`account_update_before_mapping_replays_provider_provenance_and_transaction_signature`,
który sprawdza pola `provider_id`, `provider_role` i `txn_signature` w
`PendingCurveUpdateSnapshot` oraz po replayu IPC.

Po tej korekcie `cargo test -p ghost-core ingest_integrity -- --nocapture`
pozostaje PASS 8/8, nowy test replayu jest PASS 1/1, a
`timeout 900s cargo build --release --workspace` jest PASS, exit 0. Żaden z
tych wyników nie zmienia klasyfikacji istniejących czerwonych bramek pakietowych
baseline ani nie rozpoczyna 1B.

Pełne testy pakietowe/workspace nadal są klasyfikowane wyłącznie przez ich
zapisane sygnatury baseline w receipt; nie są tutaj przedstawiane jako zielone.

## D7. Ryzyko i rollback

Ryzyko pozostaje średnie: typy przechodzą przez aktywny ingest, lecz są tylko
metadata. Największym ryzykiem byłaby przyszła utrata pola na nowym adapterze;
test raw-transaction-to-IPC oraz jawna granica `SeerEvent` stanowią regresyjną
ochronę. Rollback polega na wycofaniu addytywnych pól i typów — nie ma migracji
danych, aktywnego arbitra ani nowego zachowania decyzji do wygaszenia.
