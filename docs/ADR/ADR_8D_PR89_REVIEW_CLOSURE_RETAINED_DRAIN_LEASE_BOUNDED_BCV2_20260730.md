# ADR-8D: PR #89 review closure — retained drain ack, pre-ledger lease i bounded BCV2 evidence

**Data:** 2026-07-30
**Status:** IMPLEMENTED LOCALLY / FOCUSED VALIDATION COMPLETE / DRAFT REVIEW REQUIRED / QUALIFYING SMOKE HOLD / DAY 1 NO-GO
**Zakres:** wyłącznie trzy potwierdzone blokery z review stacked PR #89.

## D0. Kontekst

Review PR #89 potwierdził trzy luki, których wcześniejsza implementacja nie
zamykała wystarczająco:

1. `SeerOracleDrainBarrierV1` używał `AtomicBool` i `Notify`, co dopuszczało
   lost wakeup między odczytem `false` a rejestracją waitera;
2. terminalny barrier CandidateIntegrity blokował receipt staging, ale był
   instalowany po `PumpObservationLedger::observe`, więc późny canonical
   mutation mógł zostać zapisany w Ledgerze bez późniejszego handoffu;
3. normalne BCV2 `queue_full` i `worker_closed` tworzyły po jednym Tokio tasku
   dla każdego typed `RpcMissing`, a liczba takich tasków nie była ograniczona.

Nie uruchomiono smoke'a ani Dnia 1 w ramach tej decyzji.

## D1. Retained Oracle drain acknowledgement

`SeerOracleDrainBarrierV1` używa teraz `tokio::sync::watch<bool>`. Stan
acknowledgement jest zachowywany przez kanał, więc kolejność:

```text
waiter observes false
→ Oracle sets true
→ waiter arms wait
```

nie może już stracić powiadomienia. `wait_for_oracle_processed()` obserwuje
aktualną wartość odbiornika i czeka wyłącznie, gdy nadal jest `false`.

Focused test reprodukuje dokładnie wcześniejszy interleaving i wymaga, aby ack
opublikowany przed `changed().await` pozostał widoczny.

## D2. Pre-ledger, per-candidate terminal lease

Canonical ingest dla wyrównanej primary Pump mutation pobiera teraz typed
`CandidateCanonicalObservationLeaseV1` **przed** wywołaniem
`PumpObservationLedger::observe`.

```text
acquire lease
→ observe ledger
→ stage canonical receipt
→ publish synchronous integrity/inventory work and permit
→ release lease
```

Terminal cleanup ustawia kandydacki barrier pod tym samym lockiem registry.
Następnie czeka tylko na lease już wydane przed barrierem. Późniejsze ingesty
otrzymują `TerminalCleanupInProgress` przed mutacją Ledgera i są lokalnie
suppressed — bez permitu i bez globalnego zamknięcia admission.

Po zakończeniu lease terminal owner rozlicza pending receipt przez istniejące
`fail_canonical_apply`. Gdy terminal barrier jest aktywny, ta normalna ścieżka
może rozwiązać fence, ale nie może sama uprzedzić terminalnego record retirement:
jedynym właścicielem sekwencji reclaim → retirement → identity cleanup jest
`OracleRuntime::remove_pool_with_reason`. Zlikwidowano wcześniejszy drugi
reclaim w `result_rx`.

Deterministyczny test zatrzymuje ingest po lease acquisition, instaluje barrier
i uruchamia terminal cleanup równolegle, sprawdza, że drugi ingest nie dotyka
Ledgera, a pierwszy kończy receipt/permit przed reclaimem. Test kończy się
brakiem unresolved receiptów, terminalnym handoffem Ledgera i otwartym globalnym
admission.

## D3. Bounded BCV2 RpcMissing evidence

`queue_full` i `worker_closed` nie tworzą już tasku ani nie odkładają
`JoinHandle` do rosnącego `Vec`. Zamiast tego synchronnie wykonują jeden
nieblokujący `try_enqueue` do istniejącego bounded IPC egress FIFO:

```text
hydration queue full / worker closed
→ ExecutionAccountEvidence(RpcMissing)
→ bounded IPC try_enqueue
→ accepted, albo typed LocalProcessingGap
```

Brak miejsca w IPC nie jest cichym dropem: zwracany jest
`IpcError::LocalProcessingGap`, uruchamiający istniejącą fail-closed local
coverage gap control plane. Nie dodano kolejki, capacity, retry ani tasku per
failure. Normalna semantyka typed `RpcMissing` dla `queue_full` i
`worker_closed` pozostaje zachowana.

## D4. Niezmienione granice

Nie zmieniono:

- IPC capacity, polityki backpressure, priorytetów `tokio::select!` ani
  shutdown budgetów;
- Gatekeepera, Brain, ACE probe'a, EventWritera, Triggera, execution,
  Position Managera ani PR2;
- `retire_terminal_candidate()` jako safety fence;
- shadow-only / observe-only boundary ani configu rolloutowego.

## D5. Weryfikacja przed ponownym review

Wymagane focused checks dla tego commitu:

```bash
cargo fmt --all --check
cargo test -p ghost-launcher --lib seer_oracle_drain_barrier -- --nocapture
cargo test -p ghost-launcher --lib terminal_cleanup_lease_prevents_late_ledger_mutation_before_receipt_reclaim -- --nocapture
cargo test -p ghost-launcher --lib alias_conflict -- --nocapture
cargo test -p ghost-launcher --lib terminal_result_path_reclaims_staged_receipts_before_cleanup_and_blocks_late_apply -- --nocapture
cargo test -p seer --lib bcv2_hydration_ -- --nocapture
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

Po focused validation i ponownym review PR pozostaje Draft. Qualifying smoke,
zamrożenie provenance i Day 1 nie są częścią tej decyzji.
