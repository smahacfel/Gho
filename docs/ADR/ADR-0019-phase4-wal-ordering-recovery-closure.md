# ADR-0019: Faza 4 — Domknięcie durability: strict WAL ordering, restore i replay delta

**Data:** 2026-03-22
**Status:** Accepted
**Autor:** Ghost Father
**Powiązany plan:** `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md` § Faza 4

---

## Kontekst

Przed Fazą 4 system miał WAL i API snapshot, ale brakowało:
- formalnego `ReplayOrderKey` dla recovery-critical rekordów,
- deterministycznej sekwencji startu (restore → WAL delta → live ingest),
- aktywnego `PeriodicSnapshotTask` jako rzeczywistego taska runtime,
- testów restartowych pokrywających staged commit, partial WAL tail i pending live.

ADR `20260320_production_pipeline_data_flow.md` opisywał stan jako "durability częściowo aktywna". Faza 4 zamknęła tę lukę.

---

## Decyzje

### D1. ReplayOrderKey dla każdego recovery-critical rekordu WAL

Dodano typ `ReplayOrderKey` (`ghost-core/src/wal.rs`) i metodę `WalRecord::replay_order_key()`:

| Rekord | Klucz | Uzasadnienie |
|---|---|---|
| `TradeForwarded` | `TxBased(trade.tx.tx_key)` | deterministyczny porządek z blockchainu |
| `CommitStaged` | `TxBased(max tx_key z buffered_history)` | fallback: `SlotAndWallClock` gdy brak historii |
| `CommitPersisted` | `TxBased(last_committed_tx_key)` | fallback: `SlotAndWallClock` gdy brak klucza |
| `ShadowLedgerCurveUpdate` | `SlotAndWallClock { slot, ts_ms }` | write_strength arbitration gwarantuje idempotencję |
| `RollbackReevalSeed` | `SlotAndWallClock { slot, ts_ms }` | ostatni per base_mint wygrywa |
| `RawTx`, `ParsedEvent`, `Decision` | `NotRecoveryCritical` | brak restore effect przy replay |

Replay odtwarza decision-order przez explicit `sort_by(tx_key)` w `replay_shared_wal()` dla `staged_history` i `post_commit` TX — nie arrival-order pliku.

### D2. Deterministic startup restore sequence

`ghost-launcher/src/main.rs` wykonuje przy starcie, **przed** otwarciem live ingest, w kolejności:

1. `ShadowLedger::restore_from_disk(GHOST_SNAPSHOT_DIR)` — restore kurv z dysku
2. odczyt `stats.written_at_ms` jako `snapshot_watermark_ms`
3. `replay_shared_wal(wal, oracle_runtime, Some(watermark))` — delta WAL od watermark
4. `restore_committed_history_from_wal()` — committed pools z `CommitPersisted`
5. `restore_runtime_pool_state_from_wal()` — staged state z `CommitStaged`
6. `replay_live_tx_from_wal()` + `flush_replayed_live_mint_from_wal()` — pending live delta z `TradeForwarded`
7. `restore_rollback_seed_from_wal()` — rollback seeds
8. dopiero potem: start Seera i live ingest

Tryby recovery zależą od dostępności snapshot i WAL:
- `snapshot_plus_wal` — pełne deterministyczne recovery
- `wal_only` — cold start z pełnym WAL replay
- `snapshot_only` — restore bez WAL (brak `GHOST_WAL_DIR`)
- `cold_start` — brak snapshot i WAL

### D3. PeriodicSnapshotTask jako rzeczywisty task runtime

Spawned jako tokio task jeśli `GHOST_SNAPSHOT_DIR` jest ustawiony. Interwał konfigurowalny przez `GHOST_SNAPSHOT_INTERVAL_S` (fallback: `config.durability.snapshot_interval_s`). Przechowuje ostatnie 3 snapshoty (`rotate_snapshots`).

### D4. Recovery telemetry

Emitowane metryki:

| Metryka | Typ | Plik |
|---|---|---|
| `wal_replay_records_total` | counter | `ghost-core/src/wal.rs` |
| `wal_replay_duration_ms` | histogram | `ghost-launcher/src/main.rs` |
| `shadow_ledger_restore_duration_ms` | histogram | `ghost-launcher/src/main.rs` |
| `runtime_recovery_watermark_ms` | gauge | `ghost-launcher/src/main.rs` |
| `runtime_recovery_mode{mode=...}` | counter | `ghost-launcher/src/main.rs` |

