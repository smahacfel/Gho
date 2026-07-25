# PR1B baseline and final receipt — ingest hot path

**Baseline date:** 2026-07-24
**Review correction date:** 2026-07-25
**Repository:** `/root/Gho_ingest` (`smahacfel/Gho`)
**Clean merge parent:** `ea7d31a228f8db0b7ed0779dea70b696895e66c2`
**B0 harness commit:** `5136319`
**PR branch:** `agent/ingest-single-pass-pr1b-20260724`

## 1. Provenance

- `origin/main` przed utworzeniem gałęzi wskazywał
  `ea7d31a228f8db0b7ed0779dea70b696895e66c2`.
- B0 jest addytywnym harness commit:
  `test(ingest): add deterministic hot-path performance harness`.
- Runtime PR1B jest rozdzielony na B1 single-pass i B2 bounded sink isolation.
- Korekta 2026-07-25 adresuje blokery review PR #83 bez zmiany authority,
  strategii, Gatekeepera, quote math ani execution.

## 2. Baseline call graph i root cause

```text
Yellowstone SubscribeUpdateTransaction (decoded)
  -> prost encode do wewnętrznego Vec<u8>                 encode #1
  -> fast queue -> overflow queue -> blocking send
  -> prost decode w normalizatorze                       decode #1
  -> synchronous WAL clone + append
  -> evidence Base58/String/JSON/hash na event workerze
  -> parse initialize pool
       prost decode + pełny outer/inner scan              decode #2, scan #1
  -> parse trades
       prost decode + pełny outer/inner scan              decode #3, scan #2
  -> IPC send().await / Block
```

Root cause self-generated backlog:

1. zakodowanie już zdekodowanej transakcji jako transportu live;
2. trzy application-level decode na transakcję;
3. dwa pełne skany tego samego drzewa instrukcji;
4. fizyczny WAL i evidence serialization w event workerze;
5. oczekiwanie na downstream IPC w workerach;
6. blokujące zakończenie overflow ingress.

## 3. Zamrożony workload

Harness konstruuje deterministyczne protobuf fixtures i obejmuje:

- ordinary Pump.fun BUY;
- ordinary Pump.fun SELL;
- CREATE + initial BUY;
- dwie Pump mutations w jednej signature;
- PumpSwap inner-instruction trade;
- `tx_index=Some(0)`;
- provider provenance i event ordinal;
- AccountUpdate;
- 3,072-eventowy operational microburst replay (24 × 128, co 50 ms);
- slow WAL;
- slow IPC;
- capacity-two saturation episode.

## 4. Korekta parity gate

Pierwotny digest:

```text
062d36ab094fb470909fd9836318fee85d89dbed8f1a9a86080041f20a399ee2
```

obejmował jedynie strukturalny podzbiór pól. Nie jest już używany jako dowód
pełnej parity.

Autorytatywny `CanonicalParserParitySnapshotV1` zawiera pełne typed
`InitializePoolEvent` i `TradeEvent`. Normalizuje wyłącznie lokalne,
niedeterministyczne timestampy ingress. Ten sam snapshot uruchomiony na B0
`5136319` i na finalnym PR1B daje:

```text
549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202
```

Obejmuje m.in.:

- kwoty i limity instrukcji;
- virtual i real reserves;
- success/error i complete;
- signer;
- route/buy variant;
- fee/account metadata obecne w typed eventach;
- token deltas;
- provider, ordinal i pełne instruction provenance.

Negatywny test zmienia pole semantyczne i wymaga innego digestu.

## 5. Finalny call graph

```text
decoded SubscribeUpdateTransaction
  -> configurable bounded ingress FIFO / try_send
  -> direct normalization from decoded fields
  -> optional shared capture after ingress
       capture off      -> 0 prost encode
       capture required -> 1 prost encode
  -> parse_transaction_bundle
       one outer + inner scan
       one dedupe/provenance/ordinal pass
       PoolDetected before all Trade events
  -> bounded nonblocking sink handoff
       normal WAL queue -> fixed writer
       evidence queue   -> fixed writer task
       normal IPC lane  -> fixed dispatcher
       AccountUpdate    -> bounded FIFO wszystkich przyjętych obserwacji
       all local gaps   -> reserved audit router -> dedicated WAL writer
```

Operation counts dla pięciu transakcji:

| Operacja | B0 | Final capture off | Final capture required |
|---|---:|---:|---:|
| Application prost encode | 5 | 0 | 5 |
| Normalizer prost decode | 5 | 0 | 0 |
| Parser prost decode | 10 | 0 | 0 |
| Full outer/inner scan | 10 | 5 | 5 |

## 6. Capacity receipt po review

Pierwotna wartość `1,024`, wyliczona jako średni throughput razy 250 ms, była
nieprawidłowa i została usunięta.

Nowy harness uruchamia producenta i rzeczywistego konsumenta równolegle.
Konsument wykonuje direct normalization i `parse_transaction_bundle`.

Finalny release result na tym hoście:

| Metric | Final PR1B |
|---|---:|
| Input | 3,072 events |
| Batch profile | 24 × 128 events, 50 ms interval |
| Configured ingress capacity | 2,048 |
| Peak batch ingress | 73,873.871 events/s |
| Operational ingress | 2,535.427 events/s |
| Sustained drain | 2,442.683 events/s |
| Queue high-water | 134 |
| Queue dwell p99 / SLA | 47,209,510 / 250,000,000 ns — PASS |
| Oldest queued event / SLA | 54,277,899 / 500,000,000 ns — PASS |
| Missing events | 0 |
| Replay parser throughput | 2,476.122 events/s |
| receive-to-normalize p50 | 20,632 ns |
| receive-to-normalize p95 | 33,016 ns |
| receive-to-normalize p99 | 47,886 ns |
| normalize-to-bundle p50 | 397,400 ns |
| normalize-to-bundle p95 | 484,697 ns |
| normalize-to-bundle p99 | 520,419 ns |
| Steady-state RSS | 13,716 KiB |

Domyślna `ingress_queue_capacity = 2_048` jest serde-compatible i
konfigurowalna. Workload ma celowo inną liczebność niż capacity, a bramka
sprawdza opóźnienie, nie tylko brak utraty:

```text
workload                    = 3,072
configured capacity         = 2,048
observed backlog high-water = 134
queue dwell p99             = 47,209,510 ns <= 250,000,000 ns
oldest event age            = 54,277,899 ns <= 500,000,000 ns
missing                     = 0
```

To jest dowód dla tego zamrożonego replay, nie uniwersalna produkcyjna
gwarancja Yellowstone. Shadow/captured workload może ustawić większą lub
mniejszą wartość bez zmiany kodu.

## 7. Slow sink receipt

```text
slow IPC hot-path enqueue       = 7,074 ns
IPC hot-path blocking waits     = 0

slow WAL hot-path enqueue       = 621,944 ns
physical WAL writer elapsed     = 7,148,472 ns
physical writer calls/waits     = 2/2
event-worker WAL blocking waits = 0
```

## 8. AccountUpdate preservation

`AccountUpdate` nie konkuruje już o pełną normalną kolejkę business IPC.
Dedicated bounded state lane:

- jest FIFO wszystkich przyjętych obserwacji;
- nie wykonuje deduplikacji ani freshness arbitration;
- zachowuje osobno `write_version=None` i `Some(0)`;
- zachowuje same-version/different-hash observations dla PR1C;
- nadaje sequence atomowo pod tym samym lockiem co enqueue;
- scala się z normalnym FIFO według globalnego sequence number;
- nie czeka na downstream capacity w hot path;
- jest bounded przez `account_update_queue_capacity` (stara nazwa
  `account_update_coalescing_capacity` pozostaje deserialize aliasem).

Obowiązkowy test:

```text
downstream capacity = 1 i jest pełne
-> AccountUpdate enqueue kończy się bez oczekiwania
-> state pozostaje w dedicated lane
-> po zwolnieniu downstream dociera dokładnie raz

ta sama krzywa/slot/write_version
-> None, Some(0), hash A i hash B pozostają osobnymi obserwacjami

64 równoległych producentów, obie lane
-> odbiór sequence_number = 0..63
```

## 9. Durable local-gap receipt

`LocalCoverageGapV1` przechowuje:

```text
missing_event_count
first_dropped
last_dropped
before
after
queue_high_water
reason
provider_id
stream_epoch
episode_sequence
```

Capacity-two test:

```text
receiver blocked            = false
blocking wait               = 0 ns
missing_event_count         = 2
first_dropped.slot          = 2
last_dropped.slot           = 3
continuous gaps emitted     = 1
silent success              = 0
recovered                   = false
```

Ingress, WAL, evidence i IPC używają tego samego centralnego audit routera.
Rezerwowana bounded kolejka oraz dedicated WAL writer nie zależą od normalnej
kolejki `WalJob`. Test replay odczytuje wszystkie cztery reason codes.
Jeżeli wystąpił gap, ale run nie ma skonfigurowanego audit WAL dispatchera,
nieutrwalony marker wymusza błąd shutdownu; brak trwałego sinka nie może zostać
uznany za udany audyt.

## 10. Artifact contract

```text
artifact_capture_enabled
artifact_required_for_run
```

- diagnostyczny evidence (`required=false`) emituje typed gap, ale nie ustawia
  globalnego `local_segment_unreliable`;
- wymagany evidence (`required=true`) fail-closes segment;
- `required=true` bez enabled albo bez capture dir jest odrzucany przy starcie;
- stare configi defaultują `required=false`.

## 11. Controlled shutdown receipt

WAL, evidence, IPC i audit dispatchery realizują:

```text
stop accepting
-> drain accepted
-> final flush
-> join
-> return success/failure
```

WAL test:

```text
enqueue 32 records
-> shutdown_and_join
-> replay WAL
-> exactly 32 records
-> slots ordered 10,000..10,031
```

