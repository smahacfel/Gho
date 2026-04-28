# PLAN WYKONAWCZY: Domknięcie architektury pipeline, SSOT, ordering i recovery

**Data:** 2026-03-20  
**Status:** Proposed execution plan  
**Tryb pracy:** **closure mode** — celem nie jest dalsza rozbudowa pipeline, tylko semantyczne i operacyjne domknięcie już istniejącego systemu.  
**Cel nadrzędny:** doprowadzić pipeline do stanu, w którym ma jednoznaczną prawdę o swoim stanie, deterministyczny restart i brak niejawnych alternatywnych ścieżek wykonawczych.  
**Zakres:** `seer`, `ghost-launcher`, `ghost-core`, `ghost-brain`, dokumentacja ADR/runbooki, testy, recovery i telemetria.  
**Priorytet bezwzględny:** najpierw autorytet stanu i recovery, dopiero potem jakiekolwiek optymalizacje, scaling i nowe capability.

---

## 1. Charakter planu i zasada nadrzędna

To **nie jest plan kosmetyczny**. To jest plan zamknięcia systemu, który ma zakończyć etap, w którym pipeline jest funkcjonalnie mocny, ale semantycznie nadal częściowo rozszczelniony.

Wykonawca **nie może** interpretować tego planu jako zachęty do „przy okazji”:

- dokładania nowych feature'ów,
- dodawania nowych source modes,
- rozszerzania scoringu,
- przepisywania decision core,
- tworzenia nowych writerów stanu,
- zmiany publicznych kontraktów IPC/Event Bus bez jawnej potrzeby wynikającej z tego planu.

Jeżeli podczas realizacji pojawi się pomysł „to od razu dorzućmy jeszcze X”, domyślna odpowiedź brzmi: **nie**.

---

## 2. Co dokładnie zamykamy

Plan adresuje pięć klas problemów, które muszą zostać zamknięte w tej kolejności:

1. **Wiele writerów do `ShadowLedger` bez wymuszonej hierarchii autorytetu**
2. **Podwójny i niejednoznaczny bootstrap `genesis_curve()`**
3. **Multi-source ingest bez jednolitego kontraktu semantycznego eventów**
4. **WAL i snapshot jako częściowo aktywna durability, ale bez pełnego recovery lifecycle**
5. **Współistnienie canonical runtime i legacy side paths bez twardych guardów side effectów**

Plan **nie** zakłada przepisywania systemu od zera. Zakłada:

- zachowanie obecnego autorytatywnego rdzenia: `PoolTransaction -> OracleRuntime -> Gatekeeper -> IWIM -> commit/live`,
- zmniejszenie liczby semantycznie aktywnych write pathów,
- przeniesienie niejawnych reguł z „lore zakopanego w kodzie” do twardych kontraktów wymuszonych przez storage layer, recovery layer, testy i telemetrykę.

---

## 3. Zakres, którego nie wolno naruszyć

Poniższe punkty są **poza zakresem** i nie mogą zostać wciągnięte do realizacji „przy okazji”:

1. Re-architektura `Gatekeeper V2`
2. Zmiana logiki `IWIM veto`
3. Dodawanie nowych źródeł ingestu
4. Budowa nowego systemu scoringowego
5. Zmiana SSOT z `ShadowLedger` na `SnapshotEngine`
6. Migracja całego Event Bus na nowy protokół, jeśli nie wynika to bezpośrednio z potrzeby dopięcia semantyki/telemetrii z tego planu
7. Wprowadzanie nowych writerów do `ShadowLedger`
8. Zmiana ścieżki BUY execution poza niezbędnymi guardami przeciwko dubel side effectom

Jeżeli któreś z powyższych okaże się konieczne, praca ma zostać **wstrzymana**, a zmiana opisana w osobnym ADR z uzasadnieniem, dlaczego ten plan nie wystarcza.

---

## 4. Twarde invarianty realizacyjne

To są reguły bez wyjątków. Mają być wymuszone przez kod, a nie tylko opisane w dokumencie.

### 4.1. Invarianty dla `ShadowLedger`

