# ADR-8D: Ingest State Quote PR1A — kontrakty locator/order/provenance i role providerów

Status: `IMPLEMENTED / VERIFIED LOCALLY / REVIEW REQUIRED`

Typ: ADR-8D / addytywne kontrakty ingest / brak zmiany runtime authority

Data: `2026-07-24`

Repo: `/root/Gho_ingest`

Plan SSOT:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Zakres: `PR 1 / Tura 1A`

## D0. Decyzja

Wprowadzono addytywne, serializowalne kontrakty:

- `RawPumpMutationLocatorV1`;
- `CanonicalPumpOrderKeyV1`;
- `PumpMutationClaimsV1`;
- `ObservationProvenanceV1`;
- `RawProviderRoleV1::{PrimaryAuthority, SecondaryWitness}`;
- pomocnicze typowane enumy źródła, strony transakcji i limitu instrukcji.

Rola providera i `provider_id` są materializowane na wejściu Seera i
przenoszone jako metadane przez `PumpEvent`, `GeyserEvent` oraz IPC. Dotyczy
to także `EntryAnchor`, ponieważ ten event jest obserwacją Yellowstone
używaną później jako wejście do coverage/gap.
Opcjonalna `txn_signature` z account update jest przenoszona z protokołu
Yellowstone aż do `AccountStateUpdate`.

Zmiana nie przyznaje nowym typom żadnej authority. Redukcja stanu nadal
korzysta wyłącznie z dotychczasowych pól konta, a istniejący flow decyzji
nie odczytuje `provider_role`.

## D1. Problem

Przed PR1A aktywna ścieżka znała nazwę providera lokalnie w warstwie gRPC,
ale traciła ją podczas normalizacji. Account update tracił także opcjonalną
sygnaturę transakcji udostępnioną przez przypięty protokół Geyser. Nie
istniały przy tym rozdzielone kontrakty:

- identity/locator mutacji;
- kanonicznego klucza porządku;
- payloadu semantycznego;
- provenance obserwacji;
- jawnej roli primary/secondary providera.

Bez takiego rozdzielenia kolejne tury planu nie mogłyby budować deduplikacji,
deterministycznego gapu ani arbitrażu providerów bez ryzyka zlania tożsamości
zdarzenia, kolejności i authority w jeden niejawny kontrakt.

## D2. Granice kontraktów

`RawPumpMutationLocatorV1` zawiera wyłącznie:

```text
program_id
signature
outer_instruction_index
inner_instruction_path
semantic_event_ordinal
```

Nie zawiera providera, slotu, `tx_index`, strony transakcji, wartości ani
rezerw. Dzięki temu ten sam event widziany przez dwóch providerów zachowuje
tę samą tożsamość, a jedna sygnatura może opisać kilka poprawnych mutacji.

`CanonicalPumpOrderKeyV1` przechowuje osobno slot, wymagany `tx_index` i
indeksy instrukcji/eventu. Wartość `tx_index = 0` jest pełnoprawnym
porządkiem transakcji i nie jest zamieniana na brak danych.

`PumpMutationClaimsV1` opisuje deklaracje ekonomicznego sensu mutacji bez
provenance. Każde jego pole jest opcjonalne: brak oznacza `Unknown`, nie
domyślną wartość ani konflikt. Rygorystyczny validated fact po primary-raw
validation jest świadomie poza PR1A. `ObservationProvenanceV1` opisuje
źródło, provider, schema, BLAKE3 captured provider payload bytes i moment
odbioru bez wpływania na identity ani state. Reprezentację bytes definiują
`ObservationSourceFamilyV1` i `schema_id`; Yellowstone przekazuje
prost-encoded zdekodowany `SubscribeUpdateTransaction`, a nie oryginalną
ramkę gRPC. `ObservationSourceFamilyV1` opisuje rodzinę dowodu, a nie
istniejący transportowy `SourceKind`.
Deklarowana rola providera jest przenoszona osobnym polem transportowym; nie
jest częścią locatora ani kontraktu provenance z planu.

