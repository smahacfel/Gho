# Plan wykonawczy - lustrzany tryb full shadow simulation

## Cel

Zbudowac kanoniczny tryb `shadow`, ktory jest lustrzanym odbiciem sciezki live:

- ten sam ingest, Oracle, Gatekeeper, `SwapPlan`, Guardian i target policy,
- ten sam kontrakt eventowy i korelacyjny,
- jedyna roznica: brak realnego sendu on-chain,
- otwarcie i zamkniecie pozycji rozliczane syntetycznie na bazie tych samych truth-source'ow co live,
- minimalny wymagany artefakt: osobny JSONL z rekordem wejscia,
- docelowo: pelny lifecycle evidence z entry, exit, PnL i close reason.

## Stan zweryfikowany

1. `ghost-brain` ma dzis jedno startupowe SSOT dla execution mode: `ExecutionMode::{Live, Paper, Dual}`.
2. Upstream do momentu powstania `SwapPlan` jest wspolny; rozjazd live vs paper zaczyna sie dopiero w execution.
3. `PaperBackend` / `PaperBroker` nie jest lustrzanym live:
   - ma losowy delay,
   - ma jitter,
   - ma candidate sampling,
   - ma artificial fail/stress/slippage injection,
   - filluje po `lookup_nearest()` zamiast po kontrakcie "tu realnie bylby send / land / close".
4. `LiveBackend` ma osobny worker dla entry, ale live exit nie jest zamkniety w jednym backendzie:
   - entry siedzi w `ExecutionBackend`,
   - post-buy management idzie przez `MonitoringEngine -> SignalRouter -> Revolver / shot pipeline`.
5. `process_paper_swap_plan()` rejestruje pozycje w Guardianie, ale robi to po paperowym synthetic fillu, a nie po live-mirror settlement.
6. W repo nadal zyje starszy launcherowy `trigger.shadow_run`:
   - `TriggerEntryMode::{ShadowOnly, LiveAndShadow}`,
   - `trigger.shadow_run.output_path`,
   - rollout profile waliduje pare `execution_mode <-> entry_mode`.
7. Event/reporting stack zna dzis glownie lane `Live`, `Paper`, `Single`; nie ma pierwszoklasowego `Lane::Shadow`.
8. `DecisionLogger` ma juz pola `shadow_*`, ale sa w praktyce niewykorzystane jako realny handoff contract.

## Niezmienniki architektoniczne

1. **Nie budujemy drugiego systemu.** Shadow ma byc wpiety w istniejacy pipeline, nie obok niego.
2. **Nie ruszamy upstream decision path.** Seer -> Oracle -> Gatekeeper -> `SwapPlan` zostaje wspolny dla live i shadow.
3. **`ExecutionMode` pozostaje startupowym SSOT.** Rozszerzamy enum jawnie; nie wprowadzamy nowego boolean soup.
4. **`ExecutionBackend` pozostaje glownym execution handoff.** Jesli cos trzeba wyniesc wyzej, robimy to jako wspolny helper/adapter, a nie boczna sciezke.
5. **Nie przepinamy semantyki `Paper` ani `Dual` po cichu.** Legacy znaczenie pozostaje stabilne do czasu jawnej migracji.
6. **Shadow nie moze dziedziczyc paperowych artefaktow.** Zero RNG delay, zero fail injection, zero synthetic mark price jako truth.
7. **Jeden truth contract dla ceny.** Shadow entry/exit ma korzystac z tych samych lub jawnie wspoldzielonych truth-source'ow, ktore sa uznawane w live, bez dokladania trzeciej "prawdy".
8. **Live contracts pozostaja nienaruszone funkcjonalnie.** Jesli refaktorujemy live, to tylko przez wydzielenie wspolnych komponentow bez zmiany behavioru.

## Docelowy model

### 1. Jeden wspolny "execution preparation" przed rozjazdem

Przed ostatnim krokiem "wyslij tx" live i shadow maja dzielic wspolny, jawny obiekt przygotowania, np.:

- `ExecutionAttemptContext`,
- `EntryIntentContext`,
- albo podobny typ z:
  - `candidate_id`,
  - `order_id`,
  - `quote_ref`,
  - `quote_price_ref`,
  - `slot`,
  - `submit_time_ms`,
  - `timing_source`,
  - `predicted_slot` / batch metadata, jesli dotyczy,
  - `position_epoch`.

To jest granica, na ktorej:

- live idzie dalej do RPC/Jito/send/confirm,
- shadow idzie do syntetycznego settlementu,
- ale oba korzystaja z tej samej przygotowanej prawdy.

### 2. `ShadowBackend` jako pierwszoklasowy backend

Docelowy backend shadow:

- implementuje `ExecutionBackend`,
- nie uzywa `PaperBrokerConfig`,
- nie jest aliasem `PaperBackend`,
- ma wlasny deterministyczny model czasu i settlementu,
- trzyma wlasny virtual position state,
- emituje events na osobnym lane,
- loguje JSONL wymagany przez biznes.

### 3. Shadow ma lustrzanie odtworzyc nie tylko BUY, ale i pozycje

Full shadow nie konczy sie na `submit_entry()` i wpisie do JSONL. Musi objac:

- `PositionOpened`,
- monitoring pozycji,
- komendy Guardian/AEM,
- partial exits / panic / time-stop,
- `ExitSubmitted`,
- `ExitFilled`,
- `PositionClosed`,
- audytowalny PnL.

### 4. Legacy launcherowy `shadow_run` staje sie compatibility surface

Stary compare-only simulator w `ghost-launcher` nie moze pozostac konkurencyjnym execution truth.

Do wyboru operacyjnego sa tylko dwa sensowne stany:

1. deleguje do nowego kanonicznego shadow engine,
2. zostaje wyraznie oznaczony jako legacy compare-only i nie bierze udzialu w rollout proof.

## Doprecyzowania po review

1. Kontrakt korelacyjny musi zostac zamrozony szerzej niz samo `candidate_id` / `order_id`.
   Minimum SSOT dla dalszych PR:
   - `candidate_id`,
   - `order_id`,
   - `position_id`,
   - `lane`,
   - `run_id`,
   - `position_epoch`,
   - jawne reguly korelacji miedzy buy logiem, execution events, Guardianem i reportingiem.
2. Wspolny obiekt pre-send (`ExecutionAttemptContext` / analog) musi od pierwszej wersji niesc jawne pola czasu i prawdy:
   - `submit_time_ms`,
   - `quote_ts_ms`,
   - `quote_slot`,
   - `timing_source`,
   - `price_source`,
   - `stale_policy`.
   Celem jest niedopuszczenie do sytuacji, w ktorej shadow zacznie opierac sie na pol-domyslach, a live na innym kontrakcie czasu/ceny.
3. Najwyzszy risk wdrozeniowy siedzi w parity post-buy, nie w samym BUY entry. Zanim PR-4 wejdzie w pelny runtime pozycji, trzeba miec jawnie zamrozony lane-aware kontrakt routera/sinka dla `PositionOpened -> ... -> PositionClosed`.
4. Legacy `trigger.shadow_run` powinien dostac jawny warning/deprecation surface juz we wczesnej fazie migracji, zeby operator nie traktowal compare-only shadow-run jako kanonicznego proof path.
5. `Dual` pozostaje nienaruszony znaczeniowo w calym pierwszym rolloutcie. Kazde docelowe `live + shadow` wymaga osobnego, jawnego trybu i osobnego PR.

## Program PR

## PR-1 - freeze kontraktow i wydzielenie wspolnego execution-prep

### Twardy cel

Przeniesc punkt rozjazdu live vs shadow do jednego, jawnego miejsca bez zmiany zachowania live.

### Zakres

1. Zmapowac i zamrozic kontrakty:
   - `ExecutionMode`,
   - `ExecutionBackend`,
   - `candidate_id`,
   - `order_id`,
   - `position_id`,
   - `lane`,
   - `run_id`,
   - `position_epoch`,
   - event timeline,
   - pairing launcherowego `execution_mode <-> entry_mode`,
   - jawne zasady korelacji miedzy buy logiem, execution events, Guardianem i reportingiem.
2. Wyciagnac ze sciezek live/Jito wspolne przygotowanie:
   - `resolve_quote_ref_with_provider(...)`,
   - kalkulacje `candidate_id`,
   - slot/timing metadata,
   - pola czasu/prawdy: `submit_time_ms`, `quote_ts_ms`, `quote_slot`, `timing_source`, `price_source`, `stale_policy`,
   - wspolny envelope dla entry submit/fill.
3. Ustalic jeden nazwany punkt "last pre-send boundary".
4. Dolozyc testy kontraktowe pilnujace, ze:
   - nie pojawi sie nowy branching typu `dry_run`,
   - live nie dostanie bocznej logiki shadow,
   - upstream pipeline nadal rozjezdza sie tylko przez `match execution_mode`,
   - kontrakt identity/timing nie dryfuje miedzy live i shadow.

### Glowny blast radius

- `ghost-brain/src/pipeline/execution.rs`
- `ghost-brain/src/pipeline/jito_processor.rs`
- `ghost-brain/src/execution/live.rs`
- `ghost-brain/tests/execution_contract_audit_tests.rs`

### Merge gate

- zero zmiany zachowania live/paper,
- wspolny helper/preparation layer istnieje i jest wykorzystywany przez live,
- kontrakt identity/timing/pre-send boundary jest jawnie zamrozony i przetestowany,
- kontraktowe testy execution nadal przechodza.

## PR-2 - first-class `ExecutionMode::Shadow`, `Lane::Shadow` i surface telemetry

### Twardy cel

Dodac shadow jako pierwszoklasowy tryb runtime bez przepinania po cichu `Paper`.

### Zakres

1. Dodac `ExecutionMode::Shadow`.
2. Dodac `Lane::Shadow` do:
   - schema,
   - emittera,
   - validatora,
   - comparison/reportingu.
3. Dodac `execution.shadow` jako osobny config block, np. z polami:
   - `tx_build_compensation_ms` (domyslnie 250),
   - `max_quote_age_ms`,
   - `entry_log_path`,
   - opcjonalnie `lifecycle_log_path`,
   - `timing_model`,
   - `stale_policy`.
4. Wprowadzic pierwszy kanoniczny JSONL dla wejsc:
   - `timestamp`,
   - `pool_id`,
   - `mint_id`,
   - `entry_price`,
   - `slot`,
   - `timestamp_ms`,
   - opcjonalnie: `candidate_id`, `order_id`, `quote_id`, `timing_source`.
5. Podlaczyc `DecisionLogger.shadow_*` do realnego handoff:
   - `shadow_trigger_eligible`,
   - `shadow_entry_mode`,
   - `shadow_execution_outcome`.
6. Nie zmieniac jeszcze znaczenia `Paper` / `Dual`.
7. Dolozyc jawny warning/deprecation surface dla legacy `trigger.shadow_run`, nawet jesli pelna delegacja lub odciecie proof-path nastapi dopiero w PR-6.

### Glowny blast radius

- `ghost-brain/src/execution/backend.rs`
- `ghost-brain/src/config/e2e_config.rs`
- `ghost-brain/src/pipeline/builder.rs`
- `ghost-brain/src/events/{schema,emitter,comparison,validator}.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/config.rs`

### Merge gate

- runtime umie uruchomic `execution_mode=shadow`,
- zdarzenia i raporty rozumieja `Lane::Shadow`,
- legacy `trigger.shadow_run` jest juz jawnie oznaczony jako compare-only / legacy surface,
- stare profile `paper` i `dual` nie zmieniaja znaczenia.

## PR-3 - `ShadowBackend` dla entry i kanoniczny synthetic settlement

### Twardy cel

Zrobic BUY mirror live do momentu "wyslalibysmy tx", po czym rozliczyc wejscie syntetycznie, ale na tej samej przygotowanej prawdzie.

### Zakres

1. Dodac `ghost-brain/src/execution/shadow.rs`.
2. `ShadowBackend` ma:
   - implementowac `ExecutionBackend`,
   - przyjmowac wspolny `ExecutionAttemptContext`,
   - schedulowac settlement wedlug jawnego modelu czasu.
3. Model czasu ma byc jawny i telemetryczny:
   - standard path,
   - Jito/batch path,
   - fallback tylko wtedy, gdy live-specific timing metadata nie ma,
   - minimalny wymagany compensation baseline: `+250ms`.
4. Settlement entry ma korzystac z tego samego truth resolvera co live preparation:
   - zero RNG,
   - zero paper jitter,
   - zero synthetic fail injection,
   - zero candidate sampling.
5. Po successful settle:
   - emit `EntryFilled`,
   - emit `PositionOpened`,
   - zapisz rekord do `shadow_entries.jsonl`,
   - otworz pozycje shadow w runtime state.
6. Paper backend zostaje legacy; shadow niczego z niego nie dziedziczy poza ewentualnymi neutralnymi helperami.

### Glowny blast radius

- `ghost-brain/src/execution/shadow.rs` (nowy)
- `ghost-brain/src/execution/mod.rs`
- `ghost-brain/src/pipeline/execution.rs`
- `ghost-brain/src/execution/live.rs`
- `ghost-brain/src/quotes/provider.rs`

### Merge gate

- dla tego samego `SwapPlan` shadow przechodzi przez ten sam pre-send preparation co live,
- `shadow_entries.jsonl` ma wymagane pola,
- brak zaleznosci shadow od `PaperBrokerConfig`,
- timing source jest jawny w logach/testach.

## PR-4 - shadow position runtime i parity z Guardian/AEM/Revolver

### Twardy cel

Doprowadzic shadow od `PositionOpened` do `PositionClosed` tym samym systemem zarzadzania pozycja, a nie paperowym obejsciem.

To jest najwyzszy risk wdrozeniowy calego programu i wymaga najpierw zamrozenia kontraktu routera/sinka pozycji.

### Zakres

1. Zamrozic lane-aware kontrakt routera/sinka dla post-buy management (`PositionOpened -> ... -> PositionClosed`) zanim shadow dostanie pelny runtime pozycji.
2. Wydzielic lane-aware adapter/router dla post-buy management:
   - live dalej steruje realnym `Revolver`,
   - shadow dostaje wlasny router, ale z ta sama semantyka komend.
3. `MonitoringEngine` ma pozostac wspolny, ale nie moze zakladac, ze wszystkie pozycje ida do jednego live sinka.
4. Dolozyc `ShadowPositionBook` / `VirtualMagazine`:
   - ten sam model targetow i frakcji,
   - ten sam epoch/anti-zombie contract,
   - ten sam command flow z AEM/Guardian.
5. Zamknac integracje z:
   - `SignalRouter`,
   - `ManagementDecision/Outcome`,
   - `ControlCommandIssued/Applied`,
   - `ExitSubmitted`.
6. `paper_lifecycle.rs` przestaje byc rolloutowym SSOT dla symulowanego prowadzenia pozycji:
   - albo zostaje zredukowany do legacy/test helpera,
   - albo jego przydatne fragmenty sa absorbowane do shadow runtime.

### Glowny blast radius

- `ghost-brain/src/guardian/post_buy/{engine,integration,signals}.rs`
- `ghost-brain/src/execution/paper_lifecycle.rs`
- `off-chain/components/trigger/src/revolver*.rs`
- `ghost-brain/src/pipeline/builder.rs`

### Merge gate

- shadow pozycja reaguje na Guardian/AEM tak samo semantycznie jak live,
- partial exits i force exits sa mozliwe bez side effectu on-chain,
- lane-aware router/sink contract jest jawny i regresyjnie zabezpieczony,
- live router nie ma regresji.

## PR-5 - exit truth, PnL ledger i artefakty dowodowe

### Twardy cel

Domknac shadow close po cenie zgodnej z live truth contract i sprawic, ze wynik pozycji jest policzalny z artefaktow.

### Zakres

1. Wprowadzic wspolny `PriceTruthResolver` dla shadow exitow:
   - zgodny z aktualnym live sell truth contract,
   - bez synthetic mark price,
   - bez niemej zamiany na inna zrodla prawdy.
2. Ustalic jawna polityke dla braku probki ceny przy exit:
   - explicit failure / stale / backfill-required,
   - ale nigdy cichy sukces-shaped fallback.
3. Uzupelnic `PositionClosedPayload` dla shadow:
   - `entry_value_sol`,
   - `exit_value_sol`,
   - `gross_pnl_sol`,
   - `net_pnl_sol`,
   - `estimated_costs_sol`,
   - `reason`.
4. Dodac bogatszy artefakt lifecycle/PnL obok minimalnego `shadow_entries.jsonl`.
5. Zaktualizowac raporty i porownania tak, aby `Lane::Shadow` byl pierwszoklasowy, a `Lane::Paper` pozostawal tylko legacy.

### Glowny blast radius

- `ghost-brain/src/events/schema.rs`
- `ghost-brain/src/guardian/post_buy/engine.rs`
- `off-chain/components/trigger/src/entry_price_extractor.rs`
- `scripts/shadow_run_report.py`
- `ghost-brain/src/events/comparison.rs`

### Merge gate

- kazda shadow pozycja ma policzalny entry/exit/PnL,
- close reason jest jawny,
- raport potrafi rozroznic legacy `paper` od kanonicznego `shadow`.

## PR-6 - rollout migration, launcher compatibility i democja legacy paper

### Twardy cel

Operacyjnie przeniesc system z "paper + launcher shadow_run + dual confusion" do jednego kanonicznego shadow rollout path.

### Zakres

1. Zmienic launcherowe parowanie execution profile:
   - `ExecutionMode::Shadow` <-> `TriggerEntryMode::ShadowOnly`,
   - bez ruszania semantyki `Live`,
   - bez cichego przepinania `Dual`.
2. Ustalic fate `trigger.shadow_run`:
   - delegacja do nowego kanonicznego shadow,
   - albo wyrazne oznaczenie legacy compare-only i odciecie od proof path.
3. Dodac nowe rollout profile oparte o `execution_mode=shadow`.
4. Zaktualizowac:
   - `configs/rollout/*.toml`,
   - preflight,
   - runbook,
   - docs,
   - ewentualne warningi/deprecations dla `paper` w rolloutach produkcyjnych.
5. Zostawic `ExecutionMode::Paper` tylko jako legacy integration/test surface do czasu swiadomego usuniecia.

### Glowny blast radius

- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/components/trigger/shadow_run.rs`
- `configs/rollout/*.toml`
- `scripts/ghost_production_preflight.sh`
- `docs/RUNBOOK_*`

### Merge gate

- operator potrafi odpalic standalone shadow bez realnego sendu,
- rollout config nie miesza juz kanonicznego shadow z legacy compare-only shadow-run,
- stare paper profile nie udaja live mirror.

## Zakres opcjonalny po potwierdzeniu

Jesli pierwsza fala ma od razu obslugiwac rownolegly `live + shadow` w jednej sesji, to proponuje osobny, swiadomy PR po PR-6:

### PR-7 (opcjonalny) - `live_plus_shadow_compare`

1. Nie repurpose'owac dzisiejszego `Dual` po cichu.
2. Dodac nowy, jawny tryb wspolbieznosci, np.:
   - `ExecutionMode::LiveShadow`,
   - albo `ExecutionMode::DualShadow`.
3. Utrzymac rozlaczne lane i rozlaczne order/position identity.
4. Shared writer/run_id moze zostac, ale bez semantycznego klamstwa "paper == shadow".

## Ryzyka i jak je kontrolowac

### 1. Ryzyko: semantyczny chaos `Paper` vs `Shadow`

Mitigacja:

- nie reuse `Lane::Paper` dla nowego shadow,
- nie przepinac `Dual` bez osobnego PR,
- w raportach traktowac `Paper` jako legacy lane.

### 2. Ryzyko: shadow bedzie mial inny truth source niz live

Mitigacja:

- wydzielic wspolny `ExecutionAttemptContext`,
- wydzielic jawny `PriceTruthResolver`,
- telemetrycznie oznaczac `timing_source` i `price_source`.

### 3. Ryzyko: shadow stanie sie drugim systemem post-buy

Mitigacja:

- reuse `MonitoringEngine`,
- reuse command semantics,
- wprowadzic adaptery/routery zamiast drugiego Guardiana,
- przed pelnym PR-4 zamrozic lane-aware router/sink contract.

### 4. Ryzyko: legacy launcher `shadow_run` dalej bedzie wygladal jak kanoniczna prawda

Mitigacja:

- wczesny warning/deprecation surface juz przed finalna migracja,
- delegacja albo deprecacja,
- rollout docs musza to nazwac wprost,
- proof path ma czytac tylko nowy shadow lane.

### 5. Ryzyko: niedomrozony kontrakt identity / timing rozjedzie korelacje live vs shadow

Mitigacja:

- zamrozic `candidate_id`, `order_id`, `position_id`, `lane`, `run_id`, `position_epoch`,
- jawnie niesc `submit_time_ms`, `quote_ts_ms`, `quote_slot`, `timing_source`, `price_source`, `stale_policy`,
- dolozyc testy korelacji miedzy buy logiem, execution events, Guardianem i reportingiem.

## Walidacja i test matrix

1. **Kontraktowe testy execution**
   - rozszerzenie `ghost-brain/tests/execution_contract_audit_tests.rs`,
   - brak regresji w startupowym dispatchu,
   - poprawne lane identity,
   - stabilna korelacja `candidate_id/order_id/position_id/lane/run_id/position_epoch`,
   - brak driftu w kontrakcie timing/price metadata.
2. **Testy backendu shadow**
   - deterministyczny timing,
   - brak paper RNG/failure injection,
   - poprawny JSONL schema,
   - poprawna stale policy.
3. **Testy event timeline**
   - `Candidate -> EntrySubmitted -> EntryFilled -> PositionOpened -> ... -> PositionClosed`,
   - bez validator violations dla `Lane::Shadow`.
4. **Testy launcher/config compatibility**
   - poprawne parowanie `execution_mode` i `entry_mode`,
   - brak przypadkowego reuse legacy `shadow_run`,
   - jawny legacy warning dla compare-only `trigger.shadow_run`.
5. **Testy replay/reporting**
   - raport rozumie `Lane::Shadow`,
   - nie myli shadow z paper,
   - potrafi policzyc lifecycle/PnL z artefaktow.

## Proponowana kolejnosc wdrazania

1. PR-1 freeze + wspolne preparation
2. PR-2 first-class mode/lane/config/log surface
3. PR-3 shadow entry backend
4. PR-4 shadow position runtime
5. PR-5 exit truth + PnL/reporting
6. PR-6 rollout migration i democja legacy paper
7. PR-7 tylko jesli chcesz od razu rownolegly `live + shadow`

## Domyslne zalozenie tego planu

Pierwsza fala dowozi **standalone `ExecutionMode::Shadow`** jako kanoniczny full shadow mirror bez wysylki tx.

Rownolegly tryb `live + shadow` traktuje jako **osobne, swiadome rozszerzenie**, a nie warunek konieczny do postawienia pierwszej poprawnej wersji.
