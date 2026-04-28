# ADR-0015: Weryfikacja realizacji Fazy 3 normalizacji semantyki eventów cross-source

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Użytkownik zlecił zimny audit realizacji **Fazy 3** z dokumentu `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`.

Literalny kontrakt tej fazy wymagał domknięcia następujących punktów:

- downstream events mają nieść wspólny semantic envelope,
- envelope musi opisywać co najmniej:
  - `source_kind`,
  - `event_truth_kind`,
  - `slot_quality`,
  - `timestamp_quality`,
  - `completeness`,
- ma istnieć **jedna warstwa normalizacji** pomiędzy ingestem a routingiem,
- `synthetic` i `raw` nie mogą być dalej traktowane milcząco jako równoważne,
- downstream heurystyki mają czytać jawny envelope zamiast inferować jakość eventu po `source`, `synthetic` albo brakach `slot`,
- telemetryka ma emitować `event_semantic_total{source_kind=..., truth_kind=..., slot_quality=..., timestamp_quality=..., completeness=...}`.

Audit objął główne ścieżki runtime w:

- `ghost-core/src/event_semantics.rs`
- `ghost-core/src/lib.rs`
- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/ipc.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/events.rs`
- `ghost-launcher/src/components/seer.rs`
- `ghost-launcher/src/components/snapshot_listener.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-brain/src/oracle/snapshot_engine.rs`
- `docs/ADR/20260320_production_pipeline_data_flow.md`
- `PLANS/FAZA0_ZAMROZENIE_KONTRAKTOW_I_BLAST_RADIUS_20260320.md`

## Decision

Uznano, że **Faza 3 została zrealizowana zgodnie z planem i może zostać zaakceptowana**.

## Evidence

### 1. Powstał wspólny semantic envelope w `ghost-core`

W `ghost-core/src/event_semantics.rs` wprowadzono pełny kontrakt semantyczny cross-source:

- `SourceKind`
  - `Grpc`
  - `GeyserWs`
  - `HeliusWs`
  - `PumpPortal`
- `EventTruthKind`
  - `RawChain`
  - `AdaptedChain`
  - `Synthetic`
- `SlotQuality`
  - `Present`
  - `Absent`
  - `Estimated`
- `TimestampQuality`
  - `Chain`
  - `Adapter`
  - `WallClock`
- `EventCompleteness`
  - `Full`
  - `Partial`
- `EventSemanticEnvelope`

Typy i helpery zostały zre-eksportowane w `ghost-core/src/lib.rs`, więc downstream crate'y korzystają z jednego wspólnego kontraktu, a nie z lokalnych interpretacji.

### 2. Istnieje jedna warstwa normalizacji na ścieżce transakcyjnej

Centralna normalizacja została skupiona w `ghost-core/src/event_semantics.rs` przez:

- `normalize_transaction_semantics(...)`
- `normalize_account_update_semantics(...)`
- `source_kind_from_label(...)`
- `record_event_semantic_metric(...)`

Na produkcyjnej ścieżce Seera:

- `transaction_timestamp_quality(...)` w `off-chain/components/seer/src/lib.rs` wyznacza jakość czasu,
- `transaction_semantic_from_event(...)` deleguje do `normalize_transaction_semantics(...)`,
- `handle_trade_event(...)` przypisuje `trade.semantic = normalize_transaction_semantics(...)`,
- create path przypisuje `candidate.semantic = transaction_semantic_from_event(...)` przed wysyłką downstream.

To spełnia planowy warunek, aby normalizacja semantyki była wykonywana raz na wejściu Seera, a nie rozrzucona po wielu odbiorcach.

### 3. Semantic envelope jest przenoszony end-to-end przez canonical path

Audit potwierdził obecność `EventSemanticEnvelope` w całym głównym łańcuchu danych:

- `seer::types::TradeEvent`
- `seer::types::CandidatePool`
- `seer::ipc::DetectedAccountUpdateEvent`
- `ghost-launcher::events::DetectedPool`
- `ghost-launcher::events::PoolTransaction`
- `ghost-launcher::events::AccountUpdateEvent`
- `ghost-brain::oracle::snapshot_engine::TxEvent`

To oznacza, że envelope nie kończy życia na granicy Seera, tylko przechodzi przez bridge i dalej do runtime/brain.

### 4. `GhostEvent::AccountUpdate` przestał gubić producer-side metadata

W `ghost-launcher/src/events.rs` wariant:

- `GhostEvent::AccountUpdate(AccountUpdateEvent)`

zastąpił wcześniejszy zawężony inline payload.

Nowy `AccountUpdateEvent` zachowuje:

- `semantic`,
- `base_mint`,
- `bonding_curve`,
- `curve_finality`,
- `sol_reserves`,
- `token_reserves`,
- `complete`,
- `slot`,
- `detected_at`,
- `sequence_number`.

Bridge w `ghost-launcher/src/components/seer.rs` rzeczywiście przenosi te pola bez utraty.

### 5. Bridge `TradeEvent -> PoolTransaction` zachowuje semantic i prawdziwe MPCF metadata

W `ghost-launcher/src/components/seer.rs::trade_event_to_pool_transaction(...)` bridge zachowuje:

- `trade.semantic`,
- `trade.mpcf_payload`,
- `trade.mpcf_payload_missing_reason`.

To domyka wcześniejszy problem, w którym downstream mógł dostać syntetyczny lub zubożony obraz raw payloadu.

### 6. `SnapshotListener` czyta envelope zamiast zgadywać jakość eventu

W `ghost-launcher/src/components/snapshot_listener.rs`:

- `build_tx_event(...)` kopiuje `pool_tx.semantic` do `TxEvent.semantic`,
- `is_raw_chain_semantic(...)` sprawdza jawnie `pool_tx.semantic.event_truth_kind == EventTruthKind::RawChain`,
- `forward_transaction(...)` przyznaje `DataSource::HardTruth` tylko wtedy, gdy event jest `RawChain`,
- wszystkie pozostałe ścieżki pozostają `DataSource::SoftTruth`.

To jest dokładnie ten kontrakt, którego plan wymagał: `Synthetic` i `AdaptedChain` nie są już cicho awansowane do jakości `HardTruth`.

### 7. `OracleRuntime` w ścieżce fingerprintów odrzuca `Synthetic`

W `ghost-launcher/src/oracle_runtime.rs::pool_tx_to_fingerprint_event(...)` stwierdzono jawne guardy:

- `Synthetic` => `None`,
- brak `slot_quality=Present` => `None`,
- dopiero potem wymagane jest `tx.slot?`.

To jest bezpośredni dowód, że downstream heurystyka o wyższym wymaganiu jakościowym czyta semantic envelope zamiast polegać na pośrednich heurystykach.

### 8. Telemetryka Fazy 3 istnieje

W `ghost-core/src/event_semantics.rs::record_event_semantic_metric(...)` emitowany jest licznik:

- `event_semantic_total`

z labelami:

- `source_kind`,
- `truth_kind`,
- `slot_quality`,
- `timestamp_quality`,
- `completeness`.

W Seerze licznik jest wywoływany na create path, trade path oraz account-update path.

### 9. Dokumentacja została zaktualizowana zgodnie z kodem

`docs/ADR/20260320_production_pipeline_data_flow.md` zawiera osobną sekcję `Semantic envelope cross-source`, która opisuje:

- mapowanie `grpc_* -> raw_chain`,
- `websocket` / `helius` -> `adapted_chain`,
- `pumpportal` -> `synthetic`,
- zakaz używania `synthetic` jako `DataSource::HardTruth`,
- degradację `timestamp_quality` do `wall_clock` przy fallbacku `DetectedPool.timestamp_ms = detected_ms`.

Dodatkowo `PLANS/FAZA0_ZAMROZENIE_KONTRAKTOW_I_BLAST_RADIUS_20260320.md` został uzupełniony o aktualny kontrakt `GhostEvent::AccountUpdate(AccountUpdateEvent)` z pełnym semantic envelope.

## Architectural Impact

Decyzja ustala, że po Fazie 3:

1. pipeline cross-source ma już **jawny kontrakt semantyczny**, a nie zestaw ukrytych interpretacji,
2. `RawChain`, `AdaptedChain` i `Synthetic` są rozróżniane systemowo,
3. jakość slotu i czasu jest częścią danych wejściowych downstream, a nie czymś inferowanym pośrednio,
4. bridge launcherowy nie obcina już semantic i producer-side metadata,
5. truth-sensitive odbiorcy (`SnapshotListener`, fingerprint path w `OracleRuntime`) konsumują envelope bezpośrednio.

To oznacza, że warunki wejścia do kolejnych faz są istotnie mocniejsze: system przestał traktować różne klasy eventów jak ten sam byt „bo jakoś podobnie wyglądają”.

## Risk Assessment

**Rate: Low**

Ryzyko względem celu Fazy 3 jest niskie, ponieważ:

- semantic envelope istnieje w jednym wspólnym module,
- canonical hot path faktycznie go propaguje,
- co najmniej dwa krytyczne downstream consumer pathy używają go jawnie do guardów jakościowych,
- regresje normalizacji i bridge'a zostały uruchomione i przeszły.

Pozostaje drobne ryzyko dyscypliny testowej:

- audit nie znalazł jeszcze osobnego, dedykowanego testu jednostkowego wyłącznie dla reguły `SnapshotListener: HardTruth tylko dla RawChain`,
- audit nie znalazł osobnego, dedykowanego testu jednostkowego wyłącznie dla guardu fingerprintowego `Synthetic -> None`.

Nie blokuje to jednak akceptacji Fazy 3, bo kod tych ścieżek jest jednoznaczny, znajduje się na canonical hot path i nie jest jedynie martwym scaffoldingiem.

## Consequences

Co stało się łatwiejsze:

- można już audytować downstream heurystyki po jawnym kontrakcie jakości eventu,
- łatwiej odróżnić dane raw-chain od adapted/synthetic,
- bridge i event bus nie rozmywają pochodzenia i jakości eventów,
- telemetryka może pokazać realny miks jakości eventów w produkcji.

Czego ta decyzja nie oznacza:

- nie oznacza pełnego wyzerowania warningów w repo,
- nie oznacza automatycznego domknięcia wszystkich dalszych faz planu,
- nie oznacza, że każdy peryferyjny test/fixture poza canonical path ma już semantycznie bogate asercje — część z nich jedynie dopina nowe pola przez `EventSemanticEnvelope::default()`.

## Alternatives Considered

### 1. Uznać Fazę 3 za nieukończoną, bo nie ma jeszcze pełnego `cargo check --all-targets`
Odrzucono, bo użytkownik jawnie zawęził scope do canonical path, a plan Fazy 3 dotyczy kontraktu semantycznego pipeline'u, nie pełnego historycznego długu testów peryferyjnych.

### 2. Uznać Fazę 3 za częściową, bo część prawd została potwierdzona audytem kodu, a nie osobnymi testami jednostkowymi
Odrzucono, bo plan wymagał przede wszystkim zmiany kontraktu i jego zużycia przez downstream. Audit wykazał, że krytyczne hot pathy realnie czytają envelope, a nie tylko transportują nowe pole.

### 3. Zaakceptować Fazę 3 z jawną adnotacją o niewielkiej luce testowej
Przyjęto w sensie praktycznym: faza jest zaakceptowana, ale ADR odnotowuje brak dedykowanych, osobnych testów dla dwóch konkretnych guardów downstream jako drobną uwagę jakościową, nie jako blocker.

## Validation Steps

Weryfikacja została wykonana przez:

1. odczyt literalnego kontraktu Fazy 3 z `PLANS/PLAN_UPORZADKOWANIA_ARCHITEKTURY_PIPELINE_20260320.md`,
2. odczyt implementacji semantic envelope i normalizacji w `ghost-core/src/event_semantics.rs`,
3. odczyt typów i IPC w Seerze:
   - `off-chain/components/seer/src/types.rs`
   - `off-chain/components/seer/src/ipc.rs`
   - `off-chain/components/seer/src/lib.rs`,
4. odczyt launcher bridge i event contracts:
   - `ghost-launcher/src/events.rs`
   - `ghost-launcher/src/components/seer.rs`
   - `ghost-launcher/src/components/snapshot_listener.rs`
   - `ghost-launcher/src/oracle_runtime.rs`,
5. odczyt odbiorcy w `ghost-brain/src/oracle/snapshot_engine.rs`,
6. odczyt zaktualizowanej dokumentacji:
   - `docs/ADR/20260320_production_pipeline_data_flow.md`
   - `PLANS/FAZA0_ZAMROZENIE_KONTRAKTOW_I_BLAST_RADIUS_20260320.md`,
7. wykonanie testów `ghost-core`:
   - `cargo test -p ghost-core event_semantics::tests --lib -- --nocapture`
8. ustalenie pełnych ścieżek testów launcherowych przez:
   - `cargo test -p ghost-launcher --lib -- --list | grep -E 'bridge_preserves_trade_semantics_and_mpcf_payload|detected_pool_fallback_downgrades_timestamp_quality_to_wall_clock|test_build_tx_event_mapping'`
9. wykonanie testów `ghost-launcher` po pełnych ścieżkach:
   - `cargo test -p ghost-launcher --lib components::seer::tests::bridge_preserves_trade_semantics_and_mpcf_payload -- --exact`
   - `cargo test -p ghost-launcher --lib components::seer::tests::detected_pool_fallback_downgrades_timestamp_quality_to_wall_clock -- --exact`
   - `cargo test -p ghost-launcher --lib components::snapshot_listener::tests::test_build_tx_event_mapping -- --exact`
10. potwierdzenie, że powyższe testy wykonały się jako realne uruchomienia (`running 1 test` / `running 5 tests`) i zakończyły się `ok`.