## D3. Propagacja aktywnej ścieżki

Metadane account update są przenoszone następująco:

```text
Yellowstone SubscribeUpdate
  -> Provider/PumpEvent
  -> GeyserEvent
  -> Seer pending account snapshot
  -> DetectedAccountUpdateEvent (IPC)
  -> launcher AccountUpdateEvent
  -> AccountStateUpdate
```

Metadane transaction/entry są przenoszone analogicznie:

```text
Provider/PumpEvent::{Transaction, BackfillTransaction, EntryUpdate}
  -> GeyserEvent::{Transaction, EntryAnchor}
  -> BinaryParser
  -> TradeEvent / InitializePoolEvent
  -> SeerEvent::{Trade, PoolDetected} IPC
  -> launcher obserwuje ten sam typed payload przed adapterem PoolTransaction
```

Adapter `TradeEvent -> PoolTransaction` pozostaje niezmienioną granicą
canonical runtime. Nie dodano tam nowego pola ani branchu po roli providera:
metadata jest dostępna dla IPC i przyszłego Observation Ledger, lecz nie
przechodzi ukrycie do `AccountStateCore`, `MaterializedFeatureSet` ani policy.

`SubscribeUpdateAccountInfo.txn_signature` jest walidowana jako sygnatura
Solana tylko wtedy, gdy istnieje. Brak pola pozostaje `None`; nie jest
syntetyzowana pusta lub zastępcza sygnatura.

`SeerConfig` i konfiguracja launchera deklarują produkcyjny kontrakt:
`primary_raw_provider_id` oraz `secondary_raw_provider_ids`. Oba pola są
kompatybilne ze starymi plikami konfiguracyjnymi: brak primary oznacza
`"primary"`, a brak listy secondary oznacza pustą listę.

Przed uruchomieniem workerów `GrpcConnection::connect_geyser()` waliduje
synchronicznie całe zestawienie endpointów: ID muszą być niepuste po trimie
i unikalne, musi istnieć dokładnie jeden `PrimaryAuthority`, a przy
konfiguracji produkcyjnej każdy endpoint musi być zadeklarowany dokładnie w
roli zgodnej z listą primary/secondary. Błąd walidacji wraca do callera jako
`SeerError::ConfigError` przed `tokio::spawn`, przed przejęciem receiverów i
przed oznaczeniem streamu jako aktywnego. `YellowstoneConnector::run()`
powtarza walidację defensywnie, ale nie jest już pierwszą publiczną bramką.
Konstruktor kompatybilnościowy nadal tworzy pojedynczy provider o ID
`"primary"`; próba zbudowania wielu takich providerów kończy się błędem
walidacji przed startem, zamiast dopuszczać dwie authority.

Aktywna konfiguracja ma jeden endpoint i deklaruje jawnie stabilny provider:
`primary_raw_provider_id = "primary"` oraz `secondary_raw_provider_ids = []`.
Nie próbuje ukrycie materializować endpointów secondary tylko z ich ID.
Niepusta lista secondary bez odpowiadających endpointów jest błędem startu
fail-closed. Pełne zestawienie primary+secondary jest dostępne w `GrpcConfig`
przez jawne entry providerów. Samo oznaczenie secondary nie powoduje arbitrażu
ani tłumienia update'u — to pozostaje poza 1A.

## D4. Kompatybilność i parity

Nowe pola transportowe są `Option` z `#[serde(default)]` i nie są
serializowane, gdy mają wartość `None`. W konsekwencji:

- stare JSONL bez nowych pól nadal się deserializują;
- baseline JSON dla zdarzeń bez provenance pozostaje bez nowych kluczy;
- stare JSON `GeyserEvent::EntryAnchor` bez provider metadata nadal się
  deserializują;
- stare konfiguracje nie wymagają migracji: pola config mają serde defaults
  `"primary"` i pustą listę secondary;
