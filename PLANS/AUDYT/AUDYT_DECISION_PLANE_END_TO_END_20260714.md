# Audyt Decision Plane end-to-end — stan bieżący 2026-07-14

Status: **AUDIT COMPLETE / REPORT-ONLY / NO RUNTIME CHANGE**
Repozytorium: `/root/Gho_dynamic_exit_v1`
Gałąź: `main`
Badany commit: `aca107afc14b3122899af2afa2091ccfb400f498`
Aktywna konfiguracja launchera: `config.toml`
Konfiguracja polityk: `ghost-brain/ghost_brain_config.toml`
Klasa ryzyka badanego obszaru: **HIGH**

## 0. Werdykt wykonawczy

Obecny Decision Plane nie jest jednym silnikiem. Jest łańcuchem składającym się z:

1. fazy przyjmowania i redukowania zdarzeń,
2. utworzenia kanonicznego, niezmiennego `MaterializedFeatureSet`,
3. terminalnej polityki Gatekeeper V2,
4. osobnej bramki gotowości curve,
5. następczej bramki IWIM dla kandydatów V2 `BUY`,
6. shadow-only warstw V2.5 i V3,
7. dispatchu do symulowanego wykonania,
8. post-BUY runtime'u, którego aktualnie skuteczną polityką shadow exit są proste progi `+50% / -50% / 30 s bez aktywności`.

Najważniejszy stan faktyczny:

- aktywny terminalny werdykt prebuy/prereject tworzy **Gatekeeper V2**, działający na `MaterializedFeatureSet`;
- `Gatekeeper V2.5` działa obecnie przede wszystkim jako **shadow checkpoint/diagnostic plane**; z V2.5 do terminalnej polityki może zostać promowany tylko PDD, i to po spełnieniu dwóch poziomów flag;
- `Gatekeeper V3` jest **shadow/replay plane**. Pole `promotion.enabled` istnieje w konfiguracji, ale nie ma konsumenta zmieniającego terminalną decyzję;
- w repo występuje kilka różnych rzeczy nazywanych „selector”. Tylko osadzony w V2 `selector_soft_score` może po zmianie konfiguracji odrzucać kandydatów; `selector_shadow_score_v1.jsonl` i pipeline offline są obserwacyjne;
- aktywny profil jest `shadow_only` / `execution_mode="shadow"`. `BUY` oznacza zgodę na symulację i shadow lifecycle, a nie zakup on-chain;
- obecny config V2 jest raczej **collector-permissive po spełnieniu wysokich progów bazowych**: główna selektywność pochodzi z `55 tx / 41 signerów / 39 buy`, szybkości, wolumenu, market cap, bonding curve oraz curve readiness. Większość dodatkowych warstw (`selector`, Alpha, Prosperity, aktywne kary Sybil) jest wyłączona albo ustawiona bardzo łagodnie;
- najpoważniejszą luką P0 jest **semantycznie niepełny IWIM**: aktywna bramka pobiera wyniki `getSignaturesForAddress`, lecz przekazuje analizatorowi znaczniki czasu jako bajtowe placeholdery zamiast surowych transakcji. Model może więc raportować wysoką jakość/confidence bez danych potrzebnych do rozpoznania typów instrukcji, a brak wykrytych wzorców zwiększa „organic”. Do czasu naprawy IWIM nie powinien mieć władzy terminalnej;
- najpoważniejszą luką post-BUY jest rozjazd między szeroką konfiguracją a realnym wykonaniem. AEM jest skonfigurowany jako enabled, ale nie jest podłączony przez aktywny launcher; `[exit_strategy]` nie ma konsumenta Rust; sygnały LIGMA/WHF/TCF/PANIC mogą zmieniać wirtualny book, lecz prosta ścieżka shadow exit omija politykę, która wykorzystałaby te mutacje do zamknięcia pozycji.

## 1. Zakres, wyłączenia i standard dowodu

### 1.1 Zakres

Audyt obejmuje całą logiczną warstwę decyzyjną od wykrycia puli do zakończenia aktualnego shadow lifecycle:

- ingest i routing zdarzeń w zakresie, w jakim tworzą dane decyzji;
- `PoolObservationSession` i wszystkie reduktory danych prebuy;
- `MaterializedFeatureSet` jako SSOT decyzji;
- Gatekeeper V2, V2.5 i V3;
- wszystkie znaczenia „selector” obecne w repo;
- curve readiness, IWIM, Alpha, Prosperity, Sybil, PDD, TAS, DOW i APS;
- DecisionLogger, replay i metric contracts w zakresie ich wpływu lub braku wpływu na decyzję;
- dispatch i logikę post-BUY w lane'ach shadow, live, probe i paper;
- elementy aktywne, wyłączone, konfigurowalnie promowalne, obserwacyjne i osierocone.

### 1.2 Świadome wyłączenie legacy scoring engine

Zgodnie z poleceniem nie analizuję jako części bieżącego Decision Plane:

- HyperPrediction,
- Hyper Oracle,
- starego `score_pool()` i pochodnych,
- legacy `StrategySelector` z dawnego standalone `ghost-brain` pipeline'u,
- `PoolScored` jako aktywnego źródła terminalnego werdyktu.

Granica jest istotna: `config.toml:71` ma `[ghost_brain] enabled = false`, lecz launcher niezależnie ładuje konfigurację Gatekeepera i buduje aktywną ścieżkę sesja → materializacja → Gatekeeper. Wyłączenie starego standalone brain nie wyłącza Gatekeeper V2. To, że launcher nadal konstruuje niektóre stare komponenty, nie czyni ich źródłem terminalnego werdyktu sesji.

### 1.3 Warstwy dowodu

W raporcie rozróżniam:

- **istnienie kodu** — typ/funkcja/moduł istnieje;
- **konfigurację** — bieżący TOML deklaruje enabled/disabled i progi;
- **okablowanie** — aktywny launcher tworzy komponent i wywołuje go w ścieżce runtime;
- **władzę decyzyjną** — wynik może zmienić terminalne `BUY/REJECT/TIMEOUT` albo działanie post-BUY;
- **emisję dowodu** — komponent tylko loguje/shadowuje/replayuje;
- **dowód runtime** — rzeczywista emisja w konkretnym uruchomieniu.

Ten dokument dowodzi pierwszych pięciu warstw przez statyczną inspekcję kodu, konfiguracji i testów. Nie badano bieżącego procesu ani nowych artefaktów runtime, więc raport nie twierdzi, że każda możliwa emisja faktycznie wystąpiła w konkretnym runie.

## 2. Słownik statusów i mapa odpowiedzialności

| Status | Znaczenie w tym audycie |
|---|---|
| `AUTHORITATIVE` | może zmienić bieżący terminalny werdykt lub wykonaną akcję |
| `WIRED` | aktywny launcher tworzy i wywołuje komponent |
| `SHADOW` | oblicza kontrfaktyczny wynik, ale nie steruje bieżącą akcją |
| `OBSERVATIONAL` | wyłącznie telemetry/replay/reporting |
| `CONFIG-LATENT` | istnieje kompletna ścieżka władzy, którą można uruchomić konfiguracją |
| `INERT FLAG` | pole konfiguracyjne istnieje, ale nie ma konsumenta nadającego deklarowaną władzę |
| `ORPHANED` | konfiguracja lub moduł nie jest używany przez aktywną ścieżkę |
| `LEGACY/EXCLUDED` | istnieje w repo, ale nie należy do analizowanej aktywnej architektury |

### 2.1 Mapa end-to-end

```text
Yellowstone / Seer
        │
        ▼
Event Bus ──► OracleRuntime ──► PoolObservationSession
                                      │
                                      ├─ AccountStateCore
                                      ├─ TxIntelligenceEngine
                                      │    ├─ core flow/timing/diversity/dev
                                      │    ├─ FlipV2
                                      │    └─ FingerprintAggregator
                                      ├─ GatekeeperBuffer
                                      ├─ CPV / FundingSourceIndex / FSC
                                      ├─ checkpoints / trajectory / segments
                                      └─ evidence availability/degradation
                                      │
                                      ▼
                         try_materialize_features()
                                      │
                                      ▼
                      MaterializedFeatureSet (SSOT snapshot)
                                      │
                                      ▼
                         Gatekeeper V2 pure policy
                                      │
                       ┌──────────────┴──────────────┐
                       │                             │
                    REJECT                        BUY candidate
                       │                             │
                       │                     curve readiness gate
                       │                             │
                       │                         IWIM veto
                       │                             │
                       └──────────────┬──────────────┘
                                      ▼
                     terminal log + V2.5/V3/selector evidence
                                      │
                               BUY only in current path
                                      ▼
                         Trigger shadow simulation
                                      │
                            successful shadow handoff
                                      ▼
                    PostBuyRuntime shadow monitoring/exit
```

V2.5 DOW checkpointy biegną równolegle podczas obserwacji. V3 jest liczone przy wzbogacaniu terminalnego logu. Żaden z tych dwóch wyników nie zastępuje dziś terminalnego V2.

### 2.2 Macierz wszystkich istotnych komponentów

