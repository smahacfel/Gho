Zestaw żelaznych reguł, które obowiązują:

1. AccountStateCore jest jedynym source of truth dla canonical market state.
2. TxIntelligenceEngine nigdy nie zapisuje bezpośrednio do canonical state.
3. Gatekeeper nigdy nie rekonstruuje stanu samodzielnie.
4. ShadowLedger nigdy nie jest używany jako live market truth.
5. Każda pool od narodzin dostaje własną PoolObservationSession.
6. Każda decyzja Gatekeepera musi być odtwarzalna z checkpointów i feature snapshotów.
7. Stan ma być redukowany przyrostowo, nie przeliczany od zera.
8. FeatureBuilder/CheckpointEngine budują cechy; Gatekeeper interpretuje je wyłącznie jako politykę i progi decyzyjne.
9. Canonical state aktualizuje się tylko według jawnego monotonicznego porządku zdarzeń.
10. Bootstrap/pending state nigdy nie może być mylony z canonical live state.
11. Simulation state nigdy nie może nadpisywać canonical state.


# 1. Definicja architektury docelowej

Docelowa architektura dla tego repo **nie jest ani `tx-first`, ani `account-first` w czystej formie**.

To jest:

## **Event-Triggered Session Engine**

w układzie:

## **tx-bootstrap / account-state core / tx-intelligence sidecar / Gatekeeper policy verdict**

Najkrócej:

`tx trigger -> session start -> canonical account-state -> tx-intelligence enrichment -> checkpoint/feature materialization -> Gatekeeper policy verdict -> optional commit/execution handoff`

To jest **docelowy model architektoniczny** dla repo.

Ważne doprecyzowanie:

* to **nie jest wierny opis aktualnej implementacji 1:1**,
* to jest **kontrakt docelowy**, do którego obecny kod ma zostać doprowadzony,
* dziś elementy tego modelu są rozłożone między `Seer`, `OracleRuntime`, per-pool `pool_observation_task`, `SnapshotListener`, `SnapshotEngine`, `GatekeeperCommitLoop`, `LivePipeline` i `ShadowLedger`.

---

# 2. Główna idea biznesowo-techniczna

Ten system nie jest projektowany jako klasyczny sniper reagujący na pierwszy możliwy sygnał w 50–200 ms.

On jest projektowany jako:

## **silnik krótkiej sesji obserwacyjnej i selekcji jakościowej poola**

W obecnym repo okno to jest już konfigurowalne przez logikę Gatekeepera (`[gatekeeper]`, `gatekeeper_v2`, observation/max_wait window) i w praktyce odpowiada temu, co konceptualnie opisujemy jako „8-sekundową sesję”.

To oznacza, że celem nie jest:

* „wejść jak najszybciej”
* ani „zgadnąć z jednego tx”

tylko:

* **zrekonstruować możliwie wiernie stan i zachowanie poola w pierwszym krótkim oknie życia**
* i na tej podstawie wydać **deterministyczną decyzję**.

To przesuwa priorytety architektoniczne:

z:

* reaktywności za wszelką cenę,

na:

* poprawność stanu,
* jakość feature’ów,
* stabilność semantyczną,
* checkpointową analizę trajektorii,
* egzekwowalną politykę filtrów.

---

# 3. Kluczowy podział odpowiedzialności

Cała architektura stoi na czterech rozdzielonych pytaniach:

## 3.1. Kiedy zaczynam obserwację?

To jest rola:

## `tx trigger`

W aktualnym repo to odpowiada głównie torowi transakcyjnemu `Seer` + emisji `GhostEvent::NewPoolDetected` / `GhostEvent::PoolTransaction`.

## 3.2. Jaki jest rzeczywisty stan poola?

To jest rola:

## `account-state core`

