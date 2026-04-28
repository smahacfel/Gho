# PLAN WYKONAWCZY DOMKNIECIA REFAKTORU ARCHITEKTURY - 2026-04-01

## 0. Cel dokumentu

Ten dokument jest wykonawczym planem doprowadzenia repozytorium do stanu zgodnego z docelowa architektura opisana w `PLANS/REFACTOR.md`, z uwzglednieniem wszystkich ADR-ow od `ADR-0054` do `ADR-0072` oraz rzeczywistego stanu kodu zweryfikowanego w audycie.

Cel planu nie jest "zrobic cos podobnego do targetu". Celem jest:

1. domknac wszystkie pozostale braki architektoniczne,
2. nie otwierac ponownie zamknietych workstreamow bez realnej przyczyny,
3. usunac ambiguity miedzy target architecture, accepted ADR constraints i rzeczywistym kodem,
4. zminimalizowac ryzyko regresji praktycznie do zera przez sequencing, guardraile, staged validation i fail-closed rollout.

Ten plan zaklada brak presji czasowej i pelna dostepnosc zasobow. Oznacza to, ze priorytetem jest correctness, replayability, observability i literalna zgodnosc z target contract, a nie skroty wykonawcze.

---

## 1. Zrodla SSOT i zasada rozstrzygania konfliktow

### 1.1. Zrodla SSOT

Ten plan opiera sie na czterech warstwach prawdy:

1. `PLANS/REFACTOR.md`
   - to jest SSOT dla target architecture, koncowych invariantow i oczekiwanej sekwencji PR1-PR8.

2. `docs/ADR/ADR-0054-refactor-pr-by-pr-forensic-matrix.md`
   - to jest SSOT dla forensic closure criteria i operational checklists per PR.

3. `docs/ADR/ADR-0055` ... `docs/ADR/ADR-0071`
   - to sa SSOT dla accepted execution constraints, acceptance boundary, forbidden scope i pozniejszych doprecyzowan rolloutowych.

4. `docs/ADR/ADR-0072-refactor-architecture-audit-2026-04-01.md` + aktualny kod
   - to jest SSOT dla rzeczywistego stanu wejscia do tego planu.

### 1.2. Regula rozstrzygania konfliktow

Jesli wystepuje konflikt miedzy starszym ADR-em, ktory twierdzi, ze dany etap byl juz zamkniety, a aktualnym kodem i audytem, obowiazuje nastepujaca regula:

- **target contract i forbidden scope z zaakceptowanych ADR-ow pozostaja wazne,**
- **ale status wykonania bierze sie z aktualnego repozytorium i z ADR-0072.**

Inaczej mowiac:

- ADR-y `0055-0060` sa nadal wazne jako constraints, sequencing i acceptance language,
- ale nie wolno planowac na zalozeniu, ze repo faktycznie juz odpowiada ich closure claims, jesli aktualny kod tego nie potwierdza.

### 1.3. Wniosek wykonawczy

Plan nie "restartuje" refaktoru od zera.

Plan zaklada stan wejsciowy:

- PR1 = completed
- PR2 = partial
- PR3 = completed
- PR4 = completed
- PR5 = completed
- PR6 = partial
- PR7 = partial
- PR8 = not completed

To jest punkt startu do dalszych prac.

---

## 2. Stan wejsciowy po audycie

### 2.1. Co jest realnie dowiezione

1. `AccountStateCore` istnieje i jest aktywnie zasilany z `GhostEvent::AccountUpdate`.
2. `SessionManager` i `PoolObservationSession` sa aktywne w production hot-path.
3. `TxIntelligenceEngine` jest realnie karmiony przez sesyjny tx ingest.
4. `CheckpointEngine` i `materialize_features()` biora udzial w runtime przed terminalnym werdyktem.
5. `GatekeeperBuffer.on_transaction()` nie jest juz glownym production ingress path.
6. `ReconciliationRuntime` jest w praktyce diagnostic-only w observation runtime.

### 2.2. Co nadal pozostaje niedomkniete

1. `ghost-core --test account_state_core_tests` nie jest w pelni zielony; wystepuje rozjazd kontraktu `price_sol`.
2. Domyslny runtime nadal moze wejsc w `evaluate_from_features_legacy()` przez `use_three_layer_decision = false`.
3. `ShadowLedger` nadal uczestniczy w aktywnych fallbackach truth/readiness i w live `post_buy_runtime`.
4. `account_updates_enabled` nadal istnieje jako compatibility surface i zaciemnia architecture language.
5. Pozostaje legacy/compat ballast nalezacy do PR8.

### 2.3. Najwazniejszy execution insight

Najwieksze ryzyko nie lezy w "braku modulow". Najwieksze ryzyko lezy w:

- niespelnionym kontrakcie PR2,
- niepelnym domknieciu PR6 default path,
- niepelnej migracji truth source w PR7,
- oraz niepelnym literal cleanup PR8.

---

## 3. Twarde zasady wykonawcze wynikajace z ADR-0054 ... ADR-0072

### 3.1. Sekwencja jest nienegocjowalna

Docelowa kolejnosc dalszych prac jest nastepujaca:

1. Faza 0 - baseline, observability, freeze i guardraile
2. Faza 1 - PR2 contract closure
3. Faza 2 - PR6 default-cutover i final hardening policy path
4. Faza 3 - PR7 preconditions: canonical ingress health i bounded fallback proof
5. Faza 4 - PR7 truth-source completion poza observation runtime
6. Faza 5 - PR8 literal cleanup
7. Faza 6 - koncowa walidacja i staged rollout

### 3.2. Nie wolno ponownie otwierac PR3-PR5 jako glownych workstreamow