| Komponent | Faza | Bieżący stan | Czy wpływa na decyzję/akcję? | Czy sama zmiana TOML może nadać wpływ? |
|---|---|---|---|---|
| Seer/Yellowstone + Event Bus | I | wired | pośrednio, dostarcza fakty | tak, przez tryb/źródło ingestu, ale nie jest polityką |
| `OracleRuntime` | I–III | authoritative orchestration | tak, routing, deadline, terminalizacja, dispatch | tak |
| `PoolObservationSession` | I | authoritative data boundary | tak, określa przyjęty stan i snapshot | tak |
| `AccountStateCore` | I | enabled/wired | tak, tworzy canonical account/curve state | tak |
| `TxIntelligenceEngine` | I | wired | tak, tworzy flow/timing/diversity/dev/sybil inputs | częściowo |
| `GatekeeperBuffer` | I/II | wired | tak jako reducer i evaluator; dodatkowo V2.5 shadow | tak |
| CPV / FundingSourceIndex / FSC/FSCv2 | I | wired z polityką missingness | obecnie głównie evidence; może wejść przez Sybil/Prosperity/selector | tak |
| checkpoints / trajectory / segments | I | wired | TAS/PDD/V3 i evidence | częściowo |
| `MaterializedFeatureSet` | granica I→II | authoritative SSOT | tak, kanoniczny input terminalnej polityki | nie jest opcją; kontrakt architektury |
| Gatekeeper V2 | II | enabled/authoritative | **tak: główny BUY/REJECT/TIMEOUT** | tak, progi i gate'y |
| curve readiness gate | II | authoritative | tak, po V2 BUY może czekać/odrzucić | tak |
| IWIM veto | II | enabled/wired/authoritative | tak, po V2 BUY może odrzucić | tak; obecnie P0 semantycznie wadliwy input |
| V2.5 DOW | I/II | shadow enabled | nie, emituje `ShadowV25Decision` | nie jako ogólny early BUY; flaga live nie promuje DOW |
| V2.5 TAS | II | enabled | zmienia `Strong` na `Borderline`, więc pośrednio zmienia zachowanie IWIM | tak |
| V2.5 PDD | II | enabled, shadow; live off | obecnie nie; może terminalnie odrzucać | tak, ale potrzebuje `live_execution_enabled` i per-detector promotion |
| V2.5 APS | II | enabled, adaptive off | diagnostyka/sugestie i kontekst PDD | pośrednio, dopiero razem z aktywnym promowanym PDD |
| Gatekeeper V3 | II/log | shadow/replay enabled | nie | **nie**; `promotion.enabled` jest inert |
| V2 `selector_soft_score` | II | disabled | nie | **tak** w `candidate_only` lub `buy_gate` |
| `selector_shadow_score_v1.jsonl` | II/log | observational | nie | nie w tej ścieżce |
| offline selector scripts/datasets | research | offline | nie | nie bez osobnej implementacji promocji |
| state-readiness selector latch | III/entry simulation | default disabled | nie jest selektorem tokena; może fail-close symulację | tak |
| Alpha gate | II | disabled | nie | tak |
| Prosperity filter | II | disabled | nie | tak |
| Sybil combo/soft policy | II | wyłączone/łagodne | obecnie praktycznie nie | tak |
| DecisionLogger / JSONL | I–III evidence | wired | nie tworzy werdyktu; utrwala go | nie |
| metric contracts PR2A/B/C | evidence/parity | opt-in/diagnostic | nie | nie |
| WAL / approved registry / commit coordinator | orchestration/recovery | wired zależnie od trybu | nie ocenia tokena; chroni dokładność wykonania | tak |
| Trigger | III entry | shadow-only | tak, wykonuje symulację zaakceptowanego intentu | tak, przez execution mode |
| PostBuyRuntime live lane | III | kod istnieje, bieżąco nieaktywny | przy live: tak | tak przez przełączenie trybu, wysokie ryzyko |
| PostBuyRuntime shadow lane | III | aktywny po udanym shadow handoff | tak dla shadow close | tak |
| Guardian LIGMA/WHF/TCF/PANIC | III | wired do sygnałów, niespójna egzekucja | obecnie nie determinuje prostego shadow close | nie w pełni |
| AEM | III | config enabled, launcher niewired | nie | nie; wymaga kodu `set_aem` |
| TimeStopV2 | III | disabled, observe-only | nie | po włączeniu nadal tylko obserwacja |
| shadow exit replay | III/research | default disabled | nie | tylko evidence |
| shadow V2 burn-in / dynamic-exit evidence | III/research | default disabled | nie | tylko validation/evidence |
| P37 probe | III/counterfactual | default disabled | nie | tylko izolowany probe |
| `[exit_strategy]` | III config | orphaned | nie | nie; brak konsumenta Rust |

## 3. Faza I — co kreuje dane potrzebne do decyzji

### 3.1 Czas i cykl życia sesji

Każda wykryta pula otrzymuje `PoolObservationSession`. Sesja jest właścicielem:

- identyfikacji kandydata i statusu lifecycle;
- czasu startu, deadline'u i finalizacji;
- deduplikacji oraz ograniczonego bufora transakcji;
- canonical account state;
- reduktorów Tx Intelligence, Gatekeeper i metryk Sybil;
- checkpointów czasowych i diagnostyki braków danych;
- końcowej materializacji.

Istotna semantyka czasu:

- `config.toml` deklaruje bazowe okno sesji 8000 ms;
- `ghost_brain_config.toml:15` deklaruje Gatekeeper `max_wait_ms = 11111`;
- `SessionManager` wybiera maksimum z czasu sesji i Gatekeepera (`ghost-launcher/src/session/manager.rs:128-136`);
- efektywny deadline bieżącego profilu wynosi więc **11111 ms**, nie 8000 ms;
- aliasy legacy Gatekeepera są synchronizowane z aktywną konfiguracją, więc nie tworzą drugiego terminalnego policy engine, ale pozostawiają nieczytelną wielość powierzchni konfiguracyjnych.

### 3.2 Przyjmowanie zdarzeń

`OracleRuntime` kieruje do właściwej sesji między innymi:

- `NewPoolDetected`,
- `PoolTransaction`,
- `AccountUpdate`,
- `FundingTransferObserved`,
- zdarzenia czasu/deadline'u i wykonania.

Semantyka slotu, czasu, tożsamości i deduplikacji zaczyna się w Seer/Yellowstone, ale ostateczne przyjęcie do konkretnych reduktorów następuje w sesji. W `PoolObservationSession::on_transaction` Tx Intelligence jest aktualizowane przed Gatekeeperem; CPV i bounded event buffer otrzymują zdarzenie dopiero, gdy Gatekeeper uzna je za unikalne (`ghost-launcher/src/session/observation.rs:342-368`).

### 3.3 Producenci danych

#### AccountStateCore

Aktywny (`config.toml:231+`) i odpowiedzialny za stan wynikający z kont puli:

- virtual token/SOL reserves;
- price i market cap;
- bonding progress;
- state velocity;
- phase/finality/bootstrap oraz licznik aktualizacji;
- gotowość i świeżość curve state.

Jest preferowanym źródłem canonical state. Fallback i degraded state muszą być jawnie opisane w evidence, a nie ukryte przez konkurencyjne przeliczenie w policy.

#### TxIntelligenceEngine

Tworzy między innymi (`ghost-core/src/tx_intelligence/types.rs:49-102`):

- tx/buy/sell/failed counts;
- unique signers;
- buy ratio i SOL buy ratio;
- total/average volume;
- average interval, timing entropy, burst ratio;
- HHI, Gini, top-3 concentration, same-ms/bundle metrics;
- max price/sell impact;
- developer activity i udziały;
- dane potrzebne do Sybil/organic/manipulation features.

W jego obrębie działają osobne reduktory, między innymi FlipV2 i `FingerprintAggregator`.

#### GatekeeperBuffer

Utrzymuje własny obraz obserwowanego flow potrzebny do faz Gatekeepera, DOW checkpoints, PDD i trajectory. Jest historycznie większym komponentem niż sama czysta polityka. W terminalnej ścieżce powinien jednak dostarczyć assessment utworzony z `MaterializedFeatureSet`, nie stać się drugim SSOT.

#### CPV, FundingSourceIndex i FSC/FSCv2

Tworzą metryki cross-pool oraz pochodzenia finansowania:

- cross-pool velocity/activity;
- funding-transfer/deployer/buyer identity affinity;
- common funder concentration;
- confidence, coverage window i powody niedostępności;
- warianty FSC decision-time i eventual-postfill.

Te dane są dziś często bardziej materiałem diagnostycznym niż twardym kryterium, ponieważ odpowiednie gate'y są wyłączone albo mają duży limit. Mogą jednak stać się terminalne przez konfigurację Sybil, Prosperity lub selector soft-score.

#### Checkpoints, trajectory i segment sequence

Sesja utrwala próbki T0/T1/T2 oraz sekwencję segmentów. Służą do:

- TAS;
- PDD spike/ramping/flash-crash;
- DOW early/normal/extended;
- temporal deltas i decision-time series;
- V3 organic broadening oraz manipulation contradictions;
- replay i analiz counterfactual.

#### Alpha fingerprint i pozostałe pochodne

Snapshot zawiera również fingerprint momentum/demand/joint, organic broadening, manipulation contradictions, RCE pre-entry summary oraz metric-contract projection. RCE jest jawnie logging-only i nie może być traktowany jako ukryte kryterium terminalne.

### 3.4 `MaterializedFeatureSet` — kanoniczna granica I → II

`MaterializedFeatureSet` (`ghost-core/src/checkpoint/types.rs:897-950`) grupuje:

| Grupa | Przykładowa zawartość | Główne źródło |
|---|---|---|
| account state | reserves, price, market cap, bonding, velocity, finality | AccountStateCore |
| tx intelligence | counts, signerzy, buy ratio, volume, timing, HHI/Gini/top3/same-ms, dev | TxIntelligence |
| checkpoints/trajectory | T0/T1/T2, rozwój flow i ceny | checkpoint builders |
| risk/session | deadline, stage, completeness, flags | sesja |
| curve readiness | ready/fresh/finalized/wait elapsed | AccountStateCore + session latch |
| Sybil resistance | FTDI, DBIA, SFD, DES, CPV, FSC/FSCv2 i degradation | Tx Intelligence + funding/cross-pool |
| alpha fingerprint | momentum/demand/joint i evidence | fingerprint aggregator |
| temporal | segment sequence, deltas, decision-time series | session/checkpoints |
| V3 evidence | organic broadening, manipulation contradictions | feature builder |
| diagnostics | RCE logging-only, metric contracts | dedicated builders |

Produkcja używa `PoolObservationSession::try_materialize_features()` (`ghost-launcher/src/session/observation.rs:2711+`). Funkcja jest fail-closed i zwraca błąd, gdy kanoniczny snapshot nie może zostać zbudowany. W bieżącym runtime błąd terminalnej materializacji prowadzi do panic (`ghost-launcher/src/oracle_runtime.rs:17162-17170`), co chroni przed decyzją na częściowym ukrytym stanie, ale jest kosztowną polityką dostępności.

