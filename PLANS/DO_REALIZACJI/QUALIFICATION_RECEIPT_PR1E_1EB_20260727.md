# PR1E 1E-B — production authority cutover qualification receipt

Status:
`IMPLEMENTED / REVIEW REMEDIATION COMPLETE LOCALLY / CLOSED RUNTIME RUN PENDING / DRAFT PR`

Base: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`

Commit 1E-A:
`6f08297ca5d362fb593f8d53e0c9710f79f3751e`

Gałąź:
`agent/ingest-state-quote-boundary-pr1e-20260727`

## 1. Zakwalifikowany kontrakt

1E-B aktywuje jeden runtime authority dla nowych strukturalnych mutacji:

```text
primary raw wrapper + aligned ObservedPumpMutationV1
→ production PumpObservationLedgerV1
→ private CanonicalRuntimePermitV1
→ existing rich wrapper
→ Event Bus
→ actual downstream apply
→ CandidateIntegrity Ready/CAS
→ MFS
→ Gatekeeper
→ guarded submit
```

Nie istnieje produkcyjny per-event fallback do parent wrappera. Missing
observation, mismatch, brak canonical mutation, unavailable Ledger/registry,
capacity failure albo receipt ambiguity zamykają admission nowych kandydatów.
Secondary raw, parsed NLN, exact duplicate, `ContinuityOnly` i `Suppressed`
mają zero strukturalnych runtime emissions.

## 2. Zamrożone corpusy i runner

Digesty istniejących corpusów pozostały niezmienione:

| Kontrakt | BLAKE3 |
|---|---|
| PR1B legacy parser projection | `549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202` |
| PR1C AccountObservationArbiter v2 | `63839d047310638fe0d8643ee6c71148ac292f4390fc9098a2e573ce0ac1e051` |
| PR1D PumpObservationLedger v1 | `833de2bd384c964712f2e7127f9bc1db57745644633c1c66facef540cdf4c2a4` |
| PR1D PumpObservationLedger v2 | `c81d7b4f0cc3792c2bb2c4e71bfd0634fcfdd69723758d741ee2405770603415` |
| PR1D full parser snapshot v2 | `507b13704d5b90c3f724a395acbf0d0cc55fdc37a83fcb95cf67cceb6247569f` |
| PR1E manifest | `cd28c798082999cf2377842199ffabb6601f7115417da244a3cf864e5ef27208` |
| PR1E cross-layer corpus | `30fbf78344afd77958fe573af5c2414139023db4c770b03c0f710026b7cdd38c` |

Wersja wykonywalnego runnera po lokalnej remediacji review:

```text
ghost-launcher/src/pr1e_qualification.rs
SHA-256 = d67137260eaa1904f2e40bdc1fe84d54199725138833e9de253fca879d8e114b
```

Runner używa produkcyjnych:

- `PumpObservationLedgerV1`;
- `CandidateIntegrityRegistry`;
- launcherowego typed admission;
- `SessionPoolTradeBridge`;
- Event Bus adaptera;
- exact downstream apply receipt.

Fake Ledger: `0`. Fake CandidateIntegrity: `0`.

Po niezależnym review runner został rozszerzony tak, aby każdy z 23 rekordów
JSONL był rzeczywiście dispatchowany przez produkcyjny Ledger, typed permit,
Event Bus adapter i `PoolObservationSession` z typed apply result. Poprzedni
SHA-256 runnera nie jest więc receipt'em finalnego remediacyjnego diffu i nie
może być używany jako finalna bramka merge.

## 3. Targeted correctness i fault injection

| Bramka | Wynik |
|---|---|
| `cargo test -p ghost-launcher --lib pr1e_ -- --nocapture` | PASS — 16/16 |
| `cargo test -p ghost-launcher --lib candidate_integrity::tests:: -- --nocapture` | PASS — 17/17 |
| AccountObservationArbiter frozen corpus | PASS — 2/2 |
| PumpObservationLedger frozen V1/V2 corpus | PASS — 3/3 |
| PumpObservationLedger adversarial/fault corpus | PASS — 40/40 |
| PumpObservationLedger targeted unit contract | PASS — 9/9 |
| PR1B legacy parser digest | PASS |
| PR1D full parser digest | PASS |

Testy PR1E obejmują między innymi:

- missing primary observation i wrapper mismatch;
- mixed provider role/ID;
- exact duplicate, secondary raw i parsed witness suppression;
- ledger/registry capacity exhaustion i poisoned Ledger;
- incomplete inventory i per-candidate exact locator isolation;
- Event Bus delivery bez downstream apply;
- duplicate/ignored/terminal/failed apply;
- buffered permit replay, expiry i eviction;
- active MFS/evaluation guard;
- submit-before-conflict, post-submit reconciliation i confirmed-position
  quarantine;
- startup bez Event Bus/registry oraz legacy source;
- `ContinuityOnly` z zerem strukturalnych emisji;
- duplicate AccountUpdate i provider conflict;
- globalne invalidowanie issued evaluation/submit guard po close admission;
- primary IPC saturation → coverage-gap control plane → CandidateIntegrity
  denial;
- bounded terminal retirement Ledgera i registry, rollover FIFO oraz zakaz
  retirement unresolved receipt.

Powyższa tabela gate'ów jest historycznym receipt'em wcześniejszego SHA.
Końcowa macierz musi być uruchomiona ponownie na finalnym remediacyjnym SHA;
do tego momentu nie należy zapisywać `OFFLINE PASS` ani `DIFFERENTIAL PASS`.

## 4. Package/workspace gate matrix

| Polecenie | Wynik | Receipt SHA-256 |
|---|---|---|
| `cargo fmt --all --check` | PASS | pusty log |
| `git diff --check` | PASS | pusty log |
| `cargo test -p ghost-core` | EXPECTED_BASELINE_FAILURE | `c38a2bd050da7bf63d202778553eac59d4813aeb7193d01932ebc8455baaf20f` |
| `timeout 300s cargo test -p seer --no-fail-fast` | EXPECTED_BASELINE_FAILURE | `a852c9866a3b4f0e40339386b7825edb0f8cd6d2a73984b1c1ceb99003a2a5d5` |
| `cargo test -p trigger` | EXPECTED_BASELINE_FAILURE | `0500b62c0cc4379715b7375157288825d7d2b7c2cbaaf31e4bf9f415176ef627` |
| `cargo test -p ghost-launcher` | EXPECTED_BASELINE_FAILURE | `411123194a78af4a94c3d524ff3a7e0d30683a93ba5d8db0ad8ddd8789fb8591` |
| `cargo test --workspace --no-fail-fast` | EXPECTED_BASELINE_FAILURE | `bcea86a60c7d7569a9185fb793c1efa35b23820d4a1114cb901d54b11db3ed84` |
| `cargo build --release --workspace` | PASS — 588 s | `5d23aead4c1ecef7a81f2e6ff6c8a03e163136d1b0e9cf68a0a7e40517c43afa` |

Zamrożone czerwone klasy pozostały bez zmian:

- `ghost-core`: wyłącznie `InvalidTagEncoding(104)`;
- Seer: dokładnie 12 historycznych failure testów biblioteki i jeden
  `source_router`;
- trigger: dwa brakujące `status_uuid` oraz
  `presigned.size_bytes < 700`;
- launcher/workspace: wyłącznie testowe `PoolTransaction E0063` z tym samym
  missing-field setem. Każdy ujawniony target istniał już na
  `103212b` i PR1E go nie zmienił.

Nie naprawiono żadnego odziedziczonego fixture w PR1E.

## 5. Formalny parent-versus-PR1E performance protocol

Warunki:

- parent: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`;
- current: finalny frozen diff PR1E;
- osobne release test-binary;
- ten sam host, profil i workload;
- 5 warm-upów na wariant;
- 20 naprzemiennych paired rounds;
- 100 000 bootstrap samples, seed `20260726`;
- brak równoległego Cargo lub repo workload.

