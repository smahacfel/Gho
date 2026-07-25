# ADR-8D — PR #83: canonical state, trwałość gapów i kontrolowany shutdown ingestu

**Data:** 2026-07-25
**Status:** Implemented; scoped PR1B gates verified
**Zakres:** korekta blockerów review PR1B w Seer ingest, IPC, WAL/evidence,
LocalCoverageGap, parity harness i konfiguracji
**Poza zakresem:** AccountObservationArbiter PR1C, raw/NLN reconciliation PR1D,
MFS, Gatekeeper, strategia, quote math i execution

> Globalny szablon wskazany w instrukcji (`/Gho/docs/ADR/ADR_8D_SZABLON.md`)
> nie istnieje w dostępnych checkoutach. Dokument używa struktury istniejących
> ADR-8D tego repozytorium.

## 1. Problem

Pierwsza wersja PR1B poprawnie usuwała wielokrotne encode/decode/scan oraz
blokujące sinki z hot path, ale nie domykała kontraktu integralności:

1. pełna wspólna kolejka IPC mogła odrzucić canonical `AccountUpdate`;
2. `LocalCoverageGapV1` nie przechowywał liczby ani granic odrzuconych eventów;
3. gapy WAL/evidence/IPC kończyły się w tracingu zamiast w trwałym audycie;
4. structural digest nie obejmował pełnej semantyki parsera;
5. capacity `1,024` wynikała z błędnej matematyki średniego throughputu;
6. dispatchery nie miały kontrolowanego stop/drain/flush/join;
7. opcjonalny artifact writer był ukrytym globalnym warunkiem poprawności runu.

## 2. Decyzja

### 2.1. Canonical AccountUpdate ma osobną bounded state lane

IPC składa się z:

```text
normal business lane
  bounded VecDeque<SeerEvent>

canonical state lane
  bounded VecDeque<SeerEvent>
  FIFO wszystkich przyjętych AccountUpdate
  bez deduplikacji i freshness arbitration

single fixed dispatcher
  wybiera niższy front sequence_number z obu FIFO
  try_send + bounded retry poza hot path
```

Pełna normalna kolejka nie odbiera capacity canonical state lane. Dla tej samej
krzywej wszystkie obserwacje pozostają odrębne, w tym `write_version=None`,
`Some(0)` oraz same-version/different-hash. PR1B nie wybiera zwycięskiego
stanu przed AccountObservationArbiter PR1C. Lane jest bounded; launcher ustawia
jej capacity przez `account_update_queue_capacity`. Stara nazwa
`account_update_coalescing_capacity` pozostaje serde aliasem tylko dla odczytu.
Brak miejsca jest jawnym
`IpcEgressQueueSaturated`, a nie pozornym sukcesem.

Sequence number jest nadawany atomowo pod tym samym lockiem co enqueue, więc
współbieżny producent nie może zakolejkować `N+1` przed `N`. Dispatcher
zachowuje `JoinHandle`. Zamknięcie downstream lub przekroczenie deadline’u
przed dostarczeniem zaakceptowanych eventów ustawia typed failure zwracany
przez shutdown.

### 2.2. Gap zapisuje pełne rozliczenie lokalnej utraty

Addytywny, serde-compatible kontrakt zawiera:

```rust
missing_event_count: u64
first_dropped: LocalCoverageBoundaryV1
last_dropped: LocalCoverageBoundaryV1
```

Każde odrzucenie w ciągłym epizodzie zwiększa licznik i aktualizuje
`last_dropped`. Pierwsza granica pozostaje niezmienna. Deterministyczny gap ID
używa domeny `ghost_local_coverage_gap_v2` i obejmuje licznik oraz obie granice.
Stary JSON bez nowych pól nadal się deserializuje przez `#[serde(default)]`.

### 2.3. Wszystkie domeny gapów używają centralnego audit routera

```text
ingress tracker ----\
WAL tracker ---------\
evidence tracker -----+-> LocalGapAuditRouter
IPC tracker ----------/       |
                              +-> reserved bounded audit queue
                                   |
                                   +-> dedicated WAL writer
                                        append LocalCoverageGap
                                        final WAL flush
```

Audit queue i pending buffer są bounded. Nie korzystają z normalnej kolejki
`WalJob`, dlatego jej saturacja nie uniemożliwia zapisania własnego markera.
Jeżeli również rezerwowana ścieżka nie może zachować markera, shutdown zwraca
błąd zamiast raportować pełny sukces.
Run bez skonfigurowanego audit WAL dispatchera również nie może zakończyć się
sukcesem, jeżeli router ma nieutrwalone markery.

`WalRecord::LocalCoverageGap` pozostaje audit-only: replay liczy dowód, ale nie
mutuje canonical state.

### 2.4. Artifact diagnostics są oddzielone od run contract

Nowe pola:

```text
artifact_capture_enabled: bool
artifact_required_for_run: bool
```

Zachowanie:

| Capture | Required | Skutek awarii/saturacji evidence |
|---|---|---|
| false | false | writer nie jest uruchamiany |
| true | false | typed diagnostic gap; canonical path działa dalej |
| true | true | typed gap i sticky non-evaluable segment |
| false | true | config odrzucony przed startem |

Stare konfiguracje otrzymują oba pola jako `false`.

### 2.5. Parity obejmuje pełne typed eventy

`CanonicalParserParitySnapshotV1` przechowuje pełne:

- `InitializePoolEvent`;
- każde `TradeEvent`.

Normalizowane są wyłącznie lokalne, niedeterministyczne timestampy ingress.
Hash obejmuje wszystkie pozostałe pola typów, w tym ekonomię, stan, identity,
order, route i instruction provenance.

Pełny digest na B0 (`5136319`) i na finalnym PR1B:

```text
549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202
```

Osobny test zmienia pole semantyczne i wymaga innego digestu.

### 2.6. Capacity jest konfigurowalna i mierzona z równoległym konsumentem

`ingress_queue_capacity` jest serde-default polem konfiguracji. Domyślna wartość
to `2,048`.

Capacity harness używa zamrożonego, deterministycznego protobuf replay
workloadu. Producent i rzeczywisty konsument działają równolegle; konsument
wykonuje tę samą normalizację i `parse_transaction_bundle` co ścieżka
produkcyjna.

Ostatni pomiar release używa 3,072 eventów, czyli workloadu celowo innego niż
capacity, oraz twardych SLA:

```text
input events             = 3,072 (24 × 128, interval 50 ms)
configured capacity      = 2,048
peak batch ingress       = 73,873.871 events/s
operational ingress      = 2,535.427 events/s
sustained drain          = 2,442.683 events/s
queue high-water         = 134
queue dwell p99          = 47,209,510 ns <= 250,000,000 ns
oldest backlog age       = 54,277,899 ns <= 500,000,000 ns
missing_event_count      = 0
```

Brama dowodzi braku utraty i bounded dwell dla konkretnego zamrożonego profilu,
nie uniwersalnej capacity dla dowolnego przyszłego profilu Yellowstone. Pole
pozostaje konfigurowalne, aby shadow/captured workload mógł wyznaczyć
produkcyjną wartość bez zmiany kodu.

### 2.7. Każdy zaakceptowany job ma kontrolowany lifecycle

WAL, raw evidence, IPC i local-gap audit realizują:

```text
stop accepting
-> zamknij otwarty gap
-> drain zaakceptowanych elementów
-> final flush trwałego sinka
-> join worker/task
-> zwróć success albo typed failure
```

Krótkie lifecycle mutexy wykluczają wyścig `accepted after final drain`.
Launcher najpierw zatrzymuje producenta Seer, potem drenuje dispatchery, a IPC
receiver pozostawia aktywny do chwili opróżnienia egressu. Każdy dispatcher
otrzymuje pozostałą część wspólnego czterosekundowego deadline’u, a launcher
nakłada niezależny pięciosekundowy outer timeout. OS-writer, który utknie w
filesystem I/O, jest odłączany po deadline i powoduje typed shutdown failure;
Tokio evidence writer jest abortowany. Timeout/abort nie może zostać
raportowany jako sukces.

## 3. Zachowane inwarianty

- Yellowstone receiver nie czeka na WAL, evidence ani IPC downstream;
- wszystkie kolejki pozostają bounded;
- brak per-event task spawning;
- decoded transaction nadal jest normalizowana bez application-level decode;
- parser nadal wykonuje jedno pełne przejście outer + inner;
- CREATE poprzedza TRADE z tej samej transakcji;
- provider ID/role, `txn_signature`, `tx_index=Some(0)`, ordinal i provenance
  pozostają zachowane;
- canonical AccountUpdate nie konkuruje o pełną normalną kolejkę business IPC;
- LocalCoverageGap nie jest provider slot gap i nie uruchamia reconnectu;
- stare JSON/configi pozostają czytelne;
- nie zmieniono AccountStateCore arbitration, MFS, Gatekeepera, strategii,
  scoringów, quote math ani execution;
- shadow/live boundary pozostaje bez zmian.

## 4. Failure semantics

| Awaria | Wynik |
|---|---|
| ingress full | typed ingress gap, jawny count/boundaries, segment non-evaluable |
| normal WAL queue full | typed WAL gap przez niezależny audit lane |
| required evidence full/fails | typed evidence gap i non-evaluable |
| diagnostic evidence full/fails | typed evidence gap bez zatrzymania canonical path |
| IPC normal/state lane full | typed IPC gap, brak silent success |
| IPC downstream closes early | shutdown failure |
| WAL append/final flush fails | shutdown failure |
| audit lane overflows/fails | shutdown failure |
| worker panics | join/shutdown failure |

## 5. Dowody testowe

Wąskie testy obejmują:

```text
IPC downstream full
-> AccountUpdate enqueue nie blokuje
-> update jest zachowany
-> po drainie dociera dokładnie raz

2 odrzucone eventy
-> jeden gap
-> missing_event_count = 2
-> first_dropped i last_dropped zachowane

ingress/WAL/evidence/IPC gap
-> każdy trafia do reserved audit WAL
-> replay odczytuje wszystkie cztery reason codes

enqueue 32 WAL records
-> shutdown
-> drain + final flush + join
-> replay dokładnie 32 rekordów w kolejności

diagnostic evidence saturation
-> canonical runtime nadal evaluable

required evidence saturation
-> run segment non-evaluable

semantic field mutation
-> full parity digest zmienia się
```

Pełne bramki i aktualny failure signature są zapisane w
`PLANS/DO_REALIZACJI/BASELINE_RECEIPT_INGEST_HOT_PATH_PR1B_EA7D31A_20260724.md`.

Finalny `cargo build --release --workspace` przechodzi. Scoped testy PR1B,
compatibility tests, replay audit test, `cargo check -p ghost-brain --tests`,
formatowanie i kontrola diffu przechodzą. Pełne test suites nadal zatrzymują
się na opisanych w receipt, nietkniętych błędach bazowych: 14 istniejących
failures/hang w pełnym Seer oraz niekompletnych fixture’ach `PoolTransaction`
z `E0063` w testach launchera.

## 6. Ryzyka i ograniczenia

- Default `2,048` jest poparty zamrożonym replay workloadem, nie raw capture z
  każdego produkcyjnego reżimu. Produkcyjny rollout powinien kalibrować pole z
  shadow/captured peak i drain telemetry.
- Transport zachowuje wszystkie przyjęte AccountUpdate; arbitrażowa
  klasyfikacja duplicate/conflict pozostaje wyłącznie zakresem PR1C.
- PR1B nadal nie dowodzi odzyskania local gap. Każda nieodzyskana luka pozostaje
  fail-closed.
- Rezerwowana audit lane jest bounded. Jej wyczerpanie nie jest ukrywane:
  kończy się błędem shutdownu.

## 7. Rollback

Bezpieczny rollback obejmuje całą korektę review razem z B2. Nie należy:

- usuwać state lane przy pozostawieniu nieblokującego normalnego IPC;
- kierować audit gapów z powrotem przez normalną kolejkę WAL;
- usuwać drain/join przy pozostawieniu asynchronicznych writerów;
- przywracać structural digest jako dowód pełnej parity;
- przywracać stałe `1,024` wyliczone ze średniego throughputu.

Nowe pola konfiguracji są addytywne i mają serde-default, więc rollback nie
wymaga migracji istniejących plików konfiguracyjnych.
