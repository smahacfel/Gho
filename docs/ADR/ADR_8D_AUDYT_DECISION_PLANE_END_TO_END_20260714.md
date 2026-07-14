# ADR-8D: Audyt Decision Plane end-to-end 2026-07-14

Status: **IMPLEMENTED / AUDIT REPORT CREATED / DOCUMENTATION ONLY**
Typ: ADR-8D / architecture audit / Gatekeeper and post-buy decision authority
Data: 2026-07-14
Autor/Agent: Codex
Repo/branch: `/root/Gho_dynamic_exit_v1`, `main`
Badany commit: `aca107afc14b3122899af2afa2091ccfb400f498`
Poziom ryzyka badanego obszaru: **HIGH**
Poziom ryzyka zmiany dokumentacyjnej: **LOW**

Dotknięte pliki:

- `PLANS/AUDYT/AUDYT_DECISION_PLANE_END_TO_END_20260714.md`;
- `docs/ADR/ADR_8D_AUDYT_DECISION_PLANE_END_TO_END_20260714.md`.

Uwaga o szablonie:

Literalna ścieżka wymagana przez instrukcję globalną, `/Gho/docs/ADR/ADR_8D_SZABLON.md`, ani jej odpowiednik `docs/ADR/ADR_8D_SZABLON.md` nie istnieją w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D używany przez istniejące audyty repo.

## 1. Przygotowanie i zakres

Cel:

Utworzyć aktualny, evidence-backed opis całego aktywnego Decision Plane, rozdzielony na:

1. tworzenie danych prebuy/prereject;
2. kryteria i władzę decyzji prebuy/prereject;
3. rzeczywistą logikę biznesową post-BUY;
4. komponenty aktywne, config-latent, shadow, obserwacyjne, inert i legacy;
5. priorytety rozwoju i redukcji.

Świadome wyłączenie:

- HyperPrediction;
- Hyper Oracle;
- legacy `score_pool()`;
- starego standalone `StrategySelector` i elementów dawnego scoring pipeline'u.

Wyłączenie nie obejmuje IWIM ani aktywnych modułów zlokalizowanych w crate `ghost-brain`, jeżeli aktualny launcher rzeczywiście je wywołuje.

Podstawa audytu:

- bieżący kod wskazanego commita;
- `config.toml`;
- `ghost-brain/ghost_brain_config.toml`;
- testy kontraktowe i regresyjne jako pomocniczy dowód wiring/semantyki;
- repo instructions i specialist contracts.

Audyt nie korzysta z odpytywania procesu ani nowego runu. Nie przedstawia statycznego wiring jako potwierdzonej emisji konkretnego uruchomienia.

## 2. Routing specjalistyczny i użyte kontrakty

Primary specialist:

- Ghost Runtime Coordinator.

Supporting specialists:

- SSOT Feature Materialization Guardian;
- Gatekeeper Policy Auditor;
- Oracle Session Runtime Engineer;
- Decision Logging Replay Analyst;
- Config Rollout Safety Reviewer;
- Solana Execution Path Engineer;
- Seer Ingest Event Integrity Specialist w zakresie granicy wejścia.

Załadowane skille:

- `ghost-execution`;
- `trading-systems`;
- `statistical-research-engine`;
- `abstract-reasoning`.

Sprawdzone kontrakty:

- `MaterializedFeatureSet` pozostaje SSOT terminalnej decyzji;
- materializacja następuje na granicy sesja → policy;
- Gatekeeper nie powinien odczytywać konkurencyjnego mutable live state;
- terminalny wynik ma typed verdict i reason code;
- shadow nie jest live inclusion;
- submit nie jest confirmation;
- unknown execution nie jest success;
- config enabled nie jest dowodem wiring ani authority;
- DecisionLogger/replay są evidence plane, nie policy plane;
- post-BUY decision, action i outcome muszą pozostać rozdzielone.

Fast path nie został użyty: zadanie jest cross-cutting, obejmuje SSOT, Gatekeeper, config, replay, execution boundary i post-BUY.

## 3. Opis problemu — 3W2H

### What

Repo posiada kilka generacji Gatekeepera, kilka obiektów nazywanych selectorem, dodatkowe veto/gate'y i rozbudowaną konfigurację post-BUY. Samo istnienie modułu lub `enabled=true` nie ujawnia, czy komponent:

- tworzy dane;
- podejmuje decyzję;
- tylko loguje kontrfaktyczny wynik;
- jest możliwy do promocji konfiguracją;
- jest flagą bez konsumenta;
- należy do wyłączonego legacy.

### Where

Główne obszary:

- `ghost-launcher/src/session/`;
- `ghost-launcher/src/tx_intelligence/`;
- `ghost-core/src/checkpoint/`;
- `ghost-launcher/src/components/gatekeeper*.rs`;
- `ghost-launcher/src/components/iwim_veto.rs`;
- `ghost-launcher/src/oracle_runtime.rs`;
- `ghost-brain/src/oracle/decision_logger.rs`;
- `ghost-launcher/src/components/post_buy_runtime.rs`;
- `ghost-brain/src/guardian/post_buy/`;
- dwa aktywne pliki konfiguracyjne.

### Why

Brak jednej aktualnej mapy authority tworzy ryzyko:

- uznania shadow V3 za aktywny Gatekeeper;
- uznania `promotion.enabled` za działający switch;
- pomylenia selector sidecara z terminalnym selector gate;
- uznania orphaned `[exit_strategy]` za aktywną drabinę wyjścia;
- zmiany TOML, która nie zmieni zachowania;
- oceny jakości decyzji na feature'ach o różnych denominatorach;
- aktywnego veto na inputach, które nie spełniają semantyki analizatora.

### How

- zmapowano launcher od Seer/Event Bus do sesji;
- prześledzono `try_materialize_features()` i typ `MaterializedFeatureSet`;
- odtworzono dokładną kolejność pure policy V2;
- rozdzielono DOW, TAS, PDD i APS w V2.5;
- potwierdzono miejsce wywołania V3 i brak promotion consumer;
- rozdzielono pięć znaczeń selectora;
- prześledzono IWIM od RPC fetch do classifier/scoring;
- prześledzono Trigger handoff i wszystkie lane'y PostBuyRuntime;
- porównano konfigurację Guardiana/AEM/exit strategy z faktycznym wiring;
- zapisano priorytety i mierzalne acceptance gates.

### How many

Zmiana dodaje dokładnie dwa dokumenty. Nie zmienia kodu, konfiguracji, runtime behavior, danych ani trybu execution.

## 4. Root cause

Główną przyczyną problemu nie jest brak pojedynczego algorytmu. Jest nią narastanie kilku warstw o różnych klasach władzy bez jednego maszynowo czytelnego kontraktu authority:

- V2 jest terminalne;
- V2.5 łączy shadow DOW z pośrednio aktywnym TAS i config-latent PDD;
- V3 jest challengerem shadow z inert promotion flag;
- selector jest nazwą wspólną dla policy score, sidecara, pipeline'u offline i execution latch;
- post-BUY łączy proste aktywne progi z niewykorzystanymi signal actions, niewired AEM i orphaned ladder;
- config pokazuje intencję, ale launcher kopiuje tylko część pól;
- reduktory nie zawsze zaczynają od jednego event-admission universe.

Najpoważniejszy konkretny root cause dotyczy IWIM: transport przekazuje timestamp placeholders, a classifier oczekuje bajtów transakcji. Liczebność i timestamp coverage podnoszą confidence mimo braku instruction content.

## 5. Decyzja audytowa

Przyjęto następujący opis bieżącej architektury:

1. `PoolObservationSession::try_materialize_features()` tworzy kanoniczny snapshot.
2. Gatekeeper V2 jest jedynym pełnym terminalnym policy engine prebuy/prereject.
3. Curve readiness jest osobnym gate'em po V2 BUY.
4. IWIM jest terminalnym post-V2 veto, ale jego aktualna semantyka wejścia nie uzasadnia authority.
5. V2.5 DOW jest shadow; TAS ma pośredni wpływ przez strength; PDD jest config-latent przy dwupoziomowej promocji; APS jest kontekstem/sugestią.
6. V3 jest shadow/replay; config nie może sam nadać mu authority.
7. Tylko embedded V2 selector soft-score może po zmianie configu odrzucać; sidecar/offline nie mogą.
8. Bieżący BUY prowadzi do shadow simulation, nie live buy.
9. Aktualny shadow exit to +50% / -50% / 30 s inactivity full close.
10. LIGMA/WHF/TCF/PANIC nie sterują tym prostym close, AEM nie jest podłączony, TimeStopV2/replay/burn-in/probe są observational, a `[exit_strategy]` jest orphaned.

## 6. Rekomendowana strategia naprawcza

### P0

1. Odebrać IWIM authority do czasu realnego transaction-content coverage.
2. Dodać Decision Authority Manifest i fail-closed validation dla enabled-but-unwired/inert config.
3. Wprowadzić jeden immutable `AdmittedDecisionEvent` dla wszystkich reducerów.
4. Przenieść DOW/checkpoints/replay na ten sam pure evaluator i snapshot contract co terminal.