| Bramka | Wynik |
|---|---:|
| throughput geometric mean ratio | `1.026863643` |
| throughput lower one-sided 95% CI | `1.018097198` — PASS (`>= 0.98`) |
| receive-to-normalize p99 geometric mean ratio | `0.913547337` |
| receive-to-normalize p99 upper one-sided 95% CI | `0.958477979` — PASS (`<= 1.05`) |
| missing events | `0` we wszystkich 50 runach |
| silent drops | `0` we wszystkich 50 runach |
| saturation receiver blocked | `0` |
| saturation blocking wait | `0 ns` |
| slow IPC receiver blocking waits | `0` |
| parser V1/V2 digest drift | `0` |

Synthetic slow-WAL writer wykonał oczekiwane opóźnienie we własnym workerze.
Receiver enqueue pozostał nieblokujący względem writer delay: maksimum
`1.232519 ms`, przy minimalnym writer completion `6.964297 ms`.

Pozostałe opisowe mediany/zakresy oraz wszystkie pary znajdują się w
`pairs.tsv` i `summary.json`.

Raw receipts:

| Artefakt | SHA-256 |
|---|---|
| `/tmp/pr1e_perf_protocol_20260727_v1/metadata.json` | `f0e1bfe074b43918a0c75f6aab74a4cbf180db55d998b0059ceeebe019ea4684` |
| `/tmp/pr1e_perf_protocol_20260727_v1/runs.jsonl` | `3105cfa8193978ca295e00e7a7ac664ccd8af13eda1a3de9b7330479a5a6adc6` |
| `/tmp/pr1e_perf_protocol_20260727_v1/pairs.tsv` | `597c583819dd439fe9980ff43a42faf4bcf868148d6bed0ca7906327d29c25ea` |
| `/tmp/pr1e_perf_protocol_20260727_v1/summary.json` | `8e18d9564154eadee2b2d6e977acbb9b62216d892fe7fe42b29ac8bb981e363b` |
| `/tmp/pr1e_perf_protocol_20260727_v1/raw_receipts.sha256` | `e756c351e8d14ff6218f300cfbd8bddd2067abaef3ea68f29500b8b3b69d32b3` |

