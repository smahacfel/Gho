# ADR-0011: Audit realizacji Fazy 1 storage arbitration i bootstrap precedence

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Użytkownik zlecił zimny audit realizacji Fazy 1 względem dokumentu źródłowego `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`, z wymaganiem oceny zarówno kodu, jak i wszystkich nakazów, zakazów i kryteriów wyjścia tej fazy.

Faza 1 w planie wymagała w szczególności:
- jednej storage-level ścieżki arbitrażu dla write'ów do `ShadowLedger`,
- obowiązkowych metadata write'u: `write_source`, `write_strength`, `state_confidence`, `slot|slot_absent`, `reason`,
- wyznaczenia jednego kanonicznego writera bootstrapu,
- usunięcia konfliktu podwójnego bootstrapu `genesis_curve()` pomiędzy Seer i listenerem `NewPoolDetected`,
- potraktowania RPC seedera jako jawnego upgrade path `P0 -> P1`, a nie drugiego niezależnego bootstrapu,
- dodania telemetryki write/preference/rejection,
- udowodnienia precedence testami, a nie samą dokumentacją.

Audit objął aktywne ścieżki runtime w:
- `ghost-core/src/shadow_ledger/ledger.rs`
- `ghost-core/src/market_state.rs`
- `ghost-core/src/shadow_ledger/reconciliation.rs`
- `ghost-core/src/shadow_ledger/live_pipeline.rs`
- `ghost-core/src/wal.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/wal_recovery.rs`

## Decision

Uznano, że realizacja Fazy 1 jest **merytorycznie mocna i w dużej części zgodna z planem**, ale **nie jest jeszcze w 100% domknięta względem literalnego brzmienia kryteriów wyjścia**.

### Co zostało zrealizowane poprawnie

1. **Powstał realny storage-level arbitration dla curve write'ów**
   - `ghost-core/src/shadow_ledger/ledger.rs` wprowadza `apply_curve_write(...)`.
   - Storage zwraca wymagane klasy wyników:
     - `Applied`
     - `NoOpExistingEqualOrStronger`
     - `RejectedWeakerWrite`
     - `PromotedBootstrapToConfirmed`
     - `RejectedOutOfOrder`
     - `RejectedMissingMetadata`

2. **Metadata write'u zostały sformalizowane i zapisują się w storage**
   - `CurveWriteMetadata` niesie `source`, `strength`, `state_confidence`, `reason`, `slot`, `curve_finality`, `last_update_ts_ms`.
   - `ShadowBondingCurve` przechowuje provenance ostatniego zaakceptowanego write'u.
   - `ghost-core/src/wal.rs` rozszerza `ShadowLedgerCurveUpdateRecord` o te same pola metadata.

3. **Podwójny bootstrap semantyczny został wygaszony**
   - `ghost-launcher/src/main.rs` nie wykonuje już drugiego `genesis_curve()` write'u po `GhostEvent::NewPoolDetected`.
   - Listener w `main.rs` został zredukowany do obserwacji/logowania.
   - Kanoniczny bootstrap semantyczny dla nowych pooli pozostał po stronie Seera.

4. **RPC seeder został przekształcony w jawny upgrade path `P0 -> P1`**
   - `ghost-launcher/src/components/seer.rs::seed_curve_via_rpc()` używa teraz `store_confirmed_bootstrap(...)`.
   - Ta ścieżka zapisuje `write_source=RpcBootstrapSeeder`, `write_strength=ConfirmedBootstrap`, `reason=ConfirmedBootstrap`.

5. **Aktywne prod curve write'y zostały przepięte na wspólny path arbitration**
   - Seer bootstrap: `store_bootstrap_seed(...)`
   - RPC confirmed bootstrap: `store_confirmed_bootstrap(...)`
   - AccountUpdate repair: `store_repair_curve(...)`
   - Reconciliation repair/finality refresh: `apply_curve_write(...)`
   - WAL replay curve restore: `apply_curve_write(...)`

6. **Telemetryka Fazy 1 została dodana**
   - `shadow_ledger_write_total{source,strength,result}`
   - `shadow_ledger_bootstrap_total{source}`
   - `shadow_ledger_bootstrap_noop_total{source}`
   - `shadow_ledger_write_rejected_total{reason}`

7. **Precedence jest dowodzona testami storage-level**
   - duplicate bootstrap seed => no-op,
   - confirmed bootstrap => promotion,
   - weaker bootstrap => reject,
   - out-of-order repair => reject,
   - replay duplicate bootstrap => no-op pod WAL replay.

### Co pozostaje nie w pełni domknięte

1. **Nie wszystkie write'y do `ShadowLedger` przechodzą przez jeden wspólny mechanizm arbitrażu w sensie literalnym planu**
   - `commit_history()` i `append_live()` pozostały osobnymi ścieżkami z własnymi guardami.
   - Otrzymały source-aware telemetry (`commit_history_with_source`, `append_live_with_source`), ale nie są częścią tego samego kontraktu wynikowego co `apply_curve_write(...)`.
   - W planie Fazy 1 zapisano literalnie: „każdy write do `ShadowLedger` przechodzi przez wspólną warstwę arbitrażu”. Ten warunek jest spełniony **dla aktywnych curve write'ów**, ale nie dla pełnego zbioru write'ów obejmującego też canonical commit/live append.

2. **Legacy compatibility path dalej istnieje jako backdoor API**
   - Helper `store_curve_with_snapshots(...)` nadal istnieje.
   - Metody legacy typu `insert_with_slot_known*` / `insert_seed_curve*` nadal istnieją, choć w praktyce delegują do `apply_curve_write(...)`.
   - To nie łamie obecnego runtime, ale oznacza, że Faza 1 nie zredukowała jeszcze surface area do jednego jawnego API semantycznego.