- `tx_index = Some(0)` przechodzi przez dekoder i serde jako zero;
- `txn_signature = None` pozostaje `None`;
- legacy builder `AccountStateUpdate` nadal ustawia wszystkie nowe pola na
  `None`.

Test parity podaje reducerom identyczny update raz bez provenance, a raz z
providerem i sygnaturą. Wynikowy canonical state jest równy.

## D5. Zachowane inwarianty i świadome wyłączenia

Zachowano:

- brak nowego odczytu live state w Gatekeeper;
- brak nowego źródła authority dla `AccountStateCore`;
- brak zmiany `MaterializedFeatureSet`, verdictów lub reason codes;
- brak zmiany shadow/live mode;
- brak zmiany outputu dla istniejących zdarzeń z pustymi metadanymi;
- brak zależności od NLN;
- raw path nie czeka na żadne źródło wtórne.

Świadomie nie zaimplementowano elementów kolejnych tur:

- bounded queue i try-send;
- deterministic gap po saturacji;
- observation ledger;
- primary/secondary arbiter;
- exact-once state apply;
- NLN witness path;
- quote routing.

To rozdzielenie jest konieczne, aby PR1A pozostał zmianą kontraktową bez
ukrytej zmiany runtime authority.

## D6. Weryfikacja

Wymagany zestaw:

```text
cargo fmt --all --check
cargo test -p ghost-core
cargo test -p seer
cargo test -p ghost-launcher
git diff --check
```

Dodatkowe testy kontraktowe obejmują:

- dokładny kształt serializowanego locatora;
- rozróżnienie kilku mutacji wewnątrz jednej sygnatury;
- round-trip `tx_index = Some(0)`;
- account update z `txn_signature = Some(...)` i `None`;
- przeniesienie `provider_id` i roli przez Seer IPC i launcher;
- przeniesienie `provider_id` i roli z `PumpEvent::EntryUpdate` do
  `GeyserEvent::EntryAnchor`;
- odczyt starego JSON bez nowych pól;
- niezmieniony baseline JSON przy metadanych `None`;
- równość canonical state niezależnie od nowych metadanych.

Po review cofnięto wszystkie niezwiązane z PR1A mechaniczne korekty
fixture'ów `PoolTransaction`/`TradeEvent`. W diffie pozostają tylko trzy
konieczne konstruktory `AccountStateUpdate` w testach launchera oraz trzy w
`ghost-brain`, uzupełnione nowymi opcjonalnymi polami `None`. Nie naprawiano
ani nie klasyfikowano jako część PR1A bazowych fixture'ów innych typów.

Receipt baseline i bieżących bramek jest prowadzony osobno w
`PLANS/DO_REALIZACJI/BASELINE_RECEIPT_INGEST_STATE_QUOTE_88AA1B7_20260724.md`.
Po review baseline planu został formalnie przesunięty z `a12ef9c` na
`88aa1b775d51f4a1b3e512b1aaf05663e7af6db1`, ponieważ PR1A powstał na tej
późniejszej podstawie. Receipt zawiera komplet wykonanych bramek CI dla
czystego `88aa1b7`, w tym `trigger`, `ghost-launcher`, `workspace`,
`release build` i `git diff --check`. Nie raportuje czerwonych bramek jako
zielonych; zapisuje ich sygnatury baseline. Nie-CI część Change Set 0
ma po review status `DEFERRED HARD GATE`: przed 1B musi istnieć identyczny
harness/workload z throughput, p99, RSS i queue behavior, a przed 1C/1D
zamrożony corpus account/provider/raw-NLN. Szczegóły closure zapisuje
`ADR_8D_INGEST_STATE_QUOTE_PR1A_PROVENANCE_CLAIMS_CLOSURE_20260724.md`.