ADR-y `0055`, `0065` i `0072` lacznie wymuszaja nastepujaca interpretacje:

- PR3, PR4 i PR5 nie sa juz miejscem na nowy architecture redesign,
- wolno wykonywac tylko punktowe bugfixy lub adjustments potrzebne do bezpiecznego domkniecia PR2/PR6/PR7/PR8,
- nie wolno uzasadniac duzych zmian "bo i tak dotykamy tych plikow".

### 3.3. Nie wolno dokladac nowych legacy shims

W kazdej fazie obowiazuje zakaz:

- dodawania nowych fallbackow do `ShadowLedger` jako live truth,
- dodawania nowych publicznych helperow dla legacy verdict path,
- dodawania nowych config switchy, ktore pozwalaja obchodzic canonical production contract,
- dodawania nowych "tymczasowych" compat map/state carrierow.

### 3.4. Zachowac accepted hardening z ADR-0061, 0062, 0068, 0069, 0070, 0071

Ponizsze rzeczy sa juz accepted contract i nie wolno ich cofac:

1. Bootstrap market-cap fallback z `PoolObservationSession::current_account_features()` musi pozostac spójny z observed curve (`ADR-0061`).
2. Jesli legacy feature path istnieje przejsciowo, jego terminalna semantyka musi byc policy-consistent i musi niesc decision metadata (`ADR-0062`).
3. Shadow BUY metadata musi byc canonicalized i sanitized; observed metadata moze pomagac, ale nie moze stawac sie layout SSOT (`ADR-0068`).
4. `observed_buy_tx` nie moze byc gating requirement dla shadow readiness (`ADR-0069`).
5. Live SELL transport pozostaje fail-closed na Jito gRPC, a nie na RPC fallback (`ADR-0071`).
6. SELL observability z `ADR-0070` musi zostac zachowana po zmianach truth source.

### 3.5. Nie wolno usuwac fallbacku zanim canonical path nie zostanie udowodniony

`ADR-0066` narzuca twardy warunek:

- zanim cokolwiek usuniemy z fallbackow truth/readiness, musimy udowodnic zdrowie canonical AccountUpdate ingest path,
- dopoki nie ma tego dowodu, fallbacki moga byc ograniczane i telemetryzowane, ale nie usuwane "w ciemno".

### 3.6. Nie wolno udawac live validation

`ADR-0064` pozostaje obowiazujaca granica:

- brak swiezych BUY/shadow artifactow nie jest automatycznie dowodem regresji kodu,
- ale tez nie wolno oglaszac "validated", jesli runtime nie wszedl w odpowiednia galez.

### 3.7. Nie mieszac architecture closure z retuningiem Gatekeepera

Ten plan domyka PR2/PR6/PR7/PR8. Nie wolno go wykorzystywac jako bocznego nosnika dla niejawnego retune'u policy math lub fingerprint-driven scoringu.

Obowiazuja nastepujace zasady:

- production terminal verdict idzie aktywna feature-driven path, tj. przez `evaluate_from_features(...)` i `gatekeeper_policy.rs`,
- inline zmiana ograniczona do `ghost-launcher/src/components/gatekeeper.rs` nie moze byc traktowana jako production-effective change, jesli nie dotyka rowniez aktywnej policy path,
- dopoki invariant brzmi "Gatekeeper podejmuje production decyzje wylacznie z `MaterializedFeatureSet`", telemetry takie jak `early_fingerprint` dolaczane po werdykcie pozostaja logging/diagnostic surface, a nie pre-verdict decision input,
- jesli w przyszlosci zapadnie decyzja, ze fingerprint ma wplywac na live verdict, wymaga to jawnej decyzji kontraktowej o rozszerzeniu canonical feature surface oraz oddzielnego ADR / workstreamu, a nie ukrytego patcha przy okazji domykania refaktoru.

---

## 4. Implikacje z ADR-0054 ... ADR-0072 dla tego planu

| ADR | Plan-level implication |
|---|---|
| `0054` | target operational checklists sa nadal wazne |
| `0055` | nie otwierac ponownie PR3B/PR5/PR6/PR7 jako workstreamow bez twardego powodu |
| `0056` | production pool lifecycle i AccountUpdate ingest musza byc registry/account-core-first |
| `0057` | rozroznic runtime closure od repository cleanup |
| `0058` | literal cleanup PR8 wykonac w czterech etapach i bez scope creep |
| `0059` | traktowac jego closure claims jako target cleanup contract, nie jako slepy dowod aktualnego stanu |
| `0060` | integration tests musza byc przepiete na production ingest contracts; monitor naming ma byc monitoring-only |
| `0061` | bootstrap fallback market-cap i curve semantics nie moga zostac zepsute przy dalszym cleanupie |
| `0062` | dopoki istnieje legacy branch, musi byc policy-consistent i decision-complete |
| `0064` | live validation wymaga realnie osiagnietej BUY branch; nie rozluzniac triggera bez zgody |
| `0065` | dependency chain PR3 -> PR4 -> PR5 -> PR6 pozostaje execution law, nawet jesli PR4/PR5 sa juz done |
| `0066` | usuwanie fallbackow truth source musi byc poprzedzone telemetry proof dla canonical ingest |
| `0067` | rozdzielac shadow outcomes na readiness, simulation-error i success |
| `0068` | zachowac creator/account-override sanitation i metadata hardening |
| `0069` | shadow readiness ma zalezec od real prerequisites, nie od observed BUY presence |
| `0070` | utrzymac SELL observability i bullet freshness telemetry |
| `0071` | live execution transport pozostaje fail-closed na Jito gRPC |
| `0072` | actual gap inventory dla tego planu: PR2, PR6, PR7, PR8 |