### P1

1. Zbudować `PostBuyDecisionSnapshot` i typed pure PostBuyPolicy.
2. Oddzielić decision, action intent, execution outcome i reconciliation.
3. Przekazywać pełny Guardian config albo usunąć fałszywe pola.
4. Ustawić AEM false do wiring; usunąć/przenieść orphaned `[exit_strategy]`.
5. Budować V3 promotion bridge dopiero po replay, coverage, disagreement, walk-forward i execution-aware evidence.
6. Ujednoznacznić nazwy selectorów i mierzyć incremental value.

### P2

1. Generować effective-config manifest.
2. Naprawić taxonomy `legacy_live` i wersjonowanie policy.
3. Rozdzielić per-candidate materialization failure od global invariant failure.
4. W live blokować nowe entry przy utracie durable audit, zachowując możliwość exit/reconciliation.

## 7. Walidacja i zabezpieczenia

Walidacja wykonana:

- `cargo test -p ghost-launcher --test gatekeeper_v3_tests` — PASS, 9/9;
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression` — PASS, 42/42;
- `cargo test -p ghost-launcher --lib configured_rpc_url_rejects_placeholders_and_aliases` — PASS, 1/1;
- `cargo test -p ghost-launcher --lib shadow_only_emits_post_buy_submitted_for_successful_shadow_lane` — PASS, 1/1;
- `cargo test -p ghost-launcher --test post_buy_runtime_integration` — PASS, 4/4;
- `cargo test -p ghost-brain --test ghost_brain_config_load_test` — baseline FAIL: 6 passed, 1 failed. Test `gatekeeper_v3_config_loads_from_production_toml` oczekuje `min_market_cap_sol=5.0`, a bieżący produkcyjny TOML ładuje 115.0. Failure nie został wprowadzony przez dokumenty i potwierdza znaleziony config/test drift;
- kontrola symboli i wiring przez `rg` — wykonana;
- końcowe sprawdzenie whitespace i zakresu diffu — wykonywane po ostatniej aktualizacji dokumentów.

Zabezpieczenia zakresu:

- nie zmieniono Gatekeeper policy;
- nie zmieniono progów;
- nie zmieniono `MaterializedFeatureSet`;
- nie zmieniono configu;
- nie włączono live execution;
- nie wykonano transakcji;
- nie zmieniono DecisionLogger schema;
- nie implementowano rekomendacji podczas audytu;
- legacy Hyper/Oracle nie został przypadkowo przywrócony.

Ryzyka interpretacyjne:

1. **Ryzyko:** raport zostanie odczytany jako zgoda na live.
   **Zabezpieczenie:** jawnie stwierdzono, że bieżący profil jest shadow-only i audyt nie rekomenduje przełączenia.

2. **Ryzyko:** V3 zostanie włączony przez inert flag.
   **Zabezpieczenie:** raport wymaga jawnego promotion bridge i evidence gate.

3. **Ryzyko:** rekomendacja wyłączenia IWIM zostanie potraktowana jako dowód, że cały moduł jest bezwartościowy.
   **Zabezpieczenie:** problem dotyczy obecnego transportu/quality semantics; docelowa analiza realnego payloadu pozostaje wartościowa.

4. **Ryzyko:** post-BUY signal modules zostaną uznane za zupełnie niewired.
   **Zabezpieczenie:** raport rozróżnia uruchomienie i emisję sygnałów od skutecznej władzy nad prostym close.

5. **Ryzyko:** kodowe istnienie live lane zostanie pomylone z aktywnością.
   **Zabezpieczenie:** każda ocena zawiera osobno config, wiring i authority.

## 8. Rezultat i handoff

Rezultatem jest `PLANS/AUDYT/AUDYT_DECISION_PLANE_END_TO_END_20260714.md`, zawierający:

- mapę end-to-end;
- macierz wszystkich ważnych komponentów;
- analizę faz I, II i III;
- dokładną kolejność V2;
- charakterystykę V2.5 i V3;
- rozdzielenie pięciu selectorów;
- analizę IWIM;
- status wszystkich mechanizmów post-BUY;
- findings F-I/F-II/F-III;
- priorytety P0/P1/P2 z acceptance gates;
- ślad routingu specjalistycznego.

Decyzja końcowa:

Audyt zostaje zamknięty jako dokumentacyjny. Następny krok powinien być osobnym, review-gated planem P0 zaczynającym się od IWIM authority oraz Decision Authority Manifest. Nie należy równolegle promować V3, selectora ani live execution przed naprawą truth/authority contracts.
