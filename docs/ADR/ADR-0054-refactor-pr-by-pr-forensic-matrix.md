# ADR-0054: Refactor PR-by-PR Forensic Matrix

**Date:** 2026-03-29
**Status:** Accepted
**Author:** Ghost Father

## Context

Użytkownik zażądał forensycznego rozpisania planu `PLANS/REFACTOR.md` PR po PR, z rozróżnieniem między:
- implementacją artefaktów (moduły, typy, testy),
- a rzeczywistym production cutoverem runtime i usunięciem legacy paths.

Celem tego ADR jest ustalenie technicznego stanu PR1–PR8 oraz wskazanie właściwej kolejności domykania prac.

## Decision

Przyjęto następującą klasyfikację stanu planu refaktoru:

| PR | Status forensyczny |
|---|---|
| PR 1 | **Done / materially complete** |
| PR 2 | **Mostly done / parallel-integrated** |
| PR 3 | **Partial / artifact-complete, runtime cutover missing** |
| PR 4 | **Mostly done / module-complete, depends on later cutover** |
| PR 5 | **Partial / implemented, not production-wired** |
| PR 6 | **Partial / policy engine exists, runtime still legacy-driven** |
| PR 7 | **Partial / truth migration incomplete** |
| PR 8 | **Not done / final integration and cleanup not achieved** |

Twierdzenie „wszystkie 8 PR-ów zrealizowane kompletnie” jest niezgodne z repozytorium.

## PR-by-PR Matrix

### PR 1 — Kontrakty, typy i fundamenty nowej architektury

**Werdykt:** `Done / materially complete`

**Co jest dowiezione:**
- `ghost-core/src/lib.rs` eksportuje:
  - `pub mod account_state_core;`
  - `pub mod checkpoint;`
  - `pub mod session;`
  - `pub mod tx_intelligence;`
- istnieją moduły i typy planowane dla PR1:
  - `ghost-core/src/account_state_core/*`
  - `ghost-core/src/session/*`
  - `ghost-core/src/tx_intelligence/*`
  - `ghost-core/src/checkpoint/*`
- istnieją testy kontraktowe/fundamentowe:
  - `ghost-core/tests/pr1_contracts_foundations.rs`
  - `ghost-core/tests/refactor_invariants_tests.rs`

**Dowody:**
- `ghost-core/src/lib.rs:18,19,31,36`
- `PLANS/REFACTOR.md:94-375`

**Uwagi:**
- Implementacja poszła nawet dalej niż minimalny opis planu: monotonic guard używa już `write_version` jako tie-breakera pomocniczego.
- To nie jest blocker aktualnego długu; PR1 nie wygląda na miejsce, od którego trzeba zaczynać „naprawę”.

**Braki względem planu:**
- brak krytycznych braków wykazanych w audycie.

---

### PR 2 — AccountStateCore reducer i wejście account path

**Werdykt:** `Mostly done / parallel-integrated`

**Co jest dowiezione:**
- `AccountStateReducer` istnieje i jest zaimplementowany.
- `BootstrapPoolState`, `BootstrapHints`, `CanonicalPoolState`, `AccountStateUpdate` istnieją.
- monotonic update guard działa.
- `OracleRuntime` posiada `account_state_core: Arc<AccountStateReducer>`.
- `OracleRuntime` woła:
  - `apply_account_update(...)`
  - `register_pool_from_bootstrap(...)`
  - `get_canonical_state(...)`
- istnieją testy:
  - `ghost-core/tests/account_state_core_tests.rs`

**Dowody:**
- `ghost-core/src/account_state_core/reducer.rs`
- `ghost-core/src/account_state_core/monotonic_guard.rs`
- `ghost-launcher/src/oracle_runtime.rs:897,1327,2092,3244,11756,11808`
- `PLANS/REFACTOR.md:376-483`

**Co nie pozwala uznać tego za pełne domknięcie całego planu:**
- PR2 z definicji miał działać równolegle, więc sam w sobie nie jest problemem.
- Problem zaczyna się później: nowy state reducer został dodany, ale dalsze PR-y nie domknęły runtime cutoveru.

