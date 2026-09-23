# ADR-8D: Pump Research Evidence Tape V1.1 — PR-A standalone immutable raw capture

**Data:** 2026-08-13

**Status:** IMPLEMENTED / RESEARCH-ONLY / PROSPECTIVE CAPTURE NOT STARTED / PR-B PENDING

**Task:** `PUMP_RESEARCH_EVIDENCE_TAPE_V1_1_PR_A_STANDALONE_RAW_CAPTURE`

## D0. Decyzja

PR-A realizuje wyłącznie pierwszą wykonywalną część `Pump Research Evidence Tape V1.1`:

```text
primary decoded Yellowstone updates
→ source tap przed GeyserEvent/parser/filtering
→ bounded nonblocking ingress
→ dedicated writer thread
→ immutable raw V1 segments + run receipts
```

Nowa ścieżka jest uruchamiana wyłącznie jawnie przez oddzielny binary:

```bash
pump-research-tape capture \
  --config configs/rollout/pump-research-tape-v1.toml
```

Nie dodano `research_tape` do `SeerConfig`, nie zmieniono zachowania
`connect_geyser()` ani nie utworzono runtime eventów dla capture. Aktywny Seer,
Gatekeeper, `MaterializedFeatureSet`, `AccountStateCore`, canonical permit,
`PumpObservationLedgerV1`, `AccountObservationArbiter`, Event Bus i execution
pozostają poza tym PR-em.

PR-A nie certyfikuje state, nie materializuje trajectory, nie uruchamia
qualification audit, nie eksportuje windows i nie implementuje strategii.
Komendy `certify` oraz `export-window` kończą się obecnie jawnym błędem
„PR-B unavailable”; ich pozorne przyjęcie przez CLI byłoby fałszywą deklaracją
gotowości V1.1.

Punkt bazowy pozostaje zgodny z CS0:

```text
local HEAD   = 832728c9af9aec92bfa3edea8fa9518ee90f7d5b
origin/main  = 43057b296663129ca9b4f572e793474830a5452c
```

Worktree był i pozostaje dirty. PR-A nie wykonuje resetu, checkoutu ani
automatycznego czyszczenia; istniejące OBS-Lite/A0 oraz pozostałe niepowiązane
zmiany nie są elementem tej decyzji.

## D1. Granica source capture

`PumpResearchSourceConnectionV1` buduje osobny, research-only profil
`GrpcSubscriptionProfile::PumpResearchGlobalV1`. Jest to jeden primary provider,
jeden stream i commitment `processed`, z następującą zamrożoną powierzchnią:

```text
transactions:
  account_include = [Pump.fun program]
  vote            = false
  failed          = None       # successful i failed są zachowane

accounts:
  Pump-owned BondingCurve discriminator
  exact canonical Pump Global

other:
  BlockMeta enabled
  SlotUpdate enabled, filter_by_commitment = false
  Entry disabled
  PumpSwap excluded
  registry/candidate scoping disabled
  manual RPC backfill disabled
```

Tap znajduje się w `stream_loop` po otrzymaniu zdekodowanej wiadomości
`SubscribeUpdate`, lecz przed `route_update()`, projekcją do `GeyserEvent`,
parserem i candidate filteringiem. Pętla receive wykonuje tylko nieblokujące
przekazanie własności payloadu; nie wykonuje `prost`, bincode, hashów ani I/O
dyskowego.

Zachowana semantyka pozostaje dokładnie ograniczona do:

```text
decoded_protobuf_schema_lossless_v1
```

Writer przechowuje deterministyczne `prost` encoding odpowiedniego payloadu
`SubscribeUpdateTransaction`, `SubscribeUpdateAccount`, `SubscribeUpdateSlot`
lub `SubscribeUpdateBlockMeta`, wraz z BLAKE3 tych bytes. Nie deklaruje
wire-byte losslessness, unknown-field losslessness ani identyczności ramki
gRPC/HTTP2.

## D2. Standalone config i ProgramData receipts

Powstał `PumpResearchCaptureConfigV1`, ładowany tylko przez binary capture.
Zawiera wyłącznie:

- identity/endpoint/autoryzację primary Yellowstone;
- read-only RPC endpoint dla Program/ProgramData receipts;
- zamrożony Pump program ID;
- katalog wyjściowy;
- `required_for_run`;
- queue, flush i segment limits;
- zamrożony `record_max_bytes = 16 MiB`.

Sekret gRPC jest pobierany z nazwanego environment variable i nie jest
zapisywany w manifeście, receiptach ani segmentach. Przykładowa konfiguracja
nie zawiera działającego endpointu ani credentialu.

Przed utworzeniem katalogu runu i przed otwarciem streamu capture pobiera
finalized Program account oraz związany ProgramData account. Weryfikuje
upgradeable loader ownership, dekoduje `UpgradeableLoaderState`, zapisuje
Program/ProgramData identity, deployment slot (gdy jest obecny), context slot,
commitment oraz BLAKE3 pełnych raw bytes ProgramData.

Po zatrzymaniu źródła, drainie i joinie writera pobierany jest drugi finalized
receipt. Niezgodność Program ID, ProgramData identity/owner, raw ProgramData
hasha lub dostępnych deployment slotów oznacza `ProgramVersionBoundary`.
Brak start receipt blokuje start przed pierwszym admitted recordem; brak
completion receipt pozostawia run `Incomplete`. Żadna z tych sytuacji nie
kasuje już zapisanego raw evidence.

## D3. Bounded ingress, gaps i immutable segmenty

Nowy ingress ma dwie bounded lanes:

```text
data lane    = queue_capacity z capture configu
control lane = 64, tylko lifecycle / typed coverage gaps
```

`capture_sequence` zwiększa się dla każdej próby admission. Pełna data lane
nie blokuje receive tasku: tworzy jeden ciągły `LocalCoverageGapV1` o reason
`EvidenceQueueSaturated`, zapisywany jako frozen persistence adapter
`PumpRawCoverageGapV1`. Zarezerwowana control lane przenosi terminalny gap
przed opublikowaniem `source_finished`; writer nie może zakończyć pracy między
drainem danych a zapisaniem tej końcowej informacji o luce.

Każdy akceptowany payload jest serializowany tylko na dedykowanym wątku
`pump-research-tape-writer-v1`. Writer używa zamrożonego codec V1 z CS0:

```text
u32 LE payload length
+ bincode 1.3.3 fixed-int / little-endian / reject trailing bytes
+ BLAKE3-256(payload)
```

Segment jest najpierw tworzony jako `segment_XXXXX.bin.partial`; dopiero po
framed footerze, flush/sync i directory sync jest atomowo publikowany jako
`.bin`. Crash przed footerem pozostawia `.partial` i nie może zostać uznany za
zamknięty segment. Footerowy `segment_blake3` obejmuje header i rekordy przed
footerem — celowo nie obejmuje własnego footera, aby hash nie był
self-referential. Completion receipt zawiera także SHA-256 i BLAKE3 całego
opublikowanego pliku segmentu.

Rekord większy niż frozen `16 MiB` nie rozszerza formatu ani nie otwiera
alternatywnego storage. Jest konwertowany na trwały
`RecordExceedsFrozenLimit` coverage gap i run pozostaje konserwatywnie
niekwalifikowalny, gdy `required_for_run = true`.

Powstają dokładnie następujące artefakty PR-A:

```text
<output_dir>/<run_id>/raw/
  run_start_manifest.json
  segment_00000.bin ...
  run_completion_receipt.json
```

## D4. Granice zachowane przez PR-A

```text
active Seer runtime / connect_geyser()        UNCHANGED
Gatekeeper / MFS / execution                  UNCHANGED
Event Bus / OracleRuntime                     UNCHANGED
live canonical authority                      UNCHANGED
Pump parser output with capture disabled      FROZEN
CreateV2 / Mayhem accepted parser basis       PRESERVED
RPC transaction backfill into raw tape        FORBIDDEN
database / Kafka / Parquet / Arrow            NOT ADDED
unbounded queue / per-event task spawn        NOT ADDED
```

