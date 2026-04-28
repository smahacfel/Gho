# ADR-0072 — Audyt stanu refaktoru architektury względem `PLANS/REFACTOR.md` i `ADR-0054`

**Status:** Informational / audit  
**Data audytu:** 2026-04-01  
**Zakres:** `ghost-core`, `ghost-launcher`, `off-chain/components/seer`, ścieżki runtime związane z `AccountStateCore`, sesjami, gatekeeperem, `ShadowLedger`, reconciliation i post-buy runtime.

---

## 1. Cel audytu

Celem było sprawdzenie, czy architektura opisana w `PLANS/REFACTOR.md` jako docelowa oraz w `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md` jako stabilizowana / egzekwowana:

1. została faktycznie dowieziona w kodzie,
2. które etapy PR1–PR8 są realnie zamknięte,
3. które fragmenty nadal są tylko częściowo wdrożone,
4. które legacy ścieżki nadal biorą udział w aktywnym runtime.

Audyt został wykonany przez czytanie plików implementacyjnych i testowych oraz przez uruchomienie wybranych istniejących testów.

---

## 2. Werdykt skrócony

Najkrótsza prawda o repozytorium na dzień audytu:

- kod jest **bardziej zaawansowany niż sugeruje `ADR-0054`** w obszarach PR3–PR5,
- ale **nie jest jeszcze w pełni zgodny z docelowym stanem z `REFACTOR.md`**,
- szczególnie **PR6 i PR7 są tylko częściowo domknięte**,
- a **PR8 nie jest zamknięty**.

Najważniejsza rozbieżność względem wcześniejszej narracji:

- `SessionManager`, `PoolObservationSession`, `TxIntelligenceEngine`, checkpointing i `materialize_features()` są już **realnie wpięte w produkcyjny hot-path**,
- ale domyślny production policy path nadal nie jest czystym finalnym cutoverem PR6,
- a `ShadowLedger` nadal uczestniczy w kilku aktywnych ścieżkach poza bootstrap/fallback-only contractem.

---

## 3. Status PR1–PR8

| Etap | Status | Uzasadnienie |
|---|---|---|
| **PR1** | **completed** | Fundamenty istnieją: moduły `account_state_core`, `session`, `tx_intelligence`, `checkpoint` są wyeksportowane i mają docelowe typy domenowe. |
| **PR2** | **partial** | Dual-write i bootstrap promotion działają, ale cel walidacyjny nie jest w pełni spełniony: `ghost-core --test account_state_core_tests` aktualnie nie jest cały zielony. |
| **PR3** | **completed** | Produkcyjny observation flow działa przez `SessionManager` + `PoolObservationSession`; brak aktywnej mapy `OracleRuntime.pools` i brak aktywnego `PerPoolOracleState` jako głównego state carrier. |
| **PR4** | **completed** | `TxIntelligenceEngine` jest realnie karmiony z hot-path tx ingestu w sesji. |
| **PR5** | **completed** | Checkpointing i `materialize_features()` są uruchamiane w aktywnym runtime przed terminalnym werdyktem. |
| **PR6** | **partial** | Feature-driven verdict path istnieje i jest używany, ale domyślny config nadal zostawia aktywny branch `evaluate_from_features_legacy()`. |
| **PR7** | **partial** | `AccountStateCore` jest canonical-first w runtime obserwacyjnym, a `ReconciliationRuntime` jest diagnostic-only, ale `ShadowLedger` nadal żyje w aktywnych fallbackach i w live post-buy runtime. |
| **PR8** | **not completed** | Pozostają compatibility shimy, deprecated APIs i legacy decision/fallback branches. |

---

## 4. Najważniejsze ustalenia architektoniczne

### 4.1. `AccountStateCore` jest realnym canonical source w observation runtime

To jest już prawdziwe w aktywnej ścieżce `oracle_runtime`:

- `OracleRuntime` posiada `session_manager` i `account_state_core`, bez starej mapy `pools`  
  `ghost-launcher/src/oracle_runtime.rs:987-1032`
- `process_account_update()` buduje `AccountStateUpdate`, aplikuje go do `AccountStateCore`, a dopiero potem odpala reconciliation  
  `ghost-launcher/src/oracle_runtime.rs:1394-1457`, `1499-1516`
