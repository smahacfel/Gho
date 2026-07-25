# ADR-8D — PR #83: lossless AccountUpdate FIFO, ordered IPC i bounded shutdown

**Data:** 2026-07-25
**Status:** Implemented; scoped verification and final release workspace build passed
**Zakres:** drugi cykl korekt review PR1B dla transportu Seer→IPC, lifecycle
dispatcherów, capacity harnessu, dokumentacji i dowodu CI
**Poza zakresem:** AccountObservationArbiter PR1C, source arbitration, MFS,
Gatekeeper, quote math, strategia i execution

> Wskazany globalnie szablon `/Gho/docs/ADR/ADR_8D_SZABLON.md` nie istnieje w
> dostępnych checkoutach. Dokument zachowuje układ problem/decyzja/inwarianty/
> failure semantics/weryfikacja stosowany w istniejących ADR-8D repozytorium.

## 1. Problem

Pierwsza korekta PR #83 odizolowała canonical AccountUpdate od pełnej kolejki
business IPC, ale state lane sama wykonywała przedwczesny arbitraż:

```text
HashMap<bonding_curve, pending AccountUpdate>
freshness = (slot, write_version.unwrap_or_default())
```

Skutki:

1. drugi update tej samej krzywej zastępował albo znikał;
2. `write_version=None` zlewało się z `Some(0)`;
3. same-version/different-hash znikało przed PR1C;
4. provider, signature, hash i treść rezerw nie uczestniczyły w porównaniu;
5. `Coalesced` i `SupersededByPendingNewerState` były raportowane jako
   `Ok(())`, więc caller zwiększał liczniki „emitted” dla eventu, który nigdy
   nie docierał do AccountStateCore.

Dodatkowe luki:

- sequence number był pobierany przed queue lockiem, więc dwa workery mogły
  umieścić normalne eventy jako `[N+1, N]`;
- `blocking_send` i bezwarunkowy `JoinHandle::join` mogły zawiesić shutdown;
- harness używał dokładnie tylu eventów, ile wynosiło capacity, i nie miał
  twardej bramki opóźnienia;
- opis PR zawierał historyczny workload i historyczne pomiary;
- czerwone checks nie miały formalnego porównania z bieżącym `main`.

## 2. Decyzja

### 2.1. PR1B transportuje wszystkie przyjęte AccountUpdate

State lane jest bounded FIFO:

```text
normal FIFO:         VecDeque<SeerEvent>
AccountUpdate FIFO:  VecDeque<SeerEvent>

enqueue linearization point:
  lock
  -> sprawdzenie capacity właściwej lane
  -> nadanie sequence_number
  -> push_back
  -> unlock

dispatcher:
  min(normal.front.sequence, account.front.sequence)
```

PR1B nie wykonuje:

- exact duplicate detection;
- older/newer selection;
- provider arbitration;
- same-version hash conflict classification;
- interpretacji `write_version=None`.

Każde `Ok(())` oznacza realne umieszczenie dokładnie jednego eventu w bounded
FIFO. Pełna kolejka zwraca typed `LocalProcessingGap`; nie istnieje wynik
coalescing udający emisję.

Konfiguracja nazywa się `account_update_queue_capacity`. Stara nazwa
`account_update_coalescing_capacity` pozostaje deserialize-only serde aliasem,
więc istniejące konfiguracje nadal się ładują bez utrzymywania błędnej
semantyki w nowym SSOT.

### 2.2. Globalny sequence ordering ma jeden punkt linearyzacji

Sequence counter należy do `IpcEgressState` i jest modyfikowany pod tym samym
mutexem co enqueue. Dzięki temu nie istnieje okno:

```text
worker A alloc N
worker B alloc N+1
worker B enqueue
worker A enqueue
```

Oba FIFO są lokalnie ordered, a dispatcher porównuje ich fronty. Test z 64
równoległymi producentami, obejmujący obie lane, wymaga odbioru dokładnie
`0..63`.

### 2.3. IPC shutdown nie wykonuje nieprzerywalnego blocking_send

Stały dispatcher używa `tokio::mpsc::Sender::try_send`:

```text
Ok       -> następny event
Full     -> zachowaj pending event, krótki retry poza hot path
Closed   -> typed delivery failure
deadline -> typed ShutdownTimeout
```

`IpcSender::shutdown_and_join(timeout)`:

1. zatrzymuje przyjmowanie;
2. ustawia deadline;
3. czeka tylko do deadline na zakończenie fixed worker;
4. joinuje wyłącznie zakończony worker;
5. przy timeout odłącza handle i zwraca `IpcError::ShutdownTimeout`.

Pozostałe dispatchery również otrzymują deadline:

- Tokio raw-evidence writer jest abortowany po timeout;
- WAL OS writer zwraca typed timeout i oznacza segment niewiarygodny;
- local-gap audit OS writer zwraca typed timeout;
- `Seer::shutdown_dispatchers` dzieli jeden czterosekundowy budżet;
- launcher nakłada niezależny pięciosekundowy outer timeout.