---

## 5. Docelowe invariants po pelnym domknieciu planu

Po zakonczeniu wszystkich faz nastepujace stwierdzenia musza byc prawdziwe jednoczesnie:

1. `AccountStateCore` jest jedynym primary source of truth dla canonical market state.
2. `TxIntelligenceEngine` jest behavioral sidecar i nigdy nie pisze do canonical state.
3. `Gatekeeper` podejmuje production decyzje wylacznie z `MaterializedFeatureSet`.
4. `early_fingerprint` dolaczany post-verdict sluzy telemetry/loggingowi, dopoki canonical feature contract nie zostanie jawnie rozszerzony.
5. `ShadowLedger` nie jest live truth source ani dla observation runtime, ani dla live post-buy runtime.
6. `PoolObservationSession` jest jedynym runtime ownerem obserwacji per pool.
7. Kazdy terminal verdict jest replayable z `feature_snapshot` + config.
8. Bootstrap fallbacki sa internal-consistent, ale nie sa canonical truth.
9. `ReconciliationRuntime` jest monitoring-only i niczego nie "naprawia".
10. Production live BUY/SELL execution jest fail-closed na Jito gRPC.
11. `account_updates_enabled` nie jest juz architecture-ambiguous production switch.
12. Repo nie zawiera publicznego production legacy verdict path ani active compat runtime state.

---

## 6. Mapa luk do zamkniecia

### G1. PR2 - kontrakt `price_sol` i czerwony test reducera

**Stan obecny:**

- `ghost-core/tests/account_state_core_tests.rs` oczekuje `price_sol == 1.5`,
- implementacja `normalized_price_sol()` zwraca `0.0015`,
- nie ma jednoznacznie skodyfikowanego source-of-truth dla jednostek na granicy `AccountStateUpdate -> CanonicalPoolState`.

**Ryzyko:**

- ukryta niespojnosc jednostek moze propagowac bledy do `feature_builder`, `gatekeeper_policy`, logow i downstream analytics,
- naprawa "na szybko" moze poprawic test, ale rozwalic semantyke runtime.

**Target:**

- jedna, jawna, skodyfikowana definicja:
  - czy `sol_reserves` sa w lamportach,
  - czy `token_reserves` sa w raw base units,
  - jaka jest dokladna jednostka `price_sol`,
  - jaka jest jednostka `reserve_velocity_sol_per_sec`.

### G2. PR2/PR7 - zdrowie canonical AccountUpdate ingest path

**Stan obecny:**

- `ADR-0066` wskazuje realne podejrzenie dropow/race condition na granicy identity registration / account update ingest,
- obecnie brak wystarczajacego dowodu telemetrycznego, ze wszystkie potrzebne update-y docieraja do `AccountStateCore` zanim runtime ich potrzebuje.

**Ryzyko:**

- mozna usunac fallbacki z PR7 i odciac sobie jedyny degraded path zanim canonical ingest bedzie zdrowy.

**Target:**

- pelna widocznosc:
  - ile account update-ow trafia przed registration,
  - ile konczy jako `None` przy buildzie runtime update,
  - jak dlugo trwa `NewPoolDetected -> first canonical AccountUpdate`,
  - ile pooli timeoutuje z `update_count == 0`.

### G3. PR6 - domyslny runtime nadal pozwala na legacy feature terminal path

**Stan obecny:**

- `ghost-brain/src/config/ghost_brain_config.rs` nadal domyslnie ustawia `use_three_layer_decision = false`,
- `oracle_runtime.rs` nadal ma aktywny runtime branch do `evaluate_from_features_legacy(...)`.

**Ryzyko:**

- production zachowuje sie inaczej niz target architecture,
- testy i profile moga dawac mylace "feature-driven" coverage przy defaultach nadal legacy-biased.

**Target:**

- production default i shipped configs musza byc jednoznacznie feature-driven,
- legacy branch, jesli zostanie, ma byc tylko explicit compat/test path.

### G4. PR7 - truth-source migration nie jest domknieta poza observation runtime

**Stan obecny:**

- observation runtime jest canonical-first,
- ale `post_buy_runtime` nadal czyta cene z `ShadowLedger`,
- runtime helpery nadal maja shadow fallback branches, ktore nie sa jeszcze telemetrycznie dowiedzione jako bounded/degraded-only.

**Ryzyko:**

- live runtime pozostaje hybryda z dwoma truth surfaces,
- mozliwy split-brain miedzy `AccountStateCore` a `ShadowLedger`.

**Target:**

- `ShadowLedger` pozostaje tylko simulation / replay / forensics / bounded degraded fallback,
- live post-buy pricing nie opiera sie na `ShadowLedger`.

### G5. PR8 - literal cleanup i compat ballast

**Stan obecny:**

- w aktualnym repo nadal wystepuja legacy/compat surfaces nalezace do PR8,
- szczegolnie:
  - aktywna runtime mozliwosc wejscia w legacy terminal evaluation,
  - `account_updates_enabled` jako compatibility surface,
  - dormant repair language / symbols,
  - pozostaly deprecated/api ballast.

**Ryzyko:**

- repo nadal nie odpowiada literalnemu wordingowi `REFACTOR.md`,
- reviewerzy i operatorzy beda miesc dalszy runtime closure z repo hygiene.

**Target:**

- runtime i repo jednoczesnie odpowiadaja target architecture,
- brak publicznego production legacy path,
- brak architecture-ambiguous config semantics.

### G6. Gatekeeper fingerprint telemetry nie jest inputem aktywnej policy path przed werdyktem

**Stan obecny:**