- `resolve_price_context()` czyta najpierw `account_state_core.get_canonical_state(&base_mint)`  
  `ghost-launcher/src/oracle_runtime.rs:1595-1636`
- tx enrichment jest canonical-first, a `ShadowLedger` jest tylko fallbackiem bootstrap/degraded  
  `ghost-launcher/src/oracle_runtime.rs:239-257`, `265-327`, `332-385`
- curve readiness w sesji jest canonical-first  
  `ghost-launcher/src/session/observation.rs:278-309`

To oznacza, że wcześniejsza teza z `ADR-0054`, jakoby runtime session cutover i truth migration nadal czekały „od PR3B wzwyż”, jest **częściowo zdezaktualizowana**.

### 4.2. `SessionManager` i `PoolObservationSession` są już głównym runtime ownership

Production flow otwiera, wykorzystuje i zamyka sesje:

- `SessionManager` zarządza `DashMap<Pubkey, SharedSession>`  
  `ghost-launcher/src/session/manager.rs:54-167`
- `PoolObservationSession` agreguje tx buffer, embedded gatekeeper buffer, tx intelligence, checkpoint engine, materializer i diagnostics  
  `ghost-launcher/src/session/observation.rs:32-57`
- runtime robi `open_session()` i `get_session()`  
  `ghost-launcher/src/oracle_runtime.rs:1863-1905`, `3636-3659`
- tx hot-path idzie przez sesję: `ingest_transaction()`, `try_checkpoint()`, `resolve_feature_trigger_outcome()`  
  `ghost-launcher/src/oracle_runtime.rs:5770-5842`
- cleanup sesji idzie przez `close_session()` i `remove_session()`  
  `ghost-launcher/src/oracle_runtime.rs:3662-3665`, `2360-2419`

Wniosek: **PR3 jest w praktyce domknięty**.

### 4.3. `TxIntelligenceEngine`, checkpointing i materialization nie są test-only

To jest realna warstwa hot-path:

- `PoolObservationSession::ingest_transaction()` odpala `tx_intelligence.on_transaction(...)` i odświeża snapshot features  
  `ghost-launcher/src/session/observation.rs:190-218`
- `PoolObservationSession::try_checkpoint()` ocenia trigger i tworzy checkpoint  
  `ghost-launcher/src/session/observation.rs:252-276`
- `PoolObservationSession::materialize_features()` buduje realny `MaterializedFeatureSet`  
  `ghost-launcher/src/session/observation.rs:311-369`
- `CheckpointEngine` i `ObservationFeatureBuilder` mają pełną aktywną implementację  
  `ghost-core/src/checkpoint/engine.rs:67-190`, `ghost-core/src/checkpoint/feature_builder.rs:81-103`
- produkcyjny tx task wywołuje `try_checkpoint()` po zaakceptowanym tx  
  `ghost-launcher/src/oracle_runtime.rs:5835-5841`
- terminalny verdict materializuje features przed oceną  
  `ghost-launcher/src/oracle_runtime.rs:3721-3780`

Wniosek: **PR4 i PR5 są w kodzie realnie dowiezione**.

### 4.4. PR6 jest tylko częściowo dowieziony

Najważniejszy niuans:

- produkcyjny ingress nie używa już starego `GatekeeperBuffer.on_transaction()`; ta ścieżka jest ograniczona do `#[cfg(test)]`  
  `ghost-launcher/src/components/gatekeeper.rs:3686-3710`
- produkcyjny ingest używa `ingest_transaction_tracking_only()`  
  `ghost-launcher/src/components/gatekeeper.rs:2229-2295`
- finalny verdict jest budowany z features przez:
  - `evaluate_from_features(...)`
  - albo `evaluate_from_features_legacy(...)`  
  `ghost-launcher/src/components/gatekeeper.rs:1984-2066`, `2068-2144`
- runtime wybiera branch legacy, jeśli `use_three_layer_decision == false`  
  `ghost-launcher/src/oracle_runtime.rs:3740-3746`
- a default config nadal ma `use_three_layer_decision: false`  
  `ghost-brain/src/config/ghost_brain_config.rs:1264-1269`

