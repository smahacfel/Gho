# ADR-0146: Przywrócenie konsumpcji Gatekeeper decision-time feature context w selector training

Data: 2026-06-06

Status: Accepted jako naprawa offline selector/training pipeline oraz jako
guard powierzchni metryk w decision logs.

## Kontekst

W trakcie prac nad feature-rich selector/R2 pipeline oczekiwaliśmy, że
`selector_training_view_v1` będzie zawierał nie tylko flow features i R2 label,
ale również wartościowy kontekst decyzyjny, który Gatekeeper już liczył w
momencie decyzji.

Chodziło między innymi o pola:

```text
bonding_progress_pct
curve_data_known
current_market_cap_sol
price_change_ratio
hhi
top3_volume_pct
buy_ratio
sell_buy_ratio
dev_has_sold
funding_source_diagnostics
vectors_*
```

Te metryki są istotne, bo opisują stan obserwowany przez Gatekeepera w oknie
decyzyjnym. Nie są to dane post-label, nie są to dane z przyszłości i nie są to
R2 market-path labels. Są to decision-time raw metrics, czyli kontekst dostępny
w momencie, w którym Gatekeeper podejmował decyzję.

Praktyczny objaw regresji był prosty: downstream selector artifacts i P3F/P3G
diagnostics zachowywały się tak, jakby nie miały:

```text
bonding progress
current market cap
price dynamics
Gatekeeper concentration metrics
Gatekeeper vector summaries
```

Jednocześnie historyczne oraz aktualne `gatekeeper_v2_decisions.jsonl`
pokazywały, że te wartości były i nadal są emitowane przez decision logs.

To rozróżnienie jest kluczowe:

```text
problemem nie był brak runtime logging;
problemem był brak konsumpcji logowanych metryk przez selector training.
```

## Decyzja

Naprawiamy offline dataset/training consumption, nie runtime.

Uznany przepływ po naprawie:

```text
gatekeeper_v2_decisions.jsonl
-> gatekeeper_feature_context_v1.jsonl
-> selector_training_view_v1
-> P3F/P3G flow vs gk vs combined diagnostics
```

Nie zmieniamy:

```text
Rust
Gatekeeper policy
DecisionLogger runtime emission
thresholds
runtime config
FSC runtime
Helius
execution
restore path
R2 labeler semantics
```

## Co było przedmiotem prac

Prace składały się z dwóch osobnych zmian:

1. PR-P3H: adapter Gatekeeper decision-time feature context.
2. PR-GK-FEATURE-LOG-GUARD: offline guard sprawdzający, czy decision logs nadal
   emitują wymaganą powierzchnię metryk.

Te dwie zmiany rozwiązują dwa różne problemy:

```text
PR-P3H:
  decision logs już mają metryki;
  selector training musi je konsumować.

PR-GK-FEATURE-LOG-GUARD:
  przyszły run może przestać emitować metryki;
  guard ma to wykryć wcześnie.
```

## Dowód, że runtime emission nie był aktywnym problemem

Przed implementacją adaptera wykonano read-only audit aktualnego r7 source
scope:

```text
source_scope = shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag
```

Snapshot audytu:

```text
rows        = 18044
schema      = 25
legacy_live = 9022
v25_shadow  = 9022

full coverage:
  bonding_progress_pct
  curve_data_known
  current_market_cap_sol
  price_change_ratio
```

To oznaczało, że wymagane curve/market metrics były obecne w aktualnych
decision logs. Naprawianie Rust/Gatekeeper/DecisionLogger jako pierwszy krok
byłoby naprawą niewłaściwej warstwy.

Po dodaniu guarda uruchomiono go na realnym r7 `v25_shadow`. Ponieważ r7 source
scope był nadal rosnący, liczby są snapshotem z momentu uruchomienia guarda:

```text
decision_plane = v25_shadow
decision_rows  = 13514
schema         = 25

bonding_progress_pct      = 100%
curve_data_known          = 100%
current_market_cap_sol    = 100%
price_change_ratio        = 100%
observation_duration_ms   = 100%
curve_wait_ms             = 100%
curve_wait_elapsed_ms     = 100%
total_tx_evaluated        = 100%
unique_signers_evaluated  = 100%
buy_count                 = 100%
sell_buy_ratio            = 89.57%
buy_ratio                 = 79.02%
hhi                       = 69.25%
top3_volume_pct           = 69.25%
```

Wniosek:

```text
runtime aktualnie emituje wymagane curve/market metrics;
regresja była w offline selector/training consumption.
```

## Root cause

Root cause: brak formalnego adaptera pomiędzy durable Gatekeeper decision logs
a `selector_training_view_v1`.

Gatekeeper V2/V2.5 decision logs zawierały metryki, ale Phase 3 selector
pipeline nie miał oficjalnej ścieżki, która:

- czyta `gatekeeper_v2_decisions.jsonl`;
- wybiera właściwy decision row dla kandydata;
- dołącza metryki tylko do istniejących candidate rows;
- zachowuje `candidate_universe_v1` jako denominator SSOT;
- fail-closed obsługuje ambiguous joins;
- sprawdza cutoff/leakage;
- wyklucza verdict/pass/reason/threshold fields;
- przepuszcza dane przez oficjalny Phase 3 orchestrator;
- udostępnia P3F/P3G porównanie `flow`, `gk`, `combined`.

Brakujące ogniwo nie było:

```text
runtime -> decision log
```

Brakujące ogniwo było:

```text
decision log -> selector feature context -> training view
```

## Dlaczego nie zrobiono bezpośredniego joinu do training view

Bezpośrednie czytanie decision logs przez `selector_training_view_v1` byłoby
ryzykowne, bo mogłoby:

- przypadkowo użyć decision logs jako denominatora;
- stworzyć candidate rows, których nie było w `candidate_universe_v1`;
- wpuścić do modelu stare verdicts/reasons/pass flags;
- wpuścić threshold/config fields jako predykcyjny sygnał;
- użyć danych po cutoffie;
- stworzyć boczną Phase 3 ścieżkę poza oficjalnym orchestratorem;
- rozmyć różnicę pomiędzy provenance a model features.

Dlatego dodano osobny artefakt:

```text
gatekeeper_feature_context_v1.jsonl
```

Ten artefakt jest audytowalnym staging layer pomiędzy decision logs a training
view.

## PR-P3H: adapter Gatekeeper decision-time feature context

Commit:

```text
c0d3d26 PR-P3H: add Gatekeeper decision-time feature context adapter
```

Pliki:

```text
scripts/build_selector_gatekeeper_feature_context.py
scripts/build_selector_training_view.py
scripts/build_selector_phase3_r2only.py
scripts/build_selector_r2only_feature_contribution.py
scripts/build_selector_r2only_model_candidate.py
scripts/test_selector_pipeline.py
```

### Nowy skrypt adaptera

Dodano:

```text
scripts/build_selector_gatekeeper_feature_context.py
```

CLI:

```bash
python3 scripts/build_selector_gatekeeper_feature_context.py \
  --root /root/Gho \
  --scope <selector_scope> \
  --source-scope <source_scope> \
  --decision-plane v25_shadow \
  --observation-profile observation_8s_10s \
  --json
```

Inputy:

```text
datasets/selector/<scope>/candidate_universe_v1.jsonl
logs/rollout/<source_scope>/decisions/**/gatekeeper_v2_decisions.jsonl
```

Opcjonalne inputy dla cutoff/provenance:

```text
datasets/selector/<scope>/selector_training_view_v1.jsonl
datasets/selector/<scope>/r2_market_paths_v1.jsonl
```

Outputy:

```text
datasets/selector/<scope>/gatekeeper_feature_context_v1.jsonl
reports/selector/<scope>/gatekeeper_feature_context_manifest_v1.json
```

Skrypt jest offline-only. Nie zmienia runtime.

### Denominator contract

`candidate_universe_v1.jsonl` pozostaje denominator SSOT.

Decision logs są wyłącznie context source.

Adapter ma zawsze raportować:

```text
denominator_created_rows = 0
```

Jeżeli decision log zawiera pool/mint, którego nie ma w candidate universe,
nie powstaje nowy kandydat. Taki decision row jest pomijany jako unmatched.

### Join contract

Join priority:

```text
1. exact join_key, jeżeli candidate_universe ma join_key
2. candidate_id
3. pool_id + base_mint
4. base_mint + nearest first_seen_ts_ms / birth_ts_ms within +/-2000 ms
```

Jeżeli decision row matchuje wiele candidates, wynik jest fail-closed:

```text
gk_context_status = ambiguous_join
```

Taki kontekst nie jest model-usable.

### Primary context selection

Dla jednego candidate może istnieć wiele decision rows:

```text
legacy_live
v25_shadow
kolejne ewaluacje
różne observation windows
```

Wybór primary context jest deterministyczny:

```text
1. decision_plane zgodny z CLI, domyślnie v25_shadow
2. gk_context_status == ok
3. observation_profile zgodny z CLI
4. największe observation_duration_ms w profilu
5. najwcześniejsze observation_end_ts_ms przy remisie
```

Observation profiles:

```text
observation_8s_10s:
  6000 <= observation_duration_ms <= 12000

observation_60s:
  45000 <= observation_duration_ms <= 75000

other:
  wszystko poza powyższymi
```

### Cutoff / leakage contract

Każdy context row zawiera:

```text
gk_feature_context_ts_ms
gk_observation_start_ts_ms
gk_observation_end_ts_ms
gk_observation_duration_ms
gk_observation_profile
gk_cutoff_status
```

Timestamp contextu jest wyznaczany w kolejności:

```text
1. observation_end_ts_ms
2. observation_start_ts_ms + observation_duration_ms
3. first_seen_ts_ms + observation_duration_ms
4. timestamp z loga
```

Cutoff jest wyznaczany w kolejności:

```text
1. training-view feature_cutoff_ts_ms / decision_ts_ms
2. candidate-universe feature_cutoff_ts_ms / decision_ts_ms
3. r2_market_paths_v1 r2_path_start_ts_ms / feature_cutoff_ts_ms / decision_ts_ms
4. unverified
```

`gk_*` może być użyte w model scripts tylko wtedy, gdy:

```text
gk_context_status == ok
gk_cutoff_status in {ok, same_decision_time}
```

Jeżeli cutoff jest `unverified` albo context timestamp jest po cutoffie,
wartości są traktowane jako missing, nie jako zero.

### Allowed feature surface

Adapter importuje wyłącznie raw decision-time metrics z prefixem `gk_`.

Przykłady:

```text
gk_bonding_progress_pct
gk_curve_data_known
gk_current_market_cap_sol
gk_price_change_ratio
gk_total_tx_evaluated
gk_unique_signers_evaluated
gk_buy_count
gk_buy_ratio
gk_sell_buy_ratio
gk_hhi
gk_top3_volume_pct
gk_dev_has_sold
gk_fee_topology_diversity_index
gk_spend_fraction_divergence
gk_demand_elasticity_score
gk_signer_cross_pool_velocity
```

Z `funding_source_diagnostics` adapter wyciąga statusowane evidence:

```text
gk_fsc_buyer_sample_count
gk_fsc_known_source_count
gk_fsc_unknown_buyer_count
gk_fsc_known_source_rate
gk_fsc_unknown_buyer_rate
```

Nie interpretujemy unknown FSC jako safe.

Dla `vectors_*` nie wpuszczamy raw arrays do modelu. Emitowane są summary:

```text
gk_vector_event_count
gk_vector_price_first
gk_vector_price_last
gk_vector_price_return
gk_vector_price_max
gk_vector_price_min
gk_vector_price_drawdown
gk_vector_sol_sum
gk_vector_sol_max
gk_vector_interval_median
gk_vector_interval_min
gk_vector_interval_max
```

### Forbidden fields

Adapter ma hard denylist dla pól decyzyjnych starego Gatekeepera.

Zakazane jako model/context columns:

```text
decision_verdict_buy
verdict_type
decision_reason
core_pass
core1_passed
core2_passed
core3_passed
phase2_passed
phase3_passed
phase4_passed
phase5_passed
phase6_passed
phases_passed
soft_score
soft_points
legacy_soft_points
sybil_soft_points
total_soft_points
alpha_pass
prosperity_pass
prosperity_actionable
prosperity_matched_branches
matched
branches
min_* threshold fields
max_* threshold fields
gatekeeper_version
mode
```

Powód: selector nie może uczyć się starego verdictu, reason chain, phase pass
ani progów Gatekeepera jako feature modelu. To byłoby uczenie modelu decyzji
Gatekeepera, a nie rynku.

Manifest PASS wymaga:

```text
forbidden_fields_detected = []
```

### Provenance nie jest model feature

Pola poniżej są provenance/filter fields:

```text
gk_log_schema_version
gk_decision_plane
gk_observation_profile
gk_context_status
gk_cutoff_status
```

Nie mogą wejść do P3F/P3G feature set `gk`.

Powód: model nie powinien uczyć się różnic pomiędzy `v25_shadow` a
`legacy_live`, schema version albo statusem cutoffu jako predykcyjnego sygnału.
To są pola do filtrowania i audytu, nie do scoringu.

### Oficjalny Phase 3 path

Zmieniono:

```text
scripts/build_selector_phase3_r2only.py
```

Ten skrypt jest oficjalnym wejściem Phase 3, bo:

- czyta Phase 2 manifest;
- ustala inputy;
- wywołuje `training.build_training_view(...)`;
- zapisuje `selector_training_view_manifest_v1.json`;
- zapisuje `phase3_r2only_manifest_v1.json`.

Dodano minimalnie:

```text
--gatekeeper-feature-context
```

oraz pass-through do:

```text
training.build_training_view(...)
```

Manifesty dostały:

```text
input_provenance.gatekeeper_feature_context_v1
gatekeeper_feature_context_enabled = true/false
```

Nie zrobiono wrappera. Wrapper stworzyłby drugi sposób budowy Phase 3 i
ryzyko kolejnego driftu.

### P3F/P3G

Rozszerzono:

```text
scripts/build_selector_r2only_feature_contribution.py
scripts/build_selector_r2only_model_candidate.py
```

Dodano:

```text
--feature-set flow
--feature-set gk
--feature-set combined
```

Definicje:

```text
flow:
  net_quote_in_15s
  net_quote_in_30s
  trade_rate
  unique_buyers
  sell_share
  top1_wallet_share
  buyer_hhi

gk:
  dozwolone zmienne raw gk_* metrics z manifestu feature context,
  bez provenance/filter fields

combined:
  flow + gk
```

Default pozostał:

```text
--feature-set flow
```

Dzięki temu stare raporty nie zmieniają zachowania.

P3G może teraz porównać:

```text
flow-only
gk-context-only
combined
Gatekeeper accept context
```

Nadal jest to diagnostic-only. Nie jest to claim produkcyjny, tuning
Gatekeepera ani promotion.

## PR-GK-FEATURE-LOG-GUARD

Commit:

```text
e9fdb22 PR-GK-FEATURE-LOG-GUARD: add Gatekeeper feature surface guard
```

Pliki:

```text
scripts/guard_gatekeeper_decision_feature_surface.py
scripts/test_selector_pipeline.py
```

Guard rozwiązuje inny, przyszły problem:

```text
co jeśli runtime w następnym runie przestanie emitować wymagane metryki?
```

Guard nie naprawia runtime. Guard wykrywa brak emission surface.

CLI:

```bash
python3 scripts/guard_gatekeeper_decision_feature_surface.py \
  --source-scope shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag \
  --root /root/Gho \
  --decision-plane v25_shadow \
  --min-rows 100 \
  --json
```

Skanuje:

```text
logs/rollout/<source_scope>/decisions/**/gatekeeper_v2_decisions.jsonl
```

Zapisuje:

```text
reports/selector/<source_scope>/gatekeeper_decision_feature_surface_guard_v1.json
```

Hard-gated curve/market/time fields:

```text
bonding_progress_pct
curve_data_known
current_market_cap_sol
price_change_ratio
observation_duration_ms
curve_wait_ms
curve_wait_elapsed_ms
```

Raportowane flow/concentration fields:

```text
total_tx_evaluated
unique_signers_evaluated
buy_count
buy_ratio
hhi
top3_volume_pct
sell_buy_ratio
```

Default PASS gates:

```text
decision_rows >= min_rows
critical curve/market/time metrics present_rate >= 0.95
hhi/top3_volume_pct present_rate >= 0.60
```

`hhi/top3_volume_pct` mają osobny próg, bo aktualny r7 pokazał około 69-71%
coverage dla concentration metrics przy 100% coverage dla curve/market metrics.
Gdyby guard wymagał 0.95 dla koncentracji, dawałby fałszywe NO-GO dla runa,
który nadal jest poprawny dla curve/market feature context.

Fail statuses:

```text
FAIL_NO_DECISION_LOGS
FAIL_NO_REQUIRED_CURVE_METRICS
FAIL_LOW_CURVE_METRIC_COVERAGE
FAIL_LOW_CONCENTRATION_COVERAGE
```

## Walidacja

PR-P3H walidacja przed commitem:

```bash
python3 -m py_compile \
  scripts/build_selector_gatekeeper_feature_context.py \
  scripts/build_selector_training_view.py \
  scripts/build_selector_phase3_r2only.py \
  scripts/build_selector_r2only_feature_contribution.py \
  scripts/build_selector_r2only_model_candidate.py \
  scripts/test_selector_pipeline.py

python3 -m unittest scripts.test_selector_pipeline -v
git diff --check
```

Wynik:

```text
scripts.test_selector_pipeline: 61/61 PASS
git diff --check: PASS
```

PR-GK-FEATURE-LOG-GUARD walidacja przed commitem:

```bash
python3 -m py_compile \
  scripts/guard_gatekeeper_decision_feature_surface.py \
  scripts/test_selector_pipeline.py

python3 -m unittest scripts.test_selector_pipeline -v
git diff --check
```

Wynik:

```text
scripts.test_selector_pipeline: 64/64 PASS
git diff --check: PASS
```

Realny guard r7:

```bash
python3 scripts/guard_gatekeeper_decision_feature_surface.py \
  --source-scope shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag \
  --root /root/Gho \
  --decision-plane v25_shadow \
  --min-rows 100 \
  --json
```

Wynik:

```text
status = PASS
decision_rows = 13514
critical curve/market/time metrics = 100%
hhi/top3_volume_pct = 69.25%
concentration threshold = 60%
```

## Status po naprawie

Status po commitach:

```text
P3H tooling: implemented and committed
P3H runtime logging repair: not applicable / not claimed
Gatekeeper feature surface guard: implemented and committed
current r7 required curve/market emission: PASS by guard snapshot
final r7 selector-scope acceptance: pending final selector scope
```

Finalny selector scope:

```text
selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final
```

nie istniał lokalnie w czasie implementacji P3H. Dlatego nie claimowano
final-scope artifact acceptance.

Kiedy finalny scope istnieje, oczekiwany flow jest:

```bash
python3 scripts/build_selector_gatekeeper_feature_context.py \
  --root /root/Gho \
  --scope selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final \
  --source-scope shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag \
  --decision-plane v25_shadow \
  --observation-profile observation_8s_10s \
  --json

python3 scripts/build_selector_phase3_r2only.py \
  --root /root/Gho \
  --scope selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final \
  --gatekeeper-feature-context \
    /root/Gho/datasets/selector/selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final/gatekeeper_feature_context_v1.jsonl \
  --json

python3 scripts/build_selector_r2only_feature_contribution.py \
  --root /root/Gho \
  --scope selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final \
  --feature-set flow \
  --feature-set gk \
  --feature-set combined \
  --json

python3 scripts/build_selector_r2only_model_candidate.py \
  --root /root/Gho \
  --scope selector-phase1-pumpfun-sol-v1-20260605-r7-feature-rich-r2diag-final \
  --feature-set flow \
  --feature-set gk \
  --feature-set combined \
  --json
```

Final PR-P3H acceptance wymaga:

```text
context_rows_written > 0
denominator_created_rows = 0
forbidden_fields_detected = []
gk_bonding_progress_pct present_rate >= 0.80
gk_current_market_cap_sol present_rate >= 0.80
gk_price_change_ratio present_rate >= 0.80
gk_hhi present_rate >= 0.80
gk_top3_volume_pct present_rate >= 0.80
leakage_precheck = PASS
P3G reports flow-only, gk-only, combined, Gatekeeper accept
```

To jest inny gate niż early emission guard:

```text
early emission guard:
  hhi/top3 >= 0.60

final selector-context acceptance:
  gk_hhi/gk_top3 >= 0.80
```

Różnica jest intencjonalna. Guard mówi, czy warto kontynuować run i czy
runtime nadal emituje powierzchnię. Final acceptance mówi, czy finalny selector
scope ma wystarczająco bogaty context do P3F/P3G.

## Skutki naprawy

Po P3H:

- `bonding_progress_pct` wraca do training view jako
  `gk_bonding_progress_pct`;
- `current_market_cap_sol` wraca jako `gk_current_market_cap_sol`;
- `price_change_ratio` wraca jako `gk_price_change_ratio`;
- Gatekeeper flow/concentration/dev/FSC/vector summaries mogą być porównane z
  flow features;
- `candidate_universe_v1` pozostaje denominator SSOT;
- decision logs są context-only;
- verdict/reason/pass/threshold fields są zablokowane;
- oficjalny Phase 3 path pozostaje jeden;
- P3G może raportować `flow`, `gk`, `combined` i Gatekeeper accept context;
- przyszły brak emission surface zostanie wykryty guardem wcześnie.

Ryzyka pozostające:

- jeżeli finalny selector scope ma słaby join identity, context rows mogą być
  unmatched lub ambiguous;
- jeżeli finalny scope nie ma wystarczającego cutoff evidence, `gk_*` będzie
  traktowane jako missing;
- concentration metrics mają niższą coverage niż curve/market metrics i trzeba
  to raportować jawnie;
- P3H nie jest production model promotion;
- guard PASS nie oznacza, że finalny selector training view ma wystarczający
  `gk_*` join/cutoff coverage.

## Operacyjny runbook

Dla nowego selector collection, który ma używać Gatekeeper decision-time
context:

1. Po 10-15 minutach lub po przekroczeniu `min_rows` uruchomić guard:

```bash
python3 scripts/guard_gatekeeper_decision_feature_surface.py \
  --source-scope <source_scope> \
  --root /root/Gho \
  --decision-plane v25_shadow \
  --min-rows 100 \
  --json
```

2. Jeżeli status to `FAIL_NO_DECISION_LOGS`, sprawdzić routing/log path.

3. Jeżeli status to `FAIL_NO_REQUIRED_CURVE_METRICS`, zatrzymać run. Nie ma
   sensu czekać na selector dataset, który nie ma bonding progress / market cap
   / price dynamics w decision logs.

4. Jeżeli curve/market metrics PASS, ale concentration jest low, zdecydować,
   czy run nadal nadaje się do curve/market analysis. Nie claimować pełnej
   koncentracji.

5. Zbudować Gatekeeper context tylko dla istniejącego selector scope:

```bash
python3 scripts/build_selector_gatekeeper_feature_context.py \
  --root /root/Gho \
  --scope <selector_scope> \
  --source-scope <source_scope> \
  --decision-plane v25_shadow \
  --observation-profile observation_8s_10s \
  --json
```

6. Zbudować Phase 3 wyłącznie oficjalnym orchestratorem:

```bash
python3 scripts/build_selector_phase3_r2only.py \
  --root /root/Gho \
  --scope <selector_scope> \
  --gatekeeper-feature-context \
    /root/Gho/datasets/selector/<selector_scope>/gatekeeper_feature_context_v1.jsonl \
  --json
```

7. Porównać feature families:

```bash
python3 scripts/build_selector_r2only_feature_contribution.py \
  --root /root/Gho \
  --scope <selector_scope> \
  --feature-set flow \
  --feature-set gk \
  --feature-set combined \
  --json

python3 scripts/build_selector_r2only_model_candidate.py \
  --root /root/Gho \
  --scope <selector_scope> \
  --feature-set flow \
  --feature-set gk \
  --feature-set combined \
  --json
```

## Non-goals

Ta praca nie:

- zmienia Rust runtime;
- zmienia Gatekeeper policy;
- zmienia DecisionLogger emission logic;
- zmienia FSC runtime;
- zmienia R2 labeler semantics;
- zmienia P3D runtime;
- zmienia Helius/execution/restore path;
- zmienia runtime configs;
- zmienia Gatekeeper thresholds;
- promuje modelu do produkcji;
- claimuje final r7 selector acceptance przed istnieniem finalnego scope.

Jeżeli przyszły guard pokaże brak:

```text
bonding_progress_pct
current_market_cap_sol
price_change_ratio
```

w `gatekeeper_v2_decisions.jsonl`, wtedy wracamy do Rust:

```text
GatekeeperAssessment
DecisionLogger
Gatekeeper decision log serialization
```

To nie był obecny problem r7.

## Rekomendacje

Najważniejsza zasada:

```text
Najpierw sprawdź, czy metryka istnieje w gatekeeper_v2_decisions.jsonl.
```

Jeżeli nie istnieje:

```text
problem jest w runtime emission / DecisionLogger / GatekeeperAssessment
```

Jeżeli istnieje:

```text
problem jest w dataset/training consumption, joinie, cutoffie albo manifestach
```

Ten podział powinien być utrzymany w kolejnych regresjach. Chroni nas przed
naprawianiem niewłaściwej warstwy i przed niepotrzebnym ruszaniem runtime,
kiedy wystarczy naprawić offline selector pipeline.

Praktyczne wskazówki:

- nie używać decision logs jako denominatora;
- nie używać verdict/reason/pass fields jako model features;
- nie traktować `gk_log_schema_version` ani `gk_decision_plane` jako model
  features;
- nie zamieniać invalid/unverified `gk_*` na zero;
- nie budować alternatywnego wrappera dla Phase 3;
- zawsze przechodzić przez `build_selector_phase3_r2only.py`;
- guard uruchamiać wcześnie w runie;
- final acceptance liczyć dopiero na finalnym selector scope.