**Braki względem planu PR2:**
- brak krytycznego braku samego PR2; raczej jest to etap realnie dowieziony jako parallel path.

---

### PR 3 — PoolObservationSession i SessionManager

**Werdykt:** `Partial / artifact-complete, runtime cutover missing`

**Co jest dowiezione:**
- istnieją:
  - `ghost-launcher/src/session/mod.rs`
  - `ghost-launcher/src/session/manager.rs`
  - `ghost-launcher/src/session/observation.rs`
  - `ghost-launcher/tests/session_lifecycle_tests.rs`
- `SessionManager` jest zaimplementowany.
- `PoolObservationSession` jest zaimplementowana.
- sesja posiada embedded `GatekeeperBuffer`, diagnostics, timing, tx dedup i lifecycle.

**Dowody:**
- `ghost-launcher/src/session/manager.rs`
- `ghost-launcher/src/session/observation.rs`
- `PLANS/REFACTOR.md:484-602`

**Braki / dowód niedomknięcia:**
- brak use-site’ów `SessionManager` w `ghost-launcher/src/oracle_runtime.rs`.
- plan dla PR3B wymagał przepięcia `pool_observation_task` na sesje — tego nie potwierdzono.
- `PerPoolOracleState` nadal istnieje i jest aktywnie używany w runtime.
- runtime nadal posiada `pools: RwLock<HashMap<Pubkey, Arc<Mutex<PerPoolOracleState>>>>`.

**Dowody braków:**
- brak `SessionManager` / `open_session()` / `get_session()` / `close_session()` / `remove_session()` w `oracle_runtime.rs`
- `ghost-launcher/src/oracle_runtime.rs:685,893,1967,8212`

**Konkluzja forensyczna:**
- PR3A wygląda na dowiezione.
- **PR3B nie został faktycznie domknięty**.
- To jest pierwszy realny punkt, od którego należy wznowić prace nad cutoverem.

---

### PR 4 — TxIntelligenceEngine

**Werdykt:** `Mostly done / module-complete, depends on later cutover`

**Co jest dowiezione:**
- istnieją moduły:
  - `ghost-launcher/src/tx_intelligence/mod.rs`
  - `ghost-launcher/src/tx_intelligence/engine.rs`
  - `ghost-launcher/src/tx_intelligence/config.rs`
  - `ghost-launcher/tests/tx_intelligence_tests.rs`
- `TxIntelligenceEngine` ma:
  - `on_transaction()`
  - `compute_features()`
  - `get_risk_flags()`
- `PoolObservationSession` posiada `tx_intelligence: TxIntelligenceEngine`.
- dług PR3 o nieograniczonym `tx_buffer` został częściowo domknięty lepiej niż wstępna hipoteza audytu:
  - `tx_buffer` to `VecDeque<Arc<PoolTransaction>>`
  - z capem `DEFAULT_SESSION_TX_RING_CAPACITY = 128`

**Dowody:**
- `ghost-launcher/src/tx_intelligence/engine.rs:53,243,340`
- `ghost-launcher/src/session/observation.rs:40-50,94,118-129`
- `ghost-launcher/src/tx_intelligence/config.rs:5`
- `PLANS/REFACTOR.md:603-687`

**Braki / ograniczenia:**
- choć moduł istnieje i sesja go używa, sesja sama nie jest production hot-path runtime.
- więc PR4 jest funkcjonalnie wdrożony głównie w nowej ścieżce sesyjnej, ale nie jako dominująca ścieżka runtime.

**Konkluzja forensyczna:**
- nie wygląda na fake PR.
- wygląda na **realnie zaimplementowany moduł, którego pełna wartość blokuje się na niedomkniętym PR3/PR6**.

---

### PR 5 — CheckpointEngine i FeatureBuilder

**Werdykt:** `Partial / implemented, not production-wired`

**Co jest dowiezione:**
- istnieją:
  - `ghost-core/src/checkpoint/engine.rs`
  - `ghost-core/src/checkpoint/feature_builder.rs`
  - `ghost-core/tests/checkpoint_engine_tests.rs`
  - `ghost-core/tests/feature_builder_tests.rs`
