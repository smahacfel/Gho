# ADR-8D — Seer: owned BCV2 hydration i upstream drain przed shutdownem Oracle

**Data:** 2026-07-30
**Status:** IMPLEMENTED LOCALLY / QUALIFYING SMOKE PENDING / DAY 1 NO-GO DO SMOKE PASS
**Zakres:** `off-chain/components/seer`, `ghost-launcher`; wyłącznie kolejność i ownership shutdownu.

## D0. Problem

Diagnostic regression run zachował zdrowy transport podczas pracy, ale przy SIGINT
ujawnił późny producer:

```text
rpc_hydration
→ IPC egress dispatcher disconnected
→ receiver drain timeout
```

Przyczyną nie była capacity ani hot path. Launcher równocześnie sygnalizował Seer
i OracleRuntime, a BCV2 hydration worker był uruchamiany odłączonym `Handle::spawn`
bez przechowywanego `JoinHandle`. `Seer::request_shutdown()` zatrzymywał połączenia
gRPC, lecz nie ten worker; mógł on wysłać evidence po rozpoczęciu shutdownu IPC.

## D1. Decyzja

### Owned hydration

`Bcv2HydrationService` ma jeden współdzielony lifecycle:

```text
accepting = true
→ owned sender/receiver intake
→ stored JoinHandle worker
```

Podczas `shutdown_and_join()` service:

1. atomowo przestaje akceptować nowe requesty;
2. zamyka sender intake — to jest sygnał shutdown dla workera;
3. pozwala workerowi dokończyć request w locie i wszystkie requesty już zaakceptowane;
4. joinuje worker w bounded timeout;
5. dopiero po tym Seer może wywołać `IpcSender::shutdown_and_join()`.

Request obserwowany po punkcie `accepting=false` jest lokalnie odrzucony bez nowego
RPC evidence. Nie ma `PoisonError::into_inner`, resetu ani nowej kolejki.

### Dwufazowy shutdown launchera

Kanały Seer i OracleRuntime są rozdzielone:

```text
SIGINT
→ signal Seer
→ Seer core producers stop
→ BCV2 hydration drain + join
→ IPC dispatcher drain + receiver drain
→ SEER_IPC_DRAIN_COMPLETE
→ signal OracleRuntime
→ OracleRuntime stops
→ generic remaining components stop
```

Seer task zwraca teraz własny `Result`; launcher nie zamienia błędu komponentu na
sam log i sukces procesu. Każdy błąd/timeout shutdownu powoduje końcowy błąd launchera.

## D2. Terminalny kontrakt drainu

Po poprawnym joinie launcherowego IPC receivera Seer emituje dokładnie jeden marker:

```text
SEER_IPC_DRAIN_COMPLETE
events_sent
events_received
egress_backlog
downstream_backlog
```

Marker jest emitowany wyłącznie przy:

```text
events_sent == events_received
egress_backlog == 0
downstream_backlog == 0
```

Inny stan zwraca błąd komponentu i nie może zostać przedstawiony jako kontrolowany
shutdown.

## D3. Niezmienione granice

Nie zmieniono:

- IPC capacity, backpressure, fail-close ani priorytetów `tokio::select!`;
- CandidateIntegrity, receipt reclaim/cadence fixes, Gatekeepera, Brain, ACE probe'a,
  EventWritera, Triggera, Position Managera ani PR2;
- shadow-only / observe-only execution boundary;
- semantyki runtime events w stanie normalnej pracy.

## D4. Focused tests

Dodano dwa testy `Bcv2HydrationService`:

1. request zaakceptowany przed shutdownem jest wyemitowany do IPC i worker jest
   zjoinowany przed shutdownem IPC;
2. request po rozpoczęciu owned shutdownu nie emituje evidence.

Istniejący test shutdownu komponentu Seer pozostaje wymagany. Pełny release build oraz
fresh qualifying smoke są nadal bramką operacyjną.

## D5. Bramka smoke

Qualifying smoke 3–5 minut, świeży `run_id` i ścieżki, po SIGINT musi wykazać:

```text
IPC_EGRESS_SATURATED = 0
primary_coverage_gap = 0
candidate_admission_closed = 0
terminal_retirement_failed = 0
terminal_candidate_receipt_reclaim_failed = 0
BCV2_*_SEND_FAILED during shutdown = 0
SEER_IPC_DRAIN_COMPLETE present with exact equality/zero backlogs
launcher/component shutdown errors = 0
VALID_CAPTURE = true
```

Przed takim wynikiem Day 1 pozostaje NO-GO.