Launcher zatrzymuje najpierw producenta Seer, potem dispatchery. IPC receiver
pozostaje żywy do końca egress drain. Timeout, panic, append failure, final
flush failure lub early downstream close stają się błędem shutdownu.
Każdy dispatcher otrzymuje pozostały budżet ze wspólnego czterosekundowego
deadline’u Seer; launcher ma niezależny pięciosekundowy outer timeout.
IPC nie używa już `blocking_send`: pełny downstream jest obsługiwany przez
`try_send` + bounded retry do deadline’u. Test zatrzymanego konsumenta kończy
się `IpcError::ShutdownTimeout` w zadanym budżecie.

## 12. Verification

Targeted gates wykonane podczas korekty:

```text
cargo fmt --all --check
git diff --check
cargo test -p ghost-core ingest_integrity -- --nocapture
cargo test -p seer --lib canonical_account_update_survives_full_downstream_and_arrives_once -- --nocapture
cargo test -p seer --lib account_update_fifo_retains_same_version_conflicts_and_none_separately -- --nocapture
cargo test -p seer --lib concurrent_multi_lane_enqueue_is_globally_sequence_ordered -- --nocapture
cargo test -p seer --lib shutdown_is_bounded_when_downstream_stops_consuming -- --nocapture
cargo test -p seer --lib reserved_audit_lane_persists_every_local_gap_domain -- --nocapture
cargo test -p seer --lib diagnostic_evidence_saturation_does_not_block_canonical_runtime -- --nocapture
cargo test -p seer --lib required_evidence_saturation_invalidates_run_segment -- --nocapture
cargo test -p seer --lib wal_dispatcher_shutdown_drains_flushes_and_joins_all_accepted_jobs -- --nocapture
cargo test -p seer --lib raw_evidence_shutdown_drains_and_final_flushes_accepted_event -- --nocapture
cargo test -p seer --lib shutdown_fails_if_a_gap_cannot_reach_a_configured_audit_wal -- --nocapture
cargo test -p seer --lib canonical_parity_snapshot_detects_economic_and_state_drift -- --nocapture
cargo test -p ghost-launcher --lib local_coverage_gap_replays_as_audit_only_record -- --nocapture
cargo check -p ghost-brain --tests
cargo test --release -p seer --lib pr1b_hot_path_harness -- --ignored --nocapture --test-threads=1
timeout 900s cargo build --release --workspace
```

Wszystkie powyższe scoped gates przeszły, w tym finalny release workspace
build. Harness potwierdził pełny digest
`549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202`,
`missing_event_count=0` dla workloadu `3,072` przy capacity `2,048`, queue
dwell p99 `47,209,510 ns <= 250 ms`, oldest `54,277,899 ns <= 500 ms` oraz
jawny dwueventowy gap w próbie saturacji.

Pełne test suites zachowują istniejący poza zakresem failure signature:

- `timeout 300s cargo test -p seer --lib -- --test-threads=1` odtwarza te same
  14 PumpPortal/Seer failures co B0, po czym dochodzi do istniejącego zawieszenia
  `test_ultrafast_mode_keeps_forwarding_trades` i kończy się kodem `124`;
- `cargo check -p ghost-launcher --tests` zatrzymuje się na istniejących,
  nietkniętych fixture’ach
  `cpv_successful_buy_contract_tests.rs` i `tx_intelligence_tests.rs`,
  których initializery `PoolTransaction` nie zawierają pięciu pól (`E0063`);
- `timeout 600s cargo test --workspace -- --test-threads=1` zatrzymuje się na
  tej samej klasie `E0063` w nietkniętym
  `ghost-launcher/tests/oracle_continuous_sampling.rs:34`.

Korekta PR1B nie rozszerza scope’u o naprawę tych fixture’ów.

Zdalne workflowy zostały porównane z bieżącym `main`, a nie tylko opisane jako
„znany baseline”:

- `main` head i merge-base PR: `ea7d31a228f8db0b7ed0779dea70b696895e66c2`;
- Restore Lifecycle Guard na `main`, run `30119079249`: failure `E0063` w
  nietkniętym initializerze `PoolTransaction`;
- PR run `30141277590`: ta sama klasa `E0063` w innych nietkniętych fixture’ach
  testowych;
- PR Metric Contracts run `30141277591`: ta sama klasa `E0063`;
- `git diff origin/main...HEAD` dla trzech wskazanych przez CI fixture’ów jest
  pusty.

To formalnie klasyfikuje czerwone checks jako istniejący baseline tej samej
klasy kompilacji, nie jako zielony wynik PR.

## 13. Scope boundary

PR1B zmienia wyłącznie:

- transport i parser work ownership;
- bounded queue scheduling;
- canonical state egress preservation;
- lokalny gap audit;
- dispatcher lifecycle;
- parity/capacity harness;
- addytywne serde-default pola konfiguracji.

PR1B nie zmienia:

- AccountStateCore arbitration (PR1C);
- raw/NLN reconciliation (PR1D);
- MFS;
- Gatekeepera;
- strategy/scoring;
- thresholds;
- quote math;
- execution;
- shadow/live authority.
