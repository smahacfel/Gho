# ADR-8D: Pump Research G.5.2.3 — running executable inode authority

**Data:** 2026-08-17

**Status:** IMPLEMENTED AND VERIFIED LOCALLY / FINAL RELEASE AND ATTESTATION PINNED / NO PROVIDER I/O / COMBINED CERTIFY HOLD

**Task:** `PUMP_RESEARCH_G5_2_3_RUNNING_EXECUTABLE_INODE_AUTHORITY`

## D0. Problem

`env::current_exe()` zwraca pathname, nie przypięty inode wykonywanego obrazu.
Po uruchomieniu A i atomowej zamianie pathname na B proces nadal wykonuje A,
ale pathname digest może poprawnie zahashować B. Atest związany z B mógł więc
przejść mimo wykonywania A.

## D1. Decyzja: kernel-bound descriptor

Publiczny combined certifier jako pierwszą operację otwiera `/proc/self/exe`:

```text
O_RDONLY | O_CLOEXEC | O_NONBLOCK
-> fstat regular file
-> max 256 MiB
-> exact positional SHA-256/BLAKE3
-> post-read fstat type/size/dev/ino
-> Arc<File>
```

To jest dedykowany Linux helper. `/proc/self/exe` jest kontrolowanym przez
kernel magic symlinkiem do obrazu mapped przez bieżący proces. Pozostałe
operator-controlled paths nadal nie mogą przechodzić przez symlink.

## D2. Lifetime authority

Ten sam `Arc<File>` przechodzi przez:

1. pre-snapshot attestation validation;
2. post-snapshot combined authority validation;
3. full provider audit;
4. pre-exact-writer revalidation;
5. final revalidation po materializacji `.partial`, przed rename.

Żadna z tych granic nie otwiera ponownie pathname executable. Rewalidacja
wykonuje exact digest tego samego FD.

## D3. GO-E0

Przyszłe GO-E0 używa tej samej running-inode authority od początku operacji do
publikacji suitability receiptu. Historyczny Spectrum GO-E0.2 pozostaje
niezmieniony i jest klasyfikowany jako bounded provider availability/retention
preflight z wcześniejszą pathname-binding caveatą. Full audit samodzielnie
odtwarza cały qualification range i nie deleguje GO-E0 authority do promocji.

## D4. Failure semantics

- brak Linux `/proc/self/exe` blokuje qualification przed raw index/provider I/O;
- non-regular lub >256 MiB running image blokuje lokalnie;
- attested digest B przy running inode A blokuje przed snapshot/provider I/O;
- drift tego samego opened inode na późnej granicy blokuje exact publication;
- `env::current_exe()` nie jest produkcyjną provenance authority.

## D5. Regresja A -> B

Subprocess test kopiuje test executable A, uruchamia child, czeka na marker,
atomowo zastępuje pathname A odmiennym executable B i zwalnia child. Atest
fixture wiąże B. Publiczny combined hashuje `/proc/self/exe` A i wymaga:

```text
running executable mismatch
provider request count = 0
exact .partial = absent
exact final = absent
snapshot pathname = absent
```

Test ma lokalny deadline oraz zabija child przy zawieszeniu.

## D6. Zakres wpływu

Zmiana jest research-only PR-B plus future GO-E0 provenance. Nie zmienia:

- frozen raw V1;
- GO-D bytes lub historycznych receiptów;
- Spectrum GO-E0.2 bytes;
- capture ingress/writer/source request;
- aktywnego Seera, OracleRuntime i Event Busa;
- `MaterializedFeatureSet`, Gatekeepera, strategii ani execution.

Historyczne capture/preflight executable semantics nie są retroaktywnie
przepisywane przez ten amendment.

## D7. Final release i attestation

```text
target/release/pump-research-tape
SHA-256 = 8fc9c9e9e068d4b375e261f2c3d6e9aa4675007a96bc8cd4d962d102cc334932
BLAKE3  = c6fe14c43eec4804457fbdf741052ab9d750fa0d054b56a4742d8d8f9bf1ea4c
bytes   = 12641744
mode    = 0700

/protected/operator/provider_independence_attestation_g5_2_3_v1.json
SHA-256 = 501cb07f7c13d9be7a3d341ffa1afa735d1c5530a37d330f445895987c5b94e0
BLAKE3  = b66ef73e6435caf872e1e7b291cec016eb58e75ee1cccfda044abc43cf0e68da
bytes   = 4928
mode    = 0600
```

G.5.2.2 atest pozostaje historyczny i nie może autoryzować nowego certifiera.

## D8. Decyzja operacyjna

Odtworzone bramki:

```text
materializer                                      47/47 PASS
szeroki research_tape                             84 PASS, 1 ignored
subprocess running A / pathname B                  PASS
rpc_http_client                                     6/6 PASS
grpc_connection::tests                             95/95 PASS
standalone CLI                                      7/7 PASS
ghost-core frozen Pump Research                    11/11 PASS
CS0                                                 2/2 PASS
parser parity                                       1/1 PASS
future-capture supervisor                          10/10 PASS
cargo fmt/check/build                              PASS
git diff checks                                    PASS
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
receive hand-off p99           = 211 ns
SLA                            = 100000 ns
fatal -> source cancel         = 52263147 ns
```

```text
running executable inode authority       PASS LOCALLY / RELEASE PINNED
pathname A -> B regression               PASS LOCALLY / SUBPROCESS
future GO-E0 executable authority        PASS LOCALLY
GO-D                                     UNCHANGED
Spectrum GO-E0.2                         UNCHANGED / AVAILABILITY PREFLIGHT
provider I/O                             NOT RUN
combined certify                         HOLD
exact Ready                              NOT CREATED
export / strategy / execution            NO-GO
```