OS thread zablokowany wewnątrz filesystem I/O nie może zostać bezpiecznie
zabity przez Rust. Kontrakt nie twierdzi więc, że I/O zostaje przerwane; twierdzi
i testuje, że launcher nie czeka bez końca i raportuje brak pełnego drain/flush.

### 2.4. Capacity gate mierzy niezależny workload i dwell SLA

Frozen operational workload:

```text
input events              = 3,072
batches                   = 24 × 128
batch interval            = 50 ms
configured capacity       = 2,048
```

Workload count jest celowo różny od capacity. Release gate wymaga:

```text
missing_events            = 0
queue dwell p99           <= 250,000,000 ns
oldest event age          <= 500,000,000 ns
```

Pomiar z 2026-07-25:

```text
peak batch ingress        = 73,873.871 events/s
operational ingress       = 2,535.427 events/s
sustained drain           = 2,442.683 events/s
queue high-water          = 134
queue dwell p99           = 47,209,510 ns
oldest event age          = 54,277,899 ns
missing                   = 0
```

To jest bramka dla jawnego, zamrożonego profilu, nie uniwersalna deklaracja
capacity dla dowolnego ruchu Yellowstone.

### 2.5. Zdalne CI jest sklasyfikowane przez porównanie, nie założenie

Dowód:

```text
origin/main head          = ea7d31a228f8db0b7ed0779dea70b696895e66c2
PR merge-base             = ea7d31a228f8db0b7ed0779dea70b696895e66c2
main Restore run          = 30119079249, failure E0063
PR Restore run            = 30141277590, failure E0063
PR Metric Contracts run   = 30141277591, failure E0063
```

Trzy fixture’y wskazane przez PR checks nie mają diffu względem `origin/main`.
`main` ma tę samą klasę niekompletnych initializerów `PoolTransaction`.
Wniosek jest ograniczony: checks są nadal czerwone, ale failure signature jest
formalnie baseline tej samej klasy i nie została wprowadzona przez PR1B.

## 3. Zachowane inwarianty

- wszystkie kolejki są bounded;
- ingest worker nie czeka na downstream IPC capacity;
- brak per-event task spawning;
- każde `Ok(())` oznacza realnie przyjęty event;
- każde distinct AccountUpdate pozostaje dostępne dla PR1C;
- `None` nie jest interpretowane jako wersja zero;
- sequence ordering jest deterministyczne między lane;
- local saturation pozostaje typed, trwałe i fail-closed;
- diagnostic evidence nie zatrzymuje canonical runtime;
- required evidence nadal unieważnia segment;
- stara konfiguracja pozostaje czytelna przez serde alias;
- nie zmieniono AccountStateCore arbitration, MFS, Gatekeepera, strategii,
  quote math, execution ani shadow/live authority.

## 4. Failure semantics

| Sytuacja | Wynik |
|---|---|
| AccountUpdate FIFO ma miejsce | `Ok(())`, event faktycznie zakolejkowany |
| AccountUpdate FIFO pełne | typed IPC local gap, brak emitted success |
| normal FIFO pełne | istniejąca polityka drop albo typed local gap |
| downstream zamknięty | shutdown delivery failure |
| downstream pełny podczas shutdown | bounded retry, potem `ShutdownTimeout` |
| WAL/evidence/audit nie kończy przed deadline | typed shutdown failure |
| workload przekracza dwell SLA | release harness FAIL |
| parity typed eventu zmienia się | digest gate FAIL |

## 5. Weryfikacja

Przeszły:

```text
cargo test -p seer ipc::tests --no-fail-fast
  27 passed

account_update_fifo_retains_same_version_conflicts_and_none_separately
concurrent_multi_lane_enqueue_is_globally_sequence_ordered
shutdown_is_bounded_when_downstream_stops_consuming
canonical_account_update_survives_full_downstream_and_arrives_once

cargo test -p seer pr1b_hot_path_harness --release -- --ignored --nocapture
  PASS
  parity digest =
  549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202
  dwell SLA PASS
```

Finalny zestaw gates i wynik workspace build są utrzymywane w:

```text
PLANS/DO_REALIZACJI/
BASELINE_RECEIPT_INGEST_HOT_PATH_PR1B_EA7D31A_20260724.md
```

## 6. Pliki i compatibility

Runtime/test:

- `off-chain/components/seer/src/ipc.rs`;
- `off-chain/components/seer/src/lib.rs`;
- `off-chain/components/seer/src/local_gap.rs`;
- `off-chain/components/seer/src/hot_path_harness.rs`;
- `ghost-launcher/src/components/seer.rs`;
- IPC config literals w example i source-router test.

Dokumentacja:

- plan wykonawczy PR1B;
- baseline/final receipt;
- wcześniejsze ADR-8D PR1B i review closure;
- niniejszy ADR-8D.

Rollback musi cofnąć FIFO, atomic sequence, bounded shutdown i ich testy razem.
Nie wolno przywrócić coalescingu jako transportowej optymalizacji przed PR1C.