1. **Każdy write do `ShadowLedger` musi przejść przez wspólną warstwę arbitrażu precedence.**  
   Nie wolno zostawić logiki „kto może nadpisać kogo” rozsypanej po callerach.

2. **Warstwa storage musi znać co najmniej:**
   - `write_source`
   - `write_strength`
   - `slot` albo jawne `slot_absent`
   - `state_confidence`
   - `reason` dla write'ów repair/reconcile

3. **Niższa ranga write'u nie może nadpisać wyższej rangi tylko dlatego, że przyszła później czasowo.**

4. **Podwójny bootstrap ma być storage-level no-op albo explicit upgrade, nigdy ukrytym drugim bootstrapem.**

5. **Repair path nie może przepisać canonical commit state bez jawnego, testowanego trybu repair override.**

### 4.2. Invarianty dla WAL i recovery

1. **Żaden rekord recovery-critical nie może być odtwarzany wyłącznie na podstawie arrival order lub kolejności pliku.**

2. **Dla każdego recovery-critical rekordu musi istnieć jawny klucz porządkujący replay.**

3. Dopuszczalne podstawy orderingu:
   - kanoniczny `tx_key` / `signature + event_ordinal`, jeśli rekord pochodzi z tx path,
   - jawny monotoniczny sequence number runtime, jeśli rekord powstaje poza tx path.

4. Niedopuszczalne podstawy orderingu:
   - sam wall clock,
   - sama kolejność appendu w pliku,
   - kolejność systemu plików,
   - „wydaje się, że tak przychodziło w praktyce”.

5. **Replay ma odtwarzać decision-order, nie arrival-order.**  
   Jeśli te dwa porządki są różne, autorytet ma decision-order.

### 4.3. Invarianty dla event semantics

1. **Każdy event downstream musi nieść jawne metadata jakości i pochodzenia.**
2. **Event synthetic nie może być traktowany domyślnie jak raw-chain truth.**
3. **Brak `slot` lub brak chain timestamp nie może być cicho rzutowany na pełną równoważność semantyczną.**

### 4.4. Invarianty dla legacy pathów

1. **Legacy path nie może emitować realnego BUY side effectu bez explicit allowlist.**
2. **Canonical decision i legacy observation muszą być rozróżnialne w logach, metrykach i testach.**

---

## 5. Definicje pojęć używane w tym planie

Żeby nie było miejsca na nadinterpretację, poniższe pojęcia oznaczają dokładnie to:

- **bootstrap seed** — pierwszy zapis minimalnego curve state dla nowego poola, jeszcze niebędący canonical history
- **confirmed bootstrap** — upgrade bootstrap seed danymi wyższego zaufania, ale nadal nie canonical history
- **repair/reconciliation write** — zapis naprawczy wynikający z dodatkowej obserwacji lub wykrytego driftu
- **canonical commit** — jedyny write tworzący autorytatywną historię snapshotów poola
- **live append** — dopisanie tx po canonical commit dla poola już committed
- **decision-order** — kolejność logiczna, w której runtime podjął lub powinien podjąć decyzję o zmianie stanu
- **arrival-order** — kolejność fizycznego nadejścia eventów/rekordów
- **storage arbitration** — centralne miejsce, gdzie rozstrzyga się, czy write może zostać przyjęty, zignorowany albo potraktowany jako upgrade

---

## 6. Docelowy model autorytetu stanu

### 6.1. Role write'ów do `ShadowLedger`

Docelowo `ShadowLedger` przyjmuje write'y tylko w pięciu rolach:

1. **Bootstrap seed**  
   Jednorazowe, minimalne zainicjalizowanie curve state po wykryciu poola.

2. **Confirmed bootstrap enrichment**  
   Ulepszenie bootstrap seed na podstawie bardziej zaufanego źródła startowego.

3. **Repair / reconciliation**  
   Korekta driftu lub uzupełnienie braków stanu, ale nie canonical history generation.

4. **Canonical history commit**  
   Jedyny writer tworzący kanoniczną historię snapshotów.