**Odchylenie od planu:** Plan Fazy 4 specyfikował metrykę `runtime_recovery_watermark_slot`. Snapshot header przechowuje `written_at_ms` (wall-clock ms), nie numer slotu Solana — dlatego metryka jest emitowana jako `runtime_recovery_watermark_ms`. Watermark jest filtrem po `ts_ms` we wszystkich ścieżkach WAL replay, co jest spójne semantycznie.

---

## Kryteria wyjścia Fazy 4 — weryfikacja

| Kryterium | Status | Lokalizacja |
|---|---|---|
| Każdy recovery-critical rekord ma jawny `ReplayOrderKey` | ✅ | `ghost-core/src/wal.rs:99-154` |
| Replay odtwarza decision-order, nie arrival-order | ✅ | `ghost-launcher/src/wal_recovery.rs:123,160-162` |
| Restart odtwarza `ShadowLedger` i pending state bez ręcznych kroków | ✅ | `ghost-launcher/src/main.rs:336-834` |
| Istnieją testy restartu po staged commit i z częściowo przyciętym WAL tail | ✅ | `ghost-launcher/tests/wal_startup_recovery.rs` |

---

## Artefakty wyjściowe

| Artefakt | Lokalizacja |
|---|---|
| `ReplayOrderKey` enum + `replay_order_key()` | `ghost-core/src/wal.rs:146-154` |
| `replay_shared_wal()` z delta watermark | `ghost-launcher/src/wal_recovery.rs:52-183` |
| Deterministic startup sequence | `ghost-launcher/src/main.rs:333-834` |
| `PeriodicSnapshotTask` | `ghost-launcher/src/main.rs:803-834` |
| Testy — staged commit survives restart | `ghost-launcher/tests/wal_startup_recovery.rs:89` |
| Testy — partial WAL tail tolerated | `ghost-launcher/tests/wal_startup_recovery.rs:147` |
| Testy — snapshot watermark skips pre-snapshot records | `ghost-launcher/tests/wal_startup_recovery.rs:206` |
| Testy — pending live delta replay | `ghost-launcher/src/wal_recovery.rs:481` |
| Testy — staged commit dla finalizacji | `ghost-launcher/src/wal_recovery.rs:382` |
| Testy — rollback seed suppresses staged commit | `ghost-launcher/src/wal_recovery.rs:427` |

---

## Ograniczenia i znane luki

1. **Watermark jako `ts_ms`, nie slot.** `replay_all()` filtruje rekordy przez `record.ts_ms() <= watermark`. Oznacza to, że watermark jest przybliżony (wall-clock, nie blockchain slot). W praktyce gwarantuje to brak duplikacji przy normalnych warunkach; przy skokach zegara systemowego możliwe jest pominięcie lub powtórzenie granicznych rekordów. Jest to akceptowane ryzyko do czasu implementacji slot-based watermark w Fazie 5+.

2. **`replay_all()` nie sortuje globalnie po `ReplayOrderKey`.** Segmenty są sortowane po nazwie pliku (ts otwarcia), a wewnątrz segmentu odtwarza się po append order. `ReplayOrderKey` jest używane przez `replay_shared_wal()` do lokalnego porządkowania zebranych danych (sort po tx_key), nie do globalnej re-sortacji strumienia. Dla recovery-critical records (CommitStaged/CommitPersisted/TradeForwarded) jest to wystarczające, bo arbitraż `write_strength` w `ShadowLedger` zapewnia idempotencję.

3. **Brak testów `ReplayOrderKey` dla scenariusza cross-segment out-of-order.** Istniejące testy pokrywają normalny append order. Test dla rekordu CommitPersisted w starszym segmencie niż CommitStaged nie istnieje. Jest to akceptowalne, bo architektura emituje CommitStaged zawsze przed CommitPersisted w tym samym segmencie.

---

## Wpływ na następne fazy

- **PR-7** (`launcher-commit-coordinator`): kwestie durability są domknięte i nie należą do zakresu PR-7. PR-7 skupia się na finalnym odcięciu legacy GatekeeperRegistry z runtime paths (zob. ADR-0017).
- **Faza 5** (`PendingCurve`, freshness/finality): może bezpiecznie zakładać deterministyczny restart.
- Slot-based watermark pozostaje jako backlog po ukończeniu Fazy 5.