- `PoolObservationSession` ma:
  - `checkpoint_engine: CheckpointEngine`
  - `feature_builder: ObservationFeatureBuilder`
  - `checkpoints: Vec<SessionCheckpoint>`
- sesja implementuje:
  - `try_checkpoint()`
  - `materialize_features()`

**Dowody:**
- `ghost-launcher/src/session/observation.rs:49-50,167,227`
- `PLANS/REFACTOR.md:688-770`

**Braki / dowód niedomknięcia:**
- `oracle_runtime.rs` nie wywołuje:
  - `try_checkpoint()`
  - `materialize_features()`
- czyli planowane wiring w `pool_observation_task` nie jest production-wired.

**Dowody braków:**
- brak matchy dla `try_checkpoint(` i `materialize_features(` w `ghost-launcher/src/oracle_runtime.rs`

**Konkluzja forensyczna:**
- PR5 jako implementacja modułów i testów istnieje.
- PR5 jako realna warstwa materializacji używana w runtime **nie jest domknięty**.

---

### PR 6 — Gatekeeper jako policy engine

**Werdykt:** `Partial / policy engine exists, runtime still legacy-driven`

**Co jest dowiezione:**
- istnieje `ghost-launcher/src/components/gatekeeper_policy.rs`.
- istnieją funkcje:
  - `build_assessment_from_features(...)`
  - `evaluate_hard_filters(...)`
  - `evaluate_policy(...)`
- `GatekeeperAssessment` ma `feature_snapshot`.
- `GatekeeperBuffer::evaluate_from_features(...)` istnieje.
- legacy `on_transaction()` jest oznaczony `#[deprecated]`.
- istnieją testy:
  - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
  - `ghost-launcher/tests/full_pipeline_integration.rs`

