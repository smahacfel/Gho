# Plan Naprawczy: Pipeline gRPC → ShadowLedger

## Problem

Pipeline wykazuje dwa typy defektów:
1. **Straty eventów** — Pump.fun eventy interesujące nas są dropowane w 8 miejscach, zanim dotrą do ShadowLedger
2. **Latencja ~1000ms** — od momentu powstania poola do zarejestrowania stanu w ShadowLedger (cel: <200ms)

---

## Audyt: Mapa Strat Eventów (Event Loss Points)

Poniższa tabela mapuje WSZYSTKIE punkty w kodzie, gdzie Pump.fun event może zostać odrzucony/utracony:

| # | Punkt Straty | Plik | Linia | Wpływ | Odzyskiwalność |
|---|-------------|------|-------|-------|-----------------|
| L1 | [ultrafast_mode](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1872-1911) trade skip | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 1602-1609 | Dropuje WSZYSTKIE trade parse pod backpressure | Nieodwracalna — metryka `seer_ultrafast_trade_skip_total` |
| L2 | [should_forward_trade()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1142-1204) mapping miss | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 1171-1186 | Trade z nieznanym curve→mint mapping odrzucone | Częściowa — [queue_curve_mint_resolve](file:///root/Ghost/off-chain/components/seer/src/lib.rs#716-805) via RPC |
| L3 | [is_invalid_trade_pool()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1205-1209) | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 1206-1208 | Pool = default Pubkey lub WSOL | Poprawne filtrowanie |
| L4 | `parse_curve_from_account()` reject | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 907-913 | Nieznany layout konta (za krótkie dane) | Rate-limited log, ale dane utracone |
| L5 | [lookup_curve_mint()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#682-689) → None (AcctUpdate) | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 915-921 | AccountUpdate bez mapping → pending queue | Odzyskiwalna po RPC resolve |
| L6 | `SnapshotListener.forward_mode=None` | [snapshot_listener.rs](file:///root/Ghost/ghost-launcher/src/components/snapshot_listener.rs) | 130 | Wszystkie TX dropowane | Celowa konfiguracja |
| L7 | `SnapshotListener.max_pools` exceeded | [snapshot_listener.rs](file:///root/Ghost/ghost-launcher/src/components/snapshot_listener.rs) | 136-147 | Nowe poole odrzucone po limicie | Memory guard — celowy |
| L8 | `parse_miss` (trade ix present, 0 parsed) | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | 1700-1713 | Instrukcja rozpoznana ale parser nic nie zwrócił | Metryka `parse_miss_total` |

> [!CAUTION]
> **L1 jest najgroźniejszy**: Pod backpressure (queue_util > threshold) WSZYSTKIE trade parsowania są pomijane. Create events przechodzą, ale trades — nie. To oznacza, że w okresie wysokiego obciążenia bot widzi nowe poole, ale nie widzi żadnych trades na nich.

> [!WARNING]
> **L2 jest najczęstszy**: Jeśli trade Tx dotrze PRZED Create Tx dla danego poola (race condition), curve→mint mapping nie istnieje i trade jest odrzucony. System próbuje odzyskać via RPC ([queue_curve_mint_resolve](file:///root/Ghost/off-chain/components/seer/src/lib.rs#716-805)), ale to dodaje 200-2000ms latencji.

---

## Audyt: Mapa Latencji (Latency Waterfall)

Typowa ścieżka CREATE → ShadowLedger (obecny stan):

```
┌─────────────────────────────────────────────────────────────┐
│  STAGE                            │ ESTIMATED LATENCY       │
├───────────────────────────────────┼─────────────────────────┤
│ S1: Solana Validator → gRPC       │ 100-400ms (network)     │
│     (block propagation)           │                         │
├───────────────────────────────────┼─────────────────────────┤
│ S2: gRPC stream → DualLaneChannel │ <1ms                    │
│     route_update() → PumpEvent    │                         │
├───────────────────────────────────┼─────────────────────────┤
│ S3: PumpEvent → GeyserEvent       │ 50-200μs                │
│     DOUBLE PROTO DECODE           │ (raw proto→PumpEvent    │
│     (redundant!)                  │  then PumpEvent→decode) │
├───────────────────────────────────┼─────────────────────────┤
│ S4: process_event() serial chain  │ 5-50ms ⚠️               │
│     parse_initialize_pool()       │ (sequential .await:     │
│     + set_curve_mapping()         │  add_watched_mint +     │
│     + add_watched_mint().await    │  watch_pool +           │
│     + watch_pool().await          │  store_curve +          │
│     + store_curve_with_snapshots  │  enhanced_builder)      │
│     + build_enhanced_candidate    │                         │
│     + IPC.send().await            │                         │
├───────────────────────────────────┼─────────────────────────┤
│ S5: RESUB_DEBOUNCE_MS             │ 0-20ms                  │
│     (before gRPC re-subscribe     │ (debounce delay before  │
│      with new curve account)      │  new AccountUpdate sub) │
├───────────────────────────────────┼─────────────────────────┤
│ S6: First AccountUpdate arrival   │ 0-400ms ⚠️              │
│     (next state change on-chain)  │ (waits for next Solana  │
│                                   │  slot with update)      │
├───────────────────────────────────┼─────────────────────────┤
│ S7: handle_account_update()       │ <1ms (inline) or        │
│     → ShadowLedger write          │ spawn (non-blocking)    │
├───────────────────────────────────┼─────────────────────────┤
│ S8: RPC curve_resolve (IF NEEDED) │ 200-2000ms ⚠️⚠️         │
│     (when mapping miss L2)        │ (getSignatures +        │
│                                   │  getTransaction × N)    │
└───────────────────────────────────┴─────────────────────────┘

TOTAL (happy path):  ~150-670ms
TOTAL (mapping miss): ~350-2670ms  ← THIS IS THE 1000ms+ PROBLEM
```

---

## Proponowane Naprawy

### FIX-1: Eliminacja Double Decode (S3)

**Problem**: [route_update()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1169-1285) w [grpc_connection.rs](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs) tworzy [PumpEvent](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs#577-585) z raw proto bytes (linie 1197-1260), potem [pump_event_to_geyser_event()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1626-1672) ponownie dekoduje te same bytes do `GeyserEvent` (linie 1626-1671). Dwa pełne proto decode na ten sam event.

**Naprawa**: W [pump_event_to_geyser_event()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1626-1672) dla `PumpEvent::Transaction` — przechowywać zdekodowane pola w [PumpEvent](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs#577-585) zamiast raw bytes + ponownego decode.

#### [MODIFY] [grpc_connection.rs](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs)

- Dodać pole `decoded: Option<DecodedTx>` do `PumpEvent::Transaction`
- W [route_update()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1169-1285) wykonać decode raz i zapisać wynik
- [pump_event_to_geyser_event()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1626-1672) czyta z decoded cache

> [!IMPORTANT]
> Current implementation stores `raw: Vec<u8>` in PumpEvent, then does full `SubscribeUpdateTransaction::decode(raw)` in [pump_event_to_geyser_event()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1626-1672). The first decode happens implicitly in [route_update()](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1169-1285) when extracting `signature` and [slot](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#149-158) — but it doesn't preserve the full decoded result, forcing a second decode later.

---

### FIX-2: Pre-Registered Curve Mapping z Create Tx (L2)

**Problem**: Trade Tx może dotrzeć przed Create Tx → [should_forward_trade()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1142-1204) odrzuca z powodu braku curve→mint mapping → RPC fallback (200-2000ms).

**Naprawa**: W [parse_trades()](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs#2223-2498) BinaryParser'a, kiedy napotykamy Buy/Sell instruction z curve address, eagerly ekstrahuujemy mint z instruction accounts (mint jest zawsze account[2] w PumpFun Buy/Sell instrukcji).

#### [MODIFY] [binary_parser.rs](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs)

- W `parse_buy_sell_instruction()` — zawsze wywołaj `cm_reg.insert(curve_str, mint_str)` nawet jeśli mapping już istnieje (idempotent)
- To eliminuje zależność od Create Tx arrival order

#### [MODIFY] [lib.rs](file:///root/Ghost/off-chain/components/seer/src/lib.rs)

- W [process_event()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1265-1871) po [parse_trades()](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs#2223-2498) — jeśli trade ma valid pool+mint, ustawiaj mapping PRZED [should_forward_trade()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1142-1204) check (zmiana kolejności operacji)

---

### FIX-3: Session-Start Slot Guard (z poprzedniego walkthrougha)

**Problem**: Backfill transactions mogą zarejestrować stare poole.

#### [MODIFY] [lib.rs](file:///root/Ghost/off-chain/components/seer/src/lib.rs)

- Dodać `session_start_slot: AtomicU64` do struct [Seer](file:///root/Ghost/off-chain/components/seer/src/lib.rs#213-288)
- Ustawić w [run()](file:///root/Ghost/ghost-launcher/src/components/snapshot_listener.rs#80-235) po pierwszym [process_event()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1265-1871) z valid slot
- W [process_event()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1265-1871) — odrzucać [CandidatePool](file:///root/Ghost/off-chain/components/seer/src/types.rs#392-434) z `slot < session_start_slot`
- BackfillTransaction variant zawsze odrzucany dla [InitializePool](file:///root/Ghost/off-chain/components/seer/src/types.rs#174-220)

---

### FIX-4: Non-Blocking process_event() Hot Path (S4)

**Problem**: [process_event()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#1265-1871) jest single-threaded i sequentialnie `.await`-uje: [add_watched_mint](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1580-1586), [watch_pool](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1587-1593), `IPC.send`, wzbogacony builder. To blokuje odbiór kolejnych eventów.

#### [MODIFY] [lib.rs](file:///root/Ghost/off-chain/components/seer/src/lib.rs)

- **[add_watched_mint().await](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1580-1586) i [watch_pool().await](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1587-1593)** — te operacje modyfikują jedynie `DashSet`/`DashMap` (lock-free). Zmienić na synchroniczne warianty (`try_add_watched_mint()` / `try_watch_pool()`) lub wywoływać w `tokio::spawn` off hot path.
- **`build_enhanced_candidate()`** — przenieść do osobnego spawned taska (nie blokuje forwarding CandidatePool)
- **[store_curve_with_snapshots()](file:///root/Ghost/off-chain/components/seer/src/lib.rs#171-211)** — już jest spawned w non-test mode (linie 946-973), OK

> [!WARNING]
> Zmiana [add_watched_mint](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs#1580-1586) z `.await` na sync wymaga weryfikacji, że underlying `DashSet::insert` jest rzeczywiście non-blocking ([O(1)](file:///root/Ghost/ghost-launcher/src/components/oracle_pipeline.rs#222-232) amortized). Na podstawie kodu — TAK, jest to `DashSet<Vec<u8>>` z [insert()](file:///root/Ghost/ghost-core/src/shadow_ledger/ledger.rs#430-442) → [O(1)](file:///root/Ghost/ghost-launcher/src/components/oracle_pipeline.rs#222-232).

---

### FIX-5: Agresywniejsze Coverage Monitoring

**Problem**: Metryka `SEER_COVERAGE` loguje co `COVERAGE_LOG_INTERVAL` (domyślnie 60s). To za rzadko dla SLA monitoringu >=99%.

#### [MODIFY] [lib.rs](file:///root/Ghost/off-chain/components/seer/src/lib.rs)

- Zmienić `COVERAGE_LOG_INTERVAL` z 60s → 10s
- Dodać metrykę Prometheus `seer_coverage_ratio` gauge (emitowaną co log tick)
- Alert na `seer_coverage_ratio < 0.99` z oknem 5min

---

## Weryfikacja

### Istniejące Testy

| Test | Plik | Pokrywa |
|------|------|---------|
| 34 unit tests | [grpc_connection.rs](file:///root/Ghost/off-chain/components/seer/src/grpc_connection.rs) | DualLane, SlotTracker, Registry, DelayedQueue, SubscribeRequest, backfill |
| 7 unit tests | [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) | create mapping, account update, curve parse, ultrafast mode |
| Testy w [binary_parser.rs](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs) | [binary_parser.rs](file:///root/Ghost/off-chain/components/seer/src/binary_parser.rs) | CPI walk, discriminator decode, CurveMintRegistry |
| 1 test file | [tests/curve_parser_tests.rs](file:///root/Ghost/off-chain/components/seer/tests/curve_parser_tests.rs) | curve layout parsing |

### Nowe Testy do Napisania

1. **`test_trade_before_create_recovers_mapping`** w [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) — symuluje trade Tx arriving przed Create → weryfikuje że mapping jest ustawiany z instruction accounts
2. **`test_session_start_slot_rejects_old_pools`** w [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) — symuluje CandidatePool z `slot < session_start_slot`
3. **`test_backfill_tx_never_creates_candidate`** w [lib.rs](file:///root/Ghost/ghost-core/src/lib.rs) — BackfillTransaction z InitializePool → nie emituje CandidatePool

### Komenda do uruchomienia testów

```bash
# Seer unit tests
cd /root/Ghost && cargo test -p seer --lib -- --nocapture

# All seer tests including integration
cd /root/Ghost && cargo test -p seer -- --nocapture

# Specific test
cd /root/Ghost && cargo test -p seer --lib test_trade_before_create_recovers_mapping -- --nocapture
```

### Weryfikacja Manualna

Po wdrożeniu FIX-1 do FIX-5:
1. Uruchomić bota w trybie live
2. Monitorować logi `SEER_COVERAGE` — oczekiwane wartości: `parsed/emitted ratio > 99%`
3. Monitorować `mint_to_detection_latency` histogram — oczekiwane P99 < 300ms (vs obecne ~1000ms)
4. Monitorować `curve_recv_to_apply_ms` — oczekiwane P99 < 200ms
5. Monitorować `seer_ultrafast_trade_skip_total` — powinno być 0 przy normalnym obciążeniu
