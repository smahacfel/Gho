# ADR-8D: Pump Research G.5.2 — raw segment-set snapshot authority

**Data:** 2026-08-17

**Status:** IMPLEMENTED LOCALLY / NO PROVIDER I/O / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_2_RAW_SEGMENT_SET_SNAPSHOT_AUTHORITY`

## D0. Problem

G.5.1 poprawnie utrzymuje jeden resolved Spectrum endpoint od walidacji atestu
do qualification reportu. Niezależny review wykazał drugi TOCTOU: frozen raw
indexer weryfikował whole-file digests, ale zapisywał jedynie ścieżki i frame
offsets. Full audit i exact materializer ponownie otwierały te ścieżki.

Możliwy był przebieg:

```text
scan/hash segment set A
-> full audit reads A
-> coherent path replacement
-> exact materializer reads B
-> exact publication still carries audit result for A
```

Nie ma dowodu, że taki drift wystąpił. Wszystkie segmenty GO-D pozostają
zgodne. Problem blokował wyłącznie przyszły combined certify.

## D1. Decyzja: prywatny pinned snapshot

Combined entry point tworzy jeden `PumpResearchRawSegmentSetAuthorityV1` przed
provider validation i I/O. Dla każdego segmentu:

1. `symlink_metadata()` odrzuca symlink i non-regular file;
2. source jest otwierany przez `O_NOFOLLOW | O_CLOEXEC`;
3. bytes są kopiowane do create-new private staging file;
4. podczas kopiowania liczone są SHA-256 i BLAKE3;
5. oba digests muszą odpowiadać immutable completion receipt;
6. snapshot otrzymuje `0400`;
7. snapshot zostaje otwarty read-only przez `O_NOFOLLOW | O_CLOEXEC`;
8. jego filename zostaje usunięty;
9. proces zachowuje wyłącznie pinned file descriptor.

Po odlinkowaniu katalog staging jest pusty i usuwany. Otwarte descriptors
pozostają ważne do końca combined operation. Rename albo symlink swap na
oryginalnej ścieżce nie zmienia bytes odczytywanych przez audit/materializer.

## D2. Ordered segment-set authority

Każdy ordered entry zawiera:

```text
segment_index
filename
canonical_source_path
bytes
file_sha256
file_blake3
```

Deterministyczny JSON entries wraz z `schema_version = 1` jest hashowany do
`aggregate_blake3`. Authority zachowuje oddzielnie ordered pinned descriptors.

`PumpResearchRawTapeIndexV1::read_record()` nie otwiera source path, jeżeli
istnieje authority. Wykonuje offsetowy odczyt z odpowiadającego pinned
descriptor. Tym samym:

```text
audit fingerprints
account anchors
transaction replay
exact trajectories
```

czytają jedną wersję raw bytes.

## D3. Wielokrotna rewalidacja i publikacja fail-closed

Authority ponownie hashuje zarówno aktualne source paths, jak i private pinned
descriptors:

1. bezpośrednio po seal;
2. po zbudowaniu raw audit fingerprints, przed konstrukcją/wywołaniem provider
   clienta;
3. po provider audicie, przed account anchors i exact writerem;
4. po pełnej materializacji `.partial`, bezpośrednio przed atomowym rename.

Sprawdzane są także canonical path, filename, segment index, receipt digests,
bytes oraz aggregate digest. Każdy drift jest błędem. Wczesny drift nie tworzy
nawet `.partial`. Późny drift zachowuje wyłącznie `.partial`; finalny exact
directory nie powstaje.

`PumpResearchExactOutputWriterV1::finish()` przyjmuje finalny fail-closed check
i wykonuje go po zapisaniu/synchronizacji partial outputu, lecz przed rename.
Ten check ponownie weryfikuje również stabilne authority inputs G.5.1:
attestation, audit config, suitability receipt, combined executable i raw
binding/start/completion JSON. Qualification report musi zachowywać dokładnie
ten sam aggregate raw segment-set digest, który następnie trafia do exact
manifestu.

## D4. Addytywne evidence schema

Qualification report zapisuje:

```text
raw_segment_set_blake3
```

Exact manifest zapisuje:

```text
source_raw_segment_set_blake3
```

Pole exact manifestu ma `serde(default)` i jest pomijane, gdy puste. Historyczne
exact JSON pozostają kompatybilne. Zmiana nie dotyka frozen raw binary V1,
`PumpResearchRawRecordV1`, segment header/footer, framing ani golden fixtures.

## D5. Regresje

Dodano produkcyjne regresje:

- `raw_segment_set_snapshot_control_is_hash_bound_and_manifested`;
- `raw_index_rejects_segment_symlink_before_frozen_scan`;
- `raw_segment_drift_before_provider_io_fails_with_zero_requests`;
- `raw_segment_drift_after_audit_fails_before_exact_writer`;
- `raw_segment_drift_during_materialization_blocks_final_publish`;
- `exact_manifest_raw_segment_digest_is_additive_for_historical_json`.

Drift fixtures budują kryptograficznie poprawny, kompletny segment B i
atomowo zastępują source path po utworzeniu authority. Nie polegają na
przypadkowym frame corruption. Pre-provider test używa loopback request countera
i wymaga zero połączeń. Materialization test przechodzi przez pełny produkcyjny
materializer, tworzy `.partial`, mutuje source w final-check hooku i wymaga
braku finalnego outputu.

## D6. Release i nowy atest

```text
target/release/pump-research-tape
SHA-256 = 780a415eadb484dddb51d23e6356e28c273d2f6ccbf2109e5dd3c0becf770203
BLAKE3  = d65d21e14b46075b0e12cd771e360651e97c4996ed5d4c1816bea2835401b40b
bytes   = 12 600 848
mode    = 0700
```

Atesty G.5 i G.5.1 pozostały bez zmian. Nowy create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_2_v1.json
SHA-256 = b06a4a7d91b9b716c46b12c92752c3e4902383a1bbed7821d422b324633c8074
BLAKE3  = 7b3551708b92211861df1371a011bf909887cfdace1f961df261d4304aaa1d44
bytes   = 4 330
mode    = 0600
```