To jest **docelowa warstwa logiczna**, która dziś nie istnieje jeszcze jako jedna czysta struktura o tej nazwie.  
Docelowo ma być wydzielona z obecnie rozproszonych ścieżek account-derived state (`Seer` account path, `OracleRuntime` reconciliation/account handling, wybrane reduktory sesyjne).

## 3.3. Jak zachowują się uczestnicy rynku?

To jest rola:

## `tx-intelligence`

W aktualnym repo ta warstwa także nie jest jeszcze wydzielona jako osobny `TxIntelligenceEngine`; jej odpowiedniki siedzą dziś głównie w per-pool observation logic w `OracleRuntime`, analizie buforowanych transakcji, fingerprintingu i feature extraction.

## 3.4. Czy ten pool przechodzi politykę selekcji?

To jest rola:

## `Gatekeeper policy verdict`

Ważne: w tym repo trzeba odróżnić:

* **Gatekeeper jako warstwę polityki/verdyktu**
od
* **`GatekeeperCommitLoop` jako warstwę commit/handoff po werdykcie**.

To rozdzielenie jest święte.  
Jeśli te role zostaną znów wymieszane, system wróci do chaosu pojęciowego i podwójnej prawdy.

---

# 4. Ostateczny pipeline logiczny

Docelowy pipeline powinien wyglądać tak:

## Faza A — `Birth Detection`

Lekki tor transakcyjny wykrywa narodziny poola.

## Faza B — `Session Start`

Tworzona jest logiczna `PoolObservationSession`.

## Faza C — `Canonical State Tracking`

Account updates utrzymują jedyną prawdę o stanie rynku.

## Faza D — `Behavioral Semantics`

Tx feed zasila warstwę interpretacji zachowań uczestników.

## Faza E — `Checkpoint + Feature Materialization`

W ustalonych checkpointach materializowane są snapshoty i cechy.

## Faza F — `Policy Evaluation`

Gatekeeper konsumuje gotowe feature’y i wydaje `PASS/FAIL`.

## Faza G — `Post-Verdict Routing`

Po `PASS` wynik może być przekazany do commit/execution path; po `FAIL` sesja zostaje zamknięta, zapisana i ewentualnie zarchiwizowana do replay/tuningu.

Dla aktualnego repo oznacza to praktycznie:

* `PASS` może prowadzić dalej do:
  * runtime approval,
  * `GatekeeperCommitLoop`,
  * `LivePipeline`,
  * opcjonalnie execution/trigger path,
* `FAIL` kończy obserwację, czyści stan sesji i zostawia ślad diagnostyczny.

---

# 5. Docelowa rola istniejących cegieł w repo

Na bazie obecnego kodu najzdrowsze odwzorowanie istniejących komponentów wygląda tak.

## 5.1. `Seer`

`Seer` zostaje warstwą ingressu.

Ale semantyka jego roli się zmienia.

Nie powinien być traktowany jako miejsce, które „buduje prawdę rynkową z tx”.

Powinien być:

## **dual-ingest layer**

czyli dostarczać dwa niezależne tory wejściowe:

### `tx path`

Do:

* wykrywania narodzin poola,
* semantyki flow,
* emisji `NewPoolDetected` / `PoolTransaction`,
* zasilania przyszłego `TxIntelligenceEngine`.

### `account path`

Do:

* odbioru i dekodowania `AccountUpdate`,
* aktualizacji kanonicznego modelu stanu,
* zasilania przyszłego `AccountStateCore`.

Czyli `Seer` ma być producentem zdarzeń wejściowych, a nie arbitrem prawdy o rynku.

Dodatkowo, w obecnym repo trzeba jasno zaznaczyć:

* dzisiejsza ścieżka `AccountUpdate -> IPC -> GhostEvent::AccountUpdate -> OracleRuntime::process_account_update(...)` jest nadal historycznie zbudowana wokół reconciliation,
* w modelu docelowym account path nie może być już „legacy corrective path”, tylko **główna ścieżka budowy canonical state**.

---

## 5.2. `PoolObservationSession`

