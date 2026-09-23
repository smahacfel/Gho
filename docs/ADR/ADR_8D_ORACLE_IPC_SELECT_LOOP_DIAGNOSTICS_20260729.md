# ADR-8D — diagnostyka powrotu konsumenta IPC do `recv()`

**Data:** 2026-07-29
**Status:** ACCEPTED — diagnostic-only
**Zakres:** pętla `tokio::select!` launcherowego konsumenta `IpcReceiver`
**Poza zakresem:** capacity, backpressure, fail-close, priority `select!`,
CandidateIntegrity, ACE probe, Brain, Gatekeeper, Trigger, PR2 i Day 1

## 1. Problem

Poprzednia instrumentacja udowodniła, że suma czasów handlerów po granicy
`IpcReceiver` nie wyjaśnia saturacji egressu. Nie rozróżniała jednak okresów
między końcem jednego eventu a następnym `recv()`, w szczególności pracy
wykonywanej przez gałąź `prune_interval`, control plane local coverage gap albo
opóźnienia całego runtime Tokio.

## 2. Decyzja

Dodajemy wyłącznie obserwowalność do istniejącej pętli:

```text
tokio::select!
├── ipc_recv
├── prune_interval
└── local_coverage_gap_control
```

1. Każdy wybór gałęzi zwiększa licznik `select`.
2. Gałąź `prune_interval` zapisuje invocation count, total/max/histogram czasu
   oraz liczbę elementów dla:
   - `session_trade_bridge::prune_expired`;
   - `session_account_update_bridge::prune_expired`;
   - `finalize_pump_observation_ledger`;
   - `emit_finalized_decisions`;
   - całej gałęzi `total`.
3. Guard kończący obsługę eventu zapamiętuje moment zwrotu handlera. Następny
   `ipc_recv` przy niezerowym downstream albo egress backlog zapisuje gap i,
   wyłącznie powyżej 5 ms, rate-limited marker `IPC_CONSUMER_RECV_GAP`.
4. Niezależny task z intervalem 10 ms zapisuje histogram scheduling lag bez
   logowania pojedynczych ticków i jest jawnie kończony przy shutdownie
   komponentu.
5. `IpcReceiver` ma read-only snapshot egress occupancy wyłącznie dla markera
   recv-gap. Nie bierze udziału w dequeue, enqueue ani kontroli przepływu.
6. Operatorowy watcher szuka `IPC_EGRESS_SATURATED` tylko w
   `launcher.stdout.log`, robi snapshot `/metrics` i wysyła kontrolowany
   `SIGINT` natychmiast po markerze lub po limicie 10 minut.

## 3. Nowe evidence surface

```text
oracle_runtime_ipc_select_branch_selected_total{branch}
oracle_runtime_ipc_prune_invocations_total
oracle_runtime_ipc_prune_stage_duration_us{stage}
oracle_runtime_ipc_prune_stage_total_duration_us{stage}
oracle_runtime_ipc_prune_stage_max_duration_us{stage}
oracle_runtime_ipc_prune_items_total{stage}
oracle_runtime_ipc_recv_gap_with_backlog_us{last_selected_branch}
oracle_runtime_ipc_recv_gap_with_backlog_max_us{last_selected_branch}
oracle_runtime_scheduling_lag_us
```

P99 jest odczytywany jako estymata histogramowa z jawnie stałych bucketów.

## 4. Niezmienione inwarianty

- nie ma `biased`, `MissedTickBehavior::Skip`, nowej kolejki ani osobnego
  finalizera;
- kolejność i gotowość gałęzi `select!` pozostają niezmienione;
- wszystkie istniejące close, capacity i backpressure pozostają fail-closed;
- refaktoryzacja finalizacji rozdziela wyłącznie pomiar ledger finalization od
  emisji istniejących decyzji, zachowując ich kolejność i zakres;
- watcher nie modyfikuje runtime i używa wyłącznie nowego diagnostic run ID;
- Day 1 nie jest uruchamiany ani autoryzowany przez ten ADR.

## 5. Weryfikacja

```text
cargo fmt --all --check
cargo test -p ghost-launcher oracle_runtime_ipc_ --lib -- --nocapture
cargo test -p seer downstream_full_episode_survives_one_slot_delivery_with_egress_backlog --lib -- --nocapture
python3 scripts/test_ace_core_one_day_diagnostic_watch.py
cargo build --release -p ghost-launcher --bin ghost-launcher
```

Potem wykonuje się dokładnie jeden nowy diagnostic run, shadow/observe-only,
maksymalnie 10 minut albo do pierwszego `IPC_EGRESS_SATURATED`. Po nim raport
może wybrać wyłącznie jedną z klasyfikacji z zadania, bez optymalizacji.