Atest zachowuje Spectrum GO-E0.2 receipt/config/endpoint/GO-D bindings i
operator assertions, a zmienia wyłącznie combined certifier binding oraz opis
G.5.2. Fizyczna niezależność providerów nadal jest hash-pinned operator
assertion, nie automatycznym network-discovery proof.

## D7. Wpływ i rollback

Zmiana jest research-only. Nie dotyka capture ingress, Yellowstone requestu,
active Seer runtime, Event Busa, OracleRuntime, `MaterializedFeatureSet`,
Gatekeepera, execution ani strategii.

Rollback oznacza utrzymanie combined certify w stanie HOLD. Nie wolno wrócić do
path-only raw record reads ani opublikować exact bez finalnej segment-set
revalidation.

Po G.5.2:

```text
GO-D raw                         PASS / UNCHANGED
Spectrum GO-E0.2                 READY_FOR_FULL_AUDIT / UNCHANGED
G.5.1 endpoint authority         ACCEPTED
G.5.2 raw same-snapshot          PASS LOCALLY
new G.5.2 attestation            CREATED / HASH-PINNED
combined certify                 HOLD / NOT RUN
exact Ready                      NOT CREATED
export / strategy / execution    NO-GO
```

## D8. Weryfikacja lokalna

Bez provider I/O, combined `certify`, exact outputu ani zmiany GO-D przeszły:

```text
cargo fmt --all -- --check                                      PASS
cargo check -p seer --lib                                       PASS
cargo check -p seer --bin pump-research-tape                    PASS
cargo test -p seer research_tape_materializer --lib             36/36 PASS
cargo test -p seer research_tape --lib                          73 PASS, 1 ignored
cargo test -p seer rpc_http_client --lib                         6/6 PASS
cargo test -p seer grpc_connection::tests --lib                 95/95 PASS
cargo test -p seer --bin pump-research-tape                     7/7 PASS
cargo test -p ghost-core pump_research_tape --lib               11/11 PASS
cargo test -p seer --test pump_research_tape_cs0                 2/2 PASS
pr1d_v1_v2_parser_digests_remain_frozen                          1/1 PASS
python3 -m unittest scripts/test_pump_research_capture_supervisor.py
                                                                  10/10 PASS
```

Release capture-enabled harness:

```text
received / admitted / accepted = 8192 / 8192 / 8192
dropped / gaps                 = 0 / 0
persisted missing events       = 0
segments                       = 1
writer clean                   = true
writer error                   = null
capture abort                  = false
receive hand-off p99           = 231 ns
SLA                            = 100 000 ns
fatal -> source cancel         = 53 448 204 ns
```