Czyli:

- feature snapshot jest realnie obecny w verdict path,
- ale **domyślny production contract nadal utrzymuje aktywną legacy feature-eval semantics**.

To oznacza **PR6 = partial**, nie completed.

### 4.5. PR7 jest tylko częściowo dowieziony

Po stronie observation runtime:

- canonical-first truth jest realna,
- `ReconciliationRuntime` jest diagnostic-only, bez overwrite semantics  
  `ghost-core/src/shadow_ledger/reconciliation_runtime.rs:350-425`
- reconciler jest read-only  
  `ghost-core/src/shadow_ledger/reconciliation.rs:271-316`, `334-360`

Ale jednocześnie:

- `resolve_price_context()` nadal ma shadow snapshot fallback  
  `ghost-launcher/src/oracle_runtime.rs:1595-1636`
- `resolve_gatekeeper_initial_reserves()` spada do `shadow_ledger.get_latest_snapshot_internal()` po canonical i bootstrap state  
  `ghost-launcher/src/oracle_runtime.rs:1908-1937`
- live post-buy runtime nadal czyta cenę z `ShadowLedger` wprost  
  `ghost-launcher/src/components/post_buy_runtime.rs:661-689`, `733-757`

To nie jest już `ShadowLedger` jako primary canonical truth w głównym observation hot-path, ale też **nie jest jeszcze pełne ograniczenie do bootstrap/degraded/simulation only**.

### 4.6. PR8 jest wyraźnie niedomknięty

W kodzie nadal istnieją aktywne resztki legacy surface area:

- `evaluate_from_features_legacy()` nadal bierze udział w runtime branchingu  
  `ghost-launcher/src/components/gatekeeper.rs:2068-2144`
- deprecated APIs w `ShadowLedger` nadal istnieją i są używane w innych częściach systemu  
  `ghost-core/src/shadow_ledger/ledger.rs:1238-1243`, `1352-1354`
- `post_buy_runtime` ma aktywne shadow-based live reads  
  `ghost-launcher/src/components/post_buy_runtime.rs:733-757`
- `oracle_pipeline` nadal nosi compat/TODO ślad po legacy lokalnym per-pool wrapperze  
  `ghost-launcher/src/components/oracle_pipeline.rs:617-627`
- `account_updates_enabled` nadal istnieje jako compatibility gate w Seer/runtime  
  `off-chain/components/seer/src/config.rs:125-135`, `off-chain/components/seer/src/lib.rs:652-657`, `1888-1896`  
  `ghost-launcher/src/oracle_runtime.rs:6506-6520`, `6691-6824`, `7100-7131`

---

## 5. Ustalenia plik po pliku

### 5.1. Dokumenty planu

#### `PLANS/REFACTOR.md`

To jest referencyjny target architektoniczny:

- invariants końcowe po PR8  
  `PLANS/REFACTOR.md:9-26`
- reguły przejściowe PR1–PR8  
  `PLANS/REFACTOR.md:27-40`
- szczegółowy plan PR1–PR8  
  `PLANS/REFACTOR.md:94-980`

#### `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md`

Dokument stabilizujący zawiera poprawne kryteria zamknięcia PR6–PR8, ale jego tezy o stanie wykonania są już częściowo nieaktualne względem obecnego kodu:

- rollout matrix  
  `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md:571-582`
- checklists PR3–PR8  
  `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md:432-569`

### 5.2. `ghost-core`

#### `ghost-core/src/lib.rs`

- nowe moduły są wyeksportowane zgodnie z PR1  
  `ghost-core/src/lib.rs:18-38`

#### `ghost-core/src/account_state_core/types.rs`

- istnieją docelowe typy: `StatePhase`, `BootstrapHints`, `BootstrapPoolState`, `CanonicalPoolState`, `AccountStateUpdate`, `AccountStateFeatures`  
  `ghost-core/src/account_state_core/types.rs:5-160`

#### `ghost-core/src/account_state_core/monotonic_guard.rs`

- działa guard monotoniczny na `(slot, write_version, recv_seq)`  
  `ghost-core/src/account_state_core/monotonic_guard.rs:3-40`