Snapshot jest niezmiennym SSOT dla terminalnej oceny. Nie oznacza to jeszcze, że każdy producer ma identyczny event universe: `try_materialize_features()` uzupełnia część pól z GatekeeperBuffer po podstawowym `FeatureBuilder` (`observation.rs:2711-2994`). To jest spójne na poziomie końcowego obiektu, lecz wymaga ścisłych kontraktów denominatorów i parity.

### 3.5 Evidence availability jest częścią danych, nie komentarzem

Feature groups mają statusy takie jak available, insufficient sample, degraded lub unavailable wraz z reason codes. Jest to prawidłowy kierunek: brak danych nie może wyglądać jak bezpieczne zero. Polityka musi jawnie definiować dla każdej metryki:

- `fail closed`,
- `fail open`,
- `degrade`,
- `not applicable`,
- `logging only`.

### 3.6 Znalezione problemy wejścia i denominatorów

#### F-I-1 — brak jednej kanonicznej bramki event admission

`TxIntelligenceEngine::on_transaction` wywołuje FlipV2 i fingerprint przed własnymi końcowymi kontrolami duplicate/dust (`ghost-launcher/src/tx_intelligence/engine.rs:239-257`). FlipV2 ma własną kontrolę success, dust, stable identity/order i dedup. `FingerprintAggregator::ingest` nie ma równoważnego dedupu ani filtra success, a adapter `pool_tx_to_fingerprint_event` odrzuca głównie synthetic/missing-slot (`engine.rs:901-908`, `1128-1200`; `ghost-seer/src/early_fingerprint.rs:267+`).

Skutek: fingerprint może widzieć failed, duplicate albo poniżej `min_sol_threshold` zdarzenia, których nie widzą core TxIntel, Gatekeeper, CPV i bounded buffer. To podważa porównywalność Alpha/V3/selector features.

#### F-I-2 — bounded buffer tworzy inny horyzont niż reduktory strumieniowe

Bieżący `max_tx_events_per_session` wynosi 128. Bounded buffer zasila między innymi Sybil, segmenty, organic broadening i V3, podczas gdy reduktory strumieniowe mogą agregować szerszy zakres. Przy bardzo aktywnej puli najstarsze zdarzenia są obcinane. Evidence oznacza truncation, ale polityki nadal mogą porównywać metryki o innych denominatorach.

#### F-I-3 — efektywny deadline nie jest oczywisty z jednego pliku

8000 ms w configu sesji i 11111 ms w Gatekeeperze są runtime'owo rozstrzygane przez `max`, więc nie jest to dziś sprzeczność zachowania. Jest to jednak defekt ergonomii i audytowalności: operator nie widzi z jednej powierzchni rzeczywistego okna decyzji.

#### F-I-4 — fail-closed materialization kończy proces przez panic

To lepsze niż ukryte fail-open, ale nie rozróżnia awarii jednego kandydata od korupcji całego runtime'u. Docelowo potrzebny jest typed terminal `REJECT_DATA_INTEGRITY`/quarantine plus globalny escalation policy zależny od klasy błędu.

## 4. Faza II — na podstawie jakich kryteriów podejmowana jest decyzja

## 4.1 Gatekeeper V2 — bieżący terminalny policy engine

### 4.1.1 Charakter działania

Gatekeeper V2 nie jest modelem predykcyjnym. To deterministyczna, konfigurowalna polityka progowo-regułowa:

1. bierze niezmienny `MaterializedFeatureSet`;
2. tworzy `GatekeeperAssessment` z profilami fazowymi;
3. oblicza hard fails, core gates, soft signals i dodatkowe polityki;
4. emituje typed verdict, typed reason code i reason chain;
5. dla `BUY` wyznacza `GatekeeperStrength::{Strong, Borderline}`;
6. osobno stosuje curve gate;
7. dopiero potem przekazuje kandydata do IWIM.

Aktualny `mode = "long"` (`ghost_brain_config.toml:19`) oznacza akumulację do pełnego deadline'u i jedną terminalną ewaluację. Nie oznacza to braku checkpointów diagnostycznych V2.5 — te nadal mogą być liczone w tle.

Terminalna ścieżka opiera się na `GatekeeperBuffer::evaluate_from_features()` (`ghost-launcher/src/components/gatekeeper.rs:5418+`) i czystej `evaluate_policy_from_assessment()` (`gatekeeper_policy.rs:2331-2662`).

### 4.1.2 Sześć profili analitycznych

| Faza | Co sprawdza | Bieżące kluczowe progi |
|---|---|---|
| Phase 1 — Quantity | liczba tx, unique signers, liczba buy | `tx >= 55`, `signers >= 41`, `buy >= 39` |
| Phase 2 — Velocity | interval CV, burst, średni interwał, timing entropy, dust sample | zasadniczo permissive poza `avg_interval <= 222 ms` |
| Phase 3 — Diversity | unique ratio, HHI, tx/signer, Gini, top3, same-ms | większość capów `0.99`/bardzo wysoka; `volume_gini >= 0.01` |
| Phase 4 — Volume/Fingerprint | buy/SOL ratio, avg tx SOL, volume CV/total, consecutive buys i fingerprint caps | `buy_ratio >= .50`, `avg_tx >= .01 SOL`, `volume_cv >= .01`, `volume >= 70 SOL`, `consecutive buys >= 2` |
| Phase 5 — Dev | dev buy, tx/volume share, sell behavior | dev buy `<=10 SOL`, tx ratio `<=.99`, volume ratio `<=.20`, `reject_on_dev_sell=false` |
| Phase 6 — Curve | price change/impact, bonding progress, market cap | price ratio `.01..9.50`, bonding `30..70%`, market cap `>=115 SOL`; impact caps praktycznie otwarte |

Komentarz w nagłówku configu mówi o `mcap=48`, podczas gdy aktywna wartość to 115. To dokumentacyjny drift i kolejny argument za generowanym effective-config manifestem.

### 4.1.3 Ważna pułapka: „core” nie odpowiada fazom 1–3

W kodzie (`gatekeeper_policy.rs:3472-3506`, `3522-3564`) mapowanie jest następujące:

- `core1 = phase1_passed`;
- `core2 = phase4_passed`;
- `core3 = phase5_passed && phase6_passed`, jeśli dev jest znany;
- gdy dev jest nieznany, `core3` wymaga `phase4_passed` oraz własnych warunków curve/price/bonding.

Phase 2 i Phase 3 **nie są core2/core3**. Ich wyniki wchodzą przez hard filters i soft signals. Nazewnictwo może prowadzić do błędnej interpretacji logów.

### 4.1.4 Dokładna kolejność terminalnej polityki

`evaluate_policy_from_assessment()` stosuje kolejno:

1. **hard filters**;
2. **PDD live hard fail**, ale tylko jeśli PDD enabled, V2.5 live enabled i dany detector promoted;
3. **core1/core2/core3**;
4. **Sybil combo veto**;
5. **Sybil soft-point maximum**;
6. **ogólny soft-point maximum**;
7. **selector soft-score terminal gate**;
8. **Alpha gate**;
9. **Prosperity filter**;
10. `BUY`;
11. wyznaczenie `Strong/Borderline`, w tym demotion przez TAS;
12. po wyjściu z czystej polityki: **curve readiness gate**;
13. w `OracleRuntime`: **IWIM veto**.

Ta kolejność ma znaczenie. Przykładowo Alpha nie może uratować core fail, selector jest pozytywną bramką dopiero po wszystkich soft limits, a IWIM nie widzi kandydatów odrzuconych wcześniej.

### 4.1.5 Hard filters

Kod `gatekeeper_policy.rs:2158-2316` może terminalnie odrzucić za:

- sprzedaż deva, jeśli `reject_on_dev_sell=true`;
- maksymalny pojedynczy sell impact;
- maksymalny pojedynczy tx price impact;
- maksymalny price-change ratio;
- zbyt niski market cap;
- ekstremalny HHI;
- ekstremalny same-ms/bundling ratio;
- ekstremalną dominację top-3;
- ekstremalne timing bot behavior;
- zbyt wolną pulę;
- failed-tx ratio, jeśli opcjonalny próg jest ustawiony;
- strict metric threshold failure, jeśli strict gate jest faktycznie enabled.

W bieżącym profilu:

- HHI, same-ms i top3 hard caps wynoszą `.99`;
- bot guard jest faktycznie zamrożony przez `hard_fail_bot_min_tx=999999`;
- `SlowPool` przy `avg_interval > 222 ms` jest realnym, ostrym hard fail;
- strict missing policy ma wartość `hard_fail`, ale sama bramka `strict_metric_threshold_gate_enabled` nie jest skonfigurowana i pozostaje domyślnie wyłączona. Sam napis `hard_fail` nie czyni strict gate aktywnym.

### 4.1.6 Soft signals i 3-layer scoring

Soft signals opisują między innymi timing, manipulation, diversity i ecosystem. W bieżącym profilu `max_soft_points=255`, `max_soft_score=255`, a Sybil penalties wynoszą zero. Praktyczny efekt jest diagnostyczny: soft layer niemal nie odrzuca.

Nie należy mylić tego z selector soft-score. Są to dwa niezależne mechanizmy punktowe o innych skalach i semantyce.

### 4.1.7 Sybil Interference

Warstwa posiada konkretne metryki i wzorce:

- FTDI — fee topology diversity;
- DBIA — dev-buyer infrastructure affinity;
- SFD — spend fraction divergence;
- DES — demand elasticity;
- CPV — signer cross-pool velocity/activity;
- FSC/FSCv2 — funding source concentration i quality/coverage;
- kombinacje strukturalne, np. high DBIA + low FTDI + low SFD albo high FSC + high CPV + low DES/SFD.

Kod ma dwie możliwe władze:

- combo veto (`RejectSybilInterference`);
- soft excess (`RejectSybilSoftExcess`).

Bieżąco `enable_sybil_interference_layer=false`, `enable_sybil_combo_veto=false`, wszystkie penalties są 0, a maksima 255. Metryki są nadal cennym evidence, ale nie tworzą decyzji.

