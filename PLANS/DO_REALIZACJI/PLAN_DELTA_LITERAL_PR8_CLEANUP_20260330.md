# Plan delta — literalne domknięcie PR8 po production cutover (2026-03-30)

## Cel dokumentu

Ten plan **nie** otwiera ponownie PR3B / PR5 / PR6 / PR7.

Ten plan istnieje wyłącznie po to, aby doprowadzić repo do stanu, w którym końcowe sformułowania z `PLANS/REFACTOR.md` o PR8 stają się prawdziwe **literalnie**, a nie tylko operacyjnie.

Punkt wyjścia jest już ustalony:

- production hot-path działa na `SessionManager` + `PoolObservationSession` + `AccountStateCore`,
- `GhostEvent::NewPoolDetected` nie zasila już `OracleRuntime.pools`,
- startup wymusza canonical `AccountUpdate` ingest przy `account_state_core.enable=true`.

Pozostały zakres to **repo cleanup / symbolic deletion / compat ballast removal**.

---

## Twarde założenia

### Co już uznajemy za zamknięte i nietykalne

Nie wolno w tym planie ponownie otwierać jako workstreamów:

- PR3B runtime session cutover,
- PR5 checkpoint/materialization wiring,
- PR6 feature-driven policy cutover,
- PR7 canonical truth migration.

Dopuszczalne są tylko **minimalne poprawki uboczne**, jeśli okażą się konieczne do usunięcia legacy symboli.

### Czego nie wolno robić

Nie wolno:

- zmieniać policy math Gatekeepera,
- zmieniać kontraktu `AccountStateCore` jako SSOT,
- wracać do compat flow „na wszelki wypadek”,
- dodawać nowych fallbacków,
- dokładać nowych funkcji rolloutowych,
- zmieniać Seer/Trigger/Jito poza tym, co jest bezpośrednio wymagane przez literalny cleanup PR8.

### Jedyna dozwolona oś zmian

Wolno tylko:

1. wycinać legacy symbole,
2. przesuwać compat helpery do `#[cfg(test)]` / test helperów,
3. przepinać testy z legacy wrapperów na docelowy API,
4. usuwać stare feature-gate semantics, jeśli produkcyjny kontrakt jest już wymuszony gdzie indziej.

---

## Aktualny realny dług literalnego PR8

Na dziś repo nadal zawiera następujące elementy sprzeczne z literalnym wordingiem PR8:

1. `PerPoolOracleState` nadal istnieje w `ghost-launcher/src/oracle_runtime.rs`.
2. `OracleRuntime.pools` nadal istnieje jako legacy kontener stanu.
3. `register_new_pool(...)` nadal buduje `PerPoolOracleState` i zasila `self.pools`.
4. `register_pool_tx(...)`, `mark_pool_scored(...)`, `pool_count()`, `prune_stale_pools()` nadal mają compat gałęzie oparte o `self.pools`.
5. `GatekeeperBuffer::on_transaction(...)` nadal istnieje jako deprecated legacy API.
6. `PoolObservationSession::on_transaction(...)` nadal istnieje jako legacy scoring wrapper.
7. Seer nadal utrzymuje flagę `account_updates_enabled` oraz tx-only degraded path, mimo że produkcyjny launcher i tak wymusza canonical ingest przy `AccountStateCore`.
8. `ReconciliationRuntime` nadal używa języka i liczników „repair”, mimo że aktualny kontrakt jest diagnostyczny/monitoring-only.

To są dokładnie rzeczy do domknięcia. Nic więcej.

---

# Sekwencja wykonawcza

## Etap 1 — usunięcie legacy compat pool state z runtime

### Cel

Usunąć z głównego kodu runtime wszystkie symbole i helpery utrzymujące `PerPoolOracleState` / `OracleRuntime.pools`.

### Zakres obowiązkowy

#### 1.1 Usunąć definicję i pole legacy

Plik:

- `ghost-launcher/src/oracle_runtime.rs`

Zmiany:

- usunąć definicję `PerPoolOracleState`,
- usunąć pole `pools: RwLock<HashMap<Pubkey, Arc<Mutex<PerPoolOracleState>>>>`,
- usunąć `lookup_compat_pool_state(...)`,
- usunąć wszystkie prywatne helpery, które istnieją wyłącznie po to, by wspierać compat mapę.

#### 1.2 Zastąpić lub wyciąć publiczne helpery zależne od compat state

Plik:

- `ghost-launcher/src/oracle_runtime.rs`