Performance result: `PASS`. Waiver PR1D nie został użyty.

## 6. Rollback identity

```text
previous baseline binary inventory hash:
e0ee4230901002efdcdd9e4d26237d3973a24f59f714cf1a1e74d37738747d27

previous config SHA-256:
eecc6462eaa98325a899bc4de19fb5fba7387f74ac96e3c994126379fb60e737

final PR1E release binary SHA-256:
a916d87088119683bda64462f7bc6defa34f2cdee9aa26068a8ff84afa709540

final PR1E config SHA-256:
eecc6462eaa98325a899bc4de19fb5fba7387f74ac96e3c994126379fb60e737
```

Rollback jest wyłącznie atomowym przywróceniem poprzedniego binary i configu.
Nie istnieje runtime `Legacy`, produkcyjny `Observe` ani per-event fallback.

## 7. Review remediation: active safety and retention

### 7.1 Global admission closure

`close_candidate_admission()` inkrementuje globalną admission generation.
Issued evaluation/submit guards zachowują jej snapshot i odrzucają każde nowe
MFS/evaluation/BUY/submit po globalnym close. `SubmitStarted` zachowuje
wyłącznie confirmation/reconciliation, a confirmed position continuity nie
jest przerywana.

### 7.2 Bounded terminal retirement

Terminalny Oracle cleanup przekazuje candidate do bounded registry tombstone
FIFO oraz do bounded Seer-ledger tombstone FIFO. Oba tory odzyskują active
capacity, zachowują późną duplicate/witness classification i rejestrują first
eviction. Unresolved receipt oraz pełny retirement handoff fail-close new
admission zamiast silently discard.