Uwaga audytowa: plan PR1 mówił o minimalnej wersji `(slot, recv_seq)` i ewentualnym rozszerzeniu po PR8. Kod już teraz ma rozszerzenie o `write_version`, co jest architektonicznie sensowne i nie wygląda jak regres.

#### `ghost-core/src/account_state_core/reducer.rs`

- bootstrap i canonical state są rozdzielone  
  `ghost-core/src/account_state_core/reducer.rs:30-50`, `67-84`, `130-162`
- reducer jest incremental, nie rebuild-from-zero  
  `ghost-core/src/account_state_core/reducer.rs:97-119`
- dostępne są `get_canonical_state()`, `get_bootstrap_state()`, `get_features()`, `is_canonical()`  
  `ghost-core/src/account_state_core/reducer.rs:164-223`

**Nowa obserwacja z walidacji testowej:**  
`normalized_price_sol()` normalizuje rezerwy przez `1e9` / `1e6`  
`ghost-core/src/account_state_core/reducer.rs:286-299`

Przez to test `reducer_computes_price_change_and_velocity_from_previous_canonical_state` oczekuje obecnie innej skali ceny niż ta używana przez implementację.

#### `ghost-core/src/checkpoint/engine.rs`

- checkpoint triggers i checkpoint creation są kompletne  
  `ghost-core/src/checkpoint/engine.rs:67-190`

#### `ghost-core/src/checkpoint/feature_builder.rs`

- builder materializuje `MaterializedFeatureSet` i trajectory-derived fields  
  `ghost-core/src/checkpoint/feature_builder.rs:15-103`

#### `ghost-core/src/shadow_ledger/reconciliation.rs`

- reconciler jest read-only i operuje jako drift comparison layer  
  `ghost-core/src/shadow_ledger/reconciliation.rs:271-316`, `334-360`

#### `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`

- runtime reconciliation jest diagnostic-only  
  `ghost-core/src/shadow_ledger/reconciliation_runtime.rs:350-425`
- zachowane zostały jednak kompatybilne liczniki / enumy wokół `Repaired`  
  `ghost-core/src/shadow_ledger/reconciliation_runtime.rs:159-162`, `412-420`

#### `ghost-core/src/shadow_ledger/ledger.rs`

- `get_curve()` i `get_quote()` są oznaczone jako deprecated dla canonical runtime truth  
  `ghost-core/src/shadow_ledger/ledger.rs:1228-1243`, `1329-1354`

To wspiera tezę, że repozytorium samo już „wie”, iż PR7/PR8 nie są jeszcze literalnie dokończone.

### 5.3. `ghost-launcher`

#### `ghost-launcher/src/oracle_runtime.rs`

To jest najważniejszy plik audytu.

Stan pozytywny:

- `OracleRuntime` posiada `session_manager`, `account_state_core`, `runtime_pool_states`, a nie starą mapę pełnego per-pool stanu  
  `ghost-launcher/src/oracle_runtime.rs:987-1032`
- AccountUpdate najpierw hydratuje `AccountStateCore`, potem tylko diagnostycznie reconciliation  
  `ghost-launcher/src/oracle_runtime.rs:1394-1457`, `1499-1516`
- sesje są otwierane i używane w produkcyjnym tasku  
  `ghost-launcher/src/oracle_runtime.rs:3636-3659`, `5770-5842`
- terminal verdict jest feature-driven  
  `ghost-launcher/src/oracle_runtime.rs:3733-3790`

Resztki długu:

- price context i initial reserves mają shadow fallbacki  
  `ghost-launcher/src/oracle_runtime.rs:1595-1636`, `1908-1937`
- istnieje jawny compatibility gate `account_updates_enabled` dla degraded/test mode  
  `ghost-launcher/src/oracle_runtime.rs:6506-6520`, `6691-6824`, `7100-7131`

#### `ghost-launcher/src/session/manager.rs`

- `SessionManager` spełnia docelową rolę ownership layer  
  `ghost-launcher/src/session/manager.rs:54-167`

#### `ghost-launcher/src/session/observation.rs`

- sesja jest realnym kontenerem runtime state  
  `ghost-launcher/src/session/observation.rs:32-57`