Każdy z poniższych symboli musi zostać **albo** przepisany na session/runtime-state/detected-pool flow, **albo** przeniesiony do `#[cfg(test)]`, **albo** usunięty:

- `register_new_pool(...)`
- `register_pool_tx(...)`
- `pool_count()`
- `get_pool_tx_count()` — jeśli nadal odwołuje się pośrednio do compat assumptions
- `mark_pool_scored(...)`
- `prune_stale_pools(...)`
- `inspect_candidate_reserves(...)`
- `score_pool(...)`

#### 1.3 Zasada migracji testów

Jeśli test dziś używa `register_new_pool(...)`, to test należy przepiąć na jedną z dwóch dozwolonych ścieżek:

- `register_runtime_pool_detection(...)` + jawne zasianie wymaganych session/runtime-state danych,
- dedykowany helper testowy pod `#[cfg(test)]`, który nie przecieka do produkcyjnego builda.

### Exit criteria

- brak symbolu `PerPoolOracleState` w produkcyjnym kodzie,
- brak pola `OracleRuntime.pools`,
- brak produkcyjnego `register_new_pool(...)`,
- brak produkcyjnych gałęzi `if let Some(state_arc) = pools.get(...)` w runtime helpers,
- wszystkie testy wcześniej zależne od compat mapy przechodzą na docelowym albo jawnie test-only API.

### Forbidden scope

- nie wolno zmieniać logiki decyzji runtime,
- nie wolno przywracać legacy state pod nową nazwą,
- nie wolno rozbudowywać runtime state registry ponad to, co już istnieje.

---

## Etap 2 — literalne usunięcie deprecated inline scoring API

### Cel

Doprowadzić repo do stanu, w którym deprecated Gatekeeper inline path nie tylko nie bierze udziału w production flow, ale **nie istnieje jako aktywny publiczny runtime API**.

### Zakres obowiązkowy

#### 2.1 GatekeeperBuffer

Plik:

- `ghost-launcher/src/components/gatekeeper.rs`

Zmiany:

- usunąć `GatekeeperBuffer::on_transaction(...)`,
- usunąć związany z nim deprecated contract jako publiczny path,
- utrzymać wyłącznie:
  - `ingest_transaction_tracking_only(...)`
  - `prepare_feature_evaluation()`
  - `evaluate_from_features(...)`

#### 2.2 PoolObservationSession

Plik:

- `ghost-launcher/src/session/observation.rs`

Zmiany:

- usunąć `PoolObservationSession::on_transaction(...)` jako legacy wrapper,
- testy przepiąć na:
  - `session.ingest_transaction(...)`
  - `session.try_checkpoint(...)`
  - `session.materialize_features()`
  - evaluation helpers.

#### 2.3 Dokumentacja / komentarze

Pliki:

- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/session/observation.rs`
- testy / komentarze, które nadal opisują `on_transaction()` jako normalny runtime path

Zmiany:

- usunąć komentarze i nazewnictwo, które sugerują, że inline scoring to wciąż dopuszczalna główna ścieżka.

### Exit criteria

- brak publicznego runtime API `on_transaction(...)` dla GatekeeperBuffer,
- brak legacy wrappera `PoolObservationSession::on_transaction(...)`,
- testy i helpery repo nie bazują już na deprecated inline scoring path.

### Forbidden scope

- nie wolno ruszać feature-driven policy math,
- nie wolno zmieniać semantyki `evaluate_from_features(...)`.

---

## Etap 3 — Seer account path: zamknięcie flagi jako legacy ballast

### Cel

Doprowadzić repo do stanu zgodnego z literalnym kierunkiem PR8: canonical account-state path jest wymaganym produkcyjnym kontraktem, a stara flaga nie zaciemnia architektury.

### Zakres obowiązkowy

#### 3.1 Produkcyjna semantyka

Pliki:

- `off-chain/components/seer/src/config.rs`
- `off-chain/components/seer/src/lib.rs`
- `ghost-launcher/src/config.rs`
- `config.toml`

Zmiany:

Wykonać **jedną** z dwóch ścieżek i wybrać ją jawnie — bez hybrydy:

##### Opcja A — minimalna, preferowana

- zostawić `account_updates_enabled` tylko jako test/degraded field,
- usunąć jego znaczenie z production config narrative,
- ograniczyć jego użycie do test/support constructorów lub explicit degraded mode z `#[cfg(test)]` / jawnie nazwanych helperów,
- w normalnym path `handle_account_update(...)` nie może zależeć od runtime-config, jeśli launcher już wymusza production canonical ingest.