5. **Post-commit live append**  
   Jedyny writer dopisujący live tx do poola już committed.

### 6.2. Rangi `write_strength`

Każdy write ma przypisaną rangę:

| Ranga | Nazwa | Przykład | Może nadpisać | Nie może nadpisać |
|---|---|---|---|---|
| P0 | bootstrap seed | `genesis_curve()` | brak stanu | P1, P2, P3, P4 |
| P1 | confirmed bootstrap | RPC seeder, verified initial curve snapshot | P0 | P3, P4 |
| P2 | repair/reconciliation | `AccountUpdate`, periodic reconcile | P0, P1 przy zgodnej regule repair | P3 bez jawnego override |
| P3 | canonical commit | `commit_history()` | P0, P1, P2 | P4 nowszego committed stanu |
| P4 | live append | `append_live()` | wcześniejszy stan committed poola | nic niższego |

### 6.3. Kontrakt arbitrażu write'ów

Warstwa storage ma zwracać wynik co najmniej w jednej z klas:

- `Applied`
- `NoOpExistingEqualOrStronger`
- `RejectedWeakerWrite`
- `PromotedBootstrapToConfirmed`
- `RejectedOutOfOrder`
- `RejectedMissingMetadata`

Każdy taki wynik ma być metryczny i logowany.

---

## 7. Fazy realizacji i bramki przejścia

Plan jest **ściśle sekwencyjny**. Nie wolno zaczynać kolejnej fazy, jeśli poprzednia nie spełniła warunków wejścia/wyjścia.

### Faza 0 — Zamrożenie kontraktów i inwentaryzacja blast radius

**Cel:** ustalić, co jest dziś prawdą operacyjną, zanim zaczniemy cokolwiek przepinać.

#### Wejście do fazy

- brak równolegle otwartych prac dodających nowe write pathy do `ShadowLedger`
- brak planowanych zmian publicznych kontraktów Event Bus/IPC niezwiązanych z tą fazą

#### Zadania

1. Spisać aktualny kontrakt dla:
   - `GeyserEvent`
   - `SeerEvent`
   - `GhostEvent`
   - `PoolTransaction`
   - `AccountUpdate`
   - `InitPoolEvent`
   - `ShadowLedger.commit_history()`
   - `ShadowLedger.append_live()`

2. Oznaczyć wszystkie write pathy do `ShadowLedger` w kodzie produkcyjnym jako:
   - bootstrap seed,
   - confirmed bootstrap,
   - repair,
   - canonical commit,
   - live append.

3. Zidentyfikować wszystkie miejsca, które:
   - emitują `NewPoolDetected`,
   - inicjalizują `genesis_curve()`,
   - aktualizują curve state przed Gatekeeperem,
   - wykonują boczne write'y z bridge/listenerów,
   - mogą dopisać stan po restart/replay.

4. Udokumentować jeden matrix: `moduł -> funkcja -> rola -> side effects -> write_strength -> replacement plan`.

5. Wyłapać każde miejsce, gdzie precedence lub recovery ordering są dziś tylko implikowane, ale nie wymuszone przez kod.

#### Artefakty wyjściowe

- matrix writerów `ShadowLedger`
- lista recovery-critical rekordów WAL
- lista kontraktów publicznych, których nie wolno złamać w kolejnych fazach

#### Zabronione skróty

- nie wolno w tej fazie „od razu naprawiać” writerów, zanim matrix nie będzie kompletny
- nie wolno scalać ról typu bootstrap i repair „bo technicznie robią podobny write”

#### Kryterium wyjścia

- [ ] istnieje kompletna lista writerów `ShadowLedger`
- [ ] każdy writer ma przypisaną rolę i `write_strength`
- [ ] istnieje lista recovery-critical rekordów WAL
- [ ] wiadomo, które kontrakty publiczne są zamrożone

---

### Faza 1 — Wymuszenie precedence i single-writer bootstrap semantics

**Cel:** skończyć z niejednoznacznym seedowaniem `genesis_curve()` i przenieść precedence z dokumentu do storage layer.

#### Warunek wejścia