### 4.1.8 Alpha gate

To pozytywny filtr oparty o fingerprint:

- momentum;
- demand;
- joint signal;
- minimalny sample.

Po włączeniu może emitować `RejectLowAlpha`. Bieżąco `enable_alpha_gate=false`, progi `.55/.55/.35`, sample 15, więc nie ma władzy.

### 4.1.9 Prosperity filter

To późna pozytywna bramka, która porównuje kandydata z kilkoma historycznie zdefiniowanymi branchami, używając między innymi:

- market cap floor;
- CPV;
- block0 sniped supply i sell/buy ratio;
- early-slot buy dominance;
- HHI i FTDI.

Po włączeniu może emitować `RejectLowProsperity`. Bieżąco `enable_prosperity_filter=false`, a overlay też jest wyłączony.

### 4.1.10 Typed verdicts i timeout

V2 emituje między innymi:

- `Buy`;
- `RejectHardFail` z konkretnym hard-fail reason code;
- `RejectCoreFail`;
- `RejectSybilInterference` / `RejectSybilSoftExcess`;
- `RejectSoftExcess`;
- `RejectSelectorNotCandidate` / `RejectSelectorBelowBuy`;
- `RejectLowAlpha`;
- `RejectLowProsperity`;
- promowane PDD verdicts;
- typed timeout/deadline results.

Jest to prawidłowy kontrakt Decision Plane: decyzja nie powinna kończyć się niewyjaśnionym, generycznym `REJECT`.

## 4.2 Curve readiness gate — osobna bramka po V2 BUY

Po tym, gdy czysta polityka V2 zwróci `BUY`, `GatekeeperBuffer::evaluate_from_features()` stosuje `evaluate_curve_gate` (`gatekeeper.rs:5418-5514`; `gatekeeper_policy.rs:2770+`). Gate bierze pod uwagę:

- czy curve state jest wymagany;
- czy jest ready;
- freshness/staleness;
- finality i policy fallback;
- ile czasu już oczekiwano.

Domyślny/current wait to 800 ms, `require_curve_state=true`, a stale fallback prowadzi do oczekiwania/odrzucenia, nie do ukrytego zaakceptowania. To nie jest Phase 6: Phase 6 ocenia wartości ekonomiczne curve, a curve readiness gate ocenia wiarygodność/gotowość źródła tych wartości.

## 4.3 IWIM — aktywna bramka po V2 BUY

### 4.3.1 Zamierzona odpowiedzialność

IWIM (`ghost-brain/src/oracle/ultrafast/iwim.rs`) ma analizować wczesną aktywność deva i rozpoznawać wzorce takie jak:

- authority/manipulation behavior;
- create-account/token-account/transfer patterns;
- bursty activity;
- IAPP/sweep i inne rug/sybil indicators;
- organic activity i confidence/quality.

`ghost_brain_config.toml:559+` ma:

- `enabled=true`;
- `mode="pp"`;
- budget 500 ms;
- min confidence `.75`;
- rug/sybil reject threshold `.75`;
- organic minimum `.15`.

Runtime uruchamia IWIM dopiero dla V2 `BUY` (`ghost-launcher/src/oracle_runtime.rs:24254-24470`).

### 4.3.2 Macierz działania

V2 przekazuje `GatekeeperStrength`:

- dla `Strong` brak/timeout/low-quality IWIM może pozwolić kontynuować;
- dla `Borderline` brak/timeout/low-quality jest traktowany restrykcyjniej i może odrzucić;
- dla danych wysokiej jakości obowiązują rug/sybil/organic thresholds;
- typed outcomes obejmują między innymi IWIM veto, low confidence i unknown strict.

TAS może więc pośrednio zmienić terminalny wynik: sam nie odrzuca V2 BUY, ale demotion `Strong → Borderline` zmienia politykę IWIM.

### 4.3.3 F-II-1 P0 — IWIM nie otrzymuje danych, których semantycznie wymaga

`fetch_dev_signatures()` wywołuje `getSignaturesForAddress`, lecz każdy wynik zamienia na bajty tekstu w rodzaju `JSON_TX_META_TIMESTAMP_<block_time>` (`ghost-launcher/src/components/iwim_veto.rs:603-635`).

Tymczasem `IwimInput.transactions` jest kontraktem surowych bajtów transakcji (`iwim.rs:315-342`), a `parse_tx_metadata()` i classifier szukają w tych bajtach typów instrukcji (`iwim.rs:420-463`, `1002-1069`). Timestamp placeholder dostarcza czas, ale nie payload instrukcji. Wynik:

- transaction types stają się `Unknown`;
- rug/sybil pattern detection nie ma materiału do wykrywania deklarowanych wzorców;
- confidence CTP/CMM może mimo to rosnąć z liczby rekordów i obecności prawdziwych timestampów (`iwim.rs:630-649`, `748-766`);
- scoring organic premiuje brak wykrytych suspicious types, więc brak danych może wyglądać jak brak zagrożenia (`iwim.rs:853-995`);
- warstwa veto może sklasyfikować input jako wysokiej jakości przy wystarczającej liczbie rekordów i confidence, mimo że semantycznie nie ma instrukcji.

To nie jest drobny brak observability. IWIM jest aktywnym elementem terminalnej kontroli i może kreować zarówno fałszywe przepuszczenia, jak i wyniki zależne od `Strong/Borderline` na podstawie pozornej jakości.

**Wymagane działanie:** do czasu pobierania/dekodowania realnych versioned transactions wraz z meta i jawnego `content_coverage`, ustawić IWIM jako log-only albo `enabled=false`. Nie wystarczy obniżyć/zmienić progi.

## 4.4 Gatekeeper V2.5 — temporalny shadow/control overlay

V2.5 nie jest pełnym następcą V2 w terminalnym runtime. Jest zbiorem mechanizmów nakładanych na V2:

- DOW — Decision Opportunity Windows;
- TAS — Trajectory Aware Scoring;
- PDD — Pump & Dump Detector;
- APS — Adaptive Prosperity System;
- shadow checkpoints i diagnostyka promotion readiness.

Bieżący config (`ghost_brain_config.toml:274+`) ma `shadow_enabled=true`, `live_execution_enabled=false`.

### 4.4.1 DOW

DOW ocenia okna:

- early: 2–5 s;
- normal: około 5–7 s;
- extended: około 7–10 s;
- tick: 250 ms.

Emituje `ShadowV25Decision`; wynik nie jest podłączony do terminalnego dispatchu. Sama zmiana `v25.live_execution_enabled=true` nie zmienia DOW w early live BUY — ta flaga jest używana w terminalnej polityce głównie do PDD i guardów shadow/adaptive.

Istotna niespójność: early DOW ma `early_entry_min_tx_count=5`, lecz po wstępnym min-data guard ścieżka nadal stosuje pełny Phase 1 z `55 tx / 41 signerów / 39 buy` (`gatekeeper.rs:7560-7595`). Early candidate musi więc już spełnić terminalne minima ilościowe. W praktyce „early” opisuje moment czasu, a nie lekką wczesną wersję kryteriów.

Early wymaga V2 BUY, phase/quality/confidence, Sybil/drift/momentum conditions; normal opiera się na V2 BUY i confidence; extended dodaje canonical confidence i czysty PDD (`gatekeeper.rs:7758-7975`).

### 4.4.2 F-II-2 — DOW ma równoległą ścieżkę ewaluacji

Checkpointy DOW używają `run_assessment_at()` i mutable `GatekeeperBuffer::compute_decision()` (`gatekeeper.rs:7201+`, `6363+`, `7504-7604`), podczas gdy terminal V2 używa niezmiennego MFS i czystej policy. To tworzy parity risk:

- inny moment zamrożenia danych;
- możliwe inne denominatory;
- możliwość driftu logiki między dużą metodą bufferową a pure policy.

Docelowo każdy checkpoint powinien materializować checkpoint-MFS i używać dokładnie tej samej czystej funkcji policy co terminal.

### 4.4.3 TAS

TAS analizuje trajectory T0/T1/T2 i buduje score rozwoju puli. W V2.5 shadow wpływa na confidence i może powodować kontrfaktyczny low-trajectory reject. W terminalnej pure V2 policy nie odrzuca bezpośrednio; jeśli score `<0.45`, może obniżyć `GatekeeperStrength` z `Strong` do `Borderline` (`gatekeeper_policy.rs:2574-2613`). Przez macierz IWIM jest to realny, pośredni wpływ na terminalny wynik.

### 4.4.4 PDD

PDD ocenia:

- entry drift;
- spike;
- ramping;
- whale concentration;
- reserve health;
- flash crash.

W shadow może oznaczyć veto. W terminalnej polityce PDD odrzuci tylko, gdy jednocześnie:

1. `pdd.enabled=true`;
2. `v25.live_execution_enabled=true`;
3. konkretny detector ma `*_promoted_to_live=true`.

Kod tej dwupoziomowej promocji znajduje się w `gatekeeper_policy.rs:2366-2429`. Bieżąco live jest false, a promocje detectorów są false. PDD nie zmienia terminalnego werdyktu.

### 4.4.5 APS

APS wykrywa regime i proponuje regime-specific thresholds. Bieżąco:

- APS jest enabled;
- `adaptive_enabled=false`;
- shadow suggestions są enabled;
- lokalny heuristic jest dostępny;
- cross-pool outcome tracker nie stanowi pełnego aktywnego źródła kalibracji.

APS tworzy diagnostykę i może w HighVol zmodyfikować kontrfaktyczny PDD threshold, lecz terminalny skutek nadal wymaga promowanego PDD. Nie jest samodzielnym policy gate.

## 4.5 Gatekeeper V3 — dojrzały shadow policy, nie authority

### 4.5.1 Charakter

V3 (`ghost-launcher/src/components/gatekeeper_v3.rs`) jest nową, evidence-first polityką:

1. klasyfikuje evidence status i actionability;
2. ocenia hard manipulation contradictions/risk;
3. ocenia opportunity/organic broadening;
4. oblicza confidence i ograniczenia jakości;
5. emituje typed `BUY_CANDIDATE`, `REJECT`, `PENDING` albo `TIMEOUT` z reasons;
6. zapisuje replay payload, snapshot hash i config hash.