**Dowody:**
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs:1926,3363-3366`
- `PLANS/REFACTOR.md:771-867`

**Braki / dowód niedomknięcia:**
- `oracle_runtime.rs` nie woła `evaluate_from_features()`.
- runtime nie woła `build_assessment_from_features()`.
- runtime nadal jest napędzany przez legacy `GatekeeperBuffer.on_transaction(...)`.
- `evaluate_policy(...)` jest wołane na `GatekeeperAssessment`, ale assessment pochodzący z legacy path ma zwykle `feature_snapshot: MaterializedFeatureSet::default()`.

**Dowody braków:**
- `ghost-launcher/src/oracle_runtime.rs:5628` → `buffer.on_transaction(tx.clone())`
- `ghost-launcher/src/oracle_runtime.rs:5941` → `evaluate_policy(&assessment, &ctx.gatekeeper_config)`
- `ghost-launcher/src/components/gatekeeper.rs:2237-2265, 2410, 2445, 3316, 3356` → legacy assessment z `feature_snapshot: MaterializedFeatureSet::default()`
- brak `evaluate_from_features(` w `oracle_runtime.rs`
- użycie `evaluate_from_features()` potwierdzone w testach, nie w runtime:
  - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
  - `ghost-launcher/tests/full_pipeline_integration.rs:291`

**Konkluzja forensyczna:**
- PR6 jest jednym z najbardziej mylących etapów: kod istnieje, ale production runtime **nadal nie działa według planowanego feature-driven flow**.
- To nie jest „done”. To jest **policy module implemented, cutover not done**.

---

### PR 7 — Truth source migration

**Werdykt:** `Partial / truth migration incomplete`

**Co jest dowiezione:**
- `AccountStateCore` został wpięty do runtime równolegle.
- `ShadowLedger` ma deprecated annotations dla `get_curve()` i `get_quote()`.
- `ReconciliationRuntime` jest opisywany i implementowany jako warstwa diagnostyczna.

**Dowody:**
- `ghost-core/src/shadow_ledger/ledger.rs:1238-1242,1351-1354`
- `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
- `PLANS/REFACTOR.md:868-930`

**Braki / dowód niedomknięcia:**
- `oracle_runtime.rs` nadal używa `shadow_ledger.get_curve(...)` w runtime.
- `OracleRuntime` nadal posiada aktywne `reconciliation_runtime`.
- `PerPoolOracleState` i legacy enrichment nadal współistnieją w hot-path.
- nie ma dowodu pełnego przekierowania wszystkich canonical queries na `AccountStateCore`.

**Dowody braków:**
- `ghost-launcher/src/oracle_runtime.rs:924`
- `ghost-launcher/src/oracle_runtime.rs:1422`
- `ghost-launcher/src/oracle_runtime.rs:1668`
- `ghost-launcher/src/oracle_runtime.rs:685,893`

**Konkluzja forensyczna:**
- PR7 nie jest zamknięty.
- Migracja truth source zatrzymała się w stanie hybrydowym.

---

### PR 8 — Seer dual-ingest formalizacja, finalna integracja i cleanup

**Werdykt:** `Not done / final integration and cleanup not achieved`

**Co jest dowiezione:**
- istnieje `ghost-launcher/tests/full_pipeline_integration.rs`.
- `config.toml` oraz `ghost-launcher/src/config.rs` mają sekcje:
  - `[account_state_core]`
  - `[session]`
  - `[tx_intelligence]`
- Seer account update path ma wysoką wagę IPC/backpressure (`BackpressurePolicy::Block`).

**Dowody:**
- `config.toml:222,225,230`
- `ghost-launcher/src/config.rs:61,65,69,254-293`
- `off-chain/components/seer/src/ipc.rs:142,169,467,511`

**Braki / dowód niedomknięcia:**
- `SessionManager-only flow` nie jest osiągnięty.
- `PerPoolOracleState` nie został usunięty.
- deprecated Gatekeeper inline scoring path nie został usunięty.
- `enrich_pool_tx_from_shadow_ledger`-style truth dependency nadal żyje przez aktywne `shadow_ledger.get_curve(...)` w runtime.
- `account_updates_enabled` nadal istnieje jako feature gate i wspierany jest tx-only degraded mode.
- final cleanup legacy paths z planu PR8 nie nastąpił.

**Dowody braków:**
- `ghost-launcher/src/oracle_runtime.rs:685,893,1967,5628`
- `ghost-launcher/src/components/gatekeeper.rs:3363-3366`
- `off-chain/components/seer/src/lib.rs:653,826,1592,1884-1890,4525`
- brak `SessionManager` use-site w `oracle_runtime.rs`

**Konkluzja forensyczna:**
- PR8 jest najbardziej oczywiście niezamknięty.
- To właśnie PR8 miał usunąć shimy i legacy paths — a one nadal są aktywne.

## Architectural Impact

Najważniejszy efekt ustaleń jest taki, że repozytorium należy traktować jako:
- **hybrydę nowego i legacy runtime**, a nie jako zakończony refaktor,
- system z wieloma poprawnie dodanymi modułami,
- ale bez pełnego production cutoveru i bez końcowego cleanupu.

## Risk Assessment

**Rate: High**

Ryzyka dalszego udawania, że plan jest domknięty:
- błędne priorytety kolejnych prac,
- rollout decisions oparte na nieprawdziwym modelu runtime,
- utrzymywanie dwóch konkurencyjnych źródeł logiki i stanu,
- fałszywa pewność, że invariants PR8 już obowiązują.

## Consequences

Najważniejsza konsekwencja wykonawcza:

**Nie należy zaczynać od „dokańczania wszystkiego od PR1 po kolei”.**

Zamiast tego należy przyjąć kolejność zależnościową:
1. **PR 3B** — rzeczywisty cutover `pool_observation_task` na `SessionManager` / `PoolObservationSession`
2. **PR 4 runtime integration gate** — obowiązkowe potwierdzenie, że runtime session hot-path realnie karmi `TxIntelligenceEngine`
3. **PR 5 wiring** — realne `try_checkpoint()` i `materialize_features()` w runtime
4. **PR 6 cutover** — runtime ma wołać feature-driven policy path (`build_assessment_from_features` / `evaluate_from_features`), a nie legacy buffer scoring
5. **PR 7** — odcięcie hot-path canonical queries od `ShadowLedger`
6. **PR 8** — finalne usunięcie `PerPoolOracleState`, deprecated Gatekeeper pathów i feature-gate’ów degradacyjnych

PR1 i PR2 nie wyglądają na miejsce wymagające ponownego „gonienia od zera”.
PR4 jest w dużej mierze zaimplementowany i powinien zostać domknięty przez integrację wyżej wymienionych etapów, nie przez restart prac.

## Operational Closure Checklists

Poniższa sekcja zamienia wynik audytu na **plan wykonawczy bez luzu interpretacyjnego**.

### Hard execution rules

1. **Nie restartować liniowo od PR1.**
   - PR1 i PR2 traktować jako fundamenty już dowiezione.
   - Dopuszczalne są tylko poprawki regresyjne wynikające z późniejszego cutoveru.
  - To oznacza: **wolno dotykać PR1/PR2 punktowo, jeśli nowy cutover ujawni realny brak lub bug; nie wolno otwierać z nich osobnych głównych strumieni prac.**
2. **Nie zaczynać PR5/PR6/PR7/PR8 przed domknięciem PR3B.**
   - Bez runtime session cutover dalsze prace będą tylko kolejną warstwą martwych artefaktów.
3. **Nie zaczynać PR5 przed przejściem obowiązkowej bramki PR4.**
  - PR4 nie jest opcjonalnym „sanity passem”.
  - PR4 jest blokującym runtime integration gate między PR3B a PR5.
  - Jeśli runtime session path nie karmi jeszcze poprawnie `TxIntelligenceEngine`, wejście w PR5 jest zabronione.
4. **Nie oznaczać PR6 jako domkniętego, jeśli runtime nadal podejmuje decyzję na legacy `GatekeeperBuffer.on_transaction()`.**
5. **Nie oznaczać PR7 jako domkniętego, jeśli hot-path nadal odpytuje `ShadowLedger` jako truth.**
6. **Nie oznaczać PR8 jako domkniętego, jeśli `PerPoolOracleState` lub deprecated Gatekeeper scoring path nadal uczestniczą w production flow.**

### Właściwa kolejność wykonawcza

1. `PR3B` — runtime session cutover
2. `PR4 runtime integration gate` — obowiązkowa bramka po PR3B: runtime session hot-path musi faktycznie karmić `TxIntelligenceEngine`; **to nie jest restart PR4 jako osobnego dużego workstreamu, ale jest blokującym prerekwizytem PR5**
3. `PR5` — pierwszy pełny etap do domknięcia po przejściu obowiązkowej bramki PR4: checkpoint/materialization wiring
4. `PR6` — feature-driven policy cutover
5. `PR7` — canonical truth migration off `ShadowLedger`
6. `PR8` — final cleanup legacy runtime

W praktyce oznacza to:
- **pierwsze po PR3B idzie obowiązkowa bramka PR4**, a nie PR5,
- **PR5 wolno rozpocząć dopiero po zaliczeniu bramki PR4**, która potwierdzi poprawne karmienie `TxIntelligenceEngine` po cutoverze sesji,
- **nie** otwieramy osobnego dużego „PR4 redux”, ale też **nie wolno** sprowadzać PR4 do opcjonalnej lub pomijalnej wzmianki.

### PR 1 — checklista zamknięcia

**Status operacyjny:** nie otwierać ponownie jako samodzielnego workstreamu.

**Interpretacja praktyczna:**
- „nie ruszać” **nie** znaczy „zakaz jakiejkolwiek zmiany”.
- Znaczy: nie robić osobnego remontu PR1, dopóki brak nie blokuje aktualnego cutoveru.
- Jeśli przy PR3B/PR5/PR6 wyjdzie rzeczywisty defekt kontraktu, typu lub invariantu — poprawka jest dozwolona, ale ma być **minimalna i podporządkowana bieżącemu etapowi**.

**Do zrobić wyłącznie jeśli wyjdzie regresja:**
- utrzymać eksporty w `ghost-core/src/lib.rs`:
  - `account_state_core`
  - `checkpoint`
  - `session`
  - `tx_intelligence`
- utrzymać brak zależności `ghost-core -> ghost-launcher` w nowych typach domenowych
- utrzymać testy kontraktowe PR1 w stanie zielonym

**Exit criteria:**
- brak nowych importów z `ghost-launcher` w `ghost-core/src/account_state_core/**`, `session/**`, `checkpoint/**`, `tx_intelligence/**`
- testy kontraktowe/fundamentowe przechodzą

**Forbidden scope:**
- żadnego przepisywania typów tylko po to, żeby „udawać postęp”
- żadnego rozszerzania API bez bezpośredniej potrzeby wynikającej z cutoveru runtime

### PR 2 — checklista zamknięcia

**Status operacyjny:** nie restartować; traktować jako równoległy foundation already-in-place.

**Interpretacja praktyczna:**
- PR2 wolno dotknąć wtedy, gdy integracja PR7/PR8 ujawni brak w `AccountStateCore`, dual-write, bootstrap promotion albo monotonic ordering.
- Nie wolno natomiast robić z PR2 wygodnego workstreamu zastępczego tylko dlatego, że łatwiej „coś tam poprawić” niż domknąć realny dług w PR3B/PR5/PR6/PR7.

**Do utrzymania:**
- `OracleRuntime` nadal rejestruje bootstrap przez `AccountStateCore`
- `OracleRuntime` nadal aplikuje account updates do `AccountStateCore`
- monotonic update guard pozostaje aktywny
- bootstrap i canonical state pozostają rozdzielone

**Exit criteria:**
- `account_state_core` pozostaje aktywnie zasilany z `GhostEvent::AccountUpdate`
- `register_pool_from_bootstrap()` pozostaje w ścieżce rejestracji nowej puli
- brak regresji testów `ghost-core/tests/account_state_core_tests.rs`

**Forbidden scope:**
- nie przepisywać PR2 jako substytutu za brak cutoveru PR3–PR8
- nie wracać do `ShadowLedger` jako jedynego truth source

### PR 3 — checklista zamknięcia (to jest pierwszy realny blocker)

**Status operacyjny:** **domknąć teraz**.

**Musi zostać zrobione:**
- dodać produkcyjne użycie `SessionManager` w `ghost-launcher/src/oracle_runtime.rs`
- przepiąć lifecycle poola tak, aby runtime:
  - otwierał sesję przez `open_session()`
  - pobierał aktywną sesję przez `get_session()`
  - zamykał sesję przez `close_session()`
  - usuwał sesję przez `remove_session()` po cleanupie
- przepiąć `pool_observation_task` tak, aby stan obserwacji był trzymany w `PoolObservationSession`, a nie w aktywnie używanym `PerPoolOracleState`
- zakończyć aktywne tworzenie nowego runtime state przez `PerPoolOracleState::new(...)` w głównym flow

**Exit criteria:**
- `oracle_runtime.rs` ma produkcyjne callsite’y `SessionManager`
- `pool_observation_task` operuje na `PoolObservationSession`
- brak nowych runtime decision/state transitions opartych o `PerPoolOracleState`
- jeśli `PerPoolOracleState` zostaje tymczasowo w kodzie, to wyłącznie jako kompatybilny wrapper bez bycia głównym state carrierem

**Forbidden scope:**
- nie robić jeszcze truth migration z PR7
- nie robić jeszcze final cleanupu z PR8
- nie mieszać w policy semantics poza minimalnym adapterem potrzebnym do session cutoveru

### PR 4 — checklista zamknięcia

**Status operacyjny:** **obowiązkowa bramka blokująca przed PR5**; domknąć przez integrację po PR3B.

**Interpretacja praktyczna:**
- PR4 nie jest opcjonalny.
- PR4 nie jest krokiem, który wolno „zaliczyć słownie” lub pominąć dlatego, że moduły już istnieją.
- PR5 jest zablokowany, dopóki runtime session hot-path nie przejdzie pełnego runtime integration gate PR4.

**Musi zostać dopięte / zweryfikowane po cutoverze sesji:**
- każda transakcja obsługiwana przez runtime sesyjny ma przejść przez `TxIntelligenceEngine.on_transaction()`
- snapshot `TxIntelFeatures` w sesji musi być odświeżany w trakcie obserwacji
- bounded retention tx musi pozostać bounded:
  - `VecDeque`
  - cap `DEFAULT_SESSION_TX_RING_CAPACITY`
- brak bezpośredniego dostępu `TxIntelligenceEngine` do canonical state

**Exit criteria:**
- sesja w runtime rzeczywiście używa `tx_intelligence` jako warstwy behawioralnej
- brak nieograniczonej retencji tx per sesja
- testy `ghost-launcher/tests/tx_intelligence_tests.rs` pozostają zielone
- PR5 nie rusza, dopóki wszystkie powyższe warunki nie są spełnione jednocześnie

**Forbidden scope:**
- nie przenosić policy logic z powrotem do TxIntelligence
- nie robić w PR4 pracy należącej do PR6 lub PR7

### PR 5 — checklista zamknięcia

**Status operacyjny:** domknąć bezpośrednio po zaliczeniu obowiązkowej bramki PR4.

**Musi zostać zrobione:**
- w `oracle_runtime.rs` po akceptacji tx w obserwacji runtime ma wywoływać `session.try_checkpoint(now_ms)`
- przed podjęciem werdyktu runtime ma wywoływać `session.materialize_features()`
- `PoolObservationSession.checkpoints` ma być realnie zasilane w production path, a nie tylko w testach
- `checkpoint_count` oraz trajectory-derived fields mają być obecne w danych używanych do decyzji

**Exit criteria:**
- istnieją produkcyjne callsite’y `try_checkpoint()` w runtime
- istnieją produkcyjne callsite’y `materialize_features()` w runtime
- feature materialization nie jest ograniczona do testów integracyjnych

**Forbidden scope:**
- nie oznaczać PR5 jako zamkniętego tylko dlatego, że sesja ma metody i testy
- nie omijać `MaterializedFeatureSet` przez ręczne sklejanie danych w `oracle_runtime.rs`

### PR 6 — checklista zamknięcia

**Status operacyjny:** domknąć po faktycznym wiring PR5.

**Musi zostać zrobione:**
- runtime ma budować decyzję z feature-driven flow, a nie z legacy scoring flow
- produkcyjny flow ma wyglądać logicznie tak:
  - `session.on_transaction(...)`
  - `session.try_checkpoint(...)`
  - `features = session.materialize_features()`
  - `assessment = build_assessment_from_features(features, ...)` **albo** `evaluate_from_features(features, ...)`
  - `evaluate_policy(...)` na assessmentcie z realnym `feature_snapshot`
- `feature_snapshot` w production verdictach nie może być `MaterializedFeatureSet::default()`
- legacy `GatekeeperBuffer.on_transaction()` nie może pozostać głównym silnikiem decyzji runtime

**Exit criteria:**
- `oracle_runtime.rs` ma produkcyjne callsite’y `build_assessment_from_features(...)` lub `evaluate_from_features(...)`
- `oracle_runtime.rs` nie opiera decyzji buy/reject na assessmentach zbudowanych przez legacy inline scoring
- production verdict path niesie realny `feature_snapshot`

**Forbidden scope:**
- nie zostawiać policy engine jako „test-only luxury layer”
- nie uznawać deprecation annotation za substytut rzeczywistego cutoveru

### PR 7 — checklista zamknięcia

**Status operacyjny:** domknąć dopiero po PR6.

**Musi zostać zrobione:**
- przekierować canonical truth queries na `AccountStateCore`
- ograniczyć `ShadowLedger` do simulation / WAL / replay / forensics / fallback bootstrap-only zgodnie z planem
- wyeliminować hot-path użycie:
  - `shadow_ledger.get_curve(...)`
  - `shadow_ledger.get_quote(...)`
  jako primary truth source
- upewnić się, że `ReconciliationRuntime` nie wykonuje repair semantics, tylko diagnostic-only monitoring

**Exit criteria:**
- `oracle_runtime.rs` nie odpytuje `ShadowLedger` jako primary canonical truth
- `gatekeeper` / curve readiness nie bazuje na `ShadowLedger` jako primary live truth
- fallback do `ShadowLedger` jest ograniczony do jawnie dozwolonych ścieżek bootstrap/degraded

**Forbidden scope:**
- nie usuwać ShadowLedger simulation APIs potrzebnych do zdrowej roli systemowej
- nie oznaczać PR7 jako zamkniętego, jeśli w runtime pozostają canonical queries do `ShadowLedger`

### PR 8 — checklista zamknięcia

**Status operacyjny:** ostatni etap; dopiero po pełnym cutoverze PR3B/5/6/7.

**Musi zostać zrobione:**
- usunąć aktywne runtime użycie `PerPoolOracleState`
- usunąć aktywną mapę `OracleRuntime.pools` jako główny kontener legacy state, jeśli nadal istnieje w roli produkcyjnej
- usunąć deprecated Gatekeeper inline scoring path z production flow
- usunąć compatibility shimy, które podtrzymują legacy decision path po przejściu na session+features
- sformalizować Seer account path jako wymagany production path
- utrzymać ewentualne degraded/test-only flagi tylko wtedy, gdy nie podważają domyślnego production contractu

**Exit criteria:**
- brak production flow opartego o `PerPoolOracleState`
- brak production buy/reject path zależnego od deprecated `GatekeeperBuffer.on_transaction()`
- `SessionManager` jest jedynym aktywnym runtime ownerem sesji per pool
- `AccountStateCore` jest jedynym primary source of truth dla canonical market state

**Forbidden scope:**
- nie mylić „deprecated” z „removed”
- nie zostawiać legacy runtime paths „na wszelki wypadek” po ogłoszeniu zamknięcia PR8

### Minimalna tabela egzekucyjna: kto ma być goniony i o co dokładnie

| Etap | Czy gonić teraz | O co dokładnie |
|---|---|---|
| PR1 | Nie | Tylko regresje kontraktów/fundamentów |
| PR2 | Nie | Tylko utrzymanie dual-write i testów |
| PR3 | **Tak** | Runtime session cutover w `oracle_runtime.rs` |
| PR4 | **Tak, obowiązkowo po PR3B** | Blokujący runtime integration gate: `TxIntelligenceEngine` musi być realnie karmiony przez session hot-path; bez restartu modułu i bez osobnego dużego workstreamu |
| PR5 | **Tak, ale dopiero po zaliczeniu PR4 gate** | Runtime checkpoint + `materialize_features()` |
| PR6 | **Tak, po PR5** | Feature-driven policy path jako realny hot-path |
| PR7 | **Tak, po PR6** | Odcięcie truth queries od `ShadowLedger` |
| PR8 | **Tak, na końcu** | Legacy cleanup i finalne egzekwowanie invariantów |

## Alternatives Considered

### 1. Domykanie od PR1 do PR8 linearnie
Odrzucono. PR1 i PR2 są w dużej mierze dowiezione; prawdziwy dług leży od PR3B wzwyż.

### 2. Traktowanie testów integracyjnych jako dowodu zamknięcia PR8
Odrzucono. Testy dowodzą, że nowa ścieżka istnieje i działa w izolacji, ale nie dowodzą production cutoveru `oracle_runtime.rs`.

### 3. Uznanie PR6 za zamknięty tylko dlatego, że istnieje `gatekeeper_policy.rs`
Odrzucono. Runtime nadal bierze assessment z legacy `GatekeeperBuffer.on_transaction()`, a nie z `MaterializedFeatureSet` jako głównego źródła decyzji.

## Validation Steps

1. Przeczytano cały `PLANS/REFACTOR.md` i wyciągnięto wymagania PR1–PR8.
2. Zweryfikowano obecność planowanych plików i testów.
3. Zweryfikowano runtime callsite’y w `ghost-launcher/src/oracle_runtime.rs`.
4. Zweryfikowano implementacje:
   - `AccountStateReducer`
   - `PoolObservationSession`
   - `SessionManager`
   - `TxIntelligenceEngine`
   - `CheckpointEngine`
   - `ObservationFeatureBuilder`
   - `gatekeeper_policy.rs`
5. Zweryfikowano, które nowe ścieżki są używane tylko w testach, a nie w runtime.
6. Zweryfikowano aktywne legacy paths:
   - `PerPoolOracleState`
   - `GatekeeperBuffer.on_transaction()`
   - `shadow_ledger.get_curve(...)`
   - `account_updates_enabled` / tx-only degraded mode
   - aktywne deprecated methods w `gatekeeper.rs` i `shadow_ledger/ledger.rs`