##### Opcja B — literalniejsza, bardziej agresywna

- całkowicie usunąć `account_updates_enabled` z Seer config/runtime,
- usunąć tx-only degraded branch,
- testy zastąpić jawnie odrębnym harness/degraded fixture.

#### 3.2 Zasada wykonawcza

Jeśli Opcja B wymagałaby szerokiego rozwalania harnessów lub rollout diagnostics, należy wybrać Opcję A.

Ten plan nie wymaga heroicznego „purge everything”; wymaga tylko, by production contract przestał być reprezentowany przez legacy flagę.

### Exit criteria

- production narrative repo nie sugeruje już, że `account_updates_enabled=false` jest normalnym trybem pracy przy aktywnym `AccountStateCore`,
- degraded/test-only path jest wyraźnie odseparowany od production contractu,
- brak architektonicznej dwuznaczności kto jest ownerem canonical AccountUpdate ingest.

### Forbidden scope

- nie wolno rozbudowywać Seer config poza to, co służy literalnemu cleanupowi,
- nie wolno reintrodukować tx/bootstrap-only jako równoprawnego production mode.

---

## Etap 4 — Reconciliation: literalne przejście na pure monitoring language i behavior

### Cel

Dopasować kod i nazewnictwo `ReconciliationRuntime` do już zadeklarowanego kontraktu diagnostycznego.

### Zakres obowiązkowy

Pliki:

- `ghost-core/src/shadow_ledger/reconciliation_runtime.rs`
- `ghost-core/src/shadow_ledger/reconciliation.rs`
- ewentualnie komentarze/logi w `ghost-launcher/src/oracle_runtime.rs`

Zmiany:

- usunąć lub przemianować język `repair applied`, jeśli rzeczywisty kontrakt jest monitoring-only,
- upewnić się, że runtime nie zapisuje żadnych nowych canonical writes pod pozorem reconciliation,
- jeśli licznik/enum `Repaired` nadal istnieje wyłącznie jako historyczny ballast, przepisać go na neutralne nazewnictwo obserwacyjne,
- doprecyzować granicę: drift observation tak, state repair nie.

### Exit criteria

- kod i logi ReconciliationRuntime nie sugerują, że jest drugim silnikiem napraw stanu,
- brak semantycznego konfliktu między komentarzami „diagnostic-only” a runtime nazewnictwem „repair”.

### Forbidden scope

- nie wolno przebudowywać całego subsystemu reconciliation,
- nie wolno zmieniać `AccountStateCore` / `ShadowLedger` ownership semantics.

---

# Kolejność obowiązkowa

Kolejność jest sztywna:

1. **Etap 1** — usuń `PerPoolOracleState` / `OracleRuntime.pools` / compat pool helpers
2. **Etap 2** — usuń deprecated inline scoring API
3. **Etap 3** — zamknij legacy semantykę `account_updates_enabled`
4. **Etap 4** — dopasuj reconciliation do pure monitoring contract

Nie wolno zaczynać od etapu 3 albo 4, jeśli etap 1 nadal trzyma aktywą compat mapę w repo.

---

# Minimalne acceptance criteria

PR8 można uznać za literalnie domknięty dopiero wtedy, gdy jednocześnie prawdziwe są wszystkie poniższe warunki:

1. W produkcyjnym kodzie nie istnieje `PerPoolOracleState`.
2. W produkcyjnym kodzie nie istnieje `OracleRuntime.pools` jako legacy stan per-pool.
3. Nie istnieje produkcyjny `register_new_pool(...)` budujący compat state.
4. Nie istnieje publiczny runtime path `GatekeeperBuffer::on_transaction(...)`.
5. Nie istnieje publiczny runtime wrapper `PoolObservationSession::on_transaction(...)`.
6. Produkcyjny contract `AccountUpdate` nie jest semantycznie reprezentowany przez legacy compatibility flagę.
7. Reconciliation nie jest opisywany ani implementowany jako repair engine.
8. Repo można przeszukać i nie znaleźć aktywnych legacy runtime paths „zostawionych na wszelki wypadek”.

---

# Najkrótszy wniosek wykonawczy

Aby PR8 stał się prawdziwy **literalnie**, nie trzeba już robić nowej architektury.

Trzeba tylko wykonać cztery wąskie operacje:

- wyciąć compat pool state,
- wyciąć deprecated inline scoring API,
- odseparować lub usunąć legacy `account_updates_enabled`,
- oczyścić reconciliation z repair semantics.

To jest cleanup repo i kontraktu, nie nowa faza runtime refaktoru.
