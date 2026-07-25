# ADR-8D — PR1B: single-pass ingest i nieblokujące granice sinków

**Data:** 2026-07-24; korekta po review: 2026-07-25
**Status:** Implemented, Draft PR — skorygowano blokery trwałości PR #83
**Zakres:** Yellowstone live ingest → normalizacja → Pump parser → WAL/evidence → IPC
**Poza zakresem:** authority, strategia, MFS, Gatekeeper, quote math, execution, PR1C, PR1D

> Globalny szablon wskazany w instrukcji (`/Gho/docs/ADR/ADR_8D_SZABLON.md`)
> nie istnieje w tym środowisku. Dokument zachowuje strukturę i poziom dowodu
> sąsiednich ADR-8D w `docs/ADR/`.

## 1. Problem

Aktywna ścieżka live wykonywała pracę, która sama generowała backlog:

- zdekodowana przez Yellowstone transakcja była ponownie prost-encode’owana
  jako wewnętrzny transport;
- normalizator dekodował ją ponownie;
- osobne wejścia CREATE i TRADE ponownie dekodowały i dwukrotnie skanowały to
  samo drzewo outer + inner instructions;
- event worker wykonywał fizyczny WAL append i klonował raw payload;
- raw evidence budowało Base58, `String`, JSON i hash przed przekazaniem do
  writera;
- krytyczne IPC mogło czekać na capacity we wszystkich event workerach;
- ingress kończył się blokującym `overflow.send`.

Pełna kolejka była skutkiem zarówno burstu, jak i zbędnej pracy. Samo
zwiększenie capacity albo dodanie kolejnej overflow queue nie usuwałoby źródła
problemu.

## 2. Decyzja

PR1B wprowadza trzy zmiany w tej kolejności:

1. pojedynczą zdekodowaną granicę transakcji i jeden parser pass;
2. bounded, nieblokujące przekazanie do niezależnych wolnych sinków;
3. typed, deterministyczny i fail-closed local gap przy rzeczywistej saturacji.

Nie powstaje nowy równoległy model runtime. Istniejący typed event zachowuje
provider provenance, slot, `tx_index` (w tym `Some(0)`), signature, event/arrival
time, success/error, keys, outer/inner instructions, stack height, balances oraz
pre/post token balances. Captured payload jest opcjonalnym współdzielonym
buforem, a nie transportem parsera.

## 3. Call graph

### Przed

```text
Yellowstone SubscribeUpdateTransaction (decoded)
  -> route_update
  -> prost encode                                      #1 encode
  -> PumpEvent::Transaction { raw: Vec<u8> }
  -> fast queue
  -> overflow queue
  -> blocking overflow.send
  -> pump_event_to_geyser_event
  -> prost decode                                      #1 decode
  -> GeyserEvent::Transaction { raw }
  -> event worker
     -> raw.clone + wal.append_with_clock              synchronous
     -> Base58/String/JSON/evidence hash                synchronous
     -> parse_initialize_pool
        -> prost decode + outer/inner scan              #2 decode, scan #1
     -> parse_trades
        -> prost decode + outer/inner scan              #3 decode, scan #2
     -> IpcSender::send_*().await / Block
```

### Po

```text
Yellowstone SubscribeUpdateTransaction (decoded)
  -> PumpEvent::Transaction { decoded, capture_required }
  -> one bounded ingress FIFO / try_send
  -> normalize directly from decoded fields
  -> optional capture after ingress
       off      -> 0 encode
       required -> 1 encode -> shared immutable capture
  -> parse_transaction_bundle
       one outer + inner scan
       one CPI decode/dedupe/provenance/ordinal pass
       initialize_pool + all trades
  -> ordered PoolDetected, then Trade
  -> nonblocking bounded handoff
       compact WAL job -> fixed writer -> append
       Arc event       -> fixed evidence writer -> Base58/JSON/hash/file
       typed event     -> fixed IPC dispatcher -> downstream wait
```