- Faza 0 ukończona
- istnieje kompletna lista writerów i ról

#### Zadania

1. Wprowadzić **jedną storage-level ścieżkę arbitrażu** dla wszystkich write'ów do `ShadowLedger`.

2. Dodać obowiązkowe metadata write'u:
   - `write_source`
   - `write_strength`
   - `state_confidence`
   - `slot` lub `slot_absent`
   - `reason`

3. Wyznaczyć **jednego kanonicznego bootstrap writera** dla nowych pooli.

4. Rozwiązać konflikt podwójnego bootstrapu:
   - Seer bootstrap direct,
   - bootstrap listener po `NewPoolDetected`.

5. Wprowadzić regułę:
   - `genesis_curve()` może wykonać się **co najwyżej raz logicznie per pool**,
   - każdy kolejny bootstrap tej samej krzywej musi skończyć jako `NoOpExistingEqualOrStronger` albo `PromotedBootstrapToConfirmed`.

6. Upewnić się, że confirmed bootstrap (`RPC seeder`) nie konkuruje z bootstrap seedem, tylko działa jako jawny upgrade P0 -> P1.

7. Dodać telemetrię:
   - `shadow_ledger_write_total{source=...,strength=...,result=...}`
   - `shadow_ledger_bootstrap_total{source=...}`
   - `shadow_ledger_bootstrap_noop_total{source=...}`
   - `shadow_ledger_write_rejected_total{reason=...}`

#### Artefakty wyjściowe

- centralny mechanizm storage arbitration
- pojedynczy bootstrap semantics
- testy precedence dla bootstrap/replay/race

#### Zabronione skróty

- nie wolno zostawić precedence w callerach
- nie wolno polegać na „naturalnym orderingu tasków” zamiast na storage guards
- nie wolno uznać RPC seedera za drugi niezależny bootstrap

#### Kryterium wyjścia

- [ ] każdy write do `ShadowLedger` przechodzi przez wspólną warstwę arbitrażu
- [ ] dokładnie jedna ścieżka odpowiada za semantyczny bootstrap
- [ ] drugi bootstrap tej samej krzywej nie może zmienić stanu poza dozwolonym upgrade path
- [ ] precedence jest dowiedziona testem, nie tylko dokumentem

---

### Faza 2 — Twarda definicja roli `AccountUpdate` i repair plane

**Cel:** zakończyć niejednoznaczność, czy `AccountUpdate` jest fundamentem truth, czy mechanizmem korekcyjnym.

#### Warunek wejścia

- Faza 1 ukończona
- storage arbitration działa i precedence jest testowane

#### Zadania

1. Formalnie ustalić model runtime:
   - **`tx_only`** — decyzje mogą zostać podjęte bez `AccountUpdate`
   - **`tx_plus_account_repair`** — `AccountUpdate` może podnosić confidence lub naprawiać drift

2. Zmienić dokumentację i kod tak, by `AccountUpdate` był nazwany **repair plane**, nie data plane.

3. Dla `account_updates_enabled=false` zagwarantować:
   - brak zależności decision path od tej ścieżki,
   - fail-closed behavior dla freshness/curve confidence,
   - brak cichych null pathów, które później udają „prawie działa”.

4. Oznaczyć każdy write pochodzący z `AccountUpdate` lub reconciliation jako:
   - `write_source=account_update|reconciliation`
   - `write_strength=P2`

5. Dopisać metryki:
   - `curve_truth_source_total{source=...}`
   - `curve_truth_mode_total{mode=speculative|confirmed|committed|repaired}`

#### Artefakty wyjściowe

- jawna definicja `tx_only` vs `tx_plus_account_repair`
- spójny opis repair plane
- testy obu trybów runtime

#### Zabronione skróty

- nie wolno zostawić miejsc, gdzie `AccountUpdate` jest „czasem optional, czasem truth” bez nazwania tego wprost
- nie wolno używać repair pathu do obchodzenia precedence

#### Kryterium wyjścia

- [ ] istnieje jawna definicja `tx_only` vs `tx_plus_account_repair`
- [ ] wyłączenie `AccountUpdate` nie łamie kontraktu decision path
- [ ] każdy write repair ma jawny `write_strength=P2`

---

### Faza 3 — Normalizacja semantyki eventów cross-source

**Cel:** sprawić, aby downstream rozumiał nie tylko payload, ale też pochodzenie, kompletność i zaufanie eventu.

#### Warunek wejścia

- Faza 2 ukończona
- nie ma otwartych zmian dodających nowe source modes

#### Zadania

1. Zdefiniować wspólny semantic envelope dla eventów niezależnie od źródła.

2. Każdy event przekazywany dalej ma nieść jawnie:
   - `source_kind` (`grpc`, `geyser_ws`, `helius_ws`, `pumpportal`)
   - `event_truth_kind` (`raw_chain`, `adapted_chain`, `synthetic`)
   - `slot_quality` (`present`, `absent`, `estimated`)
   - `timestamp_quality` (`chain`, `adapter`, `wall_clock`)
   - `completeness` (`full`, `partial`)

3. Dla source modes synthetic dopisać kontrakt, których pól downstream **nie wolno** traktować na równi z raw chain truth.

4. Wprowadzić **jedną** funkcję/warstwę normalizacyjną między ingestem a dalszym routingiem.

5. Zmienić downstream heuristic entry points tak, aby czytały ten semantic envelope, a nie zgadywały jakość danych po źródle pośrednio.

6. Dopisać telemetrykę source quality:
   - `event_semantic_total{source_kind=...,truth_kind=...,slot_quality=...,completeness=...}`

#### Artefakty wyjściowe

- wspólny semantic envelope
- testy kontraktowe dla `raw` vs `synthetic`
- zaktualizowane downstream heuristics entry points

#### Zabronione skróty

- nie wolno dodać tylko kilku pól do `PumpPortal` i uznać, że semantyka jest zamknięta
- nie wolno zostawić source-specific interpretacji rozsianych po kilku modułach

#### Kryterium wyjścia

- [ ] każdy event downstream ma jawny opis semantyczny
- [ ] istnieje jedna warstwa normalizacji
- [ ] `synthetic` i `raw` nie są już milcząco traktowane jako równoważne

---

### Faza 4 — Domknięcie durability: strict WAL ordering, restore i replay delta

**Cel:** przejść z „WAL istnieje” do „runtime da się odtworzyć po restarcie deterministycznie”.

#### Warunek wejścia

- Faza 3 ukończona
- semantic envelope stabilny

#### Zadania

1. Formalnie określić, które rekordy WAL są recovery-critical:
   - `RawTx`
   - `ParsedEvent`
   - `Decision`
   - `CommitStaged`
   - `CommitPersisted`
   - `TradeForwarded`
   - `ShadowLedgerCurveUpdate`

2. Dla każdego recovery-critical rekordu zdefiniować **ReplayOrderKey**.

3. Ustalić jawnie, czy dany rekord jest porządkowany przez:
   - `tx_key`,
   - `signature + event_ordinal`,
   - czy runtime `sequence_number`.

4. Zmodyfikować startup launchera tak, aby zawsze wykonywał w tej kolejności:
   1. restore najnowszego snapshotu `ShadowLedger`,
   2. odczyt watermarka snapshotu,
   3. replay WAL od watermarka,
   4. rekonstrukcję pending state commit/live,
   5. dopiero potem otwarcie ingest/live.

5. Włączyć okresowe snapshotowanie `ShadowLedger` jako realny task runtime, nie tylko istniejące API.

6. Dodać recovery telemetry:
   - `wal_replay_records_total`
   - `wal_replay_duration_ms`
   - `shadow_ledger_restore_duration_ms`
   - `runtime_recovery_watermark_slot`
   - `runtime_recovery_mode{mode=snapshot_only|snapshot_plus_wal|wal_only}`

#### Artefakty wyjściowe

- jawny `ReplayOrderKey` dla każdego recovery-critical rekordu
- deterministic startup restore flow
- testy restartowe dla staged commit / partial WAL tail / pending live

#### Zabronione skróty

- nie wolno opierać replay wyłącznie na append order
- nie wolno uruchamiać live ingest przed zakończeniem restore + replay
- nie wolno uznać recovery za gotowe bez testu restartu

#### Kryterium wyjścia

- [ ] każdy recovery-critical rekord ma jawny `ReplayOrderKey`
- [ ] replay odtwarza decision-order, nie arrival-order
- [ ] restart procesu odtwarza `ShadowLedger` i pending state bez ręcznych kroków
- [ ] istnieją testy restartu po staged commit i z częściowo przyciętym WAL tail

---

### Faza 5 — Freshness, finality i policy dla `PendingCurve`

**Cel:** zamienić obecny zestaw heurystyk w formalny kontrakt jakości stanu curve.

#### Warunek wejścia

- Faza 4 ukończona
- recovery flow jest stabilny

#### Zadania

1. Ujednolicić model curve state quality:
   - `unknown`
   - `stale`
   - `fresh`
   - `committed`

2. Dodać spójny model finality:
   - `speculative`
   - `provisional`
   - `finalized`

3. Wprowadzić policy matrix dla Gatekeepera:

| freshness | finality | zachowanie |
|---|---|---|
| unknown | any | `PendingCurve` albo reject zgodnie z configiem |
| stale | speculative/provisional | fail-closed albo penalized wait |
| fresh | speculative | normal path z flagą ryzyka |
| fresh | provisional/finalized | normal path |
| committed | finalized | canonical confidence |

4. Przenieść progi freshness do jednego SSOT configu.

5. Upewnić się, że `PendingCurve` ma zawsze jawny i telemetrowany stan końcowy:
   - `recovered`
   - `rejected`
   - `timed_out`

6. Dopisać telemetrykę:
   - `shadow_ledger_curve_freshness_total{state=...}`
   - `shadow_ledger_curve_finality_total{state=...}`
   - `gatekeeper_pending_curve_total{reason=...}`
   - `gatekeeper_pending_curve_terminal_total{outcome=...}`

#### Artefakty wyjściowe

- jedna polityka freshness/finality
- jeden config SSOT
- testy `PendingCurve` dla wszystkich terminal states

#### Zabronione skróty

- nie wolno zostawić freshness jako zestawu rozproszonych booli i env override'ów
- nie wolno zostawić `PendingCurve` bez terminal telemetryki

#### Kryterium wyjścia

- [ ] freshness/finality są modelowane jawnie
- [ ] `PendingCurve` ma przewidywalną ścieżkę końcową
- [ ] config steruje polityką, a nie rozproszone override'y

---

### Faza 6 — Ostre odcięcie legacy side effects od canonical runtime

**Cel:** sprawić, by historyczne ścieżki nie wyglądały jak alternatywna prawda wykonawcza.

#### Warunek wejścia

- Faza 5 ukończona
- canonical path i recovery są już ustabilizowane

#### Zadania

1. Oznaczyć wszystkie legacy pathy jako jedno z poniższych:
   - observability-only,
   - compatibility-only,
   - disabled in production,
   - deprecated with removal date.

2. Dla `TriggerComponent` / `OraclePipeline` dopisać, które eventy są tylko legacy/observability i **nie mają prawa** emitować autorytatywnych BUY side effects.

3. Wprowadzić runtime guardy, które blokują podwójny execution path dla tej samej decyzji.

4. Ujednolicić logowanie, tak aby odróżniało:
   - canonical decision,
   - legacy observation,
   - shadow simulation,
   - post-buy monitoring.

5. Dodać liczniki:
   - `legacy_path_event_total{path=...}`
   - `legacy_path_side_effect_block_total{path=...}`

#### Artefakty wyjściowe

- klasyfikacja wszystkich legacy pathów
- guardy side effectów
- rozróżnialne logi i metryki

#### Zabronione skróty

- nie wolno zostawić legacy pathów „na chwilę” bez klasyfikacji
- nie wolno polegać wyłącznie na naming convention zamiast runtime guardów

#### Kryterium wyjścia

- [ ] da się jednoznacznie wskazać, który path może emitować realne side effecty BUY
- [ ] legacy logi nie mylą się z canonical decision logami
- [ ] brak podwójnego wykonania przy współistnieniu starych i nowych ścieżek

---

### Faza 7 — Testy własnościowe, chaos i regresja semantyczna

**Cel:** zamknąć plan dowodem, że system zachowuje się deterministycznie w krytycznych edge case'ach.

#### Warunek wejścia

- Faza 6 ukończona

#### Scenariusze obowiązkowe

1. **Double bootstrap idempotence**  
   dwa konkurencyjne bootstrapy nie powodują dwóch różnych stanów curve.

2. **RPC seed upgrade without canonical corruption**  
   confirmed bootstrap może ulepszyć seed, ale nie może cofnąć committed state.

3. **`tx_only` mode correctness**  
   runtime działa bez `AccountUpdate`, a `PendingCurve` kończy się zgodnie z policy.

4. **Cross-source semantic equivalence**  
   dwa eventy o podobnym biznesowym znaczeniu, ale różnym źródle, mają różne confidence metadata i downstream to respektuje.

5. **Restart after staged commit**  
   recovery odtwarza commit/live state bez duplikacji.

6. **WAL replay follows decision-order**  
   replay daje ten sam wynik co runtime order decyzyjny, nawet gdy arrival-order był inny.

7. **Legacy side-effect suppression**  
   canonical BUY nie ma dubla z legacy pathu.

8. **Reconciliation vs live append precedence**  
   repair i live path nie wchodzą sobie w paradę poza jawnie dozwolonym repair case.

#### Artefakty wyjściowe

- testy jednostkowe precedence
- testy integracyjne restart/recovery
- testy kontraktowe source semantics
- przynajmniej jeden soak/chaos scenariusz restart + source failover

#### Kryterium wyjścia

- [ ] istnieją testy jednostkowe dla kontraktów precedence
- [ ] istnieją testy integracyjne dla bootstrap/recovery
- [ ] istnieje test dowodzący decision-order aware replay
- [ ] istnieje co najmniej jeden soak/chaos scenariusz restart + source failover

---

## 8. Konkretna lista zadań w odpowiedniej kolejności

Poniżej kolejność wykonawcza bez przestrzeni do mieszania etapów.

### Etap A — Ustalenie prawdy stanu

1. Spisać matrix writerów `ShadowLedger` i recovery-critical rekordów WAL.
2. Wprowadzić storage arbitration i `write_strength`.
3. Wyznaczyć jeden kanoniczny bootstrap writer.
4. Zmienić drugi bootstrap tej samej krzywej w `NoOp` albo explicit upgrade.
5. Sformalizować `AccountUpdate` jako repair plane.

### Etap B — Ustalenie prawdy eventów

6. Wprowadzić wspólny semantic envelope dla eventów cross-source.
7. Oddzielić `synthetic` od `raw` na poziomie kontraktu i testów.
8. Zaktualizować downstream heurystyki tak, by korzystały z metadata jakości danych.

### Etap C — Ustalenie prawdy restartu

9. Zdefiniować `ReplayOrderKey` dla każdego recovery-critical rekordu.
10. Spiąć restore snapshotu i replay WAL w jeden deterministic startup sequence.
11. Odtworzyć pending state commit/live po restarcie.
12. Włączyć okresowe snapshotowanie jako obowiązkowy task runtime.

### Etap D — Ustalenie prawdy jakości stanu

13. Ujednolicić model freshness/finality.
14. Przenieść policy do jednego SSOT configu.
15. Domknąć terminalne zachowanie `PendingCurve`.

### Etap E — Ustalenie prawdy wykonania side effectów

16. Sklasyfikować legacy pathy.
17. Dodać guardy blokujące nieautoryzowane side effecty.
18. Ujednolicić logi i metryki canonical vs legacy.

### Etap F — Dowód zamknięcia

19. Napisać testy precedence dla `ShadowLedger`.
20. Napisać testy restart/recovery dla WAL+snapshot.
21. Napisać testy cross-source semantic normalization.
22. Napisać testy suppression dla legacy side effects.
23. Uruchomić soak/chaos scenariusze i zaktualizować ADR/runbooki.

---

## 9. Podział na sprinty

### Sprint 1 — Prawda stanu

- Etap A
- wynik: storage arbitration, `write_strength`, pojedynczy bootstrap semantics

### Sprint 2 — Prawda eventów

- Etap B
- wynik: event niesie jawny opis jakości i źródła

### Sprint 3 — Prawda restartu

- Etap C
- wynik: restart runtime staje się deterministyczny

### Sprint 4 — Prawda jakości stanu

- Etap D
- wynik: freshness/finality/PendingCurve mają jeden kontrakt

### Sprint 5 — Prawda side effectów + dowód

- Etap E i F
- wynik: brak ukrytych alternatywnych side effectów i formalny dowód odporności systemu

---

## 10. Definition of Done

Plan uznajemy za wykonany dopiero wtedy, gdy wszystkie poniższe warunki są spełnione łącznie:

1. **`ShadowLedger` ma storage-level wymuszoną hierarchię writerów**, a nie tylko opis w dokumentacji.
2. **Podwójny bootstrap nie istnieje semantycznie** — drugi bootstrap tej samej krzywej kończy jako `NoOp` albo jawny upgrade P0 -> P1.
3. **Każdy recovery-critical rekord ma `ReplayOrderKey`**, a replay odtwarza decision-order.
4. **Runtime potrafi odtworzyć stan po restarcie** z `ShadowLedger` snapshot + WAL replay + recovery pending state.
5. **`AccountUpdate` ma jedną precyzyjnie opisaną rolę** i `tx_only` mode jest pełnoprawnym, testowanym trybem.
6. **Każdy event downstream niesie jawne metadata semantyczne źródła i jakości.**
7. **Freshness, finality i `PendingCurve` są sterowane z jednego policy modelu** i mają komplet metryk oraz terminal states.
8. **Legacy pathy nie mogą emitować nieautoryzowanych side effectów**, a logi i metryki odróżniają je od canonical runtime.
9. **Istnieją testy kontraktowe, integracyjne, restartowe i replay-orderingowe** dla wszystkich krytycznych granic tego planu.

---

## 11. Najwyższe ryzyka wdrożeniowe

1. **Pozorna naprawa precedence bez storage guardów**  
   czyli dokument mówi jedno, a caller nadal może przepchnąć słabszy write.

2. **WAL replay jako iluzja**  
   rekordy istnieją, ale ordering nadal bazuje na append order zamiast decision-order.

3. **Normalizacja eventów tylko częściowa**  
   `PumpPortal` dostaje kilka pól, ale downstream dalej używa go jak raw chain truth.

4. **Legacy cleanup bez hard guardów**  
   co kończy się poprawną dokumentacją i podwójnym BUY-em.

5. **Scope creep**  
   czyli dokładanie nowych feature'ów zanim system zyska jednoznaczną prawdę o sobie.

---

## 12. Dokumenty obowiązkowe do aktualizacji po każdej fazie

Po zakończeniu każdej fazy trzeba zaktualizować:

- `/root/Gho/docs/ADR/20260320_production_pipeline_data_flow.md`
- właściwy ADR opisujący zmianę kontraktu
- runbook operacyjny hot path / recovery
- dashboardy i opisy metryk

Brak aktualizacji dokumentacji po ukończonej fazie oznacza, że faza nie jest domknięta.

---

## 13. Werdykt wykonawczy

Najpierw porządkujemy **kto ma prawo pisać prawdę**, potem **co znaczą eventy**, potem **jak system wraca po restarcie**, a dopiero później **jak zachowuje się jakość stanu i które ścieżki mogą emitować side effecty**.  
W tej kolejności repo zyskuje nie „jeszcze lepszy pipeline”, tylko **pipeline, który w końcu ma jednoznaczną prawdę o sobie i potrafi ją obronić po restarcie, race condition i source driftcie**.
