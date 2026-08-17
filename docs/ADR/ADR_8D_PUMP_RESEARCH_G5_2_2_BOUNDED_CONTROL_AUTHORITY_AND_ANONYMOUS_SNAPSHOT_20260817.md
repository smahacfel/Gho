# ADR-8D: Pump Research G.5.2.2 — bounded control authority i anonymous snapshot

**Data:** 2026-08-17

**Status:** IMPLEMENTED AND VERIFIED LOCALLY / FINAL RELEASE AND ATTESTATION PINNED / NO PROVIDER I/O / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_2_2_BOUNDED_CONTROL_AUTHORITY_AND_ANONYMOUS_SNAPSHOT`

## D0. Problem

G.5.2.1 domknął segment opens i exact-size segment reads. Publiczny combined
flow nadal mógł jednak blokować na FIFO podstawionym jako start manifest,
completion receipt lub binding, ponieważ te pliki były czytane przez
`fs::read()`. Późniejsze digests configu, suitability receiptu, atestu i raw
controls używały nieograniczonego `operator_digest_file()`.

Nazwany snapshot staging był ponadto właścicielem wyłącznie pathname. Na
ścieżce błędu rename własnego katalogu i podstawienie obcego katalogu pod tę
samą nazwę mogło skierować `Drop::remove_dir_all()` na obcy replacement.

## D1. Decyzja: jeden bounded authority reader

Wszystkie control/authority inputs combined i GO-E0 przechodzą przez jeden
helper:

```text
O_NOFOLLOW | O_CLOEXEC | O_NONBLOCK
-> post-open fstat regular file
-> explicit per-kind max_bytes
-> exact read of opened length
-> post-read fstat type/length equality
-> digest and parse from the same bytes
```

Limity są jawne i zależne od rodzaju pliku. Large/growing/special inputs
failują lokalnie. Produkcyjny combined/GO-E0 nie używa już `fs::read()` ani
`operator_digest_file()` dla tych authority inputs.

## D2. Indexed raw-control snapshot authority

Raw index zachowuje SHA-256/BLAKE3/bytes start manifestu, completion receiptu i
bindingu odczytanych podczas parse. Combined i GO-E0 ponownie czytają ścieżki
bounded helperem i wymagają equality z indexed digests.

To zamyka sekwencję:

```text
parse control B
-> restore control A
-> hash A
-> continue using parsed B
```

Semantycznie równoważny JSON z innymi bytes również jest drift, ponieważ
authority jest byte-exact.

## D3. Decyzja: brak named staging

Named staging i jego Drop guard zostały usunięte. Linux `O_TMPFILE | O_EXCL`
tworzy anonimowy inode bez directory entry. Po copy/hash/sync:

1. mode jest ustawiany descriptor-relative na `0400`;
2. ten sam dev/ino jest otwierany z `/proc/self/fd/<fd>` jako `O_RDONLY`;
3. wymagane są `st_nlink == 0`, zgodne dev/ino/len;
4. writable FD jest zamykany;
5. audit/materializer zachowuje wyłącznie `Arc<File>` read-only FD.

Drop deskryptora jest jedynym cleanupem. Nie istnieje pathname do podmiany,
rekurencyjnego usunięcia ani pozostawienia po SIGKILL.

## D4. Granice fail-closed

- brak Linux `O_TMPFILE` lub `/proc/self/fd` blokuje combined przed provider I/O;
- FIFO/symlink/device/directory authority input daje lokalny błąd;
- oversize/short read/growth/shrink daje lokalny błąd;
- parse/digest drift raw controls blokuje przed provider I/O;
- late authority drift blokuje przed exact writerem/publikacją;
- żadna z tych ścieżek nie tworzy finalnego exact outputu.

## D5. Zakres wpływu

Zmiana jest research-only PR-B. Nie zmienia:

- frozen `PumpResearchRawRecordV1` ani binary V1;
- capture ingress, source request, segment writer lub PR-A;
- aktywnego Seera, Event Busa i OracleRuntime;
- `MaterializedFeatureSet`, Gatekeepera, execution ani strategii;
- GO-D raw bytes;
- Spectrum GO-E0.2 receipt;
- G.5/G.5.1/G.5.2/G.5.2.1 historycznych attestations.

## D6. Regresje

Dodano lub rozszerzono:

- `bounded_authority_reader_rejects_regular_to_fifo_race_without_blocking`;
- `bounded_authority_reader_rejects_growth_and_per_kind_size_limit`;
- `indexed_raw_control_authority_rejects_parse_to_digest_snapshot_drift`;
- `public_combined_control_and_authority_fifos_fail_without_provider_io`;
- `late_combined_authority_revalidation_rejects_fifos_without_blocking`;
- `anonymous_raw_snapshot_has_no_pathname_cleanup_surface_on_error`.

Publiczny FIFO corpus obejmuje start, completion, binding, audit config,
suitability receipt i attestation. Late corpus obejmuje wszystkie te same
authority inputs po walidacji.

## D7. Release i attestation

Finalny certifier:

```text
target/release/pump-research-tape
SHA-256 = b0a096d6ae4773d0a08d279defbd94c4e0c394729a9f1522e918892b9d102f6f
BLAKE3  = e2d235f67e199e9cb43d7ff3bc19e7a957db3805a7480c24a23096d28362c9bd
bytes   = 12637504
mode    = 0700
```

Nowy create-new artefakt:

```text
/protected/operator/provider_independence_attestation_g5_2_2_v1.json
SHA-256 = 6b6e08ff3b23dfa6a4735cca179e2a80192be145b1be7475556557a0cf175f00
BLAKE3  = bbf781dd4eb0011f592287673b0e4391b706e453c955edd1a1e67b46def82a2c
bytes   = 4579
mode    = 0600
```

Atest wiąże niezmieniony Spectrum GO-E0.2 z finalnym G.5.2.2 certifierem.
Fizyczna niezależność pozostaje hash-pinned operator assertion, nie
automatycznym network-discovery proof.

## D8. Weryfikacja i decyzja

Odtworzone bramki:

```text
cargo fmt --all -- --check                         PASS
cargo check -p seer --lib                          PASS
cargo check -p seer --bin pump-research-tape       PASS
cargo build --locked --release                     PASS
research_tape_materializer                         46/46 PASS
szeroki research_tape                              83 PASS, 1 ignored
rpc_http_client                                     6/6 PASS
grpc_connection::tests                             95/95 PASS
standalone CLI                                      7/7 PASS
ghost-core frozen Pump Research                    11/11 PASS
CS0                                                 2/2 PASS
parser parity                                       1/1 PASS
future-capture supervisor                          10/10 PASS
git diff --check                                   PASS
git diff --cached --check                          PASS
public release CLI start-manifest FIFO             exit 1, no exact/snapshot
```

Ignored release capture-enabled harness:

```text
received / admitted / accepted = 8192 / 8192 / 8192
dropped / gaps                 = 0 / 0
persisted missing events       = 0
segments                       = 1
writer clean                   = true
writer error                   = null
capture abort                  = false
receive hand-off p99           = 290 ns
SLA                            = 100000 ns
fatal -> source cancel         = 53782632 ns
```

Stan po weryfikacji:

```text
GO-D raw                         PASS / UNCHANGED
Spectrum GO-E0.2                 READY_FOR_FULL_AUDIT / UNCHANGED
bounded control authority        PASS LOCALLY
indexed parse/digest authority   PASS LOCALLY
anonymous read-only snapshots    PASS LOCALLY
named cleanup surface            ABSENT
combined certify                 HOLD / NOT RUN
exact Ready                      NOT CREATED
export / strategy / execution    NO-GO
```