Używa stage profiles early/normal/extended oraz m.in. tx/signers/buy counts, organic ratios/growth, HHI i risk penalties. Confidence jest capowany, gdy execution nie został wykonany, co prawidłowo odróżnia decyzję od realizacji.

### 4.5.2 Faktyczne okablowanie

Config (`ghost_brain_config.toml:384+`) ma:

- `enabled=true`;
- `shadow_emit_enabled=true`;
- `replay_payload_enabled=true`;
- `promotion.enabled=false`.

V3 jest wywoływany przez `enrich_buy_log_with_v3_shadow()` (`oracle_runtime.rs:5599-5710`) po ustaleniu aktywnego V2/IWIM rezultatu. Funkcja wzbogaca log dla BUY, REJECT, TIMEOUT i IWIM, ale nie zmienia aktywnych pól werdyktu.

### 4.5.3 F-II-3 — `promotion.enabled` jest inert flag

W terminalnym routingu nie ma konsumenta, który po `promotion.enabled=true` zastąpiłby V2 wynikiem V3. Pole występuje w konfiguracji/testach/payloadach, nie w ścieżce dispatchu. V3 nie jest więc „wyłączonym live Gatekeeperem”, który można aktywować jednym wpisem TOML. Promocja wymaga osobnego, jawnego bridge'a z kontraktem rollback/parity/evidence.

## 4.6 „Selector” — pięć różnych pojęć, tylko jedno konfigurowalnie terminalne

### 4.6.1 Selector A — `gatekeeper_v2.selector_soft_score`

To osadzony w V2 pozytywny score o maksymalnie 12 regułach (`gatekeeper_policy.rs:941-1138`; config `237-268`). Punkty otrzymuje za:

1. niskie Jito tip intensity;
2. wysoki unique ratio;
3. niską CPV other-pool activity;
4. niski max single sell impact;
5. niską signer cross-pool velocity;
6. niski HHI;
7. niski average CPI depth;
8. niską top3 concentration;
9. niski delta Jito 1s→2s;
10. `same_ms_tx_ratio >= threshold`;
11. `interval_cv >= threshold`;
12. niski delta Jito 1s→3s.

Bieżące wszystkie wagi wynoszą 1, `min_candidate_score=2`, `min_buy_score=3`, missing=`no_point`, ale `enabled=false` i `policy="log_only"`.

Tryby:

- `log_only` — brak terminalnego wpływu;
- `candidate_only` — odrzucenie poniżej candidate threshold;
- `buy_gate` — odrzucenie poniżej candidate lub buy threshold.

To jedyny selector, któremu sama poprawna zmiana configu może obecnie nadać władzę odrzucania. Nie tworzy alternatywnego BUY; tylko blokuje BUY wypracowany wcześniej przez V2.

Kierunek `same_ms_tx_ratio >= .049` jako punkt pozytywny jest kontrintuicyjny względem hard-fail bundlingu i musi zostać empirycznie potwierdzony przed promocją. Nie wolno go „poprawiać intuicyjnie” bez walidacji — kod jawnie implementuje właśnie ten kierunek.

### 4.6.2 Selector B — `selector_shadow_score_v1.jsonl`

DecisionLogger oblicza osobny znormalizowany score `0..1` i zapisuje sidecar z validity, progami/top-tail i feature diagnostics (`ghost-brain/src/oracle/decision_logger.rs:105-155`, `3230-3318`, `4243-4410`, `4659-4688`).

Jest to **diagnostic-only research surface**:

- powstaje po terminalnym logu;
- nie jest inputem Gatekeepera;
- nie steruje Triggerem;
- nie jest tym samym co 0..12 `selector_soft_score`.

### 4.6.3 Selector C — pipeline offline

Skrypty i datasety selectorowe w `scripts/`, `reports/selector/` i innych artefaktach służą do budowy zbiorów, porównań, lifecycle labeling i badań edge. Nie są ładowane przez aktywny runtime jako policy model. Promocja wyniku offline wymaga jawnej specyfikacji feature contract, anti-leakage, temporal validation i implementacji runtime.

### 4.6.4 Selector D — state-readiness latch

`[selector.simcov.state_readiness_latch]` w launcher config (`ghost-launcher/src/config.rs:1057-1099`, `2144-2211`) nie wybiera tokenów. To bramka gotowości stanu dla symulacji entry, z fail-closed semantics i bez RPC fallback. Sekcja jest nieobecna w bieżącym configu, więc używa domyślnego disabled. Nazwa nie powinna być mieszana z token selectorami.

### 4.6.5 Selector E — legacy `StrategySelector`

To element starego standalone `ghost-brain` pipeline'u/strategy sizing. Nie jest aktywnym selektorem terminalnej sesji i zgodnie z zakresem jest legacy/excluded.

## 4.7 DecisionLogger, replay i metric contracts — dowód, nie policy

DecisionLogger zapisuje między innymi:

- terminalny `GatekeeperBuyLog`;
- `MaterializedFeatureSet` i phase/pass diagnostics;
- V2.5 shadow decisions;
- V3 shadow/replay payload;
- IWIM outcome;
- curve/window evidence;
- decision-time series i vectors;
- selector sidecar;
- coordination/metric-contract projections.

To kluczowy składnik audytowalności i replay, ale nie może być opisany jako kryterium zakupu. Logger rejestruje rezultat.

Znaleziony dług nazewniczy:

- stała schematu/loggera wskazuje wersję V2.5, a aktywna terminalna semantyka bywa nazywana V2.2;
- bieżące rekordy terminalnej ścieżki mają `decision_plane="legacy_live"` (`oracle_runtime.rs:5583`), mimo że są rezultatem aktywnego MFS/V2 flow.

To nie zmienia decyzji, ale utrudnia prawidłowe łączenie i interpretację datasetów.

Metric contracts PR2A/B/C sprawdzają projekcję, hash/parity i durability/comparator evidence. Nie są dodatkowym Gatekeeperem. PR2C pair logging jest opt-in i nie może być przedstawiane jako aktywna polityka.

Przy `ENOSPC` runtime może wyłączyć dalsze zapisy WAL/logów i kontynuować decyzje (`oracle_runtime.rs:3001+`, DecisionLogger analogicznie chroni runtime). Jest to rozsądne dla dostępności shadow, ale niedopuszczalne jako cichy stan przyszłego live: decyzja może nadal powstać, gdy trwały dowód przestał powstawać.

## 4.8 Co aktualnie naprawdę kreuje BUY lub REJECT

### Aktywne teraz

1. kompletność/materializowalność danych sesji;
2. Gatekeeper V2 hard filters;
3. V2 core mapping: Phase 1, Phase 4, Phase 5+6/dev-unknown branch;
4. praktycznie neutralne dziś soft limits;
5. curve readiness/freshness gate;
6. TAS przez zmianę `Strong/Borderline`;
7. IWIM przez następcze veto/strict behavior — mimo opisanej wady inputu;
8. deadline/timeout i typed runtime failures.

### Mogą zacząć wpływać po poprawnej zmianie configu

1. strict metric threshold gate;
2. bardziej restrykcyjne hard/soft thresholds;
3. Sybil interference i combo veto;
4. selector soft-score w `candidate_only`/`buy_gate`;
5. Alpha gate;
6. Prosperity filter/overlay;
7. failed-tx bot threshold;
8. PDD — tylko przy V2.5 live oraz per-detector promotion;
9. curve missing/stale policies;
10. IWIM mode/threshold/missing policy.

### Nie mogą zacząć wpływać samą zmianą configu

1. V3 promotion;
2. ogólny DOW early/live BUY;
3. `selector_shadow_score_v1`;
4. offline selector models;
5. RCE logging-only;
6. metric contracts;
7. AEM w aktywnym launcherze;
8. `[exit_strategy]`.

## 5. Faza III — co dzieje się po decyzji BUY

## 5.1 Najpierw ważne rozróżnienie: bieżący BUY nie jest zakupem

Aktywna konfiguracja ma:

- `config.toml:84`: `entry_mode="shadow_only"`;
- `config.toml:124-125`: `execution_mode="shadow"`;
- Trigger shadow simulation enabled.

Dlatego terminalny `BUY` oznacza obecnie:

1. Gatekeeper/IWIM zgodził się na intent;
2. Trigger przygotował transakcję/quote zgodnie z lane'em;
3. uruchomiono RPC simulation/shadow transport;
4. tylko udany shadow result może utworzyć post-buy handoff;
5. PostBuyRuntime otwiera syntetyczną pozycję shadow.

`TriggerBuyOutcome::ShadowSimulated` wysyła post-buy handoff tylko, gdy `shadow_event.err.is_none()` i lane jest shadow/paper. W shadow lane oczekuje bezpośredniego ACK (`oracle_runtime.rs:22519-22797`). Outcome pozostaje `bought:false`; close reason na poziomie pierwotnego pool lifecycle może być `PoolShadowedEarly`. Shadow position ma osobny lifecycle. Nie wolno z tego wnioskować submitu, inclusion ani confirmation on-chain.

## 5.2 PostBuyRuntime i lane'y

`PostBuyRuntime` jest zawsze tworzony przez launcher (`ghost-launcher/src/main.rs:2140-2215`), ale jego zachowanie zależy od execution mode i źródła handoffu.

| Lane | Bieżący stan | Odpowiedzialność |
|---|---|---|
| shadow | **aktywny** | canonical snapshot monitoring, synthetic position, proste TP/SL/inactivity close, lifecycle evidence |
| live | kod kompletny, **nieaktywny w bieżącym profilu** | confirmed BUY persistence, canonical/RPC price, pełny SELL, retry, confirmation, reconciliation |
| probe | default disabled | izolowany P37 counterfactual post-buy monitor |
| paper | compatibility, bieżąco nieaktywny | starszy `PaperPositionLifecycle` |

### 5.2.1 Shadow lane — rzeczywista bieżąca polityka exit

Launcher pobiera z brain config:

- `target_threshold=50.0`;
- `stoploss_threshold=50.0`;
- `wait_for_timestop=30000 ms`;
- TimeStopV2 config;
- exit replay config.

Następnie zawsze wywołuje `set_shadow_simple_exit_thresholds()` (`post_buy_runtime.rs:2251-2300`). `MonitoringEngine::run_shadow_runtime_tick()` widzi ustawione progi, uruchamia `run_shadow_simple_threshold_tick()` i natychmiast wraca (`engine.rs:4660-4670`).

Efektywna polityka shadow close jest zatem:

- pełne zamknięcie pozostałej pozycji przy **+50%**;
- pełne zamknięcie przy **-50%**;
- pełne zamknięcie po **30 s braku market activity**, nie po bezwzględnych 30 s wieku pozycji;
- brak synthetic fallback dla ceny wejścia;
- canonical/fail-closed price truth;
- przy time-stop i braku poprawnego truth możliwe jawne wymuszone zamknięcie bez exit truth, z odpowiednim evidence;
- lifecycle i reason code są emitowane.

Kod decyzji/proof znajduje się w `engine.rs:4983-5280`.

### 5.2.2 Guardian: LIGMA, WHF, TCF i PANIC

Pętla `MonitoringEngine::tick()` uruchamia:

- LIGMA — liquidity/tradability/impact;
- WHF — wash/manipulation/flow decay;
- TCF — trend cohesion/regime change;
- PANIC — impulse/congestion/coordinated activity;
- signal cleanup;
- AEM tick;
- shadow runtime tick.

Sygnały trafiają do `SignalRouter` i mogą wykonywać akcje typu `TightenStop` lub `PanicSell` na `ShadowPositionBook`.

Problem: prosta ścieżka threshold exit wraca przed `ShadowPositionBook.process_market_snapshot` i nie wykorzystuje tych mutacji do ustalenia bieżącego close. Moduły są więc uruchamiane i emitują sygnały/action labels, ale w aktualnym shadow close są **operacyjnie nieskuteczne**. To więcej niż „wyłączone”, lecz mniej niż działająca logika biznesowa: są wired-to-observation, nie wired-to-final-action.

### 5.2.3 F-III-1 — pełna konfiguracja Guardiana nie dociera do aktywnego launchera

`build_shadow_guardian_config()` zaczyna od `PostBuyGuardianConfig::default()` i nadpisuje tylko:

- tick interval;
- max positions;
- AEM `t_s`;
- target/stoploss/time-stop;
- TimeStopV2;
- exit replay.

Kod: `ghost-launcher/src/components/post_buy_runtime.rs:806-820`; konstrukcja configu launchera: `main.rs:2167-2204`.

Nie są przenoszone z TOML między innymi:

- `post_buy_guardian.enabled`;
- LIGMA thresholds;
- WHF thresholds;
- TCF thresholds;
- PANIC thresholds;
- aggregation/escalation settings;
- pełny AEM config.

Bieżące wartości w TOML w wielu miejscach odpowiadają defaultom, więc dziś może nie być widocznej różnicy liczbowej. Kontrakt konfiguracyjny jest jednak fałszywy: zmiana TOML może nie zmienić runtime'u, a nawet `enabled=false` nie stanowi pewnej bramki wyłączenia launchera.

### 5.2.4 F-III-2 — AEM jest skonfigurowany, ale niepodłączony

Brain config ma `[post_buy_guardian.aem] enabled=true` (`ghost_brain_config.toml:1248+`). `MonitoringEngine::new()` inicjuje `aem_runtime=None`, `aem_ledger=None` (`engine.rs:1214-1235`). Aktywna ścieżka launchera nigdy nie wywołuje `set_aem()`; robi to tylko stary standalone pipeline builder (`ghost-brain/src/pipeline/builder.rs:400+`).

Skutek: AEM tick istnieje w pętli, ale nie ma runtime/ledger i nie steruje pozycją. Flaga enabled jest myląca.

### 5.2.5 TimeStopV2

TimeStopV2 jest bieżąco `enabled=false`, a jedyny obsługiwany enum mode to `ObserveOnly` (`ghost-brain/src/guardian/post_buy/config.rs:35-99`). Po włączeniu:

- liczy vitality windows;
- emituje counterfactual records/candidates;
- nie zamyka pozycji;
- nie zastępuje prostego inactivity time-stop.

Jest to poprawnie zaprojektowany evidence plane, ale nazwa bez qualifiera może sugerować władzę, której nie ma.

### 5.2.6 Shadow exit replay, Shadow V2 burn-in i dynamic-exit evidence

- Sekcja exit replay jest nieobecna/korzysta z default disabled. Po włączeniu pozostaje research-only sidecar i nie jest konsumowana przez policy.
- Shadow V2 burn-in jest nieobecny/default disabled. Po włączeniu tworzy validation harness i evidence, nie decyzje.
- Dynamic-exit evidence powstaje w obrębie burn-in jako rekord ewaluatora/kontrfaktyczny materiał badawczy; nie wykonuje exit.
- Kod jawnie dodaje reason `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS` w evidence paths (`post_buy_runtime.rs:3081`, `3264`, `3411`).

### 5.2.7 P37 probe

P37 jest izolowanym counterfactual plane. Domyślnie `enabled=false`; launcher tworzy probe lifecycle path tylko, gdy flaga jest włączona (`main.rs:2162-2165`). Probe nie może być łączony z aktywnym shadow lifecycle ani interpretowany jako live BUY.

### 5.2.8 F-III-3 — `[exit_strategy]` jest osieroconą deklaracją

Config opisuje rozbudowaną drabinę:

- partial exits +12%, +20%, +35%, +50%;
- soft stop i hard stop -12%;
- max duration;
- emergency dev sell, price crash, max hold;
- entry wait/recheck.

W repo nie ma Rustowego konsumenta `exit_strategy` (`rg 'exit_strategy' --glob '*.rs'` nie zwraca użycia). Żaden z tych progów nie steruje aktualnym shadow ani live exit. To nie jest „wyłączona strategia”; to orphaned config.

### 5.2.9 Live lane — co zadziałałoby po legalnym przełączeniu trybu

Live post-buy path jest celowo oddzielony od MonitoringEngine. Jego SSOT jest w launcherze (`post_buy_runtime.rs:3960+`):

1. utrwala potwierdzony BUY i realną cenę wejścia;
2. bierze canonical price z AccountStateCore;
3. wykonuje bounded read-only RPC point query, jeśli canonical state jest niedostępny;
4. stosuje fixed full-position TP/SL;
5. buduje SELL przez live sell handle;
6. stosuje bounded build/execution retries;
7. śledzi confirmation i reconciliation;
8. utrwala live position registry.

Bieżące progi z `config.toml:114-115` to:

- take profit **+58%**;
- stop loss **-46%**.

To pełne wyjście 100%, bez drabiny, AEM, Guardiana i `[exit_strategy]`. Kodowy komentarz sugerujący inne progi jest dokumentacyjnym driftem; źródłem prawdy są aktualne pola configu.

Live lane jest obecnie nieaktywny. Ten audyt nie rekomenduje przełączenia na live.

## 5.3 Macierz post-BUY: aktywne, wyłączone i obserwacyjne

| Mechanizm | Config | Wired | Władza nad bieżącym shadow action | Klasyfikacja |
|---|---:|---:|---:|---|
| Trigger RPC shadow simulation | enabled | tak | otwiera/nie otwiera shadow handoff | active authoritative simulation |
| canonical shadow position | shadow mode | tak | tak | active |
| TP +50% | 50.0 | tak | tak, full close | active |
| SL -50% | 50.0 | tak | tak, full close | active |
| 30 s inactivity time-stop | 30000 | tak | tak, full close | active |
| LIGMA | enabled przez default/config | tak | nie w prostej ścieżce close | observational/action-intent only |
| WHF | enabled przez default/config | tak | nie w prostej ścieżce close | observational/action-intent only |
| TCF | enabled przez default/config | tak | nie w prostej ścieżce close | observational/action-intent only |
| PANIC | enabled przez default/config | tak | nie w prostej ścieżce close | observational/action-intent only |
| AEM | TOML true | nie | nie | inert configured feature |
| TimeStopV2 | false | kod tak | nie nawet po enable | disabled observe-only |
| exit replay | default false | kod tak | nie | disabled research-only |
| Shadow V2 burn-in | default false | kod tak | nie | disabled validation-only |
| dynamic-exit evidence | zależny od burn-in | kod tak | nie | disabled observational |
| P37 probe | default false | kod tak | nie dla active plane | isolated counterfactual |
| `[exit_strategy]` ladder/emergency | wartości obecne | nie | nie | orphaned |
| live fixed +58/-46 | wartości obecne | live code tak | nie w bieżącym mode | dormant authoritative live path |

## 6. Czego najbardziej brakuje Decision Plane

Największym brakiem nie jest kolejny score. Repo ma już wiele score'ów, gate'ów, shadow policies i raportów. Brakuje czterech fundamentów, które pozwalają ufać temu, który z nich naprawdę działa:

1. **truthful input semantics** — każdy reducer musi widzieć jawnie zdefiniowany event universe, a IWIM realny payload;
2. **Decision Authority Contract** — maszyna musi umieć powiedzieć, które enabled flagi są wired, authoritative, shadow albo inert;
3. **jeden evaluator na jeden kontrakt** — checkpoint, terminal i replay nie powinny mieć równoległych implementacji policy;
4. **spójny Post-Buy Decision Plane** — typed snapshot → typed decision → action outcome → reconciliation, zamiast kombinacji prostych progów, niewykorzystanych sygnałów i orphaned config.

Poniżej rekomendacje są uporządkowane według wartości i ryzyka.

## 6.1 P0 — działania natychmiastowe

### P0.1 Naprawić prawdziwość IWIM albo odebrać mu władzę

**Rekomendacja krótkoterminowa:** ustawić IWIM jako disabled/log-only, dopóki input nie zawiera realnych transakcji.

**Docelowy projekt:** podczas observation window prefetchować bounded listę versioned transactions + meta dla deva albo przekazywać już zdekodowane instruction facts z Seer. Nie wykonywać nieograniczonego RPC fan-out dopiero po BUY.

Wymagany typed evidence:

- requested signatures;
- signatures returned;
- raw transactions fetched;
- successfully decoded;
- instruction-type coverage;
- timestamp coverage;
- RPC/parse failure classes;
- age/freshness;
- quality nie wyższa niż najsłabsza wymagana coverage.

**Acceptance gate:** placeholder-only sample nie może osiągnąć High quality ani spowodować terminalnego „clean”. Golden test ma udowodnić, że ten sam timestamp set z payloadem i bez payloadu daje różną, prawidłowo zdegradowaną jakość.

**Mierzalna korzyść:** redukcja fałszywego organic confidence i usunięcie aktywnej bramki o pozornej jakości.

### P0.2 Wprowadzić Decision Authority Manifest

Na starcie runtime powinien powstać jeden typed manifest, np.:

```text
component = gatekeeper_v3
configured_enabled = true
wired = true
authority = shadow_only
promotion_requested = false
config_source = ghost-brain/ghost_brain_config.toml

component = post_buy_aem
configured_enabled = true
wired = false
authority = none
startup_status = CONFIGURED_BUT_UNWIRED
```

Manifest powinien objąć V2, V2.5 subcomponents, V3, selector variants, curve, IWIM, Guardian modules, AEM, TimeStopV2, replay/burn-in/probe i live lane.

Walidator powinien fail-close dla deklaracji niemożliwych, np.:

- `v3.promotion.enabled=true` bez promotion bridge;
- AEM enabled bez runtime/ledger;
- Guardian enabled=false, gdy launcher i tak go uruchomi;
- obecność `[exit_strategy]` w profilu, który go nie konsumuje;
- live mode bez durability/confirmation/reconciliation health.

**Mierzalna korzyść:** zero enabled-but-inert flags w zatwierdzonym profilu oraz jednoznaczny run manifest do audytu.

### P0.3 Jedna kanoniczna bramka Event Admission

Przed rozgałęzieniem na reduktory należy obliczyć immutable `AdmittedDecisionEvent` z:

- stable event identity/order key;
- duplicate status;
- success/failure eligibility;
- dust eligibility;
- observation-window eligibility;
- canonical direction/amount/time;
- reason code dla odrzucenia;
- `EventScopeId`/denominator id.

TxIntel, FlipV2, fingerprint, Gatekeeper, CPV, FSC, segmenty i V3 powinny konsumować ten sam admitted stream albo jawnie zadeklarowany superset/subset.

**Acceptance gate:** property/golden tests pokazują identyczne accepted-event counts dla reducerów deklarujących ten sam scope; duplicate, failed i dust nie wchodzą do fingerprintu. Truncation 128 ma własny scope id/status.

**Mierzalna korzyść:** usunięcie niewidocznego driftu feature'ów i mniejsza liczba fałszywych różnic między V2/V3/selector.

### P0.4 Jeden pure evaluator dla terminala, DOW i replay

V2.5 checkpoint powinien tworzyć checkpoint `MaterializedFeatureSet` i wywoływać tę samą `evaluate_policy_from_assessment()` co terminal. Mutable `compute_decision()` należy zdegradować do adaptera lub usunąć po migracji.

**Acceptance gate:** dla identycznego snapshot/config hash checkpoint, terminal i replay zwracają identyczny verdict, reason code, phase vector i gate trace. Różnica czasu jest dozwolona tylko przez jawne pole snapshotu/stage.

**Mierzalna korzyść:** redukcja parity bugs i kosztu utrzymywania dwóch polityk.

## 6.2 P1 — zbudować spójny post-BUY Decision Plane

### P1.1 Jeden `PostBuyDecisionSnapshot`

Snapshot powinien zawierać wyłącznie fakty dostępne w danym ticku:

- immutable entry facts;
- canonical price/liquidity/curve state z freshness;
- current position/remaining quantity;
- market activity clock i absolute age;
- LIGMA/WHF/TCF/PANIC evidence;
- AEM evidence, jeśli faktycznie wired;
- execution/confirmation health;
- evidence status i reason codes.

### P1.2 Jedna typed polityka

Proponowany output:

```text
Hold
TightenStop { new_stop, reason_code }
PartialExit { fraction_bps, reason_code }
FullExit { reason_code }
Unknown { missing_evidence, policy }
```

Następnie osobne warstwy:

```text
PostBuyDecisionSnapshot
  -> pure PostBuyPolicy
  -> ExitIntent
  -> lane-specific executor
  -> ActionOutcome (shadow fill / submit / confirmation / failure / unknown)
  -> reconciliation + durable log
```

Ta architektura pozwala przetestować tę samą politykę w shadow i dopiero później promować jej akcje do live. Nie należy łączyć „policy selected FullExit” z „SELL confirmed”.

### P1.3 Uporządkować konfigurację post-BUY

Należy wybrać jedną z dwóch opcji:

- **implementować** `[exit_strategy]` przez typed unified policy po walidacji shadow;
- albo **usunąć/przenieść do archival research config**, aby operator nie zakładał, że drabina i -12% stop działają.

Równolegle:

- przekazywać pełny `PostBuyGuardianConfig` zamiast budować go z defaultów i kilku pól;
- honorować top-level `enabled`;
- ustawić `aem.enabled=false`, dopóki aktywny launcher nie tworzy runtime/ledger;
- zachować TimeStopV2 jako jawnie nazwany counterfactual observer;
- nie podłączać Guardian/AEM do live przed osobną promotion validation.

### P1.4 Brakujące live safety przed jakąkolwiek promocją

Aktualny live exit ma poprawne rozdzielenie submit/confirmation/reconciliation, ale policy jest tylko fixed full TP/SL. Przed live potrzebne są co najmniej:

- bounded maximum hold;
- jawne unknown-price/unknown-execution policy;
- dev/emergency signal wyłącznie po zwalidowaniu inputu;
- portfolio/exposure limits i global kill switch;
- partial-exit quantity accounting/idempotency, jeśli zostanie wprowadzona drabina;
- durable audit health jako startup/runtime gate;
- shadow-to-live canary z małym, jawnie ograniczonym ryzykiem.

## 6.3 P1 — promocja V3 i selectora tylko przez evidence gate

### P1.5 V3 promotion bridge

V3 ma lepszy kontrakt evidence/replay niż wiele starszych dodatków, ale samo istnienie nie dowodzi przewagi. Promotion bridge powinien powstać dopiero po spełnieniu:

- deterministycznego replay na tym samym snapshot/config hash;
- wysokiej coverage required evidence;
- macierzy disagreement V2 vs V3 z outcome labels;
- temporal/walk-forward split bez leakage;
- robustness w różnych regime;
- osobnych kosztów false BUY i false REJECT;
- shadow execution feasibility, nie tylko terminal PnL;
- jawnego rollback do V2.

Na początku V3 powinien być challengerem: `V2 authority / V3 shadow`, następnie ewentualnie bounded veto-only, a dopiero potem pełnym authority. Nie promować V3 i selectora równocześnie, bo utrudni to identyfikację źródła efektu.

### P1.6 Zbieżność selectorów

Należy nadać unikalne nazwy i kontrakty:

- `gatekeeper_rule_score_0_12` dla embedded soft-score;
- `selector_research_score_0_1` dla sidecara;
- `state_readiness_latch` bez prefiksu sugerującego wybór tokena.

Promocja embedded selectora wymaga:

- walidacji kierunków reguł, szczególnie same-ms;
- coverage/missingness per rule;
- kalibracji score vs outcome, nie tylko wybrania threshold;
- walk-forward i regime stability;
- porównania incremental value po przejściu już aktywnych gate'ów V2;
- counterfactual reject logów z pełnym join do lifecycle.

Jeśli incremental value jest zerowa lub niestabilna, selector należy zredukować do research-only zamiast utrzymywać drugi pseudo-policy score.

## 6.4 P1/P2 — pozostałe usprawnienia i redukcje

### P1.7 Effective-config SSOT

Generować na starcie manifest zawierający co najmniej:

- effective observation deadline 11111 ms;
- curve wait 800 ms;
- V2 mode i wszystkie authority gates;
- execution lane;
- post-buy effective thresholds;
- source file + override/default dla każdego pola;
- config hash.

### P1.8 Durability health jako część rollout safety

W shadow można kontynuować po ENOSPC z głośnym degradation. W live brak DecisionLogger/WAL/position registry durability powinien blokować nowe entry, pozostawiając możliwość bezpiecznego exit/reconciliation istniejących pozycji.

### P2.1 Naprawić nazewnictwo plane/version

`legacy_live` dla aktywnej MFS/V2 ścieżki oraz jednoczesne etykiety V2.2/V2.5 tworzą błąd analityczny. Wprowadzić wersjonowany identity, np.:

```text
decision_plane = gatekeeper_v2_terminal
policy_version = <semantic version/hash>
shadow_challengers = [v25_dow, v3]
```

Migracja loggera musi być addytywna i backward-compatible.

### P2.2 Typed materialization failure zamiast bezwarunkowego panic

Rozdzielić:

- per-candidate integrity failure → typed reject/quarantine;
- global invariant corruption → process fail-fast;
- transient producer missingness → evidence policy.

Nie wolno zamieniać tego na silent fallback.

### P2.3 Zredukować nieaktywne powierzchnie

Konkretnie:

- wyłączyć IWIM authority do naprawy;
- ustawić AEM false do czasu wiring;
- usunąć/przenieść orphaned `[exit_strategy]`;
- nie eksponować inert V3 promotion flag jako operacyjnej;
- zdegradować duplicate mutable evaluator po parity migration;
- archiwizować legacy brain/Paradox/Hyper config poza aktywnym profilem albo jawnie oznaczyć `legacy_unused`.

Redukcja zmniejszy ryzyko operacyjne bardziej niż dodanie kolejnej metryki.

## 6.5 Proponowana kolejność realizacji

| Etap | Zakres | Warunek zamknięcia |
|---|---|---|
| 0 | odebranie authority IWIM + authority manifest | brak enabled-but-unwired w zaakceptowanym profilu |
| 1 | canonical Event Admission + denominator ids | parity tests wszystkich reducerów |
| 2 | checkpoint-MFS + jeden pure evaluator | terminal/checkpoint/replay golden parity |
| 3 | naprawa realnego IWIM inputu | content coverage i adversarial fixtures PASS |
| 4 | unified post-buy snapshot/policy w shadow | typed decision/action/reconciliation, zero ukrytych config defaults |
| 5 | eksperyment V3 challenger i selector incremental value | walk-forward, disagreement i execution-aware evidence |
| 6 | pojedyncza, rollbackowalna promocja | shadow acceptance + osobna decyzja architektoniczna; bez automatycznego live |

## 7. Konkluzja architektoniczna

Decision Plane ma mocny fundament w postaci:

- sesyjnego ownershipu;
- fail-closed materializacji;
- `MaterializedFeatureSet` jako immutable SSOT;
- deterministycznej pure policy V2;
- typed verdict/reason codes;
- separacji shadow/live;
- bogatego evidence i replay dla V2.5/V3;
- poprawnego rozróżnienia submit, confirmation i reconciliation w live execution code.

Jego główne słabości wynikają z narastania warstw wokół tego fundamentu:

- kilka niejednoznacznych selectorów;
- shadow challengery bez jawnego promotion bridge;
- równoległy mutable evaluator;
- config, który deklaruje więcej niż aktywny launcher konsumuje;
- post-BUY modules, których sygnały nie dochodzą do bieżącej polityki close;
- krytycznie niepełny IWIM input;
- brak jednej maszynowo czytelnej deklaracji authority.

Największy dodatni efekt przyniesie teraz nie zwiększenie liczby reguł, lecz **usunięcie fałszywej władzy, ujednolicenie wejścia i evaluatorów oraz zbudowanie jednego, replayowalnego post-BUY policy contract**. Dopiero po tym V3 lub selector powinny być oceniane jako kandydaci do promocji.

## 8. Walidacja wykonana

### 8.1 Wyniki testów

- `cargo test -p ghost-launcher --test gatekeeper_v3_tests` — **PASS**, 9/9. Potwierdzono determinism, stable/sensitive snapshot hash, evidence-first pending/timeout, hard-risk precedence oraz lokalność V3 actionability do sidecara.
- `cargo test -p ghost-launcher --test gatekeeper_v25_regression` — **PASS**, 42/42. Potwierdzono między innymi brak zmiany live verdict przez shadow V2.5, DOW stages, typed reasons, TAS/PDD availability i APS shadow-only override.
- `cargo test -p ghost-launcher --lib configured_rpc_url_rejects_placeholders_and_aliases` — **PASS**, 1/1. Potwierdzono fail-closed walidację endpointu IWIM; test nie naprawia opisanego braku transaction payload.
- `cargo test -p ghost-launcher --lib shadow_only_emits_post_buy_submitted_for_successful_shadow_lane` — **PASS**, 1/1. Potwierdzono successful shadow-lane post-buy handoff.
- `cargo test -p ghost-launcher --test post_buy_runtime_integration` — **PASS**, 4/4. Potwierdzono paper lifecycle, no early-subscribe event loss, routing live lane do sendera oraz fail-closed brak live handle bez paper fallback.
- `cargo test -p ghost-brain --test ghost_brain_config_load_test` — **FAIL baseline**, 6 passed / 1 failed. `gatekeeper_v3_config_loads_from_production_toml` oczekuje `gatekeeper_v2.min_market_cap_sol == 5.0` (`ghost-brain/tests/ghost_brain_config_load_test.rs:47`), podczas gdy bieżący produkcyjny config ładuje `115.0` (`ghost_brain_config.toml:99`). Pozostałe testy, w tym samo ładowanie/walidacja configu i post-buy thresholds, przeszły.

### 8.2 F-II-4 — test kontraktu produkcyjnego configu jest nieaktualny

Failure 5.0 vs 115.0 nie został wprowadzony przez ten audyt — zmiana zawiera tylko nowe dokumenty. Jest jednak materialnym findingiem:

- bieżący config i test nie mają wspólnego oczekiwania;
- nazwa testu sugeruje produkcyjną gwarancję, której obecnie nie zapewnia;
- potwierdza to potrzebę effective-config manifestu i testów opartych o jawnie wersjonowany profil, a nie ręcznie utrzymywaną liczbę bez wskazania policy version.

Rekomendacja: osobno ustalić, czy 115.0 jest zatwierdzonym bieżącym profilem. Jeśli tak, zaktualizować test i komentarz nagłówkowy configu w jednym review-gated zadaniu. Audyt nie zmienia żadnego z nich.

### 8.3 Walidacja dokumentów

- oba dokumenty istnieją jako nowe, untracked files;
- początkowe sprawdzenie whitespace nie wykazało błędów;
- końcowy zakres musi pozostać ograniczony do raportu i ADR-8D.

## 9. Ograniczenia audytu

- Audyt jest statyczny względem wskazanego commita i bieżących plików config.
- Nie uruchamiano live execution ani nie zmieniano trybu shadow.
- Nie potwierdzano bieżącej emisji przez poll procesu/logów; „wired” wynika z aktywnej ścieżki kodu.
- Nie wykonywano nowej analizy statystycznej datasetów; propozycje promotion gates są kontraktem walidacji, nie twierdzeniem o istniejącym edge.
- Legacy HyperPrediction/Hyper Oracle i ich podsystemy zostały świadomie wyłączone z analizy poza oznaczeniem granicy.

## 10. Główne powierzchnie źródłowe

### Konfiguracja

- `config.toml:71-149` — brain toggle, Trigger, execution mode i live exit;
- `config.toml:231+` — AccountStateCore;
- `ghost-brain/ghost_brain_config.toml:13-268` — V2 i selector soft-score;
- `ghost-brain/ghost_brain_config.toml:274-383` — V2.5;
- `ghost-brain/ghost_brain_config.toml:384+` — V3;
- `ghost-brain/ghost_brain_config.toml:559+` — IWIM;
- `ghost-brain/ghost_brain_config.toml:1180-1283` — Guardian, TimeStopV2 i AEM;
- `ghost-brain/ghost_brain_config.toml:1291-1340` — orphaned exit strategy.

### Prebuy/session/materializacja

- `ghost-launcher/src/session/observation.rs:118-368`, `2711-2994`;
- `ghost-launcher/src/session/manager.rs:128-136`;
- `ghost-core/src/checkpoint/types.rs:638-950`;
- `ghost-core/src/account_state_core/types.rs:188-204`;
- `ghost-core/src/tx_intelligence/types.rs:49-102`, `327-357`;
- `ghost-launcher/src/tx_intelligence/engine.rs:239-257`, `901-908`, `1128-1200`.

### Policy

- `ghost-launcher/src/components/gatekeeper_policy.rs:941-1138`, `2158-2662`, `2770+`, `3370-3564`;
- `ghost-launcher/src/components/gatekeeper.rs:5418-5514`, `6363+`, `7201+`, `7504-8115`;
- `ghost-launcher/src/components/gatekeeper_v3.rs:526-694`;
- `ghost-launcher/src/oracle_runtime.rs:5599-5710`, `24046-24605`;
- `ghost-launcher/src/components/iwim_veto.rs:216-635`;
- `ghost-brain/src/oracle/ultrafast/iwim.rs:315-1069`.

### Logging/replay/selector

- `ghost-brain/src/oracle/decision_logger.rs:90-157`, `3230-3318`, `4243-4410`, `4659-4688`;
- `ghost-core/src/metric_contracts/`;
- `ghost-launcher/tests/metric_contracts_pr2*_*.rs`.

### Post-BUY

- `ghost-launcher/src/main.rs:2140-2215`;
- `ghost-launcher/src/oracle_runtime.rs:22480-22797`;
- `ghost-launcher/src/components/post_buy_runtime.rs:806-820`, `2235-2350`, `3960+`;
- `ghost-brain/src/guardian/post_buy/config.rs:35-327`;
- `ghost-brain/src/guardian/post_buy/engine.rs:1171-1325`, `3490-3607`, `4660-5280`;
- `ghost-brain/src/guardian/post_buy/integration.rs`.

## 11. Routing i ślad delegacji logicznej

```yaml
delegation_trace:
  task_classification: "cross-cutting Decision Plane architecture and current-state audit"
  routing_performed: true
  primary_specialist: "Ghost Runtime Coordinator"
  supporting_specialists_considered:
    - "SSOT Feature Materialization Guardian"
    - "Gatekeeper Policy Auditor"
    - "Oracle Session Runtime Engineer"
    - "Decision Logging Replay Analyst"
    - "Config Rollout Safety Reviewer"
    - "Solana Execution Path Engineer"
    - "Seer Ingest Event Integrity Specialist"
  specialist_docs_loaded:
    - "docs/agents/ghost-runtime-coordinator.md"
    - "docs/agents/ssot-feature-materialization-guardian.md"
    - "docs/agents/gatekeeper-policy-auditor.md"
    - "docs/agents/oracle-session-runtime-engineer.md"
    - "docs/agents/decision-logging-replay-analyst.md"
    - "docs/agents/config-rollout-safety-reviewer.md"
    - "docs/agents/solana-execution-path-engineer.md"
  specialist_docs_not_loaded:
    - name: "Seer Ingest Event Integrity Specialist — pełny dokument"
      reason: "sprawdzono routing i kontrakt wejścia, ale zadanie nie zmienia parsera ani identity/order semantics"
  skills_used:
    - "ghost-execution"
    - "trading-systems"
    - "statistical-research-engine"
    - "abstract-reasoning"
  fast_path_used: false
  contracts_checked:
    - "MaterializedFeatureSet SSOT"
    - "single materialization boundary"
    - "Gatekeeper policy ordering"
    - "typed verdict and reason codes"
    - "session deadline and lifecycle"
    - "event admission and denominator parity"
    - "config compatibility and effective authority"
    - "shadow/live separation"
    - "submit vs confirmation vs reconciliation"
    - "DecisionLogger and replay evidence"
    - "post-buy action authority"
  unresolved_routing_uncertainty: []
```