### 7.3 Primary coverage gaps

Primary raw ingress/IPC local gap przechodzi przez niezależny control-plane
notice i aktywnie zamyka candidate admission. Secondary/NLN gap pozostaje
telemetry-only. Nie jest to per-event legacy fallback.

Notice nie jest pojedynczym nadpisywalnym slotem: IPC zachowuje bounded,
monotoniczny prefix distinct gap notices. Dzięki temu późniejszy secondary
notice nie może ukryć primary gap przed launcherem. Overflow samego
control-plane retention jest fail-closed, bo provider scope dalszych gaps nie
jest wtedy dowiedziony; zidentyfikowany, nieoverflowujący secondary/NLN gap
pozostaje telemetry-only.

### 7.4 Focused remediation checks (local worktree; not a merge receipt)

Po zamrożeniu remediacyjnego kodu lokalnie przeszły następujące bezpośrednie
bramki:

| Polecenie / kontrakt | Wynik |
|---|---|
| `cargo check -p ghost-core -p seer -p ghost-launcher` | PASS |
| `cargo test -p ghost-launcher --lib pr1e_ -- --nocapture` | PASS — 16/16 |
| global admission generation: evaluation / BUY-not-submitted / SubmitStarted | PASS — 3/3 |
| poisoned Ledger → issued guard cannot begin MFS | PASS |
| real IPC saturation → control-plane gap → launcher admission close | PASS |
| bounded gap-control retention preserves primary after witness and signals overflow | PASS — 2/2 |
| registry terminal retirement / unresolved receipt / FIFO rollover | PASS — 3/3 |
| ledger terminal retirement / late witness / FIFO rollover | PASS — 2/2 |
| frozen PumpObservationLedger V1/V2 executable corpus | PASS — 3/3 |
| production Oracle downstream-apply ordering barrier | PASS |
| `cargo fmt --all --check` + `git diff --check` | PASS |

Te wyniki nie zastępują końcowej macierzy package/workspace/release dla
zacommitowanego SHA, parent-versus-Enforce differential ani credentialed
closed runtime run opisanych niżej.

## 8. Nadal wymagane external gates przed merge

Gate 30 min / minimum 10 000 successful primary raw Pump mutations nie został
sfabrykowany ani zastąpiony testem. W środowisku implementacyjnym brak:

```text
GHOST_SEER_GRPC_ENDPOINT
GHOST_SEER_GRPC_X_TOKEN
```

`config.toml` pozostaje poprawnie `shadow_only` / `shadow`, ale bez endpointu
i tokenu nie można uruchomić production-like Yellowstone inputu. W związku z
tym:

- Draft PR może zostać otwarty do review;
- PR1E nie jest jeszcze merge-qualified;
- przed merge należy uruchomić finalny release binary na production-like
  configu, zapisać authority epoch receipt i wykazać minimum 30 min oraz
  10 000 successful primary mutations;
- wynik musi spełnić wszystkie gate'y sekcji 19 planu.

Status tego gate'u: `EXTERNAL ENVIRONMENT BLOCKED / REQUIRED BEFORE MERGE`.

## 9. Wniosek

Cutover nie dotyka ekonomiki, quote math, strategii, MFS schema ani
shadow/live configu. Nie wolno jednak twierdzić, że PR jest merge-qualified:

1. credentialed closed runtime run 30 min / 10k primary mutations nadal jest
   twardą external bramką;
2. parent-versus-Enforce differential musi zostać wykonany na tym samym
   production-like input i zapisany machine-readably dla finalnego SHA;
3. finalna package/workspace/release matrix musi zostać ponownie powiązana z
   tym samym SHA, bez zastępowania nowych failure signatures historycznym
   receipt'em.

Draft PR pozostaje świadomie niegotowy do merge do czasu zamknięcia tych
bramek oraz authority-epoch receipt.