- production runtime terminalizuje feature-driven verdict przez `evaluate_from_features(...)` i `gatekeeper_policy.rs`,
- `build_assessment_from_features(...)` startuje z `early_fingerprint = None`,
- `assessment.early_fingerprint` jest dolaczany dopiero po `Buy` / `Reject` / `Timeout` dla logowania i telemetry,
- `MaterializedFeatureSet` nie niesie dzis `early_fingerprint`.

**Ryzyko:**

- latwo pomylic logging coverage z realnym decision coverage,
- future soft-scoring / fingerprint patch moze trafic tylko w legacy `gatekeeper.rs` i nie zmienic live verdictu,
- ad-hoc dopiecie fingerprinta poza canonical feature contract rozwali invariant o replayable feature-driven decisions.

**Target:**

- dopoki ten plan domyka PR2/PR6/PR7/PR8, production scoring pozostaje explicit feature-driven i nie udaje, ze post-verdict telemetry jest pre-verdict inputem,
- kazda przyszla decyzja o wykorzystaniu fingerprinta w live verdict musi albo rozszerzyc canonical feature contract (`MaterializedFeatureSet` + active assessment build), albo pozostac telemetry-only do osobnego ADR / planu.

---

## 7. Program wykonawczy

## Faza 0 - Freeze, baseline i guardraile

### Cel

Ustalic baseline testowy, structuralny i telemetryczny przed pierwsza zmiana logiki lub truth-source contract.

### Kroki

1. Uruchomic baseline tylko na istniejacych testach zwiazanych z refaktorem:
   - `ghost-core/tests/account_state_core_tests.rs`
   - `ghost-core/tests/checkpoint_engine_tests.rs`
   - `ghost-core/tests/feature_builder_tests.rs`
   - `ghost-launcher/tests/session_lifecycle_tests.rs`
   - `ghost-launcher/tests/gatekeeper_policy_tests.rs`
   - `ghost-launcher/tests/full_pipeline_integration.rs`
   - `ghost-launcher/tests/tx_intelligence_tests.rs`
   - `ghost-launcher/tests/post_buy_runtime_integration.rs`
   - wybrane testy startup/config/Jito transport.

2. Zamrozic structural invariants przez repo search i/lub testy strukturalne:
   - brak production `shadow_ledger.get_curve(` w hot-path truth helpers,
   - brak production `shadow_ledger.get_quote(` jako canonical truth,
   - brak production `.on_transaction(` jako decision path,
   - zliczenie miejsc `evaluate_from_features_legacy`,
   - zliczenie miejsc `account_updates_enabled`,
   - zliczenie repair-centric surface (`Repaired`, `legacy_repair*`, `repair*`).

3. Dodac telemetry baseline dla przyszlych decyzji migracyjnych:
   - `account_update_before_identity_total`
   - `account_update_build_none_total{reason=...}`
   - `canonical_first_update_latency_ms`
   - `shadow_truth_fallback_total{site=...}`
   - `post_buy_price_source_total{source=...}`
   - `legacy_terminal_verdict_total`

4. Ustalic, ze kazda kolejna faza musi przejsc przez:
   - compile/test gate,
   - structural search gate,
   - replay/shadow gate,
   - dopiero potem rollout gate.

### Exit criteria

- baseline test matrix znana i zapisana,
- metryki i structural gates gotowe,
- blast radius zidentyfikowany przed pierwsza modyfikacja logiki.

### Forbidden scope

- bez zmian policy thresholds,
- bez usuwania fallbackow,
- bez literal cleanupu.

---

## Faza 1 - PR2 contract closure i canonical-ingest instrumentation

### Cel

Domknac kontrakt `AccountStateCore`, usunac czerwony test oraz zbudowac instrumentation i wstepny material dowodowy dla canonical account-update ingest. Finalny health proof, ktory gate'uje redukcje fallbackow, nalezy do Fazy 3.

### 1A. Kontrakt jednostek i skali

Przed zmiana kodu nalezy wykonac jawna decyzje kontraktowa:

1. Zmapowac wszystkich konsumentow:
   - `ghost-core/src/account_state_core/reducer.rs`
   - `ghost-core/src/checkpoint/feature_builder.rs`
   - `ghost-launcher/src/session/observation.rs`
   - `ghost-launcher/src/components/gatekeeper_policy.rs`
   - logi / serializacja assessmentow / telemetry.

2. Ustalic jeden canonical contract:
   - `sol_reserves` = lamports albo nie,
   - `token_reserves` = raw token base units albo nie,
   - `price_sol` = SOL per token w human units albo raw ratio w on-chain units.

3. Dopiero po tej decyzji wykonac jedna z dwoch sciezek:
   - **Sciezka preferowana:** jesli implementacja normalizacji jest poprawna, poprawic testy, dokumentacje i ewentualnych konsumentow,
   - **Sciezka alternatywna:** jesli test ujawnia blad implementacji, poprawic reducer i wszystkich konsumentow atomowo.

### 1B. Domkniecie czerwonego testu bez ukrytej regresji

Po decyzji kontraktowej:

1. doprowadzic `ghost-core --test account_state_core_tests` do pelnej zielonosci,
2. dodac dodatkowe regresje:
   - explicit scale test dla `price_sol`,
   - explicit scale test dla `market_cap_sol`,
   - explicit scale test dla `reserve_velocity_sol_per_sec`,
   - bootstrap -> canonical promotion z realnymi jednostkami,
   - monotonic same-slot/update ordering.

### 1C. Canonical-ingest instrumentation i wstepny material dowodowy

W `oracle_runtime` i `seer` nalezy dodac instrumentation, ktora odpowie na pytania z `ADR-0066`:

1. Ile `SeerEvent::AccountUpdate` dociera do launchera?
2. Ile z nich odpada przed zbudowaniem `AccountStateUpdate` i dlaczego?
3. Ile przychodzi przed tym, zanim runtime zarejestruje identity/base_mint?
4. Jak dlugo trwa od `NewPoolDetected` do pierwszego `PromotedFromBootstrap`?
5. Jakie poole timeoutuja z `session.account_features.update_count == 0`?

### 1D. Decision tree dla race condition

Na podstawie telemetry z 1C nalezy wykonac jedna z dwoch sciezek:

1. **Jesli drop/race nie wystepuje istotnie:**
   - nic nie buforowac,
   - przejsc do Fazy 2.

2. **Jesli drop/race wystepuje:**
   - dodac bounded, monotonic, TTL-limited pre-identity account-update buffer keyed by `base_mint`,
   - replayowac tylko po registration,
   - nie dopuszczac write authority poza `AccountStateCore`.

### Exit criteria

- wszystkie testy `ghost-core account_state_core` zielone,
- kontrakt jednostek zapisany w kodzie/testach,
- telemetry dla canonical ingest istnieje i daje wstepna evidence base,
- znana jest odpowiedz, czy pre-identity buffering jest potrzebny.

### Forbidden scope

- bez zmian gatekeeper thresholds,
- bez zmian post-buy runtime,
- bez usuwania `ShadowLedger` fallbackow.

---

## Faza 2 - PR6 final closure: feature-driven path jako jedyny production default

### Cel

Doprowadzic system do stanu, w ktorym feature-driven verdict path nie tylko istnieje, ale jest jedynym domyslnym production contract.

### 2A. Zachowac accepted contracts z ADR-0061 i ADR-0062

Przed default flip:

1. utrzymac bootstrap fallback contract z `ADR-0061`,
2. jesli legacy branch nadal chwilowo istnieje, utrzymac policy-consistent terminal behavior i decision metadata zgodnie z `ADR-0062`.

### 2B. Default flip

Nalezy zsynchronizowac:

1. `ghost-brain/src/config/ghost_brain_config.rs`
2. `ghost-brain/ghost_brain_config.toml`
3. `config.toml`
4. wszystkie rollout/config fixtures
5. test fixtures, ktore polegaja na explicite ustawionym `false`

Zasada:

- `use_three_layer_decision = true` ma byc production default w kodzie i w dostarczonych profilach,
- `false` ma pozostac tylko jako jawny compat/test setting.

### 2C. Runtime fencing

Po default flip nalezy doprowadzic do sytuacji, w ktorej:

1. production startup/config validation nie pozwala uruchomic niejawnie legacy terminal path,
2. `oracle_runtime.rs` nie terminalizuje production verdictow przez legacy branch,
3. `feature_snapshot` i `assessment.decision` sa obecne w kazdym terminalnym werdykcie.

### 2D. Parity i replay proof

Zanim legacy branch zostanie zredukowany do test-only/degraded, nalezy:

1. porownac legacy vs feature-driven na:
   - syntetycznych cases,
   - testach integracyjnych,
   - istniejacych fixtures/logs replay, jesli sa dostepne.

2. zidentyfikowac wszystkie rozbieznosci:
   - dopuszczalne roznice wynikajace z accepted fixes,
   - niedopuszczalne roznice oznaczajace regresje.

### 2E. Boundary dla przyszlych zmian scoringu Gatekeepera

W tej fazie nalezy explicitnie oddzielic domkniecie active feature-driven path od przyszlego retune'u scoringu:

1. nie wolno traktowac inline zmian wyłącznie w `ghost-launcher/src/components/gatekeeper.rs` jako production-effective retune'u, jesli aktywny terminal verdict nadal biegnie przez `gatekeeper_policy.rs`,
2. dopoki production contract brzmi "Gatekeeper podejmuje decyzje z `MaterializedFeatureSet`", telemetry dolaczane po werdykcie nie moze byc opisywane jako pre-verdict decision input,
3. jesli po domknieciu PR6 zapadnie decyzja o retune Layer 3 lub fingerprint-based soft scoringu, musi to byc osobny workstream skierowany w aktywna policy path, z jawna decyzja czy rozszerzamy canonical feature surface, czy pozostajemy przy telemetry-only.

### Exit criteria

- production default = three-layer enabled,
- production configs nie uruchamiaja legacy branch,
- terminal assessments zawsze niosa `feature_snapshot` i `decision`,
- istniejace regression tests dla `ADR-0061` i `ADR-0062` pozostaja zielone,
- production-relevant scoring changes nie sa juz kierowane wyłącznie w legacy surface.

### Forbidden scope

- bez przywracania raw-tx dependence do policy,
- bez "tymczasowego" obizania progow, zeby szybciej zobaczyc BUY,
- bez merge'owania patchy scoringowych ograniczonych do legacy `gatekeeper.rs` pod pretekstem production hardeningu.

---

## Faza 3 - PR7 preconditions: bounded fallback proof i canonical truth health

### Cel

Skonsumowac telemetry i material dowodowy z Faz 1-2 i udowodnic, ze canonical account-state path jest wystarczajaco zdrowy, aby mozna bylo bezpiecznie ograniczac i usuwac kolejne fallbacki truth/readiness.

### 3A. Telemetry i health gates

Przed kazda redukcja fallbacku trzeba miec widoczne:

1. `account_update_before_identity_total`
2. `account_update_promoted_from_bootstrap_total`
3. `canonical_first_update_latency_ms`
4. `timeout_without_canonical_updates_total`
5. `shadow_truth_fallback_total{site=resolve_price_context}`
6. `shadow_truth_fallback_total{site=resolve_gatekeeper_initial_reserves}`
7. `post_buy_price_source_total`

