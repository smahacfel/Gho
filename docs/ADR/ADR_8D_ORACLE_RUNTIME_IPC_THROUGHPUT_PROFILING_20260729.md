# ADR-8D — diagnostyka przepustowości `IpcReceiver → OracleRuntime`

**Data:** 2026-07-29
**Status:** ACCEPTED — diagnostic-only
**Zakres:** launcherowy most IPC Seera przed Event Busem / `OracleRuntime`
**Poza zakresem:** capacity, backpressure, fail-close, CandidateIntegrity, ACE probe, Brain, Gatekeeper, Trigger, PR2 oraz Day 1

## 1. Kontekst

Diagnostyczny smoke odtworzył saturację normalnej kolejki Seer IPC. W chwili
awarii downstream o pojemności 10 000 był pełny, normalny egress miał
10 000/10 000, a `events_sent - events_received` odpowiadało dokładnie pracy
oczekującej w obu kolejkach oraz pojedynczemu pending eventowi dispatchera.

Dotychczasowy zapis nie pozwalał jednak rozstrzygnąć, który handler po stronie
launcherowego konsumenta ogranicza przepustowość ani czy czas jest skutkiem
jednego handlera, sumy handlerów, lock contention, I/O/logowania, CPU czy
schedulingu.

## 2. Decyzja

Dodajemy obserwowalność wyłącznie na rzeczywistej granicy konsumenta:

```text
IpcReceiver::recv
→ launcherowy IPC bridge w components/seer.rs
→ Event Bus
→ OracleRuntime
```

Każdy odebrany event jest objęty guardem RAII, który także na fail-closed
`continue` zapisuje:

- całkowity czas obsługi w mikrosekundach, według `event_kind`;
- czas głównych etapów handlera;
- końcowy `handler_stage`;
- backlog bounded downstream przed i po obsłudze;
- licznik obsłużonych eventów.

Przekroczenie 5 ms może zapisać jeden na sekundę marker
`ORACLE_RUNTIME_SLOW_EVENT`. Marker zawiera event kind, czas, final stage,
aktualny backlog oraz pool/signature tylko tam, gdzie payload już je posiada.

Naprawiamy też wyłącznie diagnostyczny limiter `IPC_DOWNSTREAM_FULL`: pojedyncze
udane `try_send` nie zamyka epizodu, jeżeli egress nadal ma backlog. Epizod
resetuje się dopiero po opróżnieniu egressu.

## 3. Metryki

```text
oracle_runtime_ipc_event_processing_duration_us{event_kind}
oracle_runtime_ipc_handler_stage_duration_us{event_kind,handler_stage}
oracle_runtime_ipc_events_handled_total{event_kind,handler_stage}
oracle_runtime_ipc_downstream_backlog_before{event_kind}
oracle_runtime_ipc_downstream_backlog_after{event_kind}
```

Histogramy mają stałe, mikrosekundowe buckety pozwalające raportować p50/p95/p99
oraz sumę czasu i udział każdego rodzaju eventu. Metryki nie sterują runtime’em.

## 4. Niezmienione kontrakty

- kolejki zachowują istniejącą pojemność i kolejność;
- backpressure oraz primary local-gap fail-close nie zmieniają semantyki;
- CandidateIntegrity nie jest dotykany;
- guard nie wykonuje await, RPC, I/O ani mutacji decyzji;
- Event Bus, OracleRuntime, Gatekeeper i shadow/live boundary nie zmieniają
  zachowania;
- Day 1 nie jest uruchamiany przez tę zmianę.

## 5. Weryfikacja

- focused test rejestracji metryk i limiteru slow-event;
- focused test epizodu `IPC_DOWNSTREAM_FULL` przez pojedynczy udany send;
- `cargo fmt --all --check`;
- release build `ghost-launcher`;
- świeży run diagnostyczny maksymalnie 10 minut albo do pierwszej saturacji,
  ze snapshotem `/metrics` przed kontrolowanym shutdownem.

## 6. Kryterium zakończenia

Run kończy się jedną klasyfikacją źródła wolniejszego konsumenta:

```text
SINGLE_HANDLER_DOMINATES
MULTIPLE_HANDLERS_CUMULATIVE
LOCK_CONTENTION
SYNC_IO_OR_LOGGING_BOUND
CPU_COMPUTE_BOUND
CONSUMER_SCHEDULING_STARVATION
INSUFFICIENT_OBSERVABILITY
```

W tym ADR nie autoryzuje się optymalizacji ani modyfikacji sterowania.
