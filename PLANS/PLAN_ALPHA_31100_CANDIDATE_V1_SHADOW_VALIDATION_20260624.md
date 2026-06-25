# Plan: alpha_31100_candidate_v1 shadow validation harness

Status: DRAFT / RESEARCH-ONLY / SHADOW-ONLY / NO RUNTIME DECISION
Data: 2026-06-24
Branch: `research/alpha-31100-validation-harness-v1`
Start commit: `4d6208e gatekeeper: tighten policy availability and confidence contracts`
Powiazany backlog: `PLANS/BACKLOG_ALPHA_31100_CANDIDATE_V1_VALIDATION_HARNESS_20260624.md`

## 1. Cel

Celem tego etapu jest zbudowanie formalnego, reprodukowalnego validation harness dla kandydata `alpha_31100_candidate_v1`, obserwowanego w oknie `T0 + 31100 ms`.

Ten plan nie wdraza modelu do runtime. Ten plan falsyfikuje sygnal.

Wynik etapu ma odpowiedziec na pytania:

- czy sygnal istnieje chronologicznie out-of-sample,
- czy nie jest artefaktem missingness, blacklist fields, identyfikatorow albo leakage,
- czy utrzymuje separacje na hard-negative suite,
- czy ma dodatni top-k EV po kosztach w naturalnym rozkladzie,
- czy nadaje sie do kolejnego etapu shadow-only logging spec.

## 2. Kontrakt nieprzekraczalny

Ten branch i ten plan maja status:

- `RESEARCH`,
- `SHADOW-ONLY`,
- `NO RUNTIME DECISION`,
- `NO LIVE DEPLOY`,
- `NO BUY TRIGGER`.

Nie wolno:

- dodawac hooka BUY/REJECT do Gatekeepera,
- zmieniac progow Gatekeepera,
- zmieniac `v25_confidence`,
- podmieniac canonical confidence uproszczonym score,
- optymalizowac decyzji tradingowej pod F1,
- kopiowac regul z raportow HTML do bota,
- traktowac Segment Lab jako zrodla regul runtime,
- wykorzystywac `alpha_31100_candidate_v1` jako inputu runtime decision,
- laczyc zmian harnessu z PR1-PR4 Gatekeeper runtime contracts.

Kazda pozniejsza rozmowa o runtime wymaga osobnego planu i zaczyna sie co najwyzej od shadow-only logging.

## 3. In-scope

W zakresie tego planu sa:

- formalny schema freeze cech `31100 ms`,
- trzy artefakty schema:
  - `research/alpha_31100_candidate_v1/features_31100_v1_all.json`,
  - `research/alpha_31100_candidate_v1/features_31100_v1_safe_core.json`,
  - `research/alpha_31100_candidate_v1/features_31100_v1_blacklist.json`,
- master ledger dla runow `31100 ms`,
- deterministyczny feature vector hash,
- chronologiczne train/validation/test_oos/final_holdout,
- natural imbalance, balanced random B, hard-negative active B, target-hit A vs stop-hit B,
- metryki separacji i trading utility,
- ablation suite,
- missingness/leakage audit,
- score buckets i top-k EV po kosztach,
- raport reprodukowalny jednym poleceniem,
- shadow logging spec jako dokument/spec, bez runtime integration.

## 4. Out-of-scope

Poza zakresem sa:

- live deploy,
- nowy BUY trigger,
- zmiany Gatekeeper thresholds,
- runtime XGBoost/LightGBM/CatBoost hook,
- DecisionLogger runtime schema change dla realnego bota,
- zmiana aktywnej polityki BUY/REJECT/WATCH,
- refaktor PR1-PR4 Gatekeeper contracts,
- proba strojenia progow produkcyjnych,
- commitowanie duzych datasetow runtime/log/report artifacts do repo,
- uzywanie final_holdout do iteracji modelu.

## 5. Architektura artefaktow

Docelowa struktura:

```text
research/alpha_31100_candidate_v1/
  README.md
  features_31100_v1_all.json
  features_31100_v1_safe_core.json
  features_31100_v1_blacklist.json
  config/
    validation_harness_v1.toml
  src/
    build_feature_schema.py
    build_master_ledger.py
    leakage_audit.py
    train_eval.py
    ablation_suite.py
    score_buckets.py
    report.py
  reports/
    .gitkeep
```

Uwagi:

- `research/alpha_31100_candidate_v1/reports/` moze zawierac lokalne wyniki, ale duze raporty i datasety nie powinny byc commitowane bez osobnej decyzji.
- Jesli repo ma juz ustalony standard dla research scripts, wolno przeniesc `src/` do istniejacego katalogu skryptow, ale nazwa namespace `alpha_31100_candidate_v1` musi zostac zachowana.
- Harness ma byc odtwarzalny jednym poleceniem, ale same dane wejsciowe moga pozostac poza repo, jesli sa duze.

## 6. Model danych: schema freeze

### 6.1 `features_31100_v1_all.json`

Zawiera pelny katalog kandydackich pol widocznych w datasetach `31100 ms`, niezaleznie od tego, czy wolno je modelowi uzyc.

Kazdy rekord cechy musi miec:

```json
{
  "name": "string",
  "category": "traction/momentum | buy_pressure | organicity | concentration_toxicity | dev_toxicity | execution_toxicity | cross_pool_sybil | temporal | other",
  "dtype": "float | int | bool | categorical | string | timestamp | identifier | unknown",
  "unit": "string | null",
  "decision_time_available": true,
  "observation_cutoff_ms": 31100,
  "source_family": "string",
  "coverage_pct": null,
  "safe_core_allowed": false,
  "blacklist_reason": null,
  "notes": "string"
}
```

### 6.2 `features_31100_v1_safe_core.json`

Zawiera tylko pola:

- dostepne live przed decyzja BUY,
- obliczalne najpozniej na `T0 + 31100 ms`,
- bez outcome,
- bez identyfikatorow,
- bez absolutnych timestampow,
- bez slot/finality,
- bez join keys,
- bez pol symulacji po decyzji,
- najlepiej z sensownym coverage.

Kazda cecha musi miec jawna kategorie:

- `traction/momentum`,
- `buy_pressure`,
- `organicity`,
- `concentration_toxicity`,
- `dev_toxicity`,
- `execution_toxicity`,
- `cross_pool_sybil`,
- `temporal`,
- `other`.

### 6.3 `features_31100_v1_blacklist.json`

Zawiera pola wyrzucone z modelowania, razem z przyczyna.

Automatycznie blacklistowane sa nazwy zawierajace:

- `exit`,
- `final`,
- `pnl`,
- `profit`,
- `loss`,
- `target`,
- `stop`,
- `eval`,
- `simulation`,
- `result`,
- `future`,
- `after`.

Blacklist obejmuje tez:

- `entry_price`,
- `exit_price`,
- `exit_value_sol`,
- `sample_age_ms`,
- token id,
- mint jako feature,
- record id,
- join key,
- absolutne timestampy,
- slot/finality,
- pola outcome,
- pola niedostepne przed `T0 + 31100 ms`.

## 7. Model danych: master ledger

Master ledger jest kanonicznym datasetem laczacym evidence, outcome i split metadata.

Minimalne kolumny:

```text
run_id
mint
token_id
created_ts
observation_cutoff_ms
decision_ts
entry_ts
exit_ts
exit_reason
final_pnl_pct
label
feature_schema_version
feature_vector_hash
source_file
split
```

Kontrakt:

- `observation_cutoff_ms` musi wynosic `31100` dla kazdego rekordu w harness v1.
- `split` przyjmuje tylko: `train`, `val`, `test_oos`, `final_holdout`.
- `mint` i `token_id` moga byc w ledgerze jako identyfikatory audytowe, ale nie moga trafic do safe-core feature matrix.
- `created_ts`, `decision_ts`, `entry_ts`, `exit_ts` sa do splitow, audytu i outcome alignment; nie sa cechami modelu.
- `feature_vector_hash` musi byc deterministyczny dla uporzadkowanej listy safe-core cech i ich wartosci po preprocessing.
- `source_file` musi wskazywac pochodzenie rekordu.

Ledger ma rozdzielac:

- decision-time evidence,
- split metadata,
- post-decision outcome.

Nie wolno mieszac tych poziomow w feature matrix.

## 8. Split chronologiczny

Finalnym dowodem nie jest random split.

Docelowy split:

- `train`: starszy run `31100 ms`,
- `val`: pozniejsza chronologicznie czesc starszego runa albo osobny pozniejszy kawalek,
- `test_oos`: swiezy run `31100 ms`,
- `final_holdout`: kolejny przyszly run, nietykany do finalnej weryfikacji.

Reguly:

- sortowanie chronologiczne musi byc jawne i audytowalne,
- ten sam `mint` nie moze pojawic sie w wielu splitach,
- jesli source runs nachodza na siebie czasowo, harness musi to pokazac i przerwac final proof,
- final_holdout nie moze byc uzyty do wyboru cech, hiperparametrow, thresholdow ani kosztow.

## 9. Dataset views do raportowania