### 3B. Acceptance gates dla kolejnej fazy

Do Fazy 4 wolno przejsc dopiero, gdy:

1. account updates nie sa systemowo gubione,
2. runtime nie timeoutuje masowo z `update_count == 0`,
3. fallback usage jest rozumiane i sklasyfikowane,
4. istnieje jasne rozroznienie:
   - bootstrap fallback,
   - degraded/diagnostic fallback,
   - nieakceptowalny hidden primary fallback.

### 3C. Degraded path boundary

Nalezy explicitnie zostawic:

- read-only degraded/reconciliation paths, jesli sa potrzebne,
- ale kazdy taki path musi byc:
  - telemetryzowany,
  - nazewniczo oznaczony jako degraded/diagnostic,
  - nieudawajacy primary truth.

### Exit criteria

- zdrowie canonical ingest jest udowodnione metrykami i testami,
- wiemy, ktore fallbacki mozna usunac natychmiast, a ktore trzeba przejsciowo zostawic,
- brak "ciemnych" shadow fallbackow bez telemetry.

### Forbidden scope

- bez usuwania fallbackow tylko dlatego, ze "plan tak mowi",
- bez dotykania live transport semantics.

---

## Faza 4 - PR7 full truth-source closure poza observation runtime

### Cel

Domknac truth-source migration wszedzie tam, gdzie aktualnie `ShadowLedger` nadal uczestniczy w aktywnym runtime jako zrodlo live danych.

### 4A. Runtime helpery w `oracle_runtime.rs`

Nalezy ustawic jedna jawna kolejnosc zrodel:

1. `AccountStateCore` canonical state,
2. `BootstrapState` / session/runtime metadata tylko tam, gdzie plan dopuszcza bootstrap fallback,
3. explicit degraded/diagnostic fallback z telemetry,
4. genesis constants tylko jako final safety net.

Kazde odwolanie do `ShadowLedger` musi byc sklasyfikowane jako:

- bootstrap-only,
- degraded-only,
- diagnostic-only,
- albo usuwane.

### 4B. `post_buy_runtime` - wyprowadzenie live ceny z `ShadowLedger`

To jest najwazniejszy niedomkniety punkt PR7.

Plan wykonania:

1. Wprowadzic jeden jawny live price source contract dla post-buy lifecycle:
   - primary: `AccountStateCore` / runtime canonical state,
   - fallback: read-only point query / runtime-registry-backed data, jesli canonical state jest stale lub chwilowo niedostepny,
   - zakaz: `ShadowLedger` jako live price source w live lane.

2. Przez okres przejsciowy uruchomic dual-read compare:
   - nowy canonical/live source,
   - stary shadow source,
   - log divergence bez zmiany execution transport.

3. Po pozytywnym compare:
   - przelaczyc live `post_buy_runtime` na nowy source,
   - pozostawic `ShadowLedger` tylko dla paper/shadow/replay/forensics.

### 4C. Zachowac accepted shadow-run hardening

Przy migracji truth source nie wolno zepsuc:

1. metadata hydration contract z `ADR-0068`,
2. readiness contract z `ADR-0069`,
3. outcome classification z `ADR-0067`.

To oznacza:

- observed BUY telemetry moze enrichowac,
- creator musi pozostac valid non-default pubkey,
- override account layouts musza pozostac canonicalized,
- `shadow_skipped_not_ready`, `shadow_simulation_error`, `shadow_simulated` pozostaja rozroznialne.

### 4D. Reconciliation boundary

Po tej fazie `ReconciliationRuntime` ma miec zero ambiguity:

- monitoring-only,
- read-only,
- brak repair semantics,
- brak repair-centric operator language.

### Exit criteria

- brak live `read_price_from_shadow` w production lane,
- `ShadowLedger` nie jest juz live truth source poza jawnie dozwolonym degraded/simulation scope,
- fallback usage jest telemetrycznie bliskie zera poza degraded/test,
- `ReconciliationRuntime` jest jednoznacznie monitoring-only.

### Forbidden scope

- nie wolno usuwac simulation APIs,
- nie wolno zmieniac live BUY/SELL transport away from `ADR-0071`,
- nie wolno wprowadzac nowego split-brain source obok `AccountStateCore`.

---

## Faza 5 - PR8 literal cleanup

### Cel

Doprowadzic repo do literalnej zgodnosci z `REFACTOR.md`, po tym jak production truth i verdict path beda juz finalnie ustabilizowane.

### Zasada nadrzedna

Ta faza jest cleanup-only.

Nie wolno w tej fazie:

- ukradkiem zmieniac policy math,
- dokladac nowych feature gates,
- ponownie otwierac PR3-PR7 jako redesign.

### Etap 5.1. Runtime compat state

Najpierw trzeba zrobic re-audit aktualnego kodu i sprawdzic, co realnie jeszcze istnieje z:

- `PerPoolOracleState`
- `OracleRuntime.pools`
- `register_new_pool(...)`
- compat lookup helpers
- compat cleanup flows

Regula:

- jesli cos nadal istnieje w production build, usunac lub przepiac na final truth source,
- jesli juz zostalo usuniete, tylko potwierdzic structural acceptance i niczego nie odtwarzac.

### Etap 5.2. Legacy verdict surface

Nastepnie:

- usunac lub zafencowac do `#[cfg(test)]` wszystko, co podtrzymuje publiczny production legacy verdict path,
- w tym:
  - `evaluate_from_features_legacy(...)` jako production-reachable branch,
  - publiczne legacy verdict helpers,
  - pozostale inline scoring wrappers.

### Etap 5.3. `account_updates_enabled` semantics

Nastepnie:

- doprowadzic config i startup semantics do stanu, w ktorym `account_updates_enabled` nie wyglada juz jak production ownership switch,
- dopuszczalne sa tylko dwie formy koncowe:
  - explicit degraded/test-only ballast,
  - albo pelne usuniecie z production config surface.

### Etap 5.4. Reconciliation naming i dormant ballast

Na koncu:

- usunac repair-centric nazewnictwo z docs/logs/metrics/public surface,
- pozostawic ewentualny dormant ballast tylko tam, gdzie jest potrzebny dla kompatybilnosci testow lub danych historycznych, i tylko pod prawdziwa nazwa `legacy_*`.

### Exit criteria

- brak production legacy ownership/state carrier,
- brak production legacy verdict path,
- brak architecture-ambiguous `account_updates_enabled`,
- brak repair-centric operator language sugerujacego aktywne naprawianie stanu,
- repo search i test surface odpowiada literalnemu wordingowi targetu.

### Forbidden scope

- nie zostawiac "na wszelki wypadek" nowych helperow odtwarzajacych usunieta semantyke,
- nie mylic literal cleanupu z dowolnym cleanupem stylistycznym.

---

## Faza 6 - walidacja, canary i final acceptance

### Cel

Udowodnic, ze nowa architektura jest poprawna zrodlo-po-zrodle, path-po-path i runtime-po-runtime, bez sztucznego zawyzania claims.

### 6A. Warstwa testowa

Wymagany zestaw walidacji:

1. `ghost-core`
   - `account_state_core_tests`
   - `checkpoint_engine_tests`
   - `feature_builder_tests`
   - `reconciliation`
   - `reconciliation_runtime`

2. `ghost-launcher`
   - `session_lifecycle_tests`
   - `tx_intelligence_tests`
   - `gatekeeper_policy_tests`
   - `full_pipeline_integration`
   - `snapshot_engine_integration`
   - `post_buy_runtime_integration`
   - celowane testy startup/config invariants
   - celowane testy PR7 invariants

3. `trigger`
   - testy builder sanitation,
   - testy Jito transport contract,
   - testy revolver worker / sell observability.

### 6B. Structural acceptance

Po kazdej fazie uruchamiac repo-level acceptance checks:

1. brak production `shadow_ledger.get_curve(` jako truth source,
2. brak production `shadow_ledger.get_quote(` jako truth source,
3. brak production `read_price_from_shadow(` w live lane,
4. brak production `evaluate_from_features_legacy(` po Faze 5,
5. brak production `account_updates_enabled` jako ownership switch,
6. brak repair-centric public monitoring names po finalnym cleanupie.

### 6C. Runtime observability acceptance

Nalezy sprawdzic:

1. `shadow_truth_fallback_total` - czy spada do jawnie dopuszczalnego poziomu,
2. `post_buy_price_source_total` - czy live lane nie czyta z `ShadowLedger`,
3. `canonical_first_update_latency_ms` - czy canonical ingest jest zdrowy,
4. `legacy_terminal_verdict_total` - czy produkcja nie uzywa juz legacy terminal path,
5. SELL latency metrics z `ADR-0070`,
6. fail-closed live transport guardy z `ADR-0071`.

### 6D. Shadow/paper/live acceptance

1. Shadow/paper validation:
   - musi przejsc przed jakakolwiek live claim.

2. Live validation:
   - wolno oglosic dopiero, gdy runtime realnie wejdzie w odpowiednia galaz,
   - brak BUY/shadow artifacts przy all-reject window nie jest dowodem kodowej regresji,
   - ale tez nie jest dowodem runtime closure.

### 6E. Rollback policy

Kazda faza musi byc rollbackable do poprzedniej warstwy kontraktu:

1. rollback nie moze przywracac nowego legacy shima,
2. rollback moze przywrocic tylko poprzedni jawny feature/config gate,
3. rollback musi zachowac telemetry i logi pozwalajace zrozumiec, dlaczego byl potrzebny.

---

## 8. Pakiety zmian plik po pliku

| Plik | Zakres odpowiedzialnosci | Faza | Krytyczny guardrail |
|---|---|---|---|
| `ghost-core/src/account_state_core/reducer.rs` | kontrakt jednostek, price/mcap/velocity math | 1 | nie zmieniac skali bez atomowego update wszystkich konsumentow |
| `ghost-core/tests/account_state_core_tests.rs` | jawny contract test dla reducera | 1 | test ma potwierdzac contract, nie przypadkowe wartosci |
| `ghost-launcher/src/oracle_runtime.rs` | instrumentation canonical ingest, truth helper ordering, final removal legacy/runtime fallbackow | 1,3,4,5 | zero nowego hidden fallbacku |
| `ghost-launcher/src/session/observation.rs` | bootstrap fallback consistency, canonical freshness, brak odtworzenia legacy wrapper semantics | 1,2,4 | nie promowac bootstrap do canonical truth |
| `ghost-launcher/src/components/gatekeeper.rs` | legacy fencing, feature-driven only production path | 2,5 | zero raw-tx policy comeback i zero production-only scoring patchow ograniczonych do legacy surface |
| `ghost-launcher/src/components/gatekeeper_policy.rs` | policy semantics stable, active production verdict path | 2 | nie retunowac progow zamiast naprawic kontrakt i nie omijac active path przez patch-only w `gatekeeper.rs` |
| `ghost-brain/src/config/ghost_brain_config.rs` | default `use_three_layer_decision` | 2 | default production musi byc zgodny z target architecture |
| `ghost-launcher/src/config.rs` | startup/config fail-closed rules | 2,5 | `account_updates_enabled` nie moze byc production truth switch |
| `ghost-launcher/src/main.rs` | startup enforcement dla canonical ingest i Jito transport | 2,5 | brak silent degradation |
| `off-chain/components/seer/src/config.rs` | zredukowanie ambiguity flag | 5 | degraded/test-only semantics tylko jawne |
| `off-chain/components/seer/src/lib.rs` | account update ingress health i compatibility boundary | 1,3,5 | nie oslabic canonical feed |
| `ghost-launcher/src/components/post_buy_runtime.rs` | usuniecie `ShadowLedger` jako live price source, zachowanie SELL telemetry i Jito-only live transport | 4 | read-only fallback tak, live truth z shadow nie |
| `ghost-core/src/shadow_ledger/reconciliation.rs` | monitoring-only semantics | 4,5 | brak write authority / repair semantics |
| `ghost-core/src/shadow_ledger/reconciliation_runtime.rs` | monitoring-only runtime surface i naming | 4,5 | operator language ma byc truthful |
| `ghost-core/src/shadow_ledger/ledger.rs` | deprecated truth APIs fencing | 4,5 | nie przywrocic hot-path usage |
| `ghost-launcher/tests/*` | migracja na final ingest contracts | 2,5,6 | brak zaleznosci od legacy verdict helpers |
| `ghost-core/tests/*` | kontrakty PR2/PR7 | 1,6 | testy maja bronc architecture, nie implementation accidents |
| `off-chain/components/trigger/*` | zachowanie sanitation i Jito-only execution contract | 4,6 | observed metadata nie moze stac sie layout SSOT |