Capture zachowuje kontrakt PR1A:

```text
payload_hash_blake3 =
BLAKE3(prost-encoded decoded SubscribeUpdateTransaction)
```

Capture jest kodowany najwyżej raz i nigdy nie jest dekodowany przez aktywny
live parser. Jawnie kompatybilne/backfill helpery mogą nadal dekodować własne
wejście, ale nie są callsite’em live.

## 4. Parity i pomiar po korekcie review

Pierwotny digest `062d...` obejmował tylko strukturalny podzbiór eventów i nie
stanowił pełnego dowodu parity. Został zastąpiony przez
`CanonicalParserParitySnapshotV1`, który serializuje pełne typed
`InitializePoolEvent` i `TradeEvent`, z wyzerowaniem wyłącznie lokalnych,
niedeterministycznych timestampów ingress.

Ten sam pełny snapshot uruchomiony na B0 (`5136319`) i na finalnym PR1B daje:

```text
549d66a347a3e56b516bc5b77a5f22929604442d409ece7eb1a55525eaa51202
```

Test negatywny zmienia pole ekonomiczne eventu i potwierdza zmianę digestu.
Parity obejmuje zatem kwoty i limity, rezerwy, success/error, complete, signer,
route/buy variant, token deltas oraz pełne provenance zawarte w typed eventach.

Po przebudowie capacity testu finalny release harness na tym hoście raportuje:

| Metryka release | Final PR1B |
|---|---:|
| Throughput pełnego replay parsera | 2,429.975 events/s |
| receive→normalize p50 / p95 / p99 | 20,121 / 39,841 / 48,608 ns |
| normalize→bundle p50 / p95 / p99 | 394,084 / 555,250 / 840,725 ns |
| Burst input / capacity / missing | 2,048 / 2,048 / 0 |
| Queue high-water | 1,992 |
| Zmierzony peak ingress | 73,482.008 events/s |
| Zmierzony sustained drain | 2,650.976 events/s |
| Najstarszy event w kolejce | 744,241,363 ns |
| Steady-state RSS | 17,144 KiB |

To jest dowód dla zamrożonego, deterministycznego protobuf replay workloadu z
równoległym producentem i rzeczywistym konsumentem normalizującym oraz
wywołującym `parse_transaction_bundle`. Nie jest to uniwersalna deklaracja
produkcyjnej capacity, losslessness ani gwarancji opóźnienia Yellowstone.

## 5. Dokładna liczba encode/decode/scan

| Operacja na jednej live transaction | Przed | Po, capture off | Po, capture wymagany |
|---|---:|---:|---:|
| Application-level prost encode | 1 | 0 | 1 |
| Prost decode w normalizatorze | 1 | 0 | 0 |
| Prost decode w parserze | 2 | 0 | 0 |
| Pełny scan outer + inner | 2 | 1 | 1 |

W workloadzie pięciu transakcji oznacza to:

```text
before: encode=5, normalizer_decode=5, parser_decode=10, full_scans=10
after capture off: encode=0, normalizer_decode=0, parser_decode=0, full_scans=5
after capture required: encode=5, normalizer_decode=0, parser_decode=0, full_scans=5
```

Jedno parsed bundle zachowuje ordinary BUY, ordinary SELL, create + initial
BUY, wiele mutacji w jednej signature, PumpSwap inner instructions,
`tx_index=Some(0)`, event ordinal i instruction provenance.

## 6. Queue model

```text
Yellowstone receiver
  -> ingress FIFO: bounded, configurable; default 2,048
  -> event workers
       -> WAL queue: bounded 1,024 -> one fixed OS writer
       -> evidence queue: bounded 1,024 -> one fixed writer task
       -> IPC normal events: bounded configured capacity
       -> IPC canonical AccountUpdate:
            bounded latest-state lane per bonding curve
            ordering/freshness key = (slot, write_version)
       -> one fixed IPC dispatcher merges both lanes by retained sequence
```