Po poprawkach z review `cargo check -p ghost-brain --tests` przechodzi,
więc trzy brakujące pola w konsumentach `AccountStateUpdate` nie są już
regresją kompilacji workspace. Ukierunkowane testy ról providerów i
kompatybilności configów są zielone. Dodatkowo:

- `cargo check -p ghost-launcher --test seer_connection_mode_test` — PASS;
- `cargo test -p ghost-launcher --test seer_connection_mode_test -- --nocapture` — 7/7 PASS;
- `cargo test -p seer --lib connect_geyser_fails_closed_on_invalid_provider_role_contract -- --nocapture` — PASS;
- `cargo test -p seer --lib entry_anchor_preserves_provider_metadata -- --nocapture` — PASS;
- `cargo test -p seer --lib old_geyser_entry_anchor_json_defaults_provider_metadata -- --nocapture` — PASS;
- `cargo test -p seer --lib raw_transaction_provider_metadata_reaches_parsed_trade_and_ipc -- --nocapture` — PASS;
- `cargo test -p seer --lib provider_metadata -- --nocapture` — 5/5 PASS;
- `cargo test -p ghost-core ingest_integrity -- --nocapture` — 8/8 PASS;
- `cargo check -p ghost-brain --tests` — PASS;
- regresja trzech testów `connect_geyser_live_transaction_*` spowodowana
  przedwczesnym testowym `shutdown=true` została usunięta bez osłabiania
  fail-closed runtime; testy zamykają teraz stream przez `request_shutdown()`
  po drenażu eventu.

Aktualny diff closure przeszedł również `timeout 900s cargo build --release
--workspace` (exit 0; `Finished release profile [optimized] target(s) in 9m
42s na finalnym diffie). Pełne czerwone bramki pakietowe/workspace zachowują wyłącznie swoje
zapisane sygnatury baseline i nie są opisywane jako zielone.

Wyniki pełnych bramek, w tym istniejące failure i timeouty, są podane
wyłącznie w receipt; nie są interpretowane jako zielone ani jako naprawione
przez PR1A.

Nowe pola `AccountStateUpdate` są dopięte za historycznym układem pól serde.
Przy metadanych `None` JSONL nadal je pomija. Zachowuje to także istniejącą
sygnaturę czerwonego round-trip bincode (`InvalidTagEncoding(104)`) w
fixture baseline zamiast maskować ją albo przesuwać na późniejszy obiekt.

Osobna kontrola kompatybilności konfiguracji przeszła:

- `cargo test -p seer config::tests::` — 14/14 PASS;
- `cargo test -p ghost-launcher config::tests::test_default_config` — PASS;
- `cargo test -p ghost-launcher config::tests::test_config_serialization` —
  PASS;
- `cargo test -p ghost-launcher
  config::tests::test_legacy_config_warnings_include_shadow_compat_and_legacy_shadow_run`
  — PASS.

Ponadto diff nie zmienia żadnego pliku `Cargo.toml` ani `Cargo.lock`.
Struktura konfiguracji runtime otrzymała dwa addytywne pola serde-default;
stare konfiguracje nie otrzymują nowego wymaganego pola.

## D7. Ryzyko i rollback

Ryzyko samej implementacji jest średnie: zmiana przechodzi przez kilka warstw
transportowych, choć nie zmienia reducerów ani policy. Główne ryzyko to
przypadkowa utrata metadanych, fałszywie udany start ingestu albo
niekompatybilna serializacja; pokrywają je ukierunkowane testy każdej
granicy. Receipt `88aa1b7` jest formalnie wybraną podstawą PR1A, a jego
nie-CI część ma status `DEFERRED HARD GATE`, nie waiver: harness 1A/1B jest
warunkiem przed pierwszą zmianą zachowania transportu, a frozen corpus — przed
1C/1D.

Rollback polega na wycofaniu addytywnych typów, pól i ich propagacji wraz z
kontraktem ID providerów. Nie ma migracji danych, uruchomionego arbitra ani
nowej runtime authority wymagającej operacyjnego wygaszenia.
