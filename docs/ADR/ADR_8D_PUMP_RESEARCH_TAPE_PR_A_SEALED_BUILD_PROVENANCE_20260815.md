# ADR-8D: Pump Research Evidence Tape V1.1 — sealed build provenance, ignored fixtures i external operator config

**Data:** 2026-08-15

**Status:** IMPLEMENTED / LOCAL-ONLY VERIFICATION REQUIRED / NO PROVIDER RUN / PR-B BLOCKED

**Task:** `PUMP_RESEARCH_TAPE_PR_A_SEALED_BUILD_PROVENANCE`

## D0. Problem

Poprzedni operator-preflight hashował bieżący source/worktree oraz bieżący
executable, ale nie dowodził relacji:

```text
sealed executable B was built from sealed source S
```

Stara binary z `target/release` mogła zostać po prostu współzahashowana z
później zmodyfikowanym source. Ponadto standardowy inventory Git pomijał
ignored `corpus_manifest_v1.json`, a config operatora mógł wejść do bundle'a
przez snapshot/patch mimo redacted sidecaru.

To były blokery provenance przed każdym provider-backed capture. Nie dotyczą
frozen raw V1 storage ani aktywnego Seer runtime.

## D1. Decyzja: full source snapshot i fresh sealed build

`pump-research-tape preflight` jest uruchamiany przez non-debug bootstrap
binary. Bootstrap nie jest binarką dopuszczoną do capture. Preflight:

1. tworzy `source_snapshot/` z pełnego bieżącego source worktree;
2. zapisuje `source_snapshot_manifest_v1.json` z SHA-256 i BLAKE3 każdego
   copied regular file;
3. buduje ze snapshotu przez dokładną komendę:

   ```bash
   cargo build --locked --offline --release -p seer --bin pump-research-tape
   ```

   w nowym, samodzielnie utworzonym `CARGO_TARGET_DIR`;
4. weryfikuje source snapshot po buildzie;
5. sprawdza przed/po buildzie także redacted build environment: `RUSTFLAGS`,
   compiler-wrapper/build environment oraz digests regularnych Cargo config
   files z Cargo home, sealed source snapshot i wszystkich ancestors build cwd
   (bez kopiowania ich contents);
6. zachowuje `release/build_environment_v1.json`, `release/build.log`,
   `release/build_receipt_v1.json` i exact copied
   `release/pump-research-tape`;
7. wiąże wszystkie te digests w immutable preflight receipt.

Capture musi zostać uruchomiony przez
`<preflight-bundle>/release/pump-research-tape`. Przed pierwszym RPC porównuje
digest własnego executable, bieżący live source manifest, required ignored
fixture inventory, `Cargo.lock`, config, toolchain i każdy bundle sidecar z
receipt. Bootstrap binary, stale target binary oraz binary z innego profilu
failują przed provider I/O.

## D2. Source inventory i ignored fixture

Full source snapshot obejmuje:

- wszystkie Git-tracked istniejące regular files;
- wszystkie Git-untracked **non-ignored** regular files;
- tylko jawnie allowlistowane required ignored fixtures.

W V1 allowlista zawiera:

```text
ghost-core/tests/fixtures/pump_research_tape_v1/corpus_manifest_v1.json
```

`target/`, datasets i arbitrary ignored files nie są capture source. Brak,
nie-regularny typ, brak statusu Git-ignore, hash drift albo brak corpus fixture
w snapshotie failuje closed. Dzięki temu nie udajemy już, że
`--exclude-standard` obejmuje wszystkie kontraktowe pliki.

## D3. Operator config i credentials

Faktyczny TOML operatora musi być regularnym plikiem **poza Git worktree**.
Tracked `configs/rollout/pump-research-tape-v1.toml` jest wyłącznie
non-secret template i nie może zostać użyty do realnego preflightu. Bundle
zachowuje wyłącznie digest configu i redacted projection; nie kopiuje surowego
operator TOML.

gRPC i RPC endpointy muszą być publicznymi root-only HTTPS originami bez
userinfo, path, query i fragmentu. Credential nie może być częścią URL.
gRPC oraz opcjonalny read-only RPC credential używają wyłącznie nazwanej
zmiennej środowiskowej i headera. Wartość tokenu nie trafia do receipt,
manifestu, logu ani raw tape.

Pełny bundle nadal jest wrażliwym artefaktem forensic: source snapshot,
tracked patch lub build log może zawierać niepowiązane lokalne dane. Tworzymy
bundle z uprawnieniem `0700` na Unix, ale nie deklarujemy, że cały bundle jest
automatycznie wolny od sekretów i nie wolno go publikować.

## D4. Binding i pierwsze provider I/O

Walidacja preflight jest wydzieloną, czysto lokalną funkcją i kończy się przed
utworzeniem ProgramData RPC clienta lub Yellowstone source connection. Raw
run directory jest celowo tworzony dopiero po udanym start ProgramData receipt.
Run-local binding zatem zapisuje:

- `receipt_validated_wall_ms` — rzeczywisty czas local validation przed RPC;
- `binding_written_wall_ms` — późniejszy czas zapisu bindingu po udanym start
  receipt.

Nie twierdzimy już, że run-local binding file istnieje przy nieudanym first
RPC. Taki attempt nie admittuje source recordu ani nie otwiera raw runu.

## D5. Dowody i regresje

Dodano albo rozszerzono lokalne regresje dla:

- publicznego root-only HTTPS endpoint policy;
- configu operatora poza repo;
- inventory i snapshotu required ignored `corpus_manifest_v1.json`;
- driftu tego fixture po snapshot;
- source snapshot contents;
- integrity wszystkich persisted sidecars, nie tylko binary;
- rozdzielenia `receipt_validated_wall_ms` i `binding_written_wall_ms`;
- publicznego `capture` dla invalid preflight z testowym probe'em: zero wejść
  do fazy provider I/O;
- jawnego per-client RPC auth header bez globalnego auth active runtime.

Nadal wymagane przed GO do realnego capture:

1. lokalne wykonanie fresh sealed preflight z syntetycznym external configiem;
2. inspection bundle'a (manifest, build receipt, snapshot, binary, hashes);
3. operator approval rzeczywistych endpointów/credentialów;
4. osobny observe-only provider-backed raw capture;
5. inspection immutable runu.

## D6. Niezmienione granice i rollback

Nie zmieniono:

```text
PumpResearchRawRecordV1 / raw V1 codec / headers / footers
active connect_geyser() / SeerConfig / Event Bus
Gatekeeper / MaterializedFeatureSet / AccountStateCore / execution
PR-B materializer, certifier, exporter i qualification
```

Rollback przed provider capture polega na niewykonywaniu preflight/capture.
Nie wolno przywrócić stale binary z `target/release` jako capture artifactu,
uczynić receipt opcjonalnym, pominąć ignored corpus fixture, kopiować raw
operator configu do bundle'a ani wkładać credentialu do URL.