---

## 9. Kryteria zamkniecia per obszar

### PR2 mozna uznac za closed dopiero gdy:

1. `ghost-core --test account_state_core_tests` jest zielony,
2. kontrakt skali `price_sol` jest jednoznaczny i udokumentowany,
3. nie ma niewidzialnych dropow canonical account updates bez telemetry,
4. bootstrap -> canonical promotion jest stabilna i mierzalna.

### PR6 mozna uznac za closed dopiero gdy:

1. production default = feature-driven,
2. production configs nie aktywuja legacy terminal path,
3. kazdy terminal verdict ma `feature_snapshot` i `decision`,
4. runtime nie zalezy na legacy policy branch do normalnej pracy,
5. production-relevant scoring changes nie sa juz kierowane wyłącznie w legacy `gatekeeper.rs`.

### PR7 mozna uznac za closed dopiero gdy:

1. `AccountStateCore` jest primary truth nie tylko w observation runtime, ale tez w live post-buy pricing,
2. `ShadowLedger` jest tylko simulation/replay/forensics/degraded fallback,
3. fallback usage jest jawnie ograniczone i telemetrycznie dowiedzione,
4. `ReconciliationRuntime` jest monitoring-only bez ambiguity.

### PR8 mozna uznac za closed dopiero gdy:

1. repo nie zawiera production-reachable legacy runtime ownership,
2. repo nie zawiera production-reachable legacy verdict path,
3. `account_updates_enabled` nie jest production architecture switch,
4. operator-facing naming nie sugeruje repair authority,
5. structural acceptance i test surface odpowiadaja literalnemu wordingowi targetu.

---

## 10. Rzeczy, ktorych nie wolno popsuc po drodze

1. Nie wolno zepsuc bootstrap consistency z `ADR-0061`.
2. Nie wolno cofnac policy-consistent legacy timeout semantics z `ADR-0062`.
3. Nie wolno przywrocic observed BUY telemetry jako mandatory readiness gate z `ADR-0069`.
4. Nie wolno cofnac creator/account override sanitation z `ADR-0068`.
5. Nie wolno przywrocic live RPC submit fallback dla BUY/SELL po `ADR-0071`.
6. Nie wolno usunac SELL telemetry i bullet-freshness observability z `ADR-0070`.
7. Nie wolno glosic live validation bez realnego branch reachability zgodnie z `ADR-0064`.
8. Nie wolno przedstawiac post-verdict `early_fingerprint` telemetry jako live decision input bez jawnej zmiany canonical feature contract.

---

## 11. Ostateczna interpretacja strategiczna

Najwazniejszy blad, ktorego ten plan ma uniknac, to mieszanie trzech rzeczy:

1. target architecture,
2. historycznych closure claims,
3. rzeczywistego stanu aktualnego repo.

Dlatego plan przyjmuje:

- target contract z `REFACTOR.md` pozostaje prawidlowy,
- accepted ADR-y narzucaja sequencing i forbidden scope,
- ale actual gap inventory bierze sie z aktualnego repo i z `ADR-0072`.

Praktycznie oznacza to:

- nie ma potrzeby restartowac PR3-PR5,
- trzeba domknac PR2,
- trzeba finalizowac PR6 jako production default,
- trzeba dowiezc PR7 poza observation runtime,
- trzeba literalnie posprzatac PR8,
- i trzeba to zrobic bez tworzenia nowych pol-srodkow.

---

## 12. Koncowy execution order

1. **Freeze i telemetry baseline**
2. **PR2 contract closure + canonical-ingest instrumentation**
3. **PR6 default flip i legacy fencing**
4. **Canonical ingest health proof / acceptance gate na podstawie telemetry z Faz 1-2**
5. **PR7 truth-source completion w `oracle_runtime` i `post_buy_runtime`**
6. **PR8 literal cleanup**
7. **Final structural audit + targeted test matrix**
8. **Shadow/paper validation**
9. **Live validation tylko przy realnie osiagnietej branch reachability**

To jest jedyna kolejnosc zgodna jednoczesnie z:

- `PLANS/REFACTOR.md`,
- `ADR-0054`,
- pozniejszymi ADR-ami `0055-0071`,
- i rzeczywistym stanem repo z `ADR-0072`.