To powinno stać się nowym centrum systemu.

Dzisiaj **najbliższym odpowiednikiem** jest:

* per-pool `pool_observation_task`,
* `PoolObservationMsg`,
* `PoolObservationContext`,
* `PoolTaskHandle`,
* `GatekeeperBuffer`,
* część per-pool stanu w `OracleRuntime`.

Czyli:

**logiczna sesja istnieje już częściowo**, ale nie jako jeden jawny, spójny byt.

Docelowo każda nowa pool po wykryciu dostaje własną sesję obserwacyjną.

Sesja trwa od `t0` do końca okna obserwacyjnego albo do wcześniejszego `hard reject`.

Ta sesja jest logicznym kontenerem na wszystko, co dotyczy jednego obserwowanego poola.

Powinna zawierać co najmniej:

* identyfikator poola / mint / venue,
* czas startu,
* status sesji,
* kanoniczny stan rynku,
* stan warstwy tx-intelligence,
* historię checkpointów,
* aktywne flagi ryzyka,
* wynik końcowy,
* metadane diagnostyczne i powody odrzucenia.

To jest nowy „mózg operacyjny” pojedynczej obserwacji.

---

## 5.3. `AccountStateCore`

To jest najważniejsza warstwa architektury.

To ona ma być:

## **jedynym source of truth dla canonical market state**

Nie `ShadowLedger`, nie parser transakcji, nie Gatekeeper, nie `SnapshotEngine`.

Bardzo ważne w kontekście tego repo:

* dziś `SnapshotEngine` i `ShadowLedger` są silnie obecne operacyjnie,
* ale **nie powinny być traktowane jako docelowe źródło canonical market truth**,
* `SnapshotEngine` może zostać jako warstwa snapshot/staging/derived consumption,
* `ShadowLedger` może zostać jako sim/WAL/replay substrate,
* ale **canonical truth ma należeć do account-derived state**.

`AccountStateCore`:

* konsumuje account updates,
* redukuje stan przyrostowo,
* utrzymuje lokalny obraz rynku,
* produkuje snapshoty stanu dla checkpointów i feature buildera.

Przykładowe obszary odpowiedzialności:

* reserves / vault balances,
* bonding curve progress,
* supply / dystrybucja,
* holder concentration,
* authority / ownership changes,
* dev exposure,
* net state deltas,
* structural market health metrics.

### Najważniejszy invariant:

## `AccountStateCore` jest kanoniczną prawdą.

Wszystko inne tylko go wzbogaca albo konsumuje.

---

## 5.4. `TxIntelligenceEngine`

To nie jest Gatekeeper.  
To nie jest też rdzeń stanu.

To jest:

## **warstwa interpretacji zachowania uczestników rynku**

Odpowiada za semantykę flow:

* kto kupuje / sprzedaje,
* w jakiej kolejności,
* z jaką intensywnością,
* czy wzorzec wygląda organicznie,
* czy jest churn,
* czy są ślady bundlingu,
* czy dev wykonuje podejrzane ruchy,
* czy pojawiają się sygnały sybilowe / recyclingowe / wash-like.

Ta warstwa konsumuje tx feed i zapisuje do sesji **interpretację**, a nie „prawdę stanu”.

W kontekście aktualnego repo trzeba doprecyzować:

* dziś ta logika jest częściowo rozlana po `OracleRuntime`, `GatekeeperBuffer`, fingerprintingu, feature extraction i obserwacji per-pool tasków,
* docelowo powinna być logicznie wydzielona jako osobna warstwa odpowiedzialności,
* ale **nie musi** oznaczać osobnego crate’a albo osobnego taska od pierwszego PR — najpierw wystarczy twarde rozdzielenie kontraktów.

### Krytyczny invariant:

## `TxIntelligenceEngine` nigdy nie mutuje canonical account-derived state.

Może:

* dodawać feature’y,
* ustawiać risk flags,
* aktualizować modele zachowań,
* zapisywać semantyczne snapshoty.

Nie może:

* przeliczać reserves jako prawdy,
* nadpisywać `AccountStateCore`,
* stawać się konkurencyjnym source of truth.

To jest absolutnie kluczowe.

---

## 5.5. `CheckpointEngine` i `FeatureBuilder`

To jest brakujące rozróżnienie, które trzeba dopisać explicite, żeby Gatekeeper nie wrócił do roli „worka na wszystko”.

`CheckpointEngine` i `FeatureBuilder` to **nie Gatekeeper**.

To jest:

## **warstwa materializacji obserwacji**

Ich odpowiedzialnością jest:

* utrwalanie snapshotów poznawczych w określonych punktach,
* budowa cech na bazie:
  * canonical state,
  * tx-intelligence,
  * historii trajektorii,
  * flag ryzyka,
  * checkpointów czasowych i/lub eventowych.

### Kluczowa zasada:

## `CheckpointEngine` / `FeatureBuilder` budują obserwowalne cechy; `Gatekeeper` jedynie interpretuje je według polityki.

To oznacza:

* Gatekeeper nie oblicza źródłowych feature’ów ad hoc,
* Gatekeeper nie robi własnego ukrytego mini-reducera,
* Gatekeeper nie jest miejscem, gdzie „jeszcze coś sobie dorysujemy”.

To rozdzielenie jest niezbędne dla tego repo, bo bez niego cały ciężar znowu spłynie do `OracleRuntime`/Gatekeepera.

---

## 5.6. `Gatekeeper`

`Gatekeeper` zostaje, ale jego miejsce i odpowiedzialność muszą być wyraźnie zawężone.

Gatekeeper nie jest warstwą poznawczą.  
Nie powinien:

* budować live state,
* zgadywać market truth,
* parsować wszystkiego od zera,
* być workiem na logikę całego świata.

Powinien być:

## **policy engine + hard filters + verdict engine**

Czyli konsumuje:

* snapshoty checkpointów,
* feature’y z account-state,
* feature’y z tx-intelligence,
* risk flags,
* trajectory features,
* politykę progów,

i wydaje:

* `PASS`
* `FAIL`
* opcjonalnie `WATCHLIST`, jeśli kiedyś taki tryb zostanie świadomie dodany.

W tym repo trzeba też jasno odróżnić:

* **Gatekeeper policy/verdict**
od
* **`GatekeeperCommitLoop`**, który:
  * nie podejmuje poznawczej decyzji,
  * tylko obsługuje commit gotowych okien i handoff do live path.

W modelu produktowym Gatekeeper jest więc:

## **końcówką pipeline’u obserwacyjno-decyzyjnego**

a nie jego całością.

---

## 5.7. `SnapshotListener` i `SnapshotEngine`

To trzeba dopisać, bo te komponenty są dziś zbyt centralne, żeby je pominąć.

### `SnapshotListener`

W obecnym repo jest to:

* konsument `GhostEvent::PoolTransaction`,
* staging/replay layer dla tx,
* wejście do `SnapshotEngine`.

Docelowo nie powinien być interpretowany jako źródło canonical market truth.  
Jego zdrowa rola to:

* przyjmować tx po stronie snapshot/history pipeline,
* stagingować tx dla pooli, których identity/approval jeszcze się domyka,
* forwardować tx do `SnapshotEngine` zgodnie z polityką forwardingu.

### `SnapshotEngine`

W obecnym repo jest to centralny engine snapshotów i stagingu transakcyjnego.

Docelowo najlepiej rozumieć go jako:

## **derived snapshot / runtime staging engine**

a nie jako jedyny model prawdy o rynku.

Czyli:

* może pozostać ważną warstwą operacyjną,
* ale nie może semantycznie konkurować z `AccountStateCore`.

---

## 5.8. `ShadowLedger`