Harness musi raportowac osobno:

1. `natural_imbalance`
   - naturalny rozklad klas,
   - podstawowy OOS proof.

2. `balanced_random_b`
   - diagnostyczny balanced split,
   - nie jest metryka produkcyjna.

3. `hard_negative_active_b`
   - zle poole z mocnym ruchem,
   - testuje, czy sygnal odroznia organiczny ruch od toksycznego.

4. `target_hit_a_vs_stop_hit_b`
   - porownanie target-hit vs stop-hit,
   - testuje praktyczna separacje outcome.

## 10. Metryki

Dla kazdego modelu, splitu i dataset view raportowac:

- ROC-AUC,
- PR-AUC,
- positive_rate,
- PR baseline,
- PR lift = `PR-AUC / positive_rate`,
- precision@top_0.5%,
- precision@top_1%,
- precision@top_2%,
- precision@top_5%,
- avg_pnl@top_k,
- median_pnl@top_k,
- target_rate@top_k,
- stop_rate@top_k,
- EV_after_costs@top_k,
- liczba trade candidates,
- max consecutive losses w sekwencji chronologicznej.

Zakazy metryczne:

- F1 nie jest decision metric.
- Balanced PR-AUC nie jest production proof.
- Accuracy nie jest production proof.
- Threshold dobrany pod walidacje nie moze byc raportowany jako final OOS success.

## 11. Ablation suite

Obowiazkowe warianty:

### A. Full safe core

Wszystkie cechy z `features_31100_v1_safe_core.json`.

### B. Without momentum/traction

Usunac:

- `bonding_progress_pct`,
- `current_market_cap_sol`,
- `price_change_ratio`,
- `buy_count`,
- `total_tx`,
- `total_volume_sol`.

### C. Without toxicity/concentration

Usunac:

- `hhi`,
- `top3_volume_pct`,
- `top3_signer_volume_ratio`,
- `dev_tx_ratio`,
- `dev_volume_ratio`,
- `burst_ratio`,
- `jito_tip_intensity`,
- `compute_unit_cluster_dominance`,
- `max_single_sell_impact_pct_observed`,
- `cpv`,
- `signer_cross_pool_velocity`.

### D. Missingness-only

Uzyc tylko cech `is_missing(feature)`.

Jesli ten model osiaga podejrzanie wysoki wynik, zatrzymac temat i badac pipeline artifact.

### E. Safe-core-only without weak coverage

Usunac cechy o slabym coverage. Prog coverage musi byc zapisany w configu harnessu, nie ukryty w kodzie.

### F. Hard-negative-only train/test

Trening i test na hard negatives. Ten wariant ma falsyfikowac "duzy ruch = dobry pool".

### G. Blacklist-only sentinel

Model na `features_31100_v1_blacklist.json`.

Ten wariant nie jest do uzycia predykcyjnego. To sentinel leakage/pipeline artifact.

Jesli blacklist-only daje dobry wynik, caly proof jest niewazny do czasu naprawy pipeline.

## 12. Leakage audit

Leakage audit jest gate, nie dodatkiem.

Harness musi automatycznie:

- skanowac nazwy pol po blacklist patternach,
- sprawdzac dtype `identifier`, `timestamp`, `string`,
- sprawdzac absolutne timestampy,
- wykrywac slot/finality,
- wykrywac entry/exit price/value,
- wykrywac outcome/simulation fields,
- blokowac token id, mint, record id i join key w feature matrix,
- rozdzielac feature columns od ledger/audit columns,
- logowac powod usuniecia kazdej cechy.

Testy sentinel:

- model na blacklist features,
- model na missingness-only.

Fail conditions:

- blacklist-only osiaga wynik porownywalny z full safe core,
- missingness-only osiaga podejrzanie wysoki AUC/PR lift,
- feature vector zawiera pole z blacklist pattern,
- feature vector zawiera identyfikator,
- feature vector zawiera timestamp absolutny,
- feature vector zawiera outcome/post-decision field.

## 13. Score buckets i top-k EV

Na naturalnym `val` i `test_oos` wygenerowac bucket table:

```text
score_bucket
count
A_rate
avg_pnl
median_pnl
target_rate
stop_rate
EV_after_costs
cumulative_top_k_EV
```

Bucket boundaries:

- `0.0-0.1`,
- `0.1-0.2`,
- `0.2-0.3`,
- `0.3-0.4`,
- `0.4-0.5`,
- `0.5-0.6`,
- `0.6-0.7`,
- `0.7-0.8`,
- `0.8-0.9`,
- `0.9-1.0`.

Top-k:

- `top_0.5%`,
- `top_1%`,
- `top_2%`,
- `top_5%`.

EV musi byc liczony po kosztach. Koszty musza byc jawne w configu i raporcie.

## 14. Shadow mode spec

Shadow spec powstaje dopiero po przejsciu validation gates.

Minimalny format docelowy:

```text
token
timestamp
score_15s
score_31s
toxicity_veto_flags
final_decision
reason_codes
top_contributing_feature_families
simulated_outcome
pnl
exit_reason
```

`final_decision` moze przyjmowac:

- `WATCH`,
- `REJECT`,
- `WOULD_BUY`.

Kontrakt:

- `WOULD_BUY` nie jest live BUY,
- shadow score nie zmienia Gatekeeper verdict,
- reason codes musza byc audytowalne,
- simulated outcome pozostaje post-decision evidence,
- `score_31s` nie moze zastapic canonical Gatekeeper confidence.

## 15. Proponowany podzial na PR-y

### PR-A: Branch, plan, schema freeze

Zakres:

- utworzyc branch `research/alpha-31100-validation-harness-v1` z `4d6208e`,
- zapisac ten plan,
- utworzyc namespace `research/alpha_31100_candidate_v1/`,
- utworzyc trzy schema JSON:
  - `features_31100_v1_all.json`,
  - `features_31100_v1_safe_core.json`,
  - `features_31100_v1_blacklist.json`,
- dodac walidator schema JSON,
- dodac dokumentacje kategorii i blacklist reasons.

DoD:

- wszystkie trzy schema pliki istnieja,
- kazda cecha ma kategorie,
- safe-core nie zawiera blacklist patternow,
- blacklist zawiera powod dla kazdego pola,
- `jq`/schema validator przechodzi,
- brak zmian w `ghost-launcher/src`, `ghost-core/src`, `ghost-brain/src` poza ewentualnymi test-only helperami zatwierdzonymi osobno,
- `rg -n "alpha_31100|score_31s|WOULD_BUY|xgboost|lightgbm" ghost-launcher/src ghost-core/src ghost-brain/src` nie pokazuje runtime hooka.

### PR-B: Master ledger builder

Zakres:

- zbudowac deterministic ledger builder,
- zinwentaryzowac source files/run ids,
- materializowac ledger columns,
- liczyc `feature_vector_hash`,
- walidowac uniqueness `run_id + mint`,
- walidowac brak split leakage.

DoD:

- ledger jest odtwarzalny jednym poleceniem,
- ledger ma wymagane kolumny,
- `observation_cutoff_ms = 31100` dla wszystkich rekordow,
- split przyjmuje tylko `train/val/test_oos/final_holdout`,
- brak duplikatow `mint` miedzy splitami,
- feature vector hash stabilny przy powtorzeniu,
- raport coverage/missingness powstaje automatycznie,
- identyfikatory i timestampy nie trafiaja do feature matrix.

### PR-C: Chronological OOS evaluation harness

Zakres:

- trenowanie baseline modelu na full safe core,
- chronological split,
- raport metryk dla natural imbalance,
- raport metryk dla balanced random B,
- raport metryk dla hard-negative active B,
- raport metryk dla target-hit A vs stop-hit B.

DoD:

- jeden command odtwarza trenowanie i raport,
- raport zawiera wszystkie metryki z sekcji 10,
- random split jest oznaczony jako diagnostyczny,
- chronological OOS jest oznaczony jako final proof surface,
- final_holdout nie jest dotkniety, jesli nie ma jeszcze decyzji o final run,
- wyniki zapisane z run config hash.

### PR-D: Ablation and leakage audit

Zakres:

- modele A-G z sekcji 11,
- automatic leakage scanner,
- missingness-only sentinel,
- blacklist-only sentinel,
- permutation sanity check,
- failure report.

DoD:

- missingness-only nie wyjasnia wyniku,
- blacklist-only nie wyjasnia wyniku,
- permutation score spada w okolice losowosci,
- kazda usunieta cecha ma powod,
- ablation pokazuje, czy sygnal jest tylko momentum czy zawiera flow quality/toxicity structure,
- w przypadku fail gate harness zatrzymuje sie z czerwonym statusem.

### PR-E: Score buckets and EV report

Zakres:

- score bucket report,
- top-k EV po kosztach,
- max consecutive losses chronologicznie,
- cost model jawny w configu,
- target/stop rate na top-k.

DoD:

- natural validation/OOS ma bucket table,
- top-k EV liczone po kosztach,
- dodatni EV nie jest deklarowany bez kosztow,
- brak thresholda produkcyjnego,
- raport rozroznia statistical separability od trading utility.

