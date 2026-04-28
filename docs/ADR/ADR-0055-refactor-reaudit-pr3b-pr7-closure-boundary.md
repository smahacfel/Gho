# ADR-0055: Refactor re-audit — PR3B through PR7 closure boundary

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Po `ADR-0054` użytkownik dostarczył nową notę realizacyjną sugerującą, że ostatnie blokery PR7 zostały usunięte. Wymagane było ponowne sprawdzenie wyłącznie względem twardych checklist z `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md`, bez scope creep i bez rozmywania granicy między:
- faktycznym production cutoverem hot-path,
- a pozostawionym legacy cleanupem należącym do PR8.

Zakres tego re-audytu obejmował:
- `ghost-launcher/src/oracle_runtime.rs`,
- `ghost-launcher/src/session/manager.rs`,
- `ghost-launcher/src/session/observation.rs`,
- `off-chain/components/trigger/src/revolver_price_feed.rs`,
- oraz celowane testy dowodowe dla PR5/PR6/PR7.

## Decision

Przyjęto następujący zaktualizowany werdykt forensyczny:

| PR | Status po re-audycie 2026-03-30 |
|---|---|
| PR3B | **Closed / production-cutover achieved** |
| PR5 | **Closed / production-wired** |
| PR6 | **Closed / feature-driven runtime active** |
| PR7 | **Closed / canonical truth migration achieved with bounded fallbacks** |
| PR8 | **Open / legacy runtime cleanup still incomplete** |

### PR3B — zamknięty

Warunek `ADR-0054` dla PR3B wymagał production use-site’ów `SessionManager` oraz prowadzenia `pool_observation_task` przez `PoolObservationSession`, zamiast przez `PerPoolOracleState` jako główny nośnik decyzji runtime.

To jest obecnie spełnione:
- `OracleRuntime` posiada `session_manager: Arc<SessionManager>` oraz `account_state_core: Arc<AccountStateReducer>`.
- `new_with_config(...)` buduje współdzielony `AccountStateReducer` i przekazuje go do `SessionManager::new_with_account_state_core(...)`.
- `ensure_pool_observation_session(...)` używa produkcyjnie `get_session(...)` / `open_session(...)`.
- `finish_pool_observation(...)` używa `close_session(...)` i `remove_session(...)`.
- `pool_observation_task(...)` działa na `PoolObservationSession` i nie terminalizuje hot-path przez `PerPoolOracleState`.

### PR5 — zamknięty

Warunek `ADR-0054` dla PR5 wymagał realnego production wiring dla `try_checkpoint()` i `materialize_features()`.

To jest obecnie spełnione:
- `pool_observation_task(...)` po zaakceptowanym tx wykonuje `session.try_checkpoint(normalized_ts)`.
- `build_timeout_assessment_from_session(...)` używa `session.materialize_features()`.
- `materialize_terminal_features(...)` używa `session.materialize_features()` dla terminalnej oceny.

### PR6 — zamknięty

Warunek `ADR-0054` dla PR6 wymagał, aby runtime nie podejmował decyzji przez legacy `GatekeeperBuffer.on_transaction()`, tylko przez feature-driven policy flow.

To jest obecnie spełnione:
- produkcyjny hot-path używa `session.ingest_transaction(...)`, nie `session.on_transaction(...)`;
- `resolve_feature_trigger_outcome(...)` kieruje wynik do `evaluate_feature_driven_terminal_verdict(...)`;
- `evaluate_feature_driven_terminal_verdict(...)` wykonuje `buffer.prepare_feature_evaluation()` oraz `buffer.evaluate_from_features(features, gatekeeper_config)`;
- `build_timeout_assessment_from_policy_context(...)` korzysta z `build_assessment_from_features(...)` oraz `evaluate_policy_from_assessment(...)`.

W `oracle_runtime.rs` wywołania `.on_transaction(...)` pozostały wyłącznie w testach, nie w produkcyjnym hot-path.

### PR7 — zamknięty

Warunek `ADR-0054` dla PR7 wymagał odcięcia hot-path primary truth od `ShadowLedger`, przy pozostawieniu jedynie dozwolonych fallbacków bootstrap/degraded/diagnostic.

To jest obecnie spełnione:
- `process_account_update(...)` najpierw zasila `AccountStateCore`, następnie aktywną sesję przez `on_account_state_core_updated()`, a dopiero potem diagnostyczny `reconciliation_runtime`.
- `resolve_price_context(...)` pyta najpierw `account_state_core.get_canonical_state(...)`, a dopiero potem korzysta ze snapshot fallback `shadow_ledger.get_latest_snapshot_internal(...)`.
- `resolve_gatekeeper_initial_reserves(...)` preferuje kolejno: canonical state, bootstrap state, snapshot fallback, genesis constants.
- implementacja `resolve_price_context(...)` nie używa `shadow_ledger.get_curve(...)`.
- implementacja `resolve_gatekeeper_initial_reserves(...)` nie używa `shadow_ledger.get_curve(...)`.
- produkcyjny hot-path wzbogaca tx przez `enrich_pool_tx_from_canonical_state(...)`.
- `off-chain/components/trigger/src/revolver_price_feed.rs` preferuje `AccountStateCore` (`try_canonical_price(...)`) i przechodzi do RPC fallback; implementacja nie używa `shadow_ledger.get_quote(...)` jako źródła prawdy.

