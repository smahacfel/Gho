# ADR-8D: Pump Research Evidence Tape V1.1 — CS0 zamrożenie kontraktów trwałości

**Data:** 2026-08-13

**Status:** IMPLEMENTED / RESEARCH-ONLY CONTRACTS / CREATEV2-MAYHEM BASIS ACCEPTED / PR-A NOT STARTED

**Task:** PUMP_RESEARCH_EVIDENCE_TAPE_V1_1_CS0_FROZEN_CONTRACTS

## D0. Decyzja

CS0 wprowadza wyłącznie zamrożone kontrakty danych dla przyszłego,
research-only `Pump Research Evidence Tape V1.1`:

```text
schema-lossless decoded protobuf source capture
→ immutable V1 raw frame/segment contracts
→ future offline materialisation contracts
```

Nie uruchamia Yellowstone capture, nie zapisuje datasetu runu, nie podłącza
writerów ani nie zmienia `connect_geyser()`, `AccountStateCore`,
`PumpObservationLedgerV1`, `AccountObservationArbiter`, canonical permitu,
Gatekeepera, MFS ani execution. Zachowane addytywne pola parsera dla
`CreateV2`/Mayhem są opisane niżej; nie zmieniają subskrypcji, runtime authority
ani policyjnej semantyki aktywnej ścieżki.

Punkt odniesienia przy rozpoczęciu CS0 był następujący:

```text
local HEAD   = 832728c9af9aec92bfa3edea8fa9518ee90f7d5b
origin/main  = 43057b296663129ca9b4f572e793474830a5452c
```

Worktree był już dirty. CS0 nie resetuje go i nie obejmuje szerokiego,
niepowiązanego eksperymentu OBS-Lite/A0. Po świadomej decyzji właściciela
2026-08-13 do zaakceptowanego lokalnego basis V1.1 należą wyłącznie
source-backed zmiany potrzebne dla future tape i badań strategii:

- rozdzielenie legacy `Create` i `CreateV2`, w tym wariantowy układ kont
  użytkownika;
- bezpośredni fact Mayhem dla `CreateV2` oraz zachowanie richer `CreateEvent`;
- source-backed virtual/real reserve evidence w `CreateEvent` i `TradeEvent`;
- `PumpCreationRegimeV1`, initial virtual quote reserves i canonical birth
  order jako addytywne evidence fields;
- minimalne konstruktorowe uzupełnienia wymagane przez te addytywne pola.

CS0 dołożył do tego wyłącznie wąską ochronę kompatybilności legacy Create oraz
testowy source-fixture receipt. Nie ustanawia przez to nowej runtime authority.

Jawnie **wyłączone** pozostają istniejące zmiany OBS-Lite/A0 w
`grpc_connection`, `ipc`, aktywnym `Seer`, `ghost-launcher`, `ghost-brain`,
konfiguracjach i skryptach. Nie są resetowane, nie są stage'owane przez CS0 i
nie są częścią decyzji o `Pump Research Evidence Tape V1.1`.

Nie powstaje tym samym commitowy checkpoint dla całego dirty worktree. Ten ADR
wyznacza jedynie wąską, audytowalną granicę akceptowanego parser/evidence basis.

## D1. Zamrożony kontrakt źródła

Jedyną deklarowaną gwarancją source capture V1 jest:

```text
decoded_protobuf_schema_lossless_v1
```

Jest ona ograniczona do zamrożonych wygenerowanych typów protobuf,
zdekodowanego payloadu `update_oneof` i deterministycznego `prost`
re-encoding. CS0 jawnie **nie** deklaruje wire-byte losslessness,
unknown-field losslessness ani identity oryginalnej ramki HTTP/2/gRPC.

Zamrożone zależności zgodne z `Cargo.lock`:

```text
yellowstone-grpc-proto   = 1.14.2
yellowstone-grpc-client  = 1.15.4
prost                    = 0.12.6
bincode                  = 1.3.3
```

Corpus zawiera wygenerowany raz, bez generowania w runtime,
`FileDescriptorSet`:

```text
ghost-core/tests/fixtures/pump_research_tape_v1/yellowstone_v1_descriptor.pb
SHA-256 = 9b92e4810f4af0d100f268b31d52d0cedf55dfee8c6b512f43b7698205450acb
```

Deskryptor obejmuje `geyser.proto`, `solana-storage.proto` i importy. Został
wygenerowany z kodu źródłowego crate `yellowstone-grpc-proto-1.14.2` przez
`protoc 3.21.12` z `--include_imports`; jego committed bytes i SHA-256 są
normatywnym artefaktem V1, nie wynikiem runtime.

## D2. Zamrożony V1 storage

`ghost-core::pump_research_tape` definiuje wyłącznie storage-owned typy:

```text
PumpResearchRawRecordV1
PumpRawSegmentHeaderV1
PumpRawSegmentClosedV1
wszystkie nested V1 storage structs i enumy
```

Fizyczny frame V1 jest dokładnie:

```text
u32 little-endian payload_length
+ bincode-1.3.3 fixed integer / little endian / reject trailing bytes payload
+ 32-byte BLAKE3(payload)
```

Limit pojedynczego payloadu wynosi `16 MiB`. Przekroczenie zwraca typed
`RecordTooLarge`; PR-A ma zamienić odrzucenie admission na trwały typed local
coverage gap i nie może zwiększyć limitu lub wprowadzić alternatywnego
storage bez V2.

Pubkey, signature i hash nie są serializowane przez domenowe typy Solany:

```text
pubkey    = fixed [u8; 32]
signature = fixed [u8; 64]
hash      = fixed [u8; 32]
```

Segment zaczyna się od magic `PRTAPE01`, po którym występuje ten sam
hashowany V1 frame headera. Nie ma kompresji, Parquet, Arrow, bazy ani
drugiego WAL.

Zmiana pola, kolejności, wariantu enum, typu, reprezentacji fixed bytes,
semantyki lub bincode options wymaga `V2`; nie jest dopuszczalna jako
addytywna edycja V1.

## D3. Zamrożone evidence i exactness boundaries

CS0 zamraża:

- `PrimaryTransaction`, `PrimaryAccountUpdate`, `PrimarySlotUpdate`,
  `PrimaryBlockMeta`, `CoverageGap`, `SegmentClosed` jako jedyne raw variants;
- brak `EntryAnchor` w raw V1;
- Raw SlotUpdate jako `slot`, optional `parent` i raw numeric source status,
  bez pre-classification canonicality;
- `PumpSlotCanonicalityV1::{RootedCanonical, Dead, Unresolved}` wyłącznie jako
  wynik przyszłego offline materializera;
- start/completion Program/ProgramData receipt, z BLAKE3 raw ProgramData,
  deployment/context slot i commitment;
- program-version boundary jako fail-closed qualification state;
- only-Pump-Global mutable dependency dla fallbacku `Create/CreateV2`;
- brak fee schedule dependency dla reserve-only Buy/Sell transitions;
- exact participant **trade token account** balance, nigdy wallet-total
  inventory;
- typed `NON_CANONICAL_FORK`, `UNRESOLVED_CANONICALITY`,
  `SOURCE_COVERAGE_UNPROVEN`, `SOURCE_FILTER_CPI_COVERAGE_UNPROVEN`,
  `PROGRAM_VERSION_BOUNDARY`, `TRANSITION_DEPENDENCY_UNCAPTURED` i
  `MISSING_REQUIRED_EVIDENCE` boundaries.

`PumpCertifiedMutationV1`, birth/trajectory contracts i exporter requirement
pozostają typami danych. CS0 nie certyfikuje trajectory i nie promuje żadnej
nowej authority runtime.

## D4. Golden corpus

Do repo trafiają dwa binary fixtures dla storage V1:

```text
raw_record_v1.bin
SHA-256 = 5cd3df57769ba4d7024a10829c726a2a6e4fb4269eeec4a79e7d5d871d6c8334
BLAKE3  = 8b43537b4516605fff434d919d80acd854a85e4b76928777fd08c3bee04b5846

raw_segment_v1.bin
SHA-256 = c92cd25c9018680769067d57f1c5573439e7d0f139c954736c492ab994a31639
BLAKE3  = ff4007b65ff1b2c1dd5bc99cf9ccc9692b93835b42feb79ea767e4ca7dcdcf6f
```

Segment obejmuje header oraz wszystkie sześć frozen variants raw enum. Testy
wymagają: current decoder odczytuje old fixture, decode → canonical encode
daje identyczne bytes, SHA-256 i BLAKE3 pozostają dokładnie zgodne, a bad
digest/trailing bytes/oversize payload fail closed.

`corpus_manifest_v1.json` zamraża dodatkowo komplet 29 identyfikatorów
przypadków CS0 (w tym CreateV2, CPI/router/v0, fork canonicality, ProgramData,
Global dependency i participant balance). Jest to inventory deterministycznego
corpus, nie syntetyczny wynik certifiera: implementacja rzeczywistych fixture
transakcji i wyników należy do PR-A/PR-B. Test wymaga obecności każdego case
dokładnie raz, więc następne PR-y nie mogą cicho zawęzić corpus.

Osobny test Seera dowodzi `prost` round-trip dla frozen child payloads:
`SubscribeUpdateTransaction`, `SubscribeUpdateAccount`,
`SubscribeUpdateSlot` i `SubscribeUpdateBlockMeta`, wraz z ich
`SubscribeUpdate.update_oneof` wrapperem.

## D5. Granice i następny krok

```text
CS0 delta: active Seer authority/policy = UNCHANGED
Yellowstone subscription        = UNCHANGED
Geyser receive hot path         = UNCHANGED
parser birth evidence           = ADDITIVE / CreateV2-Mayhem accepted
pre-existing OBS-Lite wiring    = OUT OF SCOPE / UNCHANGED BY CS0
parser semantic parity V1       = FROZEN / PASS
parser source receipt V2        = CORRECTED / FROZEN / PASS
Gatekeeper / MFS / execution    = UNCHANGED
capture run                     = NOT STARTED
qualification                   = NOT STARTED
```

CS0 jest warunkiem wejścia do PR-A. Reconciliation parsera jest zakończona:
nie było dowodu funkcjonalnej regresji `CreateV2`/Mayhem; problem dotyczył
wyłącznie syntetycznego fixture source identity.

Historyczny fixture PR1D budował swoje bytes przez symbol `DISC_CREATE`. W
starym checkoutcie symbol ten wskazywał `d6 90 4c ec 5f 8b 31 b4`; po korekcie
semantyki lokalnego parsera ten discriminator należy do `CreateV2`, a canonical
legacy `Create` to `18 1e c8 28 05 1c 07 77`. Zmiana aliasu zmieniła protobuf
payload i w konsekwencji `payload_hash_blake3` w bogatym V2 snapshotcie, nawet
gdy V1 parser output pozostawał taki sam.

Dlatego CS0 nie maskuje różnicy ani nie fałszuje starego payload hash:

```text
V1 semantic parity digest (bez raw source receipt) =
549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202

superseded V2 receipt for bytes built through incorrect alias =
507b13704d5b90c3f724a395acbf0d0cc55fdc37a83fcb95cf67cceb6247569f

corrected, frozen V2 receipt for explicit legacy Create bytes =
02136d691e399dace85b112cc5b6d50c79323a2f24adcb3e7569ac68b40654a6
```

Fixture posiada teraz własny literal canonical legacy discriminator i testuje
zgodność z `DISC_CREATE`; przyszła zmiana nazwy/aliasu nie może cicho zmienić
raw bytes receipt. Osobny test gwarantuje, że malformed `CreateV2` nie wykonuje
fallbacku do legacy layoutu. To jest jawna korekta **source fixture**, nie
rebaseline wyniku ekonomicznego ani rozluźnienie exactness.

Istniejący ignored PR1B hot-path harness nadal nie przechodzi własnej bramki
bounded-ingress w tym dirty worktree. Jest to odrębny, wcześniej istniejący
problem harnessu/configuration (capacity/SLA); CS0 nie zmienia jego capacity,
SLA, IPC ani aktywnego ingestu. Należy go rozwiązać lub jawnie zakwalifikować
przed ogłoszeniem hot-path qualification PR-A, ale nie jest już blokadą decyzji
o zachowaniu `CreateV2`/Mayhem.

## D6. Weryfikacja

```text
cargo test -p ghost-core pump_research_tape
cargo test -p seer --test pump_research_tape_cs0
cargo test -p seer pr1d_v1_v2_parser_digests_remain_frozen
cargo test -p seer pr1b_hot_path_harness -- --ignored --nocapture
rustfmt --check ghost-core/src/pump_research_tape.rs \
  off-chain/components/seer/tests/pump_research_tape_cs0.rs
```

Wykonane wyniki:

```text
ghost-core PumpResearchTape CS0 tests              = PASS (10)
Seer decoded-protobuf schema-lossless tests        = PASS (2)
frozen parser semantic V1 digest                   = PASS
frozen corrected source-receipt V2 digest          = PASS
legacy fixture owns canonical Create discriminator = PASS
malformed CreateV2 fails closed                     = PASS
matching legacy CPI Create keeps CPI precedence      = PASS
ignored PR1B performance harness                   = FAIL before p99/SLA measurement
  current default ingress capacity = 2,048
  frozen workload                 = 3,072 events
  failed assertion                = configured ingress capacity must absorb the frozen operational workload
```

Powtórzona wcześniej próba tego samego harnessu w tym samym worktree przeszła
dalej, ale zakończyła się `queue dwell p99 = 2,593,063,488 ns` wobec frozen SLA
`250,000,000 ns`; nie jest to stabilny pomiar pozytywny, lecz dodatkowy powód,
aby nie zmieniać cicho capacity ani SLA. Ostrzeżenia istniejące w innych
modułach oraz ten problem harnessu nie są częścią CS0.
