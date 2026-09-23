# ADR-8D: Pump Research Capture — wspólny lock canonical output directory

**Data:** 2026-08-17

**Status:** IMPLEMENTED / LOCAL-ONLY / FUTURE CAPTURE NOT RUN

**Task:** `PUMP_RESEARCH_CAPTURE_SUPERVISOR_OUTPUT_LOCK_SCOPE`

## D0. Problem

Pierwsza wersja exact-child supervisora tworzyła lock jako:

```text
<operator_dir.parent>/.capture.lock
```

`--operator-dir` jest dowolnym argumentem CLI. Dwa równoległe supervisory
mogły więc wybrać różne drzewa logów, oba przejść pre-spawn `/proc` scan i
oba uruchomić capture do tego samego `output_dir`. Exactly-one-run
postcondition wykryłby konflikt dopiero po zakończeniu i nie zapobiegał dwóm
provider streams.

Problem dotyczył wyłącznie przyszłego supervisora. Nie naruszył GO-D raw,
qualification preparation ani eligibility istniejącego datasetu.

## D1. Decyzja: lock należy do fizycznego output directory

Supervisor rozwiązuje skonfigurowany `output_dir` przez `resolve(strict=True)`
i używa jednego locka:

```text
<canonical output_dir>/.pump-research-capture.lock
```

Lock file:

- jest otwierany przez `O_NOFOLLOW`, `O_CLOEXEC`, `O_CREAT` i `O_RDWR`;
- musi być regularnym plikiem;
- ma wymuszony tryb `0600`;
- otrzymuje nonblocking exclusive `flock`;
- pozostaje trzymany do zapisania operator execution receipt.

Lock jest przejmowany przed skanem aktywnych capture processes, sprawdzeniem
zasobów, snapshotem istniejących runów, utworzeniem `operator_dir` oraz
`Popen`. Drugi supervisor dla tego samego physical output root kończy się
fail-closed przed utworzeniem childa.

## D2. Scope i granice

Semantyka receiptu nazywa scope:

```text
canonical_output_directory_v1
```

Jest to wykluczenie wszystkich operator directories współdzielących jeden
canonical dataset root, a nie host-global lock. Wszystkie capture'y
autoryzowane przez obecny plan używają jednego canonical `output_dir`.
Równoległe capture'y do różnych roots nie są przez plan autoryzowane; ich
bezwzględny host-global zakaz wymagałby osobnej decyzji.

Skan `/proc` pozostaje addytywnym defense-in-depth dla capture'u uruchomionego
poza aktualnym supervisorem. Nie zastępuje i nie poprzedza authority locka.

## D3. Regresja współbieżności

Publiczny test uruchamia dwa rzeczywiste procesy supervisora z:

```text
ten sam output_dir
różne operator_dir.parent
ten sam syntetyczny capture binary
```

Pierwszy supervisor przejmuje lock i uruchamia dokładnie jeden child. Drugi:

- kończy się kodem non-zero na konflikcie locka;
- nie tworzy operator directory;
- nie dochodzi do `Popen`;
- nie tworzy drugiego raw runu.

Test potwierdza także tryb `0600`, właściwy lock path, zapis scope w execution
receipt i brak starego per-operator `.capture.lock`.

## D4. Wpływ i rollback

Zmiana jest ograniczona do research-only Python supervisora, jego regresji,
planu i ADR. Nie zmienia frozen raw V1, source tap, Yellowstone requestu,
parsera, aktywnego Seera, Gatekeepera, MFS ani execution. Nie wykonano RPC,
Yellowstone, GO-E0, `certify`, eksportu ani strategii.

Rollback oznacza niewykonywanie future capture supervisora. Nie usuwamy
canonical lock file podczas normalnego działania; trwały inode jest wyłącznie
miejscem dla advisory `flock`, nie markerem sukcesu ani aktywnego procesu.
