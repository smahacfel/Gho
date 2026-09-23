# ADR-8D: Pump Research G.5.2.1 — bounded raw open i pre-side-effect output gate

**Data:** 2026-08-17

**Status:** IMPLEMENTED LOCALLY / NO PROVIDER I/O / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_2_1_BOUNDED_RAW_OPEN_AND_PRE_SIDE_EFFECT_OUTPUT_GATE`

## D0. Problem

G.5.2 poprawnie przypiął full audit i exact materializer do jednego prywatnego
zestawu raw bytes. Review wykazał jednak trzy luki przedoperacyjne:

1. `symlink_metadata()` i późniejszy blokujący `open()` pozostawiały race
   regular-file → FIFO;
2. hash/copy czytały do EOF zamiast do przypiętego rozmiaru;
3. snapshot staging mógł powstać wewnątrz raw przed walidacją requested output,
   a cleanup guard nie był rozbrajany po jawnym `remove_dir()`.

Nie ma dowodu, że którykolwiek przypadek wystąpił na GO-D. Finding blokował
wyłącznie przyszły combined certify.

## D1. Decyzja: nonblocking descriptor authority

Wszystkie raw segment opens używane przez index/snapshot/revalidation wykonują:

```text
O_RDONLY | O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK
```

`symlink_metadata()` pozostaje wczesną diagnostyką. Ostatecznym authority typu
pliku jest `fstat` już otwartego descriptor. FIFO, directory, device, socket i
inne special files są odrzucane po nonblocking open; certifier nie oczekuje na
peer.

Regresja umieszcza hook dokładnie pomiędzy precheck i open, zastępuje regularny
plik FIFO i uruchamia opóźnionego writera. Poprawna implementacja zwraca błąd
przed writerem. Usunięcie `O_NONBLOCK` powoduje mierzalne oczekiwanie i czerwony
test, ale nie zawiesza całego corpusu.

## D2. Exact-size frozen scan, hash i copy

Pierwszy frozen scan przypina rozmiar z `fstat` otwartego regularnego pliku.
Reader ma limit `expected_bytes + 1`, dzięki czemu trailing/growth jest
obserwowalny bez nieograniczonego EOF loop. Po scan wymagane są:

```text
decoded offset == expected_bytes
post-read fstat length == expected_bytes
whole-file SHA-256 == completion receipt
whole-file BLAKE3 == completion receipt
```

`PumpResearchIndexedSegmentV1` zachowuje `file_bytes`. Snapshot copy i każde
późniejsze rehash czytają dokładnie tę liczbę bajtów przez pozycjonowany
`read_exact_at`. Short read albo post-read growth/shrink jest błędem.

Osobne deterministyczne regresje dopisują bytes po exact bound:

- po bounded hash;
- po bounded snapshot copy.

Obie ścieżki kończą się lokalnym błędem w krótkim czasie.

## D3. Output/raw validation przed pierwszym side effect

Publiczny combined entry point wykonuje teraz:

```text
canonicalize raw run
-> canonicalize create-new output parent/name
-> require output and raw are disjoint
-> index raw read-only
-> read hash-pinned attestation without endpoint secret resolution
-> bind source_run_id, approved decision, planned output and current executable
-> seal private raw snapshot
-> full authority validation
-> provider audit
```

Requested output znajdujący się wewnątrz raw jest odrzucany przed indeksem,
`create_dir`, stagingiem i provider clientem. Test publicznej funkcji zachowuje
przed/po mapę wszystkich raw entries i digestów, wymaga zero snapshot paths,
zero provider connections oraz braku exact/partial.

Pre-snapshot attestation validation nie rozwiązuje endpoint path credentialu.
Pełna G.5.1 authority validation nadal następuje po seal i przed provider I/O.

## D4. Atomic staging ownership i disarm

Staging jest tworzony create-new przez `DirBuilderExt::mode(0700)`. Guard z
`Option<PathBuf>` powstaje natychmiast po udanym create, przed każdym fallible
chmod/hook. Każdy błąd po create uruchamia best-effort `remove_dir_all()`.

Przy prawidłowym zamknięciu:

1. path jest wyjmowany z guarda;
2. wykonywany jest `remove_dir()` pustego stagingu;
3. przy błędzie path wraca do guarda;
4. przy sukcesie Drop widzi `None`.

Regresja odtwarza katalog pod tą samą nazwą pomiędzy remove i Drop. Foreign
marker pozostaje, co dowodzi braku double-cleanup cudzej nowej ścieżki.

## D5. Zakres wpływu

Zmiana jest ograniczona do research-only PR-B materializera i testów. Nie
zmienia:

- frozen `PumpResearchRawRecordV1`;
- raw header/footer/framing/version;
- capture ingress, source request ani writer;
- aktywnego Seera, Event Busa lub OracleRuntime;
- `MaterializedFeatureSet`, Gatekeepera, execution lub strategii;
- Spectrum GO-E0.2 receipt;
- historycznych raw/exact artefaktów.

## D6. Regresje

Dodano:

- `raw_open_regular_to_fifo_race_is_nonblocking_and_rejected`;
- `bounded_hash_rejects_growth_after_expected_bytes`;
- `bounded_snapshot_copy_rejects_growth_after_expected_bytes`;
- `public_combined_output_inside_raw_fails_before_any_side_effect_or_provider_io`;
- `raw_snapshot_staging_raii_cleans_errors_and_disarms_after_close`.

Dotychczasowe G.5/G.5.1/G.5.2 corruption, endpoint, failed-status, frozen V1 i
capture regressions pozostają wymagane.

## D7. Release i attestation

Finalny certifier:

```text
target/release/pump-research-tape
SHA-256 = dc4263207adc2ea5ec897f1c564965e7c3b02551307e1b1ed42949c1ef1c8ebb
BLAKE3  = df2f59a2073c3e44f1817de55bb844cd07d813181d2faaa1d6f3011830ddd1ec
bytes   = 12624232
mode    = 0700
```

G.5, G.5.1 i G.5.2 attestations pozostają historyczne i nie są nadpisywane.
Nowy create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_2_1_v1.json
SHA-256 = 4617ec14cc20f504d8156e152ea7038054f0039b6a13db3ca7b6e84f661dcb02
BLAKE3  = d6be6b9f1033af76bb5727da01b23481e8aeaa4265ba4787a7ca91c0237d7704
bytes   = 4403
mode    = 0600
```