Pierwotne wyliczenie `throughput × 250 ms = 1,024` zostało odrzucone. Capacity
jest teraz polem serde-default konfiguracji (`ingress_queue_capacity`, domyślnie
`2,048`). Zamrożony replay uruchamia producenta i konsumenta równolegle,
raportuje peak ingress, sustained drain, high-water i najstarszy backlog, a dla
capacity `2,048` kończy z `missing_event_count = 0`.

Dedykowana AccountUpdate lane ma capacity powiązaną w launcherze z bounded
limitem obserwowanych pul. Pełna wspólna kolejka business eventów nie usuwa
canonical state update. Nowszy stan dla tej samej krzywej może zastąpić
oczekujący starszy stan według `(slot, write_version)` bez oczekiwania hot path.

Nie ma `fast + overflow`, ogólnego spill queue, unbounded channel ani
per-event `tokio::spawn`. Event worker nie wykonuje fizycznego WAL append,
evidence JSON ani await na downstream IPC capacity.

Slow-sink harness po korekcie:

```text
slow WAL enqueue = 761,345 ns
physical writer elapsed = 7,196,949 ns
physical writer calls/waits = 2/2, wyłącznie po stronie writera
slow IPC enqueue = 11,995 ns
parser-worker IPC blocking waits = 0
```

## 7. Local gap semantics

Każda lokalna domena utraty ma typed reason:

- `ingress_queue_saturated`;
- `wal_queue_saturated`;
- `evidence_queue_saturated`;
- `ipc_egress_queue_saturated`.

`LocalCoverageGapV1` zapisuje provider ID, stream epoch, episode sequence,
granice slot/signature przed i po, `missing_event_count`, `first_dropped`,
`last_dropped`, queue high-water, czas początku/końca, reason oraz `recovered`.

Gap ID jest BLAKE3 stabilnych pól epizodu; nie zawiera losowego UUID ani
timestampu diagnostycznego. Powtarzalne wejście daje ten sam ID. Jeden ciągły
epizod saturacji daje jeden marker.

Finalny test capacity-two:

```text
receiver_blocked = false
blocking_wait_ns = 0
explicit_missing = 2
local_gap_count = 1
silent_success = 0
gap.missing_event_count = 2
gap.first_dropped.slot = 2
gap.last_dropped.slot = 3
recovered = false
```

Luka lokalna nie jest provider slot gap. Nie uruchamia reconnectu/backfillu.
PR1B nie ma proof-based local recovery, dlatego po pierwszej luce segment ma
sticky unreliable i kandydaci są fail-closed. Canonical `AccountUpdate`
pozostaje forwardowany, aby transportowa diagnostyka PR1B nie zmieniła
AccountStateCore authority.

Nowy `WalRecord::LocalCoverageGap` jest addytywny i nie jest recovery-critical.
Wszystkie cztery domeny — ingress, normalny WAL, evidence i IPC — przekazują
zamknięte markery do jednego centralnego routera. Osobna, rezerwowana kolejka i
dedykowany writer zapisują markery bez zależności od pełnej normalnej kolejki
WAL. Replay liczy je jako audit evidence, ale nie mutuje odtwarzanego stanu.

Evidence jest globalnym warunkiem poprawności wyłącznie przy
`artifact_required_for_run = true`. Saturacja diagnostycznego,
best-effort evidence nadal tworzy typed gap i audit marker, ale nie unieważnia
canonical candidate path.

Każdy dispatcher ma jawny lifecycle:

```text
stop accepting -> drain accepted -> final flush -> join -> typed failure
```

Launcher zachowuje IPC receiver podczas drainu i raportuje błąd, jeżeli
zaakceptowany event nie został dostarczony albo trwały sink nie został
domknięty.

## 8. Zachowane inwarianty