To jest jedna z najważniejszych korekt architektonicznych.

`ShadowLedger` nie powinien już pełnić roli „live-state truth”.

W aktualnym repo nadal jest mocno wpięty w:

* recovery,
* snapshot restore,
* runtime enrichment,
* commit/live pipeline,
* WAL replay,
* LivePipeline flush,
* okresowe snapshoty na dysk.

To oznacza, że **dziś** jest bardziej centralny, niż powinien być w modelu docelowym.

Docelowo powinien zejść do roli:

## **simulation + WAL + replay + forensic trail**

Czyli:

* ahead-of-time simulation dla własnych planowanych akcji,
* zapis i odtwarzanie decyzji,
* audit trail,
* diagnostyka po fakcie,
* replay danych do tuningu i testów,
* ewentualna trwałość stanu pomocniczego.

To jest zdrowa rola.

Czyli:

* `AccountStateCore` = canonical truth
* `ShadowLedger` = simulation / replay substrate

To porządkuje system.

---

# 6. Finalny model odpowiedzialności komponentów

Docelowo wygląda to tak:

## Wejście

* `Seer` tx path jako **BirthDetector / TransactionIngress**
* `Seer` account path jako **AccountUpdateIngress**

## Orkiestracja sesji

* `OracleRuntime` jako obecny host sesji runtime
* logiczna `PoolObservationSession`
* przyszły/jawny `SessionManager`

## Prawda stanu

* `AccountStateCore`
* `AccountStateReducer`

## Semantyka zachowania

* `TxIntelligenceEngine`
* pomocniczo np. fingerprinting / wallet profiling / dev behavior / flow pattern analysis

## Checkpointy i cechy

* `CheckpointEngine`
* `FeatureBuilder`
* `TrajectoryAnalyzer`

## Decyzja

* `HardFilterEngine`
* `GatekeeperVerdictEngine`

## Commit / live routing

* `GatekeeperCommitLoop`
* `LivePipeline`

## Snapshot / staging downstream

* `SnapshotListener`
* `SnapshotEngine`

## Symulacja / ślad / forensics

* `ShadowLedger`
* `WalRecorder`
* `ReplayExporter`

To jest właściwy logiczny podział systemu **w zgodzie z obecnym repo**.

---

# 7. Ostateczna semantyka przepływu danych

Poniżej pełny przepływ danych, już w formie końcowej.

## 7.1. Trigger

Tx/log/instruction path w `Seer` wykrywa narodziny poola.

## 7.2. Session Open

`OracleRuntime` (docelowo przez jawny `SessionManager`) zakłada per-pool sesję obserwacyjną.

## 7.3. State Ingestion

Account path dostarcza `AccountUpdate` do `AccountStateCore`.

## 7.4. State Reduction

`AccountStateCore` aktualizuje canonical market state przyrostowo.

## 7.5. Tx Intelligence Ingestion

Tx path dostarcza transakcje do `TxIntelligenceEngine`.

## 7.6. Semantic Enrichment

`TxIntelligenceEngine` dokleja semantykę zachowania do sesji: flow, wallet roles, churn, burst, dev patterns itd.

## 7.7. Checkpoints

`CheckpointEngine` w określonych momentach tworzy snapshoty poznawcze sesji.

## 7.8. Feature Materialization

`FeatureBuilder` buduje końcowe cechy z:

* account-state,
* tx-intelligence,
* historii checkpointów,
* trajectory metrics,
* risk flags.

## 7.9. Policy Evaluation

`HardFilterEngine` + `GatekeeperVerdictEngine` oceniają politykę i wydają werdykt.

## 7.10. Post-Decision Routing

Po `PASS` sesja może być przekazana do:

* runtime approval,
* `GatekeeperCommitLoop`,
* `LivePipeline`,
* opcjonalnie execution handoff.

Po `FAIL` sesja zostaje zamknięta, opisana powodami i wysłana do replay/WAL/telemetrii.

