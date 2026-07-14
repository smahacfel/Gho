# ADR-8D: Zapis planu korekty kontraktów interpretacyjnych metryk V1

Status: `SUPERSEDED_BY_PLAN_V1_1 / HISTORICAL / NO_RUNTIME_CHANGE`

Następca:
`docs/ADR/ADR_8D_PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_1_20260711.md`

Uwaga: plan pozostaje pod tą samą ścieżką, ale jego treść została zastąpiona
zaakceptowaną wersją V1.1. Niniejszy ADR dokumentuje wyłącznie historyczną V1.

Typ: ADR-8D / implementation-plan decision record

Data zapisu: 2026-07-11

Repo: `/root/Gho_dynamic_exit_v1`

Dokument wejściowy:
`PLANS/AUDYT/RAPORT_AUDYT_KOREKTY_INTERPRETACJI_METRYK_20260710.md`

Dokument decyzyjny:
`PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Poziom ryzyka tej zmiany: `LOW` — wyłącznie dokumentacja, bez kodu/configu/runtime

Uwaga o szablonie: wymagana przez instrukcje repo ścieżka
`docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. ADR zachowuje
lokalny format sekcyjny i jawnie dokumentuje problem, decyzję, granice,
konsekwencje, weryfikację oraz scope control.

## 1. Problem

Audyt dziesięciu rodzin metryk wykazał współistnienie mylących nazw, różnych
populacji i denominatorów, przeciążonych wartości zero, niejednoznacznych
statusów evidence oraz pomieszania powierzchni active, compat, shadow,
export-only i logging-only.

Po korekcie raportu potrzebny był jeden implementowalny plan, który:

- utrzyma `MaterializedFeatureSet` jako SSOT,
- zachowa kompatybilność logów, replay i konfiguracji,
- nie zmieni terminalnej polityki przed zebraniem runtime proof,
- naprawi wszystkie dziesięć kontraktów jako spójny program prac,
- ograniczy liczbę jednostek integracyjnych do kilku większych PR-ów,
- zdefiniuje fail-closed gate przed jakimkolwiek cutoverem.

Sam raport audytowy nie stanowił jeszcze zgody na implementację ani nie opisywał
wystarczająco dokładnie kolejności producer -> MFS -> policy/log/replay ->
burn-in -> cutover.

## 2. Decyzja

Przyjęto i zapisano plan:

```text
PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md
```

Plan organizuje wykonanie w dokładnie trzech większych PR-ach:

1. `Contract Foundation` — registry, typed evidence, schema compatibility,
   rollout mode i parity-only reads.
2. `Producers and Dual Compute` — poprawni producenci, MFS, DecisionLogger,
   replay, comparator, narzędzie audytowe i burn-in.
3. `Conditional V2 Cutover` — wąskie przełączenie autorytatywnych odczytów
   dopiero po pełnym PASS oraz kwarantanna legacy bez usuwania pól.

Przyjęto jeden jawny rollout mode:

```text
Legacy -> DualCompute -> V2
```

`Legacy` pozostaje serde defaultem. W `DualCompute` legacy jest jedynym
terminalnym torem, a V2 jest read-only comparator. W `V2` legacy pozostaje
read-only comparator i prostą ścieżką rollbacku.

## 3. Zamknięte decyzje semantyczne

Plan utrwala następujące rozstrzygnięcia:

- runtime FTDI zachowuje `unique_topologies / unique_samples`; HHI diversity
  pozostaje osobną coordination-risk export-only surface;
- legacy dev field oznacza first-observed, a nowy canonical primary dev buy
  preferuje create-signature match i fallbackuje deterministycznie do
  najwcześniejszego eligible `TxKey`;
- `dev_volume_ratio` jest gross turnover share, nie total exposure;
- exact same-ms, `<50 ms` cluster i RCE recent są oddzielnymi kontraktami;
- top3 zachowuje skalę ratio 0..1 i jeden compatibility helper;
- legacy flip pozostaje zamrożone, a flip V2 jest osobnym bounded,
  deduped/successful/non-dust evidence producerem poza policy i selectorem;
- legacy FSC i FSC v2 otrzymują osobne statusy; istniejące `fsc` pozostaje
  compatibility aliasem legacy;
- per-pool FSC v2 nie jest promowane do aktywnej polityki;
- manipulation `high_*` nie jest traktowane jako measured facts, gdy pochodzi z
  default false; policy-derived flags mają threshold/stage/config provenance;
- reserve velocity otrzymuje interval i validity status bez zmiany legacy f64;
- recent buy/sell otrzymuje raw counts, optional ratio oraz bounded buy share,
  lecz pozostaje logging-only.

## 4. Historyczny gate przed cutoverem — nieobowiązujący

Ta sekcja dokumentuje wycofany etap V1 i nie nadaje authority bieżącemu PR2C.
Bieżący audit może potwierdzić wyłącznie spójność evidence/replay/equivalence;
prospective burn-in ani cutover nie są autoryzowane.

Przyjęto fail-closed warunek: pełny dostępny replay oraz jednocześnie co najmniej:

```text
8 godzin jednego spójnego runa
700 unikalnych decyzji
100 decyzji dev-known
100 clean flip-v2 evaluable
30 rzeczywistych rozjazdów legacy/v2 dev
```

