# ROZWÓJ PARSERA — PLAN WYKONAWCZY

## 0. Cel dokumentu

Ten dokument opisuje **bezpieczny, etapowy rozwój parsera Seer** tak, aby:

1. odzyskać **execution-tree provenance** bez regresji coverage,
2. odróżnić **wykrycie trade'a** od **atrybucji pochodzenia flow**,
3. rozbroić ryzyka wokół dwóch warstw arbitrażu/dedupu,
4. nie przemycać do systemu nieudowodnionych założeń typu „Mayhem jest root cause”,
5. zachować kompatybilność z mostem `TradeEvent -> PoolTransaction` i downstream runtime.

To nie jest plan „przepisać parser od zera”. To plan **kontrolowanej ewolucji** istniejącego kodu, z minimalnym blast radius i z obowiązkową telemetrią przed zmianą semantyki.

---

## 1. Stan wyjściowy repo — fakty, od których wychodzimy

### 1.1. Ścieżki parsowania transakcji

W repo istnieją dziś trzy istotne ścieżki związane z parsowaniem transakcji:

1. **Live gRPC raw path**
   - plik: `off-chain/components/seer/src/binary_parser.rs`
   - funkcja: `PumpParser::parse_transaction_raw(...)`
   - to jest główna ścieżka dla surowych gRPC/proto bytes.

2. **Backfill / decoded RPC path**
   - plik: `off-chain/components/seer/src/binary_parser.rs`
   - funkcja: `PumpParser::parse_geyser_transaction(...)`
   - używana dla `PumpEvent::BackfillTransaction`.

3. **Decoded runtime path wewnątrz BinaryParser**
   - plik: `off-chain/components/seer/src/binary_parser.rs`
   - funkcja: `BinaryParser::parse_transaction_from_decoded(...)`
   - ta ścieżka już dziś stempluje `event_ordinal` i ma bogatszy model ordynacji instruction-level.

### 1.2. Aktualny kontrakt eventów i mostu downstream

Już dziś istnieją przydatne elementy, których nie wolno rozwalić:

- `seer::types::TradeEvent` posiada:
  - `event_ordinal: Option<u32>`
  - `timestamp_ms`
  - `arrival_ts_ms`
  - `semantic: EventSemanticEnvelope`
  - `is_mayhem_mode: Option<bool>`
- `ghost-launcher/src/components/seer.rs::trade_event_to_pool_transaction(...)` już dziś zachowuje:
  - `event_ordinal`
  - `timestamp_ms`
  - `arrival_ts_ms`
  - `is_mayhem_mode`
- `ghost-core/src/event_semantics.rs::EventSemanticEnvelope` jest dziś wykorzystywany do:
  - source/truth quality,
  - slot/timestamp quality,
  - completeness.

### 1.3. Aktualny problem architektoniczny

W `PumpParser::parse_transaction_raw(...)` oraz `PumpParser::parse_geyser_transaction(...)`:

- `InnerInstructionGroup.index` jest dostępny w typach,
- ale nie jest używany do powiązania inner group z outer instruction,
- przez co parser **traci provenance wrappera / execution-tree context**.

To oznacza:

- parser dalej może wykryć inner Pump CPI,
- ale **nie wie, z jakiego outer flow to pochodzi**,
- nie potrafi więc wiarygodnie tagować flow jako `wrapped`, `aggregator`, `mayhem`, itd.

### 1.4. Aktualne dwie warstwy arbitrażu / dedupu

Ważne: w parserze są dziś **dwa różne poziomy arbitrażu**, których nie wolno mylić.

#### A. `dedup_trade_events(...)`
- plik: `off-chain/components/seer/src/binary_parser.rs`
- poziom: `ParsedPumpEvent`
- rola: strukturalne preferowanie CPI eventów nad ix-level dublem
- charakter: **strukturalny ix-vs-CPI**, nie timestampowy

#### B. `dedup_trade_candidates(...)`
- plik: `off-chain/components/seer/src/binary_parser.rs`
- poziom: `TradeEvent`
- rola: arbitraż między kandydatami trade po złożeniu ich z `ParsedPumpEvent`
- charakter: bardziej heurystyczny niż `dedup_trade_events`, bo używa jakości kandydata, sygnatury, side, amount, quote amount oraz `event_ordinal`

To rozróżnienie jest krytyczne dla dalszych prac.

### 1.5. Aktualna semantyka `is_mayhem_mode`

`TradeEvent.is_mayhem_mode` nie jest dziś twardą klasyfikacją on-chain parsera. To pole jest pass-through telemetry z innych źródeł i nie może być traktowane jako SSOT dla wrapper provenance.

### 1.6. Aktualne ograniczenie najważniejsze dla tego planu

Największy brak nie brzmi dziś:

> „parser nie widzi Pump trade'ów”

Największy brak brzmi:

> „parser nie zachowuje pełnego kontekstu pochodzenia trade'a i ma dwa poziomy arbitrażu, które trzeba najpierw opomiarować, a dopiero potem zmieniać”

---

## 2. Zasady architektoniczne dla rozwoju parsera

### 2.1. Najpierw telemetria, potem zmiana semantyki

Nie wolno zaczynać od przebudowy dedupu albo dokładania `MayhemFlow=true/false`.

Kolejność jest obowiązkowa:

1. dodać obserwowalność,
2. dodać provenance,
3. dopiero potem zmieniać arbitraż.

### 2.2. Nie wolno przeciążać `EventSemanticEnvelope`

`EventSemanticEnvelope` opisuje dziś:
- source kind,
- truth kind,
- slot/timestamp quality,
- completeness.

Nie należy wpychać tam surowych danych typu:
- `outer_program_id`,
- `outer_instruction_index`,
- `wrapper_kind`,
- `stack_height`.

Te dane powinny żyć w **osobnej strukturze provenance**, bo to inny wymiar semantyki.

### 2.3. Najpierw zapisujemy surowy provenance, dopiero potem klasyfikujemy Mayhem

Pierwszy krok nie powinien polegać na dodaniu pola:
- `is_mayhem_flow: bool`

Tylko na zapisaniu:
- `outer_program_id`
- `outer_instruction_index`
- `inner_group_index`
- `stack_height`
- `invoked_program_id`

Dopiero później, gdy telemetryka pokaże rzeczywisty rozkład wrapperów, można zbudować klasyfikację:
- `NativePump`
- `Wrapped`
- `Mayhem`
- `Unknown`

### 2.4. Live i backfill muszą dojść do jednego modelu

Nie wolno rozwijać tylko jednej ścieżki. Docelowo live raw path i decoded/backfill path mają dawać:

- ten sam model eventów,
- ten sam model provenance,
- tę samą semantykę ordynacji,
- ten sam kontrakt dla downstream.

### 2.5. Nie wolno zakładać, że dedup jest winny, zanim to pokażą liczniki

Każda zmiana w:
- `dedup_trade_events(...)`
- `dedup_trade_candidates(...)`

musi być poprzedzona telemetrią:
- before/after count,
- drop reason,
- retained winner kind,
- liczba przypadków z różnym provenance przy tym samym signature.

---

## 3. Docelowy stan architektury parsera

### 3.1. Parser ma mieć wspólny model wykonania instrukcji

Docelowo parser ma pracować na wspólnym modelu:

- outer instruction stream,
- inner groups powiązane przez `group.index`,
- jawny kontekst wykonania przekazywany do dekoderów.

### 3.2. Każdy `ParsedPumpEvent` ma znać pochodzenie

Minimalny docelowy zestaw provenance dla eventów parsera:

- `outer_instruction_index: Option<u32>`
- `inner_group_index: Option<u32>`
- `outer_program_id: Option<String>`
- `invoked_program_id: String`
- `stack_height: Option<u32>`
- `from_cpi: bool`

### 3.3. `TradeEvent` ma przenosić provenance bez utraty obecnych pól

`TradeEvent` nie może stracić:
- `event_ordinal`
- `timestamp_ms`
- `arrival_ts_ms`
- `semantic`

Zamiast tego ma dostać **nowy optional provenance payload**.

### 3.4. Dedup ma być obserwowalny i reprodukowalny

Po zakończeniu programu rozwoju parsera musi być możliwe odpowiedzenie z logów/metryk:

- ile eventów weszło przed każdym poziomem arbitrażu,
- ile wypadło po `dedup_trade_events`,
- ile wypadło po `dedup_trade_candidates`,
- jaki był powód zachowania lub odrzucenia,
- czy kandydaci różnili się provenance.

---

## 4. Plan wdrożenia — faza po fazie

# FAZA 0 — BASELINE, TELEMETRIA, BRAK ZMIAN SEMANTYKI

## Cel fazy

Dodać obserwowalność i punkty pomiarowe bez zmiany decyzji parsera.

## Pliki obowiązkowe

- `off-chain/components/seer/src/binary_parser.rs`
- opcjonalnie małe dopięcia testów w:
  - `ghost-launcher/src/components/seer.rs`

## Kroki implementacyjne

### Krok 0.1 — Policz oba poziomy arbitrażu osobno

W `binary_parser.rs` dodać telemetryczne liczniki dla:

1. `dedup_trade_events(...)`
   - wejście: liczba `ParsedPumpEvent`
   - wyjście: liczba po dedupie
   - labelki minimalne:
     - `event_kind`
     - `decision=kept|dropped`
     - `reason=pumpfun_cpi_match|pumpswap_cpi_match|none`

2. `dedup_trade_candidates(...)`
   - wejście: liczba `TradeEvent`
   - wyjście: liczba po dedupie
   - labelki minimalne:
     - `decision=kept|replaced|merged_into_existing`
     - `reason=exact_match|weak_duplicate|none`
     - `incoming_score`
     - `existing_score`

### Krok 0.2 — Zlicz przypadki z różnym `event_ordinal`

W `dedup_trade_candidates(...)` dodać jawne metryki/logi dla przypadków:
- `trade_ordinals_differ(...) == true`
- przypadek odrzucony z powodu różnego ordinału
- przypadek bez ordinału po live path

To pozwoli zobaczyć, czy live path nadal gubi porządek zdarzeń.

### Krok 0.3 — Zlicz orphan inner groups

W obu walkerach:
- `parse_transaction_raw(...)`
- `parse_geyser_transaction(...)`

dodać telemetryczny check:

- jeśli `inner_set.index` wskazuje outer index poza zakresem top-level instructions,
- zalogować `warn!` + policzyć metrykę `ghost.parser.orphan_inner_group_total`.

To nie zmienia semantyki, ale natychmiast pokaże, czy adaptery źródeł dostarczają spójne grupy.

### Krok 0.4 — Zlicz brak provenance, którego dziś nie umiemy odzyskać

Dodać telemetryczne liczniki:
- inner CPI pod non-pump outer,
- inner CPI bez możliwego outer programu,
- inner CPI z outer programem != invoked program.

Na tym etapie wystarczy liczyć lokalnie w walkerze, bez dodawania jeszcze nowych pól do eventów.

## Testy obowiązkowe

Dopisać / rozszerzyć testy w `binary_parser.rs`:

- `orphan_inner_group_is_counted_not_crashed`
- `dedup_trade_events_emits_decision_metrics`
- `dedup_trade_candidates_emits_decision_metrics`
- `live_path_without_event_ordinal_is_visible_in_metrics` (jeśli rzeczywiście występuje)

## Merge gate

Nie wolno merge’ować, jeśli po Fazie 0:
- parser zmienia liczbę emitowanych trade’ów bez jawnego uzasadnienia,
- dodane logi lub metryki wprowadzają panic / unwrap w hot path,
- nie da się rozróżnić strat na poziomie `ParsedPumpEvent` i `TradeEvent`.

---

# FAZA 1 — ODBUDOWA EXECUTION-TREE PROVENANCE

## Cel fazy

Dodać pełny kontekst outer↔inner do parsera, bez zmiany obecnej logiki dekodowania instrukcji i bez zmiany downstream business semantics.

## Pliki obowiązkowe

- `off-chain/components/seer/src/binary_parser.rs`
- `off-chain/components/seer/src/types.rs`

## Kroki implementacyjne

### Krok 1.1 — Wprowadzić nową strukturę provenance

W `types.rs` dodać nową strukturę, np.:

- `InstructionProvenance`

Minimalne pola:
- `outer_instruction_index: Option<u32>`
- `inner_group_index: Option<u32>`
- `outer_program_id: Option<String>`
- `invoked_program_id: String`
- `stack_height: Option<u32>`
- `from_cpi: bool`

Na tym etapie pole ma być optional i w pełni backward-compatible.

### Krok 1.2 — Rozszerzyć `ParsedPumpEvent`

Dodać do `ParsedPumpEvent` pole:
- `provenance: Option<InstructionProvenance>`

Każde miejsce tworzące `ParsedPumpEvent` w `binary_parser.rs` ma zostać zaktualizowane tak, by:
- top-level event miał provenance z `outer_instruction_index = Some(idx)` i `from_cpi = false`,
- inner event miał provenance z:
  - `outer_instruction_index = Some(inner_set.index)`
  - `inner_group_index = Some(inner_set.index)`
  - `outer_program_id = program top-level instruction wskazanej przez index`
  - `invoked_program_id = program inner ix`
  - `stack_height = inner_ix.stack_height`
  - `from_cpi = true`

### Krok 1.3 — Przebudować walker inner groups tak, by korzystał z `group.index`

W obu funkcjach:
- `parse_transaction_raw(...)`
- `parse_geyser_transaction(...)`

zamiast traktować `inner_instructions` jako anonimową listę group, należy:

1. zbudować pomocniczą mapę/program lookup dla outer instructions,
2. dla każdego `inner_set` odczytać odpowiadający outer program,
3. przekazać ten kontekst do eventów i dekodera.

**Ważne:**
- to nadal nie znaczy, że outer non-pump ma zostać dekodowany jak trade,
- chodzi o zachowanie pochodzenia, nie o poszerzanie allowlisty programu.

### Krok 1.4 — Ujednolicić stamping ordinałów

Na tym etapie trzeba jawnie zdecydować, czy:
- `event_ordinal` jest ordinalem top-level/inner execution order,
- czy tylko ordinalem semantycznego eventu po dedupie.

Rekomendacja dla tego repo:
- zachować `event_ordinal` jako **stabilny ordinal eventu w obrębie tx**,
- nie mieszać go z surowym `stack_height` ani `group.index`.

Następnie:
- doprowadzić do tego, aby live raw path i decoded/backfill path stemplowały eventy w ten sam sposób.

### Krok 1.5 — NIE dodawać jeszcze `MayhemFlow`

W tej fazie **nie** dodawać:
- `is_mayhem_flow`
- `wrapper_kind=Mayhem`

Najpierw zachować surowy `outer_program_id`, dopiero po danych telemetrycznych budować klasyfikację.

## Testy obowiązkowe

W `binary_parser.rs` dopisać testy:

- `inner_group_index_is_bound_to_outer_program`
- `non_pump_outer_with_inner_pump_preserves_provenance`
- `cpi_event_provenance_survives_event_build`
- `live_and_decoded_paths_assign_compatible_event_ordinals`

## Merge gate

Nie wolno merge’ować, jeśli po Fazie 1:
- parser zmienił istniejącą semantykę BUY/SELL,
- `event_ordinal` stał się mniej stabilny niż przed zmianą,
- inner Pump CPI dalej nie mają żadnego śladu outer programu,
- provenance jest dodany tylko do jednej ścieżki (`raw` lub `decoded`).

---

# FAZA 2 — PROPAGACJA PROVENANCE DO `TradeEvent`

## Cel fazy

Przenieść execution provenance z poziomu `ParsedPumpEvent` na poziom `TradeEvent`, bez rozwalania obecnego mostu do launchera.

## Pliki obowiązkowe

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- `ghost-launcher/src/components/seer.rs`
- opcjonalnie: `ghost-launcher/src/events.rs`

## Kroki implementacyjne

### Krok 2.1 — Dodać provenance do `TradeEvent`

Do `TradeEvent` dodać nowe optional pole, np.:
- `provenance: Option<InstructionProvenance>`

Nie upychać tego do `semantic`.

### Krok 2.2 — Przepiąć mapowanie `ParsedPumpEvent -> TradeEvent`

W `binary_parser.rs`, w ścieżkach budowy `TradeEvent` z:
- `ParsedEventKind::Trade`
- `ParsedEventKind::CpiTrade`
- `ParsedEventKind::SwapTrade`
- `ParsedEventKind::CpiSwapBuy`
- `ParsedEventKind::CpiSwapSell`

należy zachować provenance z eventu źródłowego.

### Krok 2.3 — Zachować kompatybilność bridge do launchera

W `ghost-launcher/src/components/seer.rs::trade_event_to_pool_transaction(...)` na początku **nie trzeba** jeszcze przepychać całego provenance do `PoolTransaction`, jeśli downstream go nie potrzebuje do correctness.

Rekomendowany porządek:

1. najpierw dopuścić provenance do `TradeEvent`,
2. dopiero potem zdecydować, które pola mają wejść do `PoolTransaction`.

### Krok 2.4 — Jeżeli provenance ma wejść do `PoolTransaction`, zrobić to jako optional payload

Jeżeli runtime albo diagnostyka launchera mają tego potrzebować, dodać do `PoolTransaction` optional pola:
- `outer_program_id`
- `outer_instruction_index`
- `inner_group_index`
- `cpi_stack_height`

Wszystkie jako optional, z pełną kompatybilnością wstecz.

## Testy obowiązkowe

- w `binary_parser.rs`:
  - `trade_event_preserves_provenance_from_cpi_source`
- w `ghost-launcher/src/components/seer.rs`:
  - `bridge_preserves_event_ordinal`
  - nowy test: `bridge_preserves_provenance_when_enabled`

## Merge gate

Nie wolno merge’ować, jeśli:
- `TradeEvent` traci któreś z dziś używanych pól,
- bridge do `PoolTransaction` przestaje zachowywać `event_ordinal`,
- provenance znika przy konwersji do `TradeEvent`.

---

# FAZA 3 — OPIOMIAROWANIE I ROZDZIELENIE DWÓCH ARBITRAŻY

## Cel fazy

Rozdzielić w praktyce dwa problemy:

1. ix-vs-CPI structural arbitration,
2. candidate-vs-candidate arbitration po zbudowaniu `TradeEvent`.

## Pliki obowiązkowe

- `off-chain/components/seer/src/binary_parser.rs`

## Kroki implementacyjne

### Krok 3.1 — Dodać jawne reason codes w `dedup_trade_events(...)`

Każda decyzja dedupu ma mieć czytelny powód, np.:
- `drop_ix_trade_because_matching_cpi_trade`
- `drop_ix_swap_because_matching_cpi_swap`
- `keep_ix_swap_because_cpi_has_unresolved_mint`

### Krok 3.2 — Dodać jawne reason codes w `dedup_trade_candidates(...)`

Każda decyzja ma mówić:
- czy był `exact_match`
- czy był `weak_duplicate_match`
- czy kandydaci różnili się `event_ordinal`
- czy kandydaci różnili się provenance
- czy wykonano `merge_trade_optional_accounts(...)`
- który kandydat wygrał i dlaczego (`score_existing`, `score_incoming`)

### Krok 3.3 — Rozszerzyć `merge_trade_optional_accounts(...)`

Na tym etapie nie zmieniać jeszcze logiki „drop/keep”, ale:
- przepiąć nowy provenance tak, aby nie ginął przy merge,
- jeśli istnieją komplementarne pola między ix i CPI, rozważyć osobny helper `merge_trade_provenance(...)`.

### Krok 3.4 — Zrobić raport before/after per transaction sample

Dla wybranych tx z rolloutu przygotować telemetryczny raport pokazujący:
- liczba `ParsedPumpEvent` przed dedup,
- liczba po `dedup_trade_events`,
- liczba `TradeEvent` przed `dedup_trade_candidates`,
- liczba po `dedup_trade_candidates`,
- rodzaje winnerów,
- presence/absence provenance.

Nie musi to być jeszcze osobne CLI; wystarczy kontrolowany log/metryka.

## Testy obowiązkowe

- `parsed_event_dedup_keeps_higher_confidence_cpi`
- `trade_candidate_dedup_does_not_merge_different_ordinals`
- `trade_candidate_dedup_preserves_provenance_on_replacement`
- `trade_candidate_dedup_logs_reason_codes`

## Merge gate

Nie wolno merge’ować, jeśli nadal:
- nie da się powiedzieć, który poziom arbitrażu wyciął event,
- merge gubi provenance,
- różne eventy z różnym ordinalem mogą dalej zlać się bez telemetrii.

---

# FAZA 4 — DOPIERO TERAZ EWENTUALNA PRZEBUDOWA ARBITRAŻU

## Cel fazy

Zmienić logikę arbitrażu dopiero po zebraniu twardych danych z Fazy 0–3.

## Pliki obowiązkowe

- `off-chain/components/seer/src/binary_parser.rs`

## Kroki implementacyjne

### Krok 4.1 — Ustalić, który poziom naprawdę powoduje utratę informacji

Na podstawie metryk odpowiedzieć osobno:

1. Czy problem jest w `dedup_trade_events(...)`?
2. Czy problem jest w `dedup_trade_candidates(...)`?
3. Czy problem jest w samym budowaniu kandydatów przed dedupem?

Bez tej odpowiedzi nie wolno ruszać semantyki dedupu.

### Krok 4.2 — Jeżeli problemem jest ix-vs-CPI arbitration, wprowadzić merge zamiast gołego dropu

Możliwy kierunek:
- zamiast natychmiastowego dropu ix-level eventu,
- zbudować `merged trade view`, który:
  - bierze side z bardziej autorytatywnego źródła,
  - bierze mint/pool/account metadata z bogatszego źródła,
  - zachowuje provenance obu kandydatów albo לפחות winner + source notes.

### Krok 4.3 — Jeżeli problemem jest candidate-level dedup, dodać provenance do exact/weak match

W `dedup_trade_candidates(...)` warunki dopasowania powinny móc uwzględnić:
- różny `event_ordinal`
- różne `outer_program_id`
- różny `outer_instruction_index`
- różny `stack_height`

Jeśli kandydaci mają to samo `signature + side + amount`, ale różny provenance, nie wolno ich scalać bez dodatkowych reguł.

### Krok 4.4 — Rozważyć jawne źródło autorytetu per typ eventu

Dla każdego rodzaju eventu ustalić, co jest autorytatywne:

- Pump.fun `CpiTrade` vs ix `Trade`
- PumpSwap `CpiSwapBuy/Sell` vs ix `SwapTrade`
- routed buy (`DISC_PUMP_BUY_ROUTED`) vs event

To ma być zapisane jako jawna polityka, a nie rozproszone po score'ach i merge helperach.

## Testy obowiązkowe

- `same_signature_same_side_different_provenance_survives`
- `routed_buy_ix_and_cpi_merge_without_data_loss`
- `candidate_score_never_beats_distinct_provenance_without_rule`

## Merge gate

Nie wolno merge’ować, jeśli po przebudowie:
- coverage rośnie, ale semantyka side lub amount staje się niepewna,
- różne legalne eventy pod jedną sygnaturą są nadal sklejane,
- provenance przestaje wskazywać, skąd wygrał finalny event.

---

# FAZA 5 — KLASYFIKACJA WRAPPERÓW / FLOW TYPES (OPCJONALNA, PO DANYCH)

## Cel fazy

Dopiero po wdrożeniu provenance i zebraniu rozkładu outer programów zbudować flow typing.

## Pliki obowiązkowe

- `off-chain/components/seer/src/types.rs`
- `off-chain/components/seer/src/binary_parser.rs`
- opcjonalnie `ghost-launcher/src/components/seer.rs`

## Kroki implementacyjne

### Krok 5.1 — Wprowadzić neutralny enum flow/wrapper type

Nie zaczynać od `Mayhem` jako jedynej kategorii. Lepiej wprowadzić np.:
- `NativePump`
- `WrappedUnknown`
- `WrappedKnown(String)` lub enum rozszerzalny
- później ewentualnie `Mayhem`

### Krok 5.2 — Dodać mapowanie program_id -> wrapper kind za feature flag albo config

Jeżeli `MAyhSmz...` ma być rozpoznawany, to:
- nie hardcode’ować go jako prawdy architektonicznej bez telemetryki,
- najpierw dopuścić konfigurację albo stałą z wyraźnym komentarzem dowodowym.

### Krok 5.3 — Nie mieszać flow classification z dedup correctness

Flow type ma być:
- dodatkową etykietą analityczną,
- nie warunkiem istnienia trade'a.

## Testy obowiązkowe

- `wrapper_classification_preserves_trade_detection`
- `known_outer_program_maps_to_wrapper_kind`
- `unknown_outer_program_does_not_drop_trade`

## Merge gate

Nie wolno merge’ować, jeśli:
- trade detection zaczyna zależeć od wrapper classification,
- brak klasyfikacji wrappera powoduje drop poprawnego trade'a.

---

## 5. Plan cięcia na PR-y

### PR-1 — `parser-provenance-baseline`
Zakres:
- Faza 0
- bez zmian semantyki
- tylko liczniki, logi, orphan checks, reason codes

### PR-2 — `parser-execution-tree-context`
Zakres:
- Faza 1
- provenance na `ParsedPumpEvent`
- binding `group.index`
- ujednolicenie ordinałów

### PR-3 — `parser-trade-provenance-bridge`
Zakres:
- Faza 2
- provenance na `TradeEvent`
- ewentualne optional propagation do `PoolTransaction`

### PR-4 — `parser-dedup-observability-and-contract`
Zakres:
- Faza 3
- pełny rozdział obu arbitraży i ich telemetryka

### PR-5 — `parser-arbitration-refinement`
Zakres:
- Faza 4
- tylko jeśli metryki pokażą realny false-positive drop

### PR-6 — `parser-wrapper-taxonomy`
Zakres:
- Faza 5
- opcjonalny, analityczny, po danych

---

## 6. Konkretne miejsca w kodzie, które trzeba dotknąć

### `off-chain/components/seer/src/binary_parser.rs`
Najważniejsze funkcje:
- `PumpParser::parse_transaction_raw(...)`
- `PumpParser::parse_geyser_transaction(...)`
- `BinaryParser::parse_transaction_from_decoded(...)`
- `BinaryParser::parse_pump_events(...)`
- `dedup_trade_events(...)`
- `dedup_trade_candidates(...)`
- `merge_trade_optional_accounts(...)`
- budowa `TradeEvent` z `ParsedEventKind::*`

### `off-chain/components/seer/src/types.rs`
Najważniejsze typy:
- `InnerInstructionGroup`
- `TradeEvent`
- nowy `InstructionProvenance`

### `ghost-launcher/src/components/seer.rs`
Najważniejsze miejsca:
- `trade_event_to_pool_transaction(...)`
- `trade_has_forwardable_identity(...)`
- testy mostu i forwardingu

### `ghost-launcher/src/events.rs`
Tylko jeśli provenance ma być transportowane dalej jako część `PoolTransaction`.

### `ghost-core/src/event_semantics.rs`
Nie zmieniać bez twardej potrzeby. Ten plik ma zostać jakościowym envelope, nie magazynem raw execution provenance.

---

## 7. Macierz testów i walidacji

## 7.1. Testy jednostkowe parsera

Docelowy minimalny zestaw regresji:

- `inner_group_index_is_bound_to_outer_program`
- `non_pump_outer_with_inner_pump_preserves_provenance`
- `parsed_event_dedup_keeps_higher_confidence_cpi`
- `trade_candidate_dedup_does_not_merge_different_ordinals`
- `trade_candidate_dedup_does_not_merge_distinct_provenance`
- `routed_buy_ix_and_cpi_merge_without_data_loss`
- `orphan_inner_group_is_counted_not_crashed`

## 7.2. Testy mostu do launchera

W `ghost-launcher/src/components/seer.rs`:

- `bridge_preserves_event_ordinal`
- `bridge_preserves_failed_status`
- nowy: `bridge_preserves_provenance_when_present`
- nowy: `trade_with_unresolved_identity_is_not_forwarded_as_final`

## 7.3. Testy integracyjne

Użyć istniejących testów / miejsc integracji, jeśli wymagane:
- `ghost-launcher/tests/seer_shadow_ledger_bridge_tests.rs`
- `ghost-launcher/tests/snapshot_engine_integration.rs`

Celem integracji nie jest na tym etapie mierzenie PnL/runtime policy, tylko potwierdzenie:
- że parser nie gubi multi-event tx,
- że bridge nie gubi ordinału/provenance,
- że coverage telemetry jest czytelna.

## 7.4. Walidacja na realnych danych

Na wybranej próbce rolloutowej porównać:

1. liczba `ParsedPumpEvent` przed i po `dedup_trade_events`
2. liczba `TradeEvent` przed i po `dedup_trade_candidates`
3. liczba eventów z `outer_program_id != invoked_program_id`
4. liczba eventów z brakiem ordinału
5. liczba eventów z orphan group

Bez tej tabeli nie wolno mówić, że „dedup jest winny” albo „Mayhem jest winny”.

---

## 8. Kryteria sukcesu

Po wykonaniu Fazy 0–4 parser ma spełniać następujące warunki:

1. dla każdego trade'a umiemy powiedzieć:
   - z jakiego programu został wywołany,
   - czy był CPI,
   - jaki miał outer context,
   - jaki miał `event_ordinal`
2. oba poziomy arbitrażu mają jawne liczniki i reason codes
3. live path i backfill path dają spójny model provenance
4. most do launchera nie gubi `event_ordinal`, timestamp contract ani provenance
5. dalsze decyzje o „Mayhem support” są podejmowane na podstawie danych, nie intuicji

---

## 9. Czego świadomie NIE robimy w tym planie

- Nie przepisujemy całego parsera od zera.
- Nie zakładamy z góry, że Mayhem jest root cause coverage gap.
- Nie dokładamy na ślepo `is_mayhem_flow` bez surowego provenance.
- Nie zmieniamy od razu obu poziomów dedupu tylko dlatego, że ktoś ma mocny ton wypowiedzi.
- Nie przeciążamy `EventSemanticEnvelope` danymi, które nie pasują do jego obecnego kontraktu.

---

## 10. Rekomendacja końcowa — kolejność o najwyższym ROI

Jeżeli robić to pragmatycznie i bez rozlewania zmian, kolejność powinna być dokładnie taka:

1. **telemetria obu dedupów i orphan groups**
2. **execution-tree provenance przez `group.index`**
3. **propagacja provenance do `TradeEvent`**
4. **walidacja on real sample / rollout logs**
5. **dopiero potem przebudowa arbitrażu**
6. **na końcu klasyfikacja wrapperów typu Mayhem**

To jest najkrótsza droga do odpowiedzi na pytanie:

> „czy parser realnie gubi trade'y, czy tylko nie umie jeszcze dobrze opisać ich pochodzenia i arbitrażu?”

A to właśnie jest dziś najważniejsze pytanie techniczne w tym obszarze.