Interpretacja zamknięcia PR7 jest ściśle ograniczona:
- `ShadowLedger` może pozostać jako snapshot fallback, cache write-through, replay/forensics oraz input dla diagnostyki driftu;
- `ShadowLedger` nie może wrócić jako primary truth source dla canonical hot-path.

### PR8 — nadal otwarty

PR8 nie jest zamknięty, ponieważ legacy runtime cleanup pozostaje realnie aktywny:
- `PerPoolOracleState` nadal istnieje jako typ runtime.
- `OracleRuntime` nadal utrzymuje `pools: RwLock<HashMap<Pubkey, Arc<Mutex<PerPoolOracleState>>>>`.
- rejestracja puli nadal konstruuje `PerPoolOracleState::new(...)` i wkłada go do `self.pools`.
- `lookup_compat_pool_state(...)`, `build_runtime_state_pool_snapshot(...)`, `lookup_base_mint_for_pool(...)` i powiązane compatibility flows nadal czytają legacy runtime state.
- `remove_pool(...)` nadal sprząta zarówno legacy `pools`, jak i nowe session state, co potwierdza trwanie modelu hybrydowego na warstwie cleanup/compat.

Wniosek operacyjny: zamknięcie PR3B/PR5/PR6/PR7 nie oznacza zamknięcia PR8.

## Architectural Impact

System nie powinien być już opisywany jako „PR3B–PR7 incomplete hybrid cutover”.

Poprawny opis po tym re-audycie brzmi:
- runtime hot-path obserwacji, checkpointingu, feature materialization, policy evaluation i primary truth został przepięty na nową architekturę;
- legacy runtime state nadal istnieje jako warstwa compatibility/cleanup i dlatego PR8 pozostaje otwarty.

To zmienia priorytety wykonawcze:
- nie należy ponownie otwierać PR3B/PR5/PR6/PR7 jako głównych workstreamów,
- dalsze prace powinny być ograniczone do PR8 cleanup albo do punktowych regresji wykrytych w już zamkniętych etapach.

## Risk Assessment

**Rate: Medium**

Ryzyka:
- błędne uznanie, że PR8 też jest zamknięty, mimo że legacy runtime state nadal żyje;
- regresja, jeśli ktoś ponownie przywróci `ShadowLedger.get_curve()` / `get_quote()` do hot-path truth decisions;
- rozmycie granicy odpowiedzialności między `AccountStateCore` a compatibility runtime state.

Ryzyko zostało obniżone przez obecność testów inwariantów i przez wyraźne rozdzielenie: PR7 zamknięty, PR8 otwarty.

## Consequences

Co staje się łatwiejsze:
- można uczciwie zamknąć spór, czy PR7 nadal blokują stare truth-source leaks;
- można zawęzić pozostały zakres wyłącznie do PR8 cleanup;
- łatwiej utrzymać SSOT: `AccountStateCore` jest primary truth dla hot-path.

Co pozostaje trudniejsze:
- repo nadal utrzymuje warstwę kompatybilności i cleanupu, więc pełne uproszczenie runtime nie zostało jeszcze osiągnięte;
- trzeba pilnować, żeby compatibility helpers nie zaczęły ponownie uczestniczyć w decyzjach hot-path.

## Alternatives Considered

### 1. Pozostawić werdykt `ADR-0054` bez zmian

Odrzucone, ponieważ nowy stan runtime i testów dowodowych pokazuje rzeczywisty production cutover dla PR3B/PR5/PR6 oraz usunięcie wcześniejszych blokerów PR7.

### 2. Oznaczyć również PR8 jako zamknięty

Odrzucone, ponieważ byłoby to niezgodne z aktualnym kodem:
- `PerPoolOracleState` nadal jest tworzony i przechowywany,
- `pools` nadal istnieje,
- compatibility lookupi i cleanup nadal obsługują legacy runtime state.

### 3. Otworzyć ponownie PR1/PR2 jako część tego re-audytu

Odrzucone jako scope creep i sprzeczne z `ADR-0054`. Obecny dowód nie wskazuje, aby PR1/PR2 były głównym niedomkniętym zakresem.

## Validation Steps

Weryfikacja wykonana wprost na źródłach i testach:

1. Inspekcja kodu:
   - `ghost-launcher/src/oracle_runtime.rs`
   - `ghost-launcher/src/session/manager.rs`
   - `ghost-launcher/src/session/observation.rs`
   - `off-chain/components/trigger/src/revolver_price_feed.rs`

2. Potwierdzone zielone testy dowodowe:
   - `cargo test -p ghost-launcher --lib pool_observation_task_wires_pr5_checkpoint_and_materialization`
   - `cargo test -p ghost-launcher --lib build_timeout_assessment_from_materialized_features_populates_pr6_fields`
   - `cargo test -p ghost-launcher --lib resolve_feature_trigger_outcome_terminalizes_without_legacy_on_transaction`
   - `cargo test -p ghost-launcher --lib test_process_account_update_primes_active_session_account_state`
   - `cargo test -p ghost-launcher --lib test_pr7_invariant_oracle_runtime_keeps_canonical_truth_primary`
   - `cargo test -p trigger --lib shadow_ledger_price_oracle_does_not_use_deprecated_shadow_quote_truth`

3. Dodatkowe negatywne inwarianty do utrzymania:
   - brak produkcyjnego `shadow_ledger.get_curve(...)` w `oracle_runtime.rs` hot-path truth helpers,
   - brak produkcyjnego `shadow_ledger.get_quote(...)` w `revolver_price_feed.rs`,
   - brak produkcyjnego `.on_transaction(...)` w `oracle_runtime.rs` poza testami.