- tx ingest, account refresh, checkpointing i materialization są realne  
  `ghost-launcher/src/session/observation.rs:190-218`, `220-249`, `252-369`

Ważny niuans:

- `current_account_features()` ma fallback bootstrap/legacy semantics, jeśli canonical updates jeszcze nie przyszły  
  `ghost-launcher/src/session/observation.rs:467-535`

To jest zgodne z przejściową fazą, ale oznacza, że część runtime nadal działa w logice „pre-canonical”.

#### `ghost-launcher/src/tx_intelligence/engine.rs`

- engine realnie akumuluje features i risk flags z tx feedu  
  `ghost-launcher/src/tx_intelligence/engine.rs:150-357`

#### `ghost-launcher/src/components/gatekeeper.rs`

Stan:

- produkcyjny ingest = `ingest_transaction_tracking_only()`  
  `ghost-launcher/src/components/gatekeeper.rs:2229-2295`
- docelowy path = `evaluate_from_features()`  
  `ghost-launcher/src/components/gatekeeper.rs:1984-2066`
- legacy feature-eval path nadal istnieje i jest aktywny branchowo  
  `ghost-launcher/src/components/gatekeeper.rs:2068-2144`
- stare `on_transaction()` jest już test-only  
  `ghost-launcher/src/components/gatekeeper.rs:3686-3710`

#### `ghost-launcher/src/components/gatekeeper_policy.rs`

- policy engine na `MaterializedFeatureSet` istnieje i jest kompletne  
  `ghost-launcher/src/components/gatekeeper_policy.rs:45-165`, `360-520`

#### `ghost-launcher/src/components/gatekeeper_commit_loop.rs`

- commit/handoff layer wygląda zgodnie z planowaną rolą „bez zmian roli”  
  `ghost-launcher/src/components/gatekeeper_commit_loop.rs:28-177`

Nie wygląda na główne źródło długu refaktoru.

#### `ghost-launcher/src/components/post_buy_runtime.rs`

To jest jeden z najważniejszych powodów, dla których PR7/PR8 nie można uznać za domknięte:

- live sell loop bierze cenę z `ShadowLedger`  
  `ghost-launcher/src/components/post_buy_runtime.rs:661-689`
- `read_price_from_shadow()` ma nawet legacy direct-mint fallback  
  `ghost-launcher/src/components/post_buy_runtime.rs:730-757`

To jest aktywne live dependency od `ShadowLedger`, nie tylko forensics/simulation/bootstrap.

#### `ghost-launcher/src/components/oracle_pipeline.rs`

- pipeline nadal jest konstruowany wokół `Arc<ShadowLedger>`  
  `ghost-launcher/src/components/oracle_pipeline.rs:58-65`, `222-260`
- pozostaje TODO mówiące, że telemetry nadal czeka na pełne przejście na canonical runtime/session artifacts  
  `ghost-launcher/src/components/oracle_pipeline.rs:617-627`

### 5.4. `off-chain/components/seer`

#### `off-chain/components/seer/src/config.rs`

- `account_updates_enabled` nadal istnieje jako explicit compatibility flag  
  `off-chain/components/seer/src/config.rs:125-135`

#### `off-chain/components/seer/src/lib.rs`

Pozytyw:

- AccountUpdate jest opisany jako primary canonical feed do `OracleRuntime` / `AccountStateCore`  
  `off-chain/components/seer/src/lib.rs:1970-2002`

Dług:

- wciąż istnieje gate `account_updates_enabled`
- tx-only / degraded mode jest nadal wspierany jako jawny kontrakt  
  `off-chain/components/seer/src/lib.rs:652-657`, `1573-1598`, `1883-1896`

To jest kolejny powód, dla którego PR8 nie jest gotowy.

---

## 6. Walidacja testowa wykonana w audycie

Uruchomione zostały istniejące testy punktowo związane z refaktorem.

### 6.1. Testy, które przeszły

#### `ghost-core`

- `cargo test -q -p ghost-core --test checkpoint_engine_tests -- --nocapture`  
  **wynik:** `4 passed; 0 failed`
- `cargo test -q -p ghost-core --test feature_builder_tests -- --nocapture`  
  **wynik:** `3 passed; 0 failed`