### PR-F: Shadow-only logging specification

Zakres:

- przygotowac spec logu `WOULD_BUY`,
- opisac reason code/family attribution,
- opisac toxicity veto flags,
- opisac simulated outcome fields,
- opisac replay/audit expectations,
- nie implementowac runtime hooka bez osobnego planu.

DoD:

- spec istnieje jako dokument,
- jasno mowi, ze `WOULD_BUY` nie jest BUY,
- jasno mowi, ze score nie zmienia Gatekeeper verdict,
- zawiera migration path do przyszlego shadow logging PR,
- nie zmienia kodu bota.

## 16. Acceptance gates przed rozmowa o runtime

Minimalne gates:

- natural OOS `31100 ms` ROC-AUC `>= ~0.78`,
- PR lift wzgledem baseline `>= 2.5x`,
- hard-negative ROC-AUC `>= ~0.62`,
- target-vs-stop balanced PR-AUC `>= ~0.70`,
- top-k EV after costs dodatni na przynajmniej jednym konserwatywnym top-k,
- missingness-only nie wyjasnia wyniku,
- blacklist-only nie wyjasnia wyniku,
- raport reprodukowalny jednym poleceniem,
- final_holdout pozostaje nietkniety do ostatniego proof.

Niespelnienie gate nie jest porazka implementacyjna. Jest poprawnym falsification result.

## 17. Ryzyka i kontrole

### Leakage

Ryzyko:
Model widzi outcome albo przyszlosc przez nazwe pola, timestamp, join key, symulacje albo final price.

Kontrola:
Blacklist scanner, schema freeze, blacklist-only sentinel, feature matrix audit.

### Missingness artifact

Ryzyko:
Model uczy sie, ktory pipeline wygenerowal rekord, zamiast rynku.

Kontrola:
Missingness-only model, coverage report, weak-coverage ablation.

### Chronological contamination

Ryzyko:
Random split zawyza metryki przez podobne rezimy w train/test.

Kontrola:
Chronological OOS jako final proof, random split tylko diagnostycznie.

### Hard-negative collapse

Ryzyko:
Sygnal wykrywa tylko "duzy ruch", nie organic quality.

Kontrola:
Hard-negative active B i ablation without toxicity/concentration.

### EV illusion

Ryzyko:
Wysokie AUC nie daje dodatniego EV po kosztach.

Kontrola:
Top-k EV after costs, target/stop rate, max consecutive losses.

### Runtime boundary drift

Ryzyko:
Research score zaczyna byc traktowany jak runtime decision.

Kontrola:
Zakaz runtime hooka, branch research-only, final shadow spec jako dokument, osobny plan dla integracji.

## 18. Minimalne komendy walidacyjne per PR

PR-A:

```bash
jq . research/alpha_31100_candidate_v1/features_31100_v1_all.json >/dev/null
jq . research/alpha_31100_candidate_v1/features_31100_v1_safe_core.json >/dev/null
jq . research/alpha_31100_candidate_v1/features_31100_v1_blacklist.json >/dev/null
rg -n "alpha_31100|score_31s|WOULD_BUY|xgboost|lightgbm" ghost-launcher/src ghost-core/src ghost-brain/src
git diff --check
```

PR-B:

```bash
python3 -m research.alpha_31100_candidate_v1.src.build_master_ledger --config research/alpha_31100_candidate_v1/config/validation_harness_v1.toml
python3 -m research.alpha_31100_candidate_v1.src.leakage_audit --config research/alpha_31100_candidate_v1/config/validation_harness_v1.toml --ledger <ledger_path>
git diff --check
```

PR-C to PR-E:

```bash
python3 -m research.alpha_31100_candidate_v1.src.report --config research/alpha_31100_candidate_v1/config/validation_harness_v1.toml --suite full
git diff --check
```

Final harness command:

```bash
python3 -m research.alpha_31100_candidate_v1.src.report --config research/alpha_31100_candidate_v1/config/validation_harness_v1.toml --suite full --reproduce
```

## 19. Aktualny nastepny krok

Najblizszy krok po akceptacji tego planu:

1. PR-A: utworzyc namespace `research/alpha_31100_candidate_v1/`.
2. Zinwentaryzowac dostepne source files/run artifacts `31100 ms`.
3. Wygenerowac `features_31100_v1_all.json`.
4. Recznie zatwierdzic `safe_core` i `blacklist`.
5. Dopiero potem budowac master ledger.

Nie zaczynac od modelu.
Nie zaczynac od thresholdow.
Nie zaczynac od runtime logging.