Równolegle:

* `SnapshotListener` / `SnapshotEngine` mogą nadal obsługiwać downstream tx snapshot pipeline,
* ale nie stają się przez to canonical truth of market state.

---

# 8. Zasady twarde — invariants architektoniczne

To powinny być twarde reguły projektu.

## Invariant 1

**Każda pool ma własną logiczną `PoolObservationSession`.**

W aktualnym repo może to być jeszcze rozproszone między per-pool task, buffer, runtime state i checkpoint state, ale semantycznie ma to być jedna sesja.

## Invariant 2

**`AccountStateCore` jest jedynym source of truth dla canonical market state.**

## Invariant 3

**`TxIntelligenceEngine` nie może mutować canonical state.**

## Invariant 4

**`CheckpointEngine` i `FeatureBuilder` budują cechy; `Gatekeeper` wyłącznie interpretuje je jako politykę i progi.**

## Invariant 5

**`Gatekeeper` nie buduje stanu, tylko ocenia politykę na gotowych feature’ach.**

## Invariant 6

**`ShadowLedger` nie jest live-state truth.**

## Invariant 7

**`SnapshotEngine` nie jest canonical market truth; jest warstwą snapshot/staging/derived downstream.**

## Invariant 8

**Stan jest redukowany przyrostowo, a nie liczony od zera na każdym ticku.**

## Invariant 9

**Każdy werdykt `PASS/FAIL` musi być odtwarzalny z checkpoint snapshotów i feature setu.**

## Invariant 10

**Tx służy do triggera i semantyki zachowań; account updates służą do stanu rynku.**

## Invariant 11

**Canonical state aktualizuje się tylko według jawnego monotonicznego porządku zdarzeń (`slot`, provider/write ordering jeśli dostępny, lokalny receive ordering).**

## Invariant 12

**Bootstrap/pending state nigdy nie może być mylony z canonical live state.**

To jest szczególnie ważne przy cold starcie nowej pool.

## Invariant 13

**Simulation state nigdy nie może nadpisywać canonical state.**

Dotyczy to przede wszystkim `ShadowLedger` i wszelkich future bundle simulations.

To są fundamenty, których trzeba pilnować bez kompromisów.

---

# 9. Jak rozumieć checkpointy w tym modelu

Checkpointy są tu fundamentalne.

To nie jest tylko mechanizm techniczny.  
To jest centralny mechanizm poznawczy.

Bo celem nie jest wyłącznie znać stan końcowy po oknie obserwacyjnym, tylko rozumieć:

* jak pool się rozwijał,
* czy wzrost był organiczny,
* czy buy pressure rosło czy gasło,
* czy dystrybucja poprawiała się czy degenerowała,
* czy aktywność signerów była zdrowa czy recyklingowana,
* czy metryki ryzyka rosły czy malały.

Dlatego system powinien mieć:

* checkpointy czasowe,
* opcjonalnie checkpointy eventowe przy istotnych mutacjach,
* zapis stanu i feature’ów w tych punktach.

Checkpoint nie jest werdyktem.  
Checkpoint jest **obserwacyjnym snapshotem poznawczym**, na którym później pracuje polityka.

W aktualnym repo część tej logiki siedzi już w observation taskach i scoring/gating flow, ale powinna zostać nazwana i wyodrębniona kontraktowo.

---

# 10. Jak dokładnie rozumieć relację: tx-intelligence vs Gatekeeper

To jest jeden z najważniejszych punktów końcowej koncepcji.

## `TxIntelligenceEngine`

to:

* evidence builder,
* semantic interpreter,
* behavioral analyzer.

## `Gatekeeper`

to:

* threshold enforcer,
* policy evaluator,
* deterministic verdict engine.

## `CheckpointEngine` / `FeatureBuilder`

to:

* observation materialization layer,
* snapshot/feature producer,
* trajektoryjny i diagnostyczny adapter między stanem/semantyką a polityką.