Istniejące `CreateV2`/Mayhem evidence changes są zachowane jako zaakceptowana
baza CS0. PR-A celowo nie dodaje research inventory ani transition math do
aktywnego parsera; są to obowiązki PR-B.

## D5. Weryfikacja wykonana

Po implementacji wykonano:

```text
cargo fmt --all -- --check
cargo check -p seer --lib
cargo check -p seer --bin pump-research-tape
cargo test -p seer research_tape --lib --no-fail-fast
cargo test -p seer pump_research_profile_is_source_global_without_touching_primary_profile --lib -- --nocapture
cargo test -p seer --test pump_research_tape_cs0 -- --nocapture
cargo test -p seer --bin pump-research-tape -- --nocapture
cargo test -p seer pr1d_v1_v2_parser_digests_remain_frozen -- --nocapture
cargo test --release -p seer --lib pr1b_hot_path_harness -- --ignored --nocapture --test-threads=1
```

Wyniki:

```text
Rust formatting                                      PASS
Seer library check                                   PASS
standalone capture binary check                      PASS
raw capture unit tests                               PASS (9)
research subscription profile isolation              PASS
CS0 schema-lossless / frozen source tests            PASS (2)
capture CLI argument contract                        PASS
capture-disabled parser parity digest                PASS
existing PR1B release hot-path harness               PASS
```

Unit tests pokrywają deterministic source payload encoding,
Transaction/Account/Slot/BlockMeta, canonical Pump Global account role, jedną
ciągłą saturation gap episode, terminal gap hand-off, clean writer drain,
crash `.partial`, frozen record limit i ProgramData receipt mismatch.

Repo emituje istniejące ostrzeżenia w `ghost-core`, legacy `ShadowLedger` i
niepowiązanych zmienionych modułach Seera. Nie są one wprowadzane przez
standalone capture i nie są wyciszane w tym PR-ze.

Powtórzony release harness PR1B przeszedł na obecnym local worktree z
`missing_events = 0`, `spilled_events = 0`, `blocking_wait_ns = 0`,
`queue_dwell_p99_ns = 44,983,102` wobec SLA `250,000,000`,
`oldest_event_age_ns = 51,053,905` wobec SLA `500,000,000` i high-water `126`
przy capacity `2,048`. Ten wynik jest lokalnym receipt hot-path istniejącej
ścieżki; nie jest jeszcze pomiarem prospective standalone capture u realnego
providera.

## D6. Stan kwalifikacji i dalszy krok

Nie wykonano prospective capture ani qualification runu: konfiguracja ma
celowo placeholder endpointy i wymaga realnego primary Yellowstone,
finalized RPC oraz osobnej decyzji o użyciu credentialu. Nie wolno na podstawie
testów lokalnych oznaczyć tape jako `PUMP_RESEARCH_TAPE_V1_READY`.

PR-A jest gotowy jako implementation boundary, a kolejny bezpieczny etap jest
operacyjny, nie architektoniczny:

1. uzupełnić standalone config właściwymi endpointami i credentialem;
2. uruchomić observe-only prospective raw capture;
3. zachować immutable raw run wraz z completion receipt;
4. dopiero potem rozpocząć PR-B materializer/certifier na tym materiale.

PR-B nadal musi dostarczyć complete mutation inventory, slot
RootedCanonical/Dead/Unresolved materialization, offline replay ledgera i
arbiterów, minimalny Global decoder, exact trajectory proof, participant
trade-token-account evidence, exporter oraz read-only independent audit.

## D7. Rollback

Rollback ma zerowy wpływ na aktywny runtime: nie wybiera się nowego profilu
przez produkcyjny config, a binary capture nie jest uruchamiany przez
`ghost-launcher`. W przypadku wykrycia błędu PR-A należy zaprzestać uruchamiania
`pump-research-tape capture`, zachować już opublikowane raw artefakty i
naprawić kod w nowej zmianie. Nie wolno nadpisywać manifestów/segmentów,
przepisywać frozen V1 bytes ani wstawiać danych z RPC do tape jako zastępstwa
utraconego source evidence.