Wiąże niezmieniony Spectrum GO-E0.2 receipt z finalnym G.5.2.1 combined
certifierem. Fizyczna niezależność providerów pozostaje hash-pinned operator
assertion, nie automatycznym network-discovery proof.

## D8. Weryfikacja i decyzja

Lokalnie przeszły:

```text
cargo fmt --all -- --check                                      PASS
cargo check -p seer --lib                                       PASS
cargo check -p seer --bin pump-research-tape                    PASS
cargo test -p seer research_tape_materializer --lib             41 passed
cargo test -p seer research_tape --lib --no-fail-fast           78 passed, 1 ignored
cargo test -p seer rpc_http_client --lib --no-fail-fast          6 passed
cargo test -p seer grpc_connection::tests --lib                 95 passed
cargo test -p seer --bin pump-research-tape --no-fail-fast       7 passed
cargo test -p ghost-core pump_research_tape --lib               11 passed
cargo test -p seer --test pump_research_tape_cs0                 2 passed
parser digest freeze                                              1 passed
python3 -m unittest scripts/test_pump_research_capture_supervisor.py
                                                                 10 passed
cargo build --locked --release -p seer --bin pump-research-tape PASS
git diff --check                                                 PASS
git diff --cached --check                                        PASS
```

Finalny ignored release capture-enabled harness:

```text
received / admitted / accepted = 8192 / 8192 / 8192
dropped / gaps                 = 0 / 0
persisted missing events       = 0
segments                       = 1
writer clean                   = true
writer error                   = null
capture abort                  = false
receive hand-off p99           = 180 ns
SLA                            = 100000 ns
fatal -> source cancel         = 53253932 ns
```

Po pełnym local-only self-review obowiązuje:

```text
GO-D raw                         PASS / UNCHANGED
Spectrum GO-E0.2                 READY_FOR_FULL_AUDIT / UNCHANGED
G.5.2 same-snapshot              PASS
G.5.2.1 bounded raw open         PASS LOCALLY
G.5.2.1 exact-size hash/copy     PASS LOCALLY
G.5.2.1 output/raw gate          PASS LOCALLY
G.5.2.1 staging ownership        PASS LOCALLY
combined certify                 HOLD / NOT RUN
exact Ready                      NOT CREATED
export / strategy / execution    NO-GO
```