- provider ID, provider role i source;
- transaction signature i `tx_index=Some(0)`;
- wiele mutacji jednej signature;
- instruction provenance i event ordinal;
- create-before-trade ordering;
- semantyka BUY/SELL i istniejące route variants;
- account update forwarding;
- canonical AccountUpdate zachowany mimo pełnego wspólnego IPC;
- PR1A payload hash i old JSON/config compatibility;
- shadow/live boundary;
- authority i AccountStateCore arbitration;
- MFS, Gatekeeper, verdicts/reason codes;
- scoring, strategia, progi, quote math i execution.

Dowód zakresu: diff PR1B nie dotyka plików Gatekeepera, MFS, konfiguracji
strategii, quote math ani execution. Differential corpus zachowuje business
digest.

## 9. Testy i ograniczenia

Przeszły:

```text
cargo test -p ghost-core ingest_integrity -- --nocapture
cargo test -p seer --lib pr1b_ -- --nocapture
cargo test -p seer --lib one_continuous_saturation_episode_produces_one_deterministic_gap -- --nocapture
cargo test -p seer --lib bounded_ingress_saturation_is_nonblocking_and_emits_one_gap -- --nocapture
cargo test -p seer --lib ipc::tests -- --nocapture
cargo test -p seer --lib canonical_account_update_survives_full_downstream_and_arrives_once -- --nocapture
cargo test -p seer --lib reserved_audit_lane_persists_every_local_gap_domain -- --nocapture
cargo test -p seer --lib wal_dispatcher_shutdown_drains_flushes_and_joins_all_accepted_jobs -- --nocapture
cargo test -p seer --lib raw_evidence_shutdown_drains_and_final_flushes_accepted_event -- --nocapture
cargo test -p seer --lib shutdown_fails_if_a_gap_cannot_reach_a_configured_audit_wal -- --nocapture
cargo test -p seer --lib provider_metadata -- --nocapture
cargo test -p seer --lib account_update_preserves_provider_and_optional_transaction_signature -- --nocapture
cargo test -p ghost-launcher --lib local_coverage_gap_replays_as_audit_only_record -- --nocapture
cargo check -p ghost-brain --tests
cargo test --release -p seer --lib pr1b_hot_path_harness -- --ignored --nocapture --test-threads=1
timeout 900s cargo build --release --workspace
```

Pełne testy zachowują bazowy failure signature:

- pełny Seer timeout po tych samych 14 błędach PumpPortal/Seer co B0;
- `ghost-launcher --tests` i workspace zatrzymują się na istniejących
  fixture’ach `PoolTransaction` z `E0063`.

PR1B nie maskuje i nie naprawia niezwiązanego długu fixture’ów.

## 10. Świadomie odłożone

PR1C:

- typed AccountObservationArbiter;
- provider/account duplicate i conflict arbitration;
- zmiana account authority.

PR1D:

- Observation Ledger;
- primary raw / NLN reconciliation;
- correlation exact/singleton/ambiguous/unmatchable;
- candidate integrity outcome dla raw/NLN conflict.

Poza PR1B pozostaje również proof-based odzyskanie lokalnej luki i provider
reconnect/backfill state machine dla semantycznie dowodliwego provider gap.

## 11. Rollback

Rollback całego PR1B polega na odwróceniu trzech logicznych commitów oraz
korekty review do merge parenta `ea7d31a228f8db0b7ed0779dea70b696895e66c2`.

Nie należy częściowo przywracać starego parser transportu przy zachowaniu
nowych dispatcherów ani usuwać fail-closed gap semantics przy pozostawieniu
nieblokujących `try_send`: taki miks mógłby ponownie wprowadzić silent loss albo
podwójną authority parsowania.

Bezpieczny rollback jest całościowy:

```text
B2 nonblocking sinks/local gap
-> B1 single decoded transaction/single parser pass
-> B0 measurement harness/receipt
```

B0 jest diagnostyczny i może pozostać, jeżeli potrzebny jest jedynie rollback
behavior. Nowe pola konfiguracji mają serde-default, więc stare konfiguracje
pozostają czytelne. Rollback nie zmienia strategii ani authority.