Niedobór któregokolwiek minimum daje `NOT_EVALUABLE` i blokuje cutover.

Dodatkowo wymagane jest zero błędów schema/deserialization/hash/replay, zero
duplicate/malformed/mixed-run evidence, pełne candidate evidence, exact zero
drift verdict/primary reason/phase vector/soft points oraz comparator
`p99 <= 1 ms`.

Dozwolone klasy audytu:

```text
PASS_EVIDENCE_CONSISTENT
NOT_EVALUABLE
FAIL_SCHEMA_OR_REPLAY
FAIL_POLICY_DRIFT
```

Policy drift nie może być maskowany zmianą progów w tym planie.

## 5. Granice decyzji

Plan nie daje zgody na:

- zmianę BUY/REJECT/TIMEOUT przed warunkowym PR3,
- zmianę thresholdów, wag, phase order, reason codes lub selector score,
- promocję coordination-risk, FSC v2, flip V2, RCE albo reserve velocity do
  aktywnej polityki,
- usunięcie albo reinterpretację istniejących JSON/TOML fields,
- odczyt live state podczas policy evaluation,
- ominięcie MFS albo stworzenie drugiego SSOT w compat buffer,
- zmianę IWIM, post-buy, sendera, builderów, submit/confirmation lub live mode,
- przywrócenie legacy HyperPrediction/Chaos/scoring path,
- szeroki refactor niezwiązanych komponentów.

Każdy z trzech przyszłych PR-ów wymaga osobnego ADR-8D i jawnego proof braku
scope creep.

## 6. Konsekwencje

Pozytywne:

- implementacja ma jedną kolejność i wspólny contract namespace;
- stany pośrednie PR1 i PR2 pozostają bezpieczne dla terminalnego runtime;
- schema, config i replay compatibility są projektowane przed cutoverem;
- 10 metryk jest naprawianych pionowo, bez dziesięciu rozproszonych migracji;
- dane niewystarczające nie mogą zostać omyłkowo uznane za PASS;
- rollback nie wymaga cofania schematu ani utraty historycznego evidence.

Koszty i trade-offy:

- utrzymanie legacy i V2 równolegle czasowo zwiększy schema i test surface;
- dual compute dodaje kontrolowany koszt do terminalnej ewaluacji;
- rygorystyczne minimum 30 rzeczywistych rozjazdów dev może wydłużyć burn-in;
- pełny replay i exact parity mogą ujawnić drift wymagający osobnego planu;
- trzy większe PR-y wymagają rygorystycznego wewnętrznego review order i
  allowlisty, aby nie stały się broad refactorami.

## 7. Pliki tej zmiany

Utworzono wyłącznie:

- `PLANS/DO_REALIZACJI/PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`
- `docs/ADR/ADR_8D_PLAN_KOREKTY_KONTRAKTOW_INTERPRETACJI_METRYK_V1_20260710.md`

Nie zmodyfikowano Rust, Python, TOML, testów ani runtime artifacts.

## 8. Weryfikacja wymagana dla zapisu

- oba dokumenty istnieją pod zaakceptowanymi ścieżkami;
- plan zawiera dokładnie trzy PR-y i wszystkie dziesięć rodzin metryk;
- gate zawiera dokładnie 8 h / 700 / 100 dev-known / 100 flip-v2 / 30 dev
  divergences;
- `NOT_EVALUABLE` jawnie blokuje cutover przy niedoborze;
- `FAIL_POLICY_DRIFT` nie może być rozwiązane dostrojeniem w tym planie;
- plan zachowuje MFS SSOT, schema compatibility, replay i shadow/live boundary;
- trailing whitespace oraz Markdown diff check nie zgłaszają błędów;
- `git status` potwierdza brak dotknięcia niezwiązanych zmian użytkownika.

Nie uruchamia się testów runtime dla documentation-only zapisu. Testy kodu są
wyszczególnione jako acceptance przyszłych PR-ów w planie.

## 9. Scope control

Ta zmiana zapisuje zaakceptowaną decyzję planistyczną. Nie implementuje żadnego
punktu planu i nie jest dowodem działania producerów, dual compute, replay ani
cutoveru.

Nazwa pliku zachowuje datę `20260710`, ponieważ odpowiada dacie źródłowego audytu
i zaakceptowanej wersji planu. Faktyczny zapis dokumentu wykonano 2026-07-11.

```yaml
delegation_trace:
  task_classification: localized documentation persistence
  routing_performed: true
  primary_specialist: none
  supporting_specialists_considered:
    - Ghost Runtime Coordinator
    - SSOT Feature Materialization Guardian
    - Decision Logging Replay Analyst
  specialist_docs_loaded: []
  specialist_docs_not_loaded:
    - name: Ghost runtime specialist documents
      reason: this task only persists the already accepted plan and makes no new runtime or architecture decision
    - name: Solana Execution Path Engineer
      reason: execution is explicitly out of scope and no execution file is changed
  skills_used:
    - ghost-execution
  fast_path_used: true
  contracts_checked:
    - fidelity to the accepted three-PR plan
    - MaterializedFeatureSet SSOT boundary
    - backward-compatible schema and config intent
    - replay and shadow/live separation intent
    - mandatory ADR creation
    - documentation-only scope
  unresolved_routing_uncertainty: []
```