Czyli formalnie:

`Gatekeeper = f(AccountStateFeatures, TxIntelFeatures, CheckpointFeatures, RiskFlags, Policy)`

To jest właściwe ujęcie.

Nie wolno pojęciowo zlać tych warstw w jedną, bo wtedy Gatekeeper znów zacznie puchnąć w monolit.

---

# 11. Docelowa odpowiedź na problem obecnego repo

W ujęciu obecnego repo najważniejsze przesunięcia są następujące:

## Co zostaje

* `Seer` jako ingress,
* event bus,
* observation window,
* `OracleRuntime` jako obecny host runtime logic,
* Gatekeeper jako decyzja końcowa,
* `GatekeeperCommitLoop` jako commit/handoff layer,
* WAL / replay,
* downstream execution path,
* `SnapshotListener` / `SnapshotEngine` jako tx-snapshot pipeline.

## Co zmienia rolę

* `Seer` przestaje być miejscem lokalnego budowania prawdy rynkowej z tx,
* `AccountUpdate` przestaje być tylko reconcile/fallback path,
* `Gatekeeper` przestaje być workiem na wszystko,
* `ShadowLedger` przestaje być live market truth,
* `SnapshotEngine` przestaje być interpretowany jako canonical state authority.

## Co staje się nowe centrum

* logiczna `PoolObservationSession`,
* `AccountStateCore`,
* `TxIntelligenceEngine`,
* `CheckpointEngine`,
* `FeatureBuilder`.

To jest najważniejsza architektoniczna odpowiedź na obecną strukturę repo.

---

# 12. Ostateczna nazwa modelu

Najbardziej precyzyjna nazwa tej architektury to:

## **Event-Triggered Session Hybrid**

z rozwinięciem:

## **tx-bootstrap / account-state core / tx-intelligence sidecar / Gatekeeper policy verdict**

To najlepiej oddaje sens.

Jeśli chcesz nazwę jeszcze bardziej „repo-friendly”, to można też używać:

## **session-based tx-trigger / account-state / policy-gated runtime**

ale pierwsza nazwa jest bardziej zwięzła i czytelna.

---

# 13. Ostateczna wersja koncepcji — skrót wykonawczy

Jeśli miałbym to zamknąć w jednej zwartej definicji, to brzmiałaby tak:

To jest architektura, w której wykrycie narodzin poola następuje przez lekki tor transakcyjny, po czym system otwiera izolowaną sesję obserwacyjną per pool. W trakcie tej sesji account updates budują i utrzymują jedyny kanoniczny model stanu rynku, natomiast tx feed równolegle zasila niezależną warstwę semantycznej analizy zachowań uczestników. Na bazie checkpointów i zmaterializowanych feature’ów Gatekeeper egzekwuje politykę progową i wydaje deterministyczny werdykt `PASS/FAIL`. `GatekeeperCommitLoop` obsługuje commit/handoff po decyzji, a `ShadowLedger` nie pełni roli live-state truth, lecz jest sprowadzony do funkcji symulacyjnej, WAL i replay/forensics. `SnapshotListener` i `SnapshotEngine` pozostają downstreamowym pipeline’em snapshot/staging dla toru transakcyjnego, ale nie stają się przez to kanoniczną prawdą o rynku.

To jest końcowa wersja koncepcji dopasowana do obecnego repo.

---

# 14. Najkrótsze końcowe podsumowanie

W czterech zdaniach:

* **tx** otwiera sesję i niesie semantykę zachowań,
* **account-state** utrzymuje jedyną prawdę o stanie rynku,
* **Gatekeeper** nie buduje wiedzy, tylko egzekwuje politykę na gotowych feature’ach,
* **`GatekeeperCommitLoop`**, `SnapshotEngine` i `ShadowLedger` są warstwami wykonawczo-operacyjnymi, a nie kanonicznym źródłem live market truth.