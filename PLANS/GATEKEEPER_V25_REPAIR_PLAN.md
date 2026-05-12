# GATEKEEPER V2.5 REPAIR PLAN — FINAL

> **Data:** 2026-05-05  
> **Wersja:** 2.0  
> **Status:** finalny plan naprawczy po audycie kodu, konfiguracji, logów i rollout contracts  
> **Cel:** doprowadzić Gatekeeper V2.5 do stanu **rzetelnego, audytowalnego i promotowalnego shadow-first decision plane**, bez łamania legacy live contracts i bez kolejnego "fałszywego modelu", który istnieje tylko w logach.

---

## 1. Executive verdict

Obecny problem **nie sprowadza się wyłącznie do złych progów**. System jest popsuty na czterech poziomach naraz:

1. **decision-plane ambiguity** — legacy live verdict i V2.5 shadow verdict są mieszane w jednym wpisie JSONL,
2. **threshold collapse** — aktywny rollout ma wyjęte bezpieczniki, więc V2.5 nie jest uczciwie testowany,
3. **partial SSOT parity** — obie ścieżki assessmentu nie niosą tego samego semantycznego payloadu,
4. **coverage + execution gaps** — Phase-1 starvation i martwy shadow execution zrywają evidence chain.

Wniosek: naprawa musi być **architektoniczna**, nie tylko konfiguracyjna.

Ten plan naprawia problem w kolejności:

1. **ustalenie granic systemu i kontraktów,**
2. **przywrócenie sensownych guardraili shadow rollout,**
3. **rozdzielenie legacy live plane od V2.5 shadow plane,**
4. **wymuszenie invariants wewnątrz V2.5 plane,**
5. **doprowadzenie SSOT/feature-availability do spójności,**
6. **naprawa coverage/acceptance,**
7. **naprawa shadow execution + reconciliation,**
8. **clean shadow-burnin + promotion gates.**

---

## 2. Boundary decisions — rzeczy niepodlegające negocjacji

To jest najważniejsza część planu. Bez tych decyzji naprawa znowu skończy się mieszaniem semantyk.

### B1. Legacy live contracts pozostają nienaruszone

- `mode = "long"` i `mode = "standard"` **nie zmieniają semantyki live verdict** w tym repair streamie.
- `v25.shadow_enabled = true` i `v25.live_execution_enabled = false` oznacza:
  - legacy pipeline nadal produkuje live verdict,
  - V2.5 produkuje **shadow verdict**, nie live override.

### B2. V2.5 ma być first-class shadow decision plane

Naprawa **nie polega** na tym, że shadow modules zaczynają po cichu przepisywać legacy BUY na REJECT.  
Naprawa polega na tym, że:

- V2.5 shadow verdict ma własne typed fields,
- V2.5 shadow verdict ma własny reason chain,
- V2.5 shadow verdict trafia do własnych log streams,
- promotion do live jest osobnym krokiem po walidacji i ADR.

### B3. PDD jest absolutnym veto wewnątrz V2.5 plane

W V2.5:

- `pdd_hard_fail != None` => `v25_shadow_verdict != BUY`,
- `v25_confidence == 0.0` => `v25_shadow_verdict != BUY`,
- `tas_score < hard_reject_threshold` => `v25_shadow_verdict = REJECT_LOW_TRAJECTORY`.

Ale dopóki `live_execution_enabled = false`, **to veto działa w plane V2.5**, nie jako cichy rewrite legacy live verdict.

### B4. Nie wolno "zgadywać" brakujących feature'ów

Jeżeli `MaterializedFeatureSet` nie zawiera danych potrzebnych do:

- ramping,
- flash crash,
- segmentowej trajektorii,
- pełnego confidence,

to Path B ma zwrócić:

- `None`, albo
- `unavailable_reason`,

zamiast syntetycznie rekonstruować dane i udawać parity.

### B5. Najpierw coverage, potem tuning

Dopóki:

- `TIMEOUT_PHASE1`,
- `TIMEOUT_NO_DATA`,
- niska `truth_to_runtime_accept_pct`,
- słaba seer/runtime acceptance,

dominują próbę, **nie wolno interpretować braku BUY jako samego problemu thresholdów**.

### B6. Shadow-only nie może zależeć od live payer contract

Zgodnie z istniejącym rollout contract:

- `shadow_only` **nie może wymagać production keypair_path**,
- `live` i `live_and_shadow` **muszą** wymagać jawnie skonfigurowanego payera,
- ewentualny ephemeral signer, jeśli naprawdę potrzebny do `simulateTransaction`, może istnieć wyłącznie w izolowanej ścieżce `shadow_only`, a nie w shared payer loader dla live.

### B7. Nie ruszać HyperPrediction Oracle coupling

Ten plan **nie** wpina HyperPrediction z powrotem do Gatekeepera.  
To pozostaje poza zakresem.

### B8. Nie obiecywać PnL jako Definition of Done

`65% win rate`, `avg loss < -15%`, `profit 30%+` to **cele walidacyjne dla shadow**, nie kryteria "merge-ready" dla kodu naprawczego.

---

## 3. Co było nie tak w poprzednim draftcie i co tu poprawiamy

Ten finalny plan celowo **usuwa** kilka ryzykownych założeń z draftu 1.0:

1. **Usunięte:** założenie, że przy `live_execution_enabled=false` PDD shadow ma przepisywać finalny live verdict na REJECT.  
   **Powód:** łamałoby shadow-first rollout i mieszało planes.

2. **Usunięte:** blanket ephemeral fallback w `load_configured_payer()` dla `live_and_shadow`.  
   **Powód:** zagraża execution integrity i może maskować brak realnego payera.

3. **Usunięte:** obietnica "pełnej równości Path A i Path B" przez odtwarzanie brakujących danych z niczego.  
   **Powód:** groziłaby synthetic parity i uncalibrated confidence.

4. **Usunięte:** destrukcyjne `mv` dużych historycznych logów jako część code PR.  
   **Powód:** operacyjnie ryzykowne i zbędne; lepszy jest nowy clean output path.

5. **Usunięte:** `clippy`/`fmt` jako obowiązkowy merge gate bez potwierdzenia, że repo ich używa operacyjnie.  
   **Powód:** trzymamy się istniejących build/test commands repo.

6. **Usunięte:** traktowanie RC6 (coverage starvation) jako skutku samych progów.  
   **Powód:** to osobny root cause i wymaga osobnego workstreamu.

---

## 4. Target end-state po naprawie

Po zakończeniu planu system ma wyglądać tak:

### 4.1 Decision planes

Każda decyzja ma jawnie rozdzielone:

- `legacy_live_verdict_type`
- `legacy_live_reason_chain`
- `v25_shadow_verdict_type`
- `v25_shadow_reason_chain`
- `v25_shadow_confidence`
- `v25_promotion_state`

Opcjonalnie w przyszłości:

- `v25_live_verdict_type`

### 4.2 Log contract

Logi są routowane co najmniej po:

- `rollout_profile`
- `gatekeeper_version`
- `decision_plane`
- `config_hash`

czyli np.:

```text
logs/rollout/shadow-burnin/decisions/
  shadow-burnin/
    v2.5/
      shadow/
        <config_hash>/
          gatekeeper_decisions.jsonl
          gatekeeper_buys.jsonl
```

Historyczny mixed log pozostaje immutable jako artefakt audytowy.

### 4.3 V2.5 verdict integrity

Nigdy więcej nie może istnieć rekord V2.5, w którym jednocześnie:

- `v25_shadow_verdict_type = BUY`
- `pdd_hard_fail != None`

albo:

- `v25_shadow_verdict_type = BUY`
- `v25_confidence == 0.0`

### 4.4 SSOT and availability integrity

Path A i Path B są spójne semantycznie:

- dla pól wyprowadzalnych z `MaterializedFeatureSet` -> wartości zgodne,
- dla pól niewyprowadzalnych -> jawny `unavailable_reason`,
- bez synthetic backfill.

### 4.5 Coverage and execution integrity

- coverage audit tłumaczy TIMEOUT-heavy behavior,
- shadow simulation działa end-to-end,
- reconciliation zamyka evidence chain,
- reason codes obejmują data/ingest/execution failures.

---

## 5. Workstream 0 — freeze architecture boundary

### Cel

Zamrozić semantykę naprawy zanim zaczniemy grzebać w kodzie.

### Scope

- dopisać do planu i SSOT jawne rozróżnienie:
  - legacy live plane,
  - V2.5 shadow plane,
  - future promoted V2.5 live plane,
- ustalić, że ten repair stream **nie dodaje nowego `GatekeeperMode::V25`**,
- ustalić, że repair stream naprawia obecny shadow-first rollout w ramach istniejących kontraktów.

### Uzasadnienie

To jest najważniejsza korekta względem draftu 1.0.  
Bez niej dalej będziemy mylić "V2.5 policzył REJECT" z "live system realnie odrzucił pool".

### Pliki

- `PLANS/GATEKEEPER_V25_REPAIR_PLAN.md`
- `PLANS/GATEKEEPER_V25_SSOT_CONTRACTS.md`

### Definition of Done

- [ ] SSOT explicite rozróżnia planes
- [ ] naprawa nie wymaga nowego `GatekeeperMode`
- [ ] shadow-first rollout pozostaje niezmieniony

---

## 6. Workstream 1 — restore strict shadow guardrails in the active rollout

### Problem

Aktywny rollout ma wyjęte bezpieczniki:

- `0.0`,
- `9999.0`,
- `false`,

w miejscach, które miały być guardrailami V2.5.

### Co naprawiamy

Przywracamy **aktywny shadow-burnin config surface** do wartości zgodnych z planem Precision Strike:

- DOW strict,
- TAS strict,
- PDD active,
- APS active w shadow/offline mode,
- alpha/prosperity włączone dla shadow oceny.

### Ważne zastrzeżenie

To nie jest "obniżmy thresholdy, żeby zobaczyć BUY".  
To jest "przywróćmy thresholdy, żeby V2.5 w ogóle było uczciwie testowane".

### Pliki

- `ghost-brain/ghost_brain_config.toml` **albo** dedykowany overlay używany przez canonical `shadow-burnin`
- w razie potrzeby rollout profile docs

### Zmiany obowiązkowe

#### V2.5 sections

- `[gatekeeper_v2.dow]`
- `[gatekeeper_v2.tas]`
- `[gatekeeper_v2.pdd]`
- `[gatekeeper_v2.aps]`

wracają do planowych wartości shadow-first.

#### Promotion gates

Wszystkie:

- `entry_drift_promoted_to_live`
- `spike_promoted_to_live`
- `ramping_promoted_to_live`
- `whale_promoted_to_live`
- `reserve_promoted_to_live`
- `flash_crash_promoted_to_live`

pozostają `false`.

### Dodatkowy wymóg

Każdy rollout po tej zmianie musi wypisywać swój `config_hash` i `rollout_profile`, żeby nie było już niejednoznaczności który config generował logi.

### Definition of Done

- [ ] aktywny shadow rollout nie ma neutralized guardrails
- [ ] V2.5 shadow jest uruchamiany na sensownych progach
- [ ] żadne live promotion flags nie zostały odblokowane
- [ ] parse/config tests dla gatekeepera przechodzą

---

## 7. Workstream 2 — separate decision planes and repair logger semantics

### Problem

To jest główny root cause semantyczny.  
Obecnie jeden rekord potrafi oznaczać jednocześnie:

- legacy BUY,
- V2.5 shadow reject,
- PDD hard fail,
- zero confidence.

To nie jest "dziwny log". To jest **broken decision contract**.

### Co naprawiamy

Decision logger przestaje udawać, że jeden verdict opisuje dwa różne systemy.

### Nowy kontrakt logowania

Każdy rekord musi mieć osobne pola dla:

```text
legacy_live_verdict_type
legacy_live_reason_chain
v25_shadow_verdict_type
v25_shadow_reason_chain
v25_shadow_confidence
v25_shadow_observation_stage
v25_promotion_state
decision_plane
rollout_profile
config_hash
```

### Schema version

Ponieważ to jest **realna zmiana semantyki logów**, plan zakłada bump:

- z obecnego `v16`
- do `v17`

`v16` już istnieje; repair stream nie może udawać, że to nadal ta sama semantyka.

### Routing

Logger routuje po:

- rollout profile,
- gatekeeper version,
- decision plane,
- config hash.

### Hashing

`config_hash` nie może być ręcznie składanym skrótem z kilku progów.  
Musi powstawać z:

- deterministycznej,
- kanonicznej,
- serializowanej reprezentacji gatekeeper config surface używanej w danym rolloucie.

Preferowany kierunek:

- canonical serde/toml/json serialization
- następnie BLAKE3

Zamiast ręcznie wybieranej listy pól, którą ktoś później zapomni rozszerzyć.

### Pliki

- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- ewentualnie wspólne helpery config hashing w `ghost-brain/src/config/*`

### Definition of Done

- [ ] mixed semantics nie istnieją w pojedynczym typed verdict field
- [ ] logger rozróżnia planes jawnie
- [ ] nowy clean rollout zapisuje logi do nowego path
- [ ] historyczny mixed log pozostaje tylko artefaktem offline
- [ ] schema bump jest jawny i udokumentowany

---

## 8. Workstream 3 — enforce V2.5 invariants inside V2.5 plane

### Problem

V2.5 potrafi obecnie zalogować BUY mimo:

- `pdd_hard_fail = WHALE`
- `pdd_score = 0.0`
- `v25_confidence = 0.0`
- `shadow_*_verdict = REJECT_PUMP_AND_DUMP`

To jest failure mode:

- **uncalibrated confidence**
- **execution drift from intent**
- **broken ordered decision precedence**

### Co naprawiamy

W obu ścieżkach:

- `compute_decision()`
- `evaluate_policy_from_assessment()`

V2.5 plane zawsze przechodzi przez:

1. hard fails,
2. PDD,
3. core/sybil/alpha/prosperity,
4. TAS confidence,
5. DOW gate.

### Kluczowa zasada

**Nie zmieniamy legacy live verdict przez side effect shadow path.**

Zamiast tego:

- `v25_shadow_verdict_type = REJECT_*` jeśli PDD/TAS zabiło kandydaturę,
- `legacy_live_verdict_type` zostaje tym, czym był legacy verdict,
- `decision_plane` rozstrzyga, który verdict dotyczy którego systemu.

### Invariants do zakodowania i testowania

#### V25-I1

`v25_shadow_verdict_type = BUY` jest zabronione gdy `pdd_hard_fail != None`

#### V25-I2

`v25_shadow_verdict_type = BUY` jest zabronione gdy `v25_shadow_confidence == 0.0`

#### V25-I3

`extended` BUY jest zabronione gdy `PDD` nie jest clean

#### V25-I4

`shadow_only` może wygenerować `legacy_live_verdict_type = BUY` i `v25_shadow_verdict_type = REJECT_*`, ale **nigdy nie w jednym polu i nigdy bez jawnego rozdzielenia planes**

### Pliki

- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`

### Definition of Done

- [ ] V2.5 plane nie generuje sprzecznych BUY
- [ ] legacy live verdict nie jest cicho przepisywany przez shadow modules
- [ ] tests pokrywają invariant failures
- [ ] typed reasons wyjaśniają każdy reject w plane V2.5

---

## 9. Workstream 4 — SSOT parity with explicit feature availability

### Problem

Draft 1.0 słusznie wykrył asymetrię Path A / Path B, ale chciał ją naprawić zbyt agresywnie.  
Nie wszystkie pola V2.5 da się uczciwie policzyć z `MaterializedFeatureSet`.

### Poprawna zasada

Nie dążymy do "pełnej równości wartości za wszelką cenę".  
Dążymy do:

1. **równości dla pól wyprowadzalnych z SSOT**,  
2. **jawnej niedostępności dla pól niewyprowadzalnych**,  
3. **braku synthetic parity**.

### Co wprowadzamy

#### 4.1 Capability matrix

Dla każdego pola V2.5 określamy:

- `available_from_buffer`
- `available_from_materialized_features`
- `requires_sequence`
- `requires_price_anchor`
- `confidence_safe_to_compute`

#### 4.2 Availability/degraded fields

Do assessment/log contract dodajemy jawne informacje, np.:

```text
tas_available
tas_unavailable_reason
pdd_sequence_signals_available
v25_confidence_available
v25_confidence_unavailable_reason
```

#### 4.3 Confidence policy

`v25_confidence` jest liczone tylko gdy:

- wszystkie wejścia wymagane przez jego definicję są dostępne,
- score nie jest mieszanką partial placeholders,
- wynik da się sensownie interpretować.

W przeciwnym razie:

- `v25_confidence = None`
- `v25_confidence_unavailable_reason = ...`

### Path B

Path B może wypełnić:

- observation stage,
- entry drift / anchor quality (jeśli dostępne),
- część PDD,
- część APS,
- tylko taką trajektorię, którą da się wyprowadzić z realnie zapisanych segmentowych danych.

Path B **nie ma prawa** odtwarzać:

- ramping,
- flash crash,
- sekwencyjnych spike motifs,

jeżeli SSOT tego nie niesie.

### Timeout path

Timeout path dostaje te same availability semantics co pozostałe ścieżki.  
Timeout nie może być "pustą ścieżką V2.5".

### Pliki

- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper_pdd.rs`
- `ghost-launcher/src/components/gatekeeper_trajectory.rs`
- `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- testy kontraktowe

### Definition of Done

- [ ] parity jest zdefiniowane per field capability, nie na zasadzie życzeniowej
- [ ] Path B nie udaje danych, których nie ma
- [ ] timeout path niesie availability-aware V2.5 payload
- [ ] confidence nie jest liczone z partial fiction

---

## 10. Workstream 5 — Phase-1 coverage and ingest reliability

### Problem

To jest brakujący workstream w draftcie 1.0.

Jeżeli duża część pooli kończy jako:

- `TIMEOUT_PHASE1`
- `TIMEOUT_NO_DATA`

to bez naprawy coverage nie wiadomo, czy problem jest w:

- thresholdach,
- ingestion,
- dedup,
- dust filter,
- time semantics,
- window close logic.

### Co naprawiamy

#### 5.1 Coverage observability

Rozszerzamy coverage audit tak, aby dla każdego okna było widać:

- chain truth count,
- seer received,
- seer emitted,
- runtime seen,
- runtime accepted,
- filtered reasons,
- close reason,
- effective time source,
- duplicate suppression stats.

#### 5.2 Timeout taxonomy

Rozdzielamy timeouty na:

- genuine-no-interest,
- ingest-miss,
- filter-drop,
- stale/late arrival,
- window-close-too-early,
- invariant-broken bookkeeping.

#### 5.3 Filter audit

Osobno audytujemy wpływ:

- `min_sol_threshold`,
- dedup cache,
- time-source fallback,
- observation origin,
- failed-tx filtering,
- runtime acceptance rules.

#### 5.4 Promotion block

Żaden threshold tuning ani interpretacja "V2.5 nie generuje BUY" nie może być uznana za wiarygodną, dopóki coverage workstream nie domknie timeout taxonomy.

### Pliki

- `logs/rollout/shadow-burnin/decisions/seer_runtime_coverage_audit.jsonl`
- instrumentacja coverage / runtime
- ewentualnie seer/runtime filters

### Definition of Done

- [ ] timeout-heavy behavior ma jawnie rozpisaną strukturę przyczyn
- [ ] wiadomo, jaka część starvation to real market quality, a jaka data path loss
- [ ] tuning V2.5 nie odbywa się na skażonej próbce

---

## 11. Workstream 6 — shadow execution and reconciliation hardening

### Problem

`shadow_only` kończy się błędem payera, mimo że istniejące rollout docs mówią, że shadow-only nie powinno wymagać production keypair.

### Zasada naprawy

Naprawiamy **kontrakt execution path**, nie dokładamy ryzykownego shared fallbacku.

### Co naprawiamy

#### 6.1 Rozdzielenie payer semantics

- `shadow_only`
  - nie może zależeć od live payer contract,
  - jeśli do symulacji potrzebny jest signer, wybór strategii signera ma być lokalny dla shadow path,
  - provenance signera ma być logowane.

- `live`
  - bez jawnego keypair -> hard fail

- `live_and_shadow`
  - bez jawnego keypair -> hard fail

#### 6.2 Jeśli signer jest potrzebny do shadow simulation

Wtedy plan dopuszcza:

- `shadow_payer_strategy = configured | ephemeral`

ale tylko:

- w ścieżce `shadow_only`,
- bez dotykania shared payer loader używanego przez live submit.

#### 6.3 Reconciliation

Shadow simulation result musi zostać dowieziony do reconciliation z klasyfikacją:

- data problem
- authority problem
- timing/blockhash problem
- fee/compute problem
- network/provider problem
- simulation mismatch
- logic/invariant problem

#### 6.4 Idempotency

Każdy shadow dispatch musi mieć:

- idempotency key,
- single reconciliation path,
- duplicate suppression.

### Ważne

Plan **nie zakłada z góry**, że ephemeral signer na pewno zadziała z aktualnym builderem.  
Najpierw trzeba potwierdzić, czy builder naprawdę wymaga podpisu w shadow path i w którym miejscu.

### Pliki

- `ghost-launcher/src/components/trigger/component.rs`
- shadow execution path
- reconciliation runtime
- rollout profile docs/config

### Definition of Done

- [ ] `shadow_only` działa bez production payer dependency
- [ ] live i live_and_shadow nie tracą safety guarantees
- [ ] shadow results trafiają do reconciliation
- [ ] failure classes są jawne

---

## 12. Workstream 7 — clean validation and promotion gates

### Problem

Sama kompilacja i testy jednostkowe nie wystarczą.  
Trzeba udowodnić, że naprawiony V2.5 jest:

- audytowalny,
- spójny,
- coverage-sound,
- promotowalny.

### 7.1 Testy kodowe

Uruchamiamy istniejące repo tests/build commands używane operacyjnie przez projekt, w szczególności:

- parser/config tests,
- gatekeeper regression tests,
- workspace tests związane z Gatekeeper/Trigger/Logger.

Nie dokładamy nowych merge gates tylko dlatego, że "fajnie byłoby je mieć".

### 7.2 Testy kontraktowe obowiązkowe

Minimum:

1. `v25_shadow_buy_cannot_coexist_with_pdd_hard_fail`
2. `v25_shadow_buy_cannot_coexist_with_zero_confidence`
3. `legacy_live_and_v25_shadow_planes_are_logged_separately`
4. `path_b_marks_unavailable_instead_of_guessing_sequence_features`
5. `shadow_only_does_not_depend_on_live_payer_contract`
6. `shadow_result_reaches_reconciliation`
7. `config_hash_changes_when_gatekeeper_behavior_changes`

### 7.3 Clean rollout

Nie przenosimy starego 577MB pliku w ramach code PR.  
Zamiast tego:

- uruchamiamy nowy clean rollout do nowego path,
- np. `shadow-burnin-v25-repair` albo nowego podkatalogu per config/profile.

### 7.4 Shadow validation gates przed promocją

V2.5 może być uznany za promotowalny dopiero gdy clean shadow run pokaże:

- brak sprzecznych verdictów,
- brak mixed-plane ambiguity,
- coverage taxonomy zamkniętą,
- osobny raport false rejects / false accepts,
- osobne ablation dla:
  - Phase-1 viability,
  - PDD,
  - TAS,
  - APS,
  - Alpha,
  - Prosperity,
  - Sybil,
- brak nowego źródła danych poza Yellowstone w decision path,
- brak RPC w Gatekeeper decision path,
- jawny ADR przed jakimkolwiek `live_execution_enabled = true`.

### 7.5 Czego nie uznajemy za sukces

To **nie** jest sukces, jeżeli:

- BUY pojawiają się tylko po zjechaniu thresholdów do minimum,
- logi są "czystsze", ale coverage dalej jest słabe,
- shadow simulation dalej nie domyka reconciliation,
- confidence jest liczone, ale bez availability discipline.

---

## 13. Merge order

Kolejność jest istotna. Nie wolno robić tego losowo.

1. **Workstream 0** — freeze boundary
2. **Workstream 1** — restore strict shadow guardrails
3. **Workstream 2** — separate planes + logger semantics
4. **Workstream 3** — enforce V2.5 invariants inside V2.5 plane
5. **Workstream 4** — SSOT parity with availability semantics
6. **Workstream 5** — coverage/Phase-1 reliability
7. **Workstream 6** — shadow execution + reconciliation
8. **Workstream 7** — clean validation and promotion gates

### Dlaczego taka kolejność

- bez boundary freeze znowu pomylimy planes,
- bez strict shadow guardrails nie przetestujemy uczciwie V2.5,
- bez log separation nie da się wiarygodnie ocenić poprawy,
- bez coverage repair nie da się interpretować scarcity,
- bez reconciliation nie zamkniemy evidence chain.

---

## 14. Final acceptance criteria

Plan jest wykonany dopiero wtedy, gdy jednocześnie zachodzą wszystkie poniższe warunki:

### A. Boundary and contracts

- [ ] legacy live plane i V2.5 shadow plane są jawnie rozdzielone
- [ ] shadow-first rollout contract nie został złamany
- [ ] SSOT contracts są zaktualizowane o nowe invariants i availability semantics

### B. Logger integrity

- [ ] schema semantycznie odróżnia planes
- [ ] config/profile/version/hash routing działa
- [ ] nowe clean logi są odseparowane od historycznego mixed dump

### C. V2.5 decision integrity

- [ ] brak V2.5 BUY z `pdd_hard_fail`
- [ ] brak V2.5 BUY z `v25_confidence == 0.0`
- [ ] extended BUY wymaga clean PDD
- [ ] reason codes są kompletne

### D. SSOT and feature integrity

- [ ] Path B nie zgaduje sequence-only features
- [ ] unavailable semantics są jawne
- [ ] confidence nie jest partial-fiction score

### E. Coverage integrity

- [ ] timeout taxonomy jest znana
- [ ] wiadomo, jaka część starvation wynika z ingestion/filter path
- [ ] tuning interpretowany jest dopiero na coverage-sound próbce

### F. Execution integrity

- [ ] shadow-only działa bez production payer dependency
- [ ] reconciliation zamyka evidence chain
- [ ] failure classes są jawne i użyteczne

### G. Validation integrity

- [ ] istniejące testy repo przechodzą
- [ ] nowe kontraktowe testy V2.5 przechodzą
- [ ] clean shadow-burnin jest gotowy do uruchomienia
- [ ] promotion gates są jawne i zablokowane bez ADR

---

## 15. Final note

Ten plan **nie próbuje sztucznie "odblokować BUY"**.  
Ten plan ma zrobić coś trudniejszego i ważniejszego:

1. przywrócić prawidłowe granice między live a shadow,
2. przywrócić uczciwe thresholdy,
3. zamknąć broken evidence chain,
4. naprawić coverage,
5. sprawić, że V2.5 będzie można wreszcie mierzyć jako **realny system oceny**, a nie wydmuszkę z pseudo-telemetrią.

Jeżeli po tej naprawie BUY nadal będą rzadkie, to będzie to wreszcie **informacja diagnostycznie wiarygodna**, a nie artefakt:

- złych logów,
- zneutralizowanych progów,
- mieszanych decision planes,
- albo martwego execution path.

---

**To jest finalny plan naprawczy.**
