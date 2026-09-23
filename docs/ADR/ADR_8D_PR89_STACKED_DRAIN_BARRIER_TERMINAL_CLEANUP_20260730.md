# ADR-8D: PR #89 stacked — Oracle drain barrier, terminal cleanup i owned BCV2 shutdown

**Data:** 2026-07-30
**Status:** IMPLEMENTED LOCALLY / FOCUSED VALIDATION IN PROGRESS / DRAFT REVIEW REQUIRED / QUALIFYING SMOKE NOT STARTED / DAY 1 NO-GO
**Zakres:** wyłącznie pięć potwierdzonych blockerów shutdownu i lifecycle’u dla ACE Core V3.

## D0. Kontekst i decyzja o zakresie

PR #89 jest stacked na `agent/ace-core-one-day-kill-test-v3`, pozostaje Draft i
nie jest już przedstawiany jako wąski diff względem `main`. Poprzedni marker
`SEER_IPC_DRAIN_COMPLETE` dowodził tylko opróżnienia lokalnego IPC Seera; nie
dowodził przetworzenia eventów już przekazanych do Event Busu przez
`OracleRuntime`.

Audyt potwierdził ponadto cztery niezależne problemy:

1. snapshot receiptów przed cleanupem nie linearyzował równoległego stagingu
   dla tego samego kandydata;
2. normalne `Full` i `Closed` BCV2 hydration enqueue nie wysyłały już typed
   `RpcMissing` do IPC;
3. wspólny czterosekundowy shutdown dispatcherów nie był wystarczającym
   budżetem dla kolejki hydration o pojemności 1 024;
4. launcher nie kończył fail-fast, gdy `seer_handle` zakończył się podczas
   normalnej pracy.

Ta decyzja naprawia tylko te pięć punktów. Nie uruchamia smoke’a ani Dnia 1.

## D1. Oracle end-to-end drain

Po lokalnym drainie IPC Seer emituje do istniejącego Event Busu jeden
`GhostEvent::SeerOracleDrainBarrier`. Jest on umieszczony po wszystkich
eventach przekazanych przez IPC receiver Seera. Oracle potwierdza go wyłącznie
po odebraniu go w swoim głównym receiverze Event Busu:

```text
Seer IPC receiver drained
→ Event Bus barrier
→ Oracle receives all earlier bus events in FIFO broadcast order
→ ORACLE_EVENT_BUS_DRAIN_COMPLETE
→ Seer returns success
→ launcher may signal Oracle shutdown
```

Seer czeka na ack maksymalnie pięć sekund. Brak subskrybenta, timeout albo
`broadcast::Lagged` nie tworzy fałszywego sukcesu: ten ostatni już globalnie
zamyka candidate admission i przez health gate unieważnia capture.

## D2. Atomowy terminal barrier per candidate

`fail_pending_canonical_applies_for_candidate()` pod jednym lockiem registry:

1. ustawia `terminal_cleanup_barriers[candidate]`;
2. pobiera snapshot nierozwiązanych receiptów;
3. zwalnia lock i rozlicza każdy receipt wyłącznie przez
   `fail_canonical_apply()`.

`stage_canonical_mutation()` sprawdza tę barierę pod tym samym lockiem i dla
późnej mutacji zwraca `TerminalCleanupInProgress`. Seer nie wystawia wtedy
permitu ani nie zamyka globalnego admission: jest to lokalna, terminalna
mutacja tego jednego kandydata. Bariera jest zwalniana dopiero po cleanupie
runtime identity i sprawdzeniu braku nierozwiązanych receiptów.

Każdy inny registry/fence error pozostaje istniejącym globalnym fail-close.
`retire_terminal_candidate()` nie został osłabiony, a receiptów nie usuwa się
force-delete.

## D3. BCV2 hydration — runtime semantics i realistyczny drain

Normalne błędy enqueue w aktywnym Tokio runtime:

```text
queue full   → typed ExecutionAccountEvidence(RpcMissing, queue_full)
worker closed → typed ExecutionAccountEvidence(RpcMissing, worker_closed)
```

są ponownie emitowane przez IPC. Ich taski są owned przez lifecycle hydration
i są joinowane przed shutdownem IPC, więc nie stają się późnym, odłączonym
producentem. `RejectedDuringShutdown` pozostaje bez nowego evidence, ponieważ
następuje po liniaryzacji `accepting=false`. Brak Tokio runtime nadal zapisuje
lokalne typed `RpcMissing`; nie można fizycznie uruchomić asynchronicznego IPC
send bez runtime’u, tak jak przed zmianą ownership.

Budżet hydration nie współdzieli już arbitralnych czterech sekund raw/WAL.
Jest wyprowadzony z aktualnego kontraktu retry:

```text
max request = 3 × 750 ms + (0 + 250 + 750) ms = 3 250 ms
accepted max = 1 in-flight + 1 024 queued = 1 025
hydration drain bound = 3 331 250 ms
```

Outer shutdown Seera ma osobny 3 600 s limit, a launcherowy join 3 660 s,
czyli zawiera calculated hydration bound oraz downstream margin. Jeśli owned
worker mimo tego nie zdrenuje się, shutdown kończy się błędem; nie abortuje się
go po czterech sekundach i nie raportuje czystego capture.

## D4. Fail-fast supervision

Główny `tokio::select!` launchera nadzoruje także `seer_handle`. Każde jego
zakończenie przed sygnałem shutdown — sukces, `Err` komponentu albo `JoinError`
— kończy launcher kodem `EXIT_SEER_COMPONENT_STOPPED`. Oracle nie może dalej
działać z cicho martwym canonical ingestem.

## D5. Granice niezmienione

Nie zmieniono:

- IPC capacity, backpressure, kolejki ani priorytetu `tokio::select!`;
- Gatekeepera, Brain, ACE probe’a, EventWritera, Triggera, execution,
  Position Managera ani PR2;
- `retire_terminal_candidate()` i jego fence semantics;
- shadow-only / observe-only boundary.

## D6. Weryfikacja przed review

Wymagane lokalne checks dla tego commitu:

```bash
cargo fmt --all --check
cargo test -p ghost-launcher terminal_cleanup_barrier_blocks_receipt_staging_between_reclaim_and_retirement --lib -- --nocapture
cargo test -p ghost-launcher seer_oracle_drain_barrier_acknowledges_only_after_bus_delivery --lib -- --nocapture
cargo test -p seer bcv2_hydration_ --lib -- --nocapture
cargo test -p ghost-launcher candidate_alias --lib -- --nocapture
cargo test -p ghost-launcher ace_core_one_day_probe --lib -- --nocapture
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

Po zielonym review dopuszczalny jest dokładnie jeden świeży qualifying smoke
3–5 minut. Day 1 pozostaje zakazany do `finalize=0`, `VALID_CAPTURE`,
`verify-probe=0`, zerowych trzech PR1E counters oraz obecnego end-to-end drain
markera.