#### `ghost-launcher`

- `cargo test -q -p ghost-launcher --lib resolve_feature_trigger_outcome_terminalizes_without_legacy_on_transaction -- --nocapture`  
  **wynik:** pass
- `cargo test -q -p ghost-launcher --lib test_oracle_runtime_pool_registration_wires_into_reconciliation_runtime -- --nocapture`  
  **wynik:** pass
- `cargo test -q -p ghost-launcher --lib test_oracle_runtime_pool_deregistration_removes_from_reconciliation_runtime -- --nocapture`  
  **wynik:** pass

### 6.2. Test, który nie przeszedł

- `cargo test -q -p ghost-core --test account_state_core_tests -- --nocapture`  
  **wynik:** `2 passed; 1 failed`

Nieprzechodzący przypadek:

- `reducer_computes_price_change_and_velocity_from_previous_canonical_state`

Objaw:

- test oczekuje `state.price_sol == 1.5`
- implementacja zwraca `0.0015`

Powiązane miejsca:

- oczekiwanie testu  
  `ghost-core/tests/account_state_core_tests.rs:173-180`
- normalizacja ceny w reducerze  
  `ghost-core/src/account_state_core/reducer.rs:286-299`

Interpretacja audytowa:

- architektura PR2 jest wdrożona,
- ale **warstwa kontraktu testowego dla `AccountStateCore` nie jest w pełni zielona**,
- dlatego PR2 nie może być uczciwie oznaczony jako całkowicie zamknięty operacyjnie.

---

## 7. Najważniejsze rozbieżności względem stanu docelowego

1. **`ADR-0054` zaniża faktyczny postęp w PR3–PR5**  
   Dzisiejszy kod ma już realny session hot-path, tx intelligence, checkpointing i materialization.

2. **`REFACTOR.md` przeszacowuje gotowość PR6–PR8, jeśli ktoś zakładał „zrobione poza PR8”**  
   To nie jest prawda. PR6 i PR7 są tylko częściowe, PR8 nie jest domknięty.

3. **`AccountStateCore` nie jest jeszcze w pełni jedynym źródłem runtime truth wszędzie**  
   Observation runtime: prawie tak.  
   Post-buy / część compat flow: jeszcze nie.

4. **Legacy surface area nadal istnieje nie tylko jako martwy kod**  
   Najważniejsze aktywne przykłady:
   - `evaluate_from_features_legacy()`
   - `account_updates_enabled`
   - `post_buy_runtime` shadow price polling
   - deprecated `ShadowLedger` APIs

---

## 8. Rekomendowana kolejność domknięcia

Na podstawie realnego stanu kodu sensowna kolejność dalszych prac wygląda tak:

1. **Domknąć PR2 validation gap**  
   Uzgodnić kontrakt skali `price_sol` w `AccountStateCore` i doprowadzić `account_state_core_tests` do zielonego stanu.

2. **Domknąć PR6**  
   Przełączyć domyślny production config na pełny docelowy feature-driven path albo usunąć branch `evaluate_from_features_legacy()`.

3. **Domknąć PR7**  
   Usunąć aktywne live zależności od `ShadowLedger` poza jawnie dozwolonym bootstrap/degraded/simulation.

4. **Domknąć PR8**  
   Wyciąć compatibility gates i deprecated APIs, które dziś utrzymują legacy surface area.

---

## 9. Końcowy werdykt

Repozytorium **nie jest w stanie „ukończone wszystko poza PR8”**.

Bardziej precyzyjny stan jest taki:

- **PR1 — completed**
- **PR2 — partial**
- **PR3 — completed**
- **PR4 — completed**
- **PR5 — completed**
- **PR6 — partial**
- **PR7 — partial**
- **PR8 — not completed**

Najważniejsza praktyczna konkluzja:

To nie jest już „stary runtime z martwymi nowymi modułami obok”.  
To jest **hybryda z realnie wykonanym cutoverem PR3–PR5**, ale jeszcze **bez pełnego operacyjnego domknięcia PR6–PR8** i z jednym istotnym sygnałem regresji / niespójności kontraktu w testach PR2.