3. **Odrzucony write może nadal wykonać alias mutation przed werdyktem**
   - `apply_curve_write(...)` wykonuje `register_curve_alias(base_mint, bonding_curve)` przed finalnym rozstrzygnięciem precedence.
   - W obecnych aktywnych pathach runtime nie widać produkcyjnego callera, który eksploatowałby to błędnie na innym `bonding_curve`, ale semantycznie oznacza to, że wynik `Rejected*` nie jest całkowicie side-effect free.
   - To nie obala realizacji Fazy 1, ale zostawia ważny dług semantyczny na granicy alias plane vs precedence plane.

## Architectural Impact

Audit ustala nowy obraz systemu:
- precedence bootstrap/repair przestało być wyłącznie „umową między callerami” i zostało przeniesione do storage layer dla aktywnych curve write'ów,
- `genesis_curve()` ma jednego semantycznego właściciela w runtime produkcyjnym,
- startup replay `ShadowLedgerCurveUpdate` został wzbogacony o provenance metadata i podlega temu samemu arbitration contract,
- commit/live history mają już source-aware telemetry, ale nadal są osobną klasą write pathów.

Wpływ na kolejne fazy:
- **Faza 2** może już opierać się na realnym P0/P1/P2 arbitration dla curve truth,
- ale jeśli ktoś chce literalnie zamknąć Fazę 1 bez żadnych zastrzeżeń, trzeba jeszcze rozstrzygnąć relację wspólnego arbitration contract do `commit_history()` / `append_live()` oraz side-effect semantics alias mapy.

## Risk Assessment

**Rate: Medium**

### Niskie ryzyko
- bootstrap duplication została realnie usunięta,
- replay duplicate bootstrap jest chroniony storage-level no-opem,
- weaker / out-of-order writes są blokowane testowalnie.

### Średnie ryzyko
- planowy wymóg „wszystkie write'y przez jedną warstwę arbitrażu” nie jest jeszcze spełniony literalnie dla `commit_history()` i `append_live()`,
- alias registration wykonywane przed wynikiem arbitration może kiedyś wprowadzić subtelną niespójność identity binding, jeśli pojawi się konkurencyjny caller z innym `bonding_curve`.

## Consequences

Co stało się łatwiejsze:
- bootstrap semantics jest jednoznaczniejsze,
- precedence P0/P1/P2 jest teraz wymuszana przez storage, nie tylko przez kolejność tasków,
- WAL replay curve update odzyskał provenance i przestał być ślepym slot-based heuristics only,
- testy regresyjne opisują realne kontrakty precedence.

Co nadal jest trudniejsze:
- pełne domknięcie jednego arbitration contract dla curve + commit + live,
- formalne rozdzielenie alias-only mutation plane od write precedence plane bez żadnych efektów ubocznych,
- usunięcie legacy helperów bez naruszenia kompatybilności istniejących testów i callsite'ów pomocniczych.

## Alternatives Considered

### 1. Uznać Fazę 1 za w pełni ukończoną bez zastrzeżeń
Odrzucono, bo literalne kryterium „każdy write do `ShadowLedger` przechodzi przez wspólną warstwę arbitrażu” nie jest jeszcze prawdziwe dla `commit_history()` i `append_live()`.

### 2. Uznać Fazę 1 za nieudaną
Odrzucono, bo główne cele tej fazy — centralne precedence dla curve write'ów, single semantic bootstrap, RPC promotion path, telemetry i testy precedence — zostały zrealizowane realnie, a nie pozornie.

### 3. Ocenić Fazę 1 jako „praktycznie ukończoną, lecz z jawnymi resztkami do domknięcia”
Przyjęto, bo najlepiej odpowiada stanowi repo: duża część ryzyka została zdjęta, ale istnieją jeszcze resztki architektoniczne, których nie wolno zamiatać pod dywan w audycie zgodności.

## Validation Steps

Audit został zweryfikowany przez:
1. analizę diffów aktywnych plików Fazy 1,
2. potwierdzenie usunięcia bootstrap write'u z `ghost-launcher/src/main.rs`,
3. sprawdzenie aktywnych callsite'ów Seer / reconciliation / WAL replay / runtime replay,
4. sprawdzenie, że `ShadowLedgerCurveUpdateRecord` niesie wymagane metadata,
5. sprawdzenie source-aware wariantów `commit_history_with_source(...)` i `append_live_with_source(...)`,
6. kompilację kontrolną:
   - `cargo test -p seer --lib --no-run`
   - `cargo test -p ghost-launcher --lib --no-run`
7. wykonanie testów:
   - `cargo test -p ghost-core --lib test_duplicate_bootstrap_seed_is_storage_level_noop -q`
   - `cargo test -p ghost-core --lib test_confirmed_bootstrap_promotes_existing_seed -q`
   - `cargo test -p ghost-core --lib test_weaker_bootstrap_cannot_overwrite_confirmed_curve -q`
   - `cargo test -p ghost-core --lib test_out_of_order_repair_is_rejected -q`
   - `cargo test -p ghost-core --lib commit_history_reports_noop_for_existing_history -q`
   - `cargo test -p ghost-core --lib test_append_live_rejects_slot_zero -q`
   - `cargo test -p ghost-launcher --lib wal_recovery::tests::replay_duplicate_bootstrap_seed_is_noop_under_storage_arbitration -- --exact -q`
8. odnotowanie, że wskazane testy przeszły, a kompilacja kontrolna dla `seer` i `ghost-launcher` zakończyła się powodzeniem.
