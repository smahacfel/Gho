# BUSINESS_LABEL_CONTRACT_50_50_CHANGE_REPORT

## Zakres zadania
Zadanie: **PR-BUSINESS-LABEL-CONTRACT-50-50**  
Cel: zmiana kontraktu business labelingu z `+40% / -40%` na `+50% / -50%`  
Zakres ograniczony do pipeline analitycznego / datasetów (`R2`, Segment Lab, datasety do modelowania).  
Nie dotyczy live execution/Gatekeeper policy.

## Aktualne → docelowe wartości
- `target`: `+50%` (z `+40%`)
- `stop`: `-50%` (z `-40%`)
- `dirty_mae_pct`: `-50%` (z `-40%`)
- `horizon_ms`: bez zmian (`horizon_ms` pozostaje taki jak w danym eksperymencie, np. `60000` ms)

## Zmiany wykonane w kodzie
### Zmiany zgodne z wymaganiem
- `scripts/gatekeeper_outcome_labeler.py`
  - `DEFAULT_TARGET_PCT`: `40.0 -> 50.0`
  - `DEFAULT_STOP_PCT`: `40.0 -> 50.0`
  - Docstring nagłówka zaktualizowany: `+50% / -50%`.
- `scripts/v3_p37_outcome_label_v2.py`
  - `DEFAULT_TARGET_PCT`: `40.0 -> 50.0`
  - `DEFAULT_STOP_PCT`: `40.0 -> 50.0`
  - `DEFAULT_DIRTY_MAE_PCT`: `-40.0 -> -50.0`
  - Docstring nagłówka zaktualizowany: `+50` (kontrakt biznesowy), zachowano nazwy pól outputu.
- `scripts/build_selector_r2_market_paths.py`
  - W `manifest["r2_config"]["profile"]` usunięto hardcoded `r2_40_40_60s_v1`.
  - Ustawiono dynamiczny zapis:
    `f"r2_{target_net_pct:g}_{stop_net_pct:g}_{horizon_ms // 1000}s_v1"`.
- `scripts/test_v3_p37_outcome_label_v2.py`
  - Dodatkowy test `test_defaults` waliduje `50.0 / 50.0 / -50.0`.
  - `threshold()` z domyślnymi wartościami dostosowane do nowego kontraktu.
- `scripts/test_selector_pipeline.py`
  - `classify_r1` testy jednostkowe przełączone na `pnl_target_net_pct=50`.
  - Dodany/zmieniony test manifestu `r2_market_paths` sprawdzający profil `r2_50_50_60s_v1`.

## Wynik przeszukania referencji (325 wpisów, `tee`)
Polecenie:
`rg -RIn "DEFAULT_TARGET_PCT|DEFAULT_STOP_PCT|DEFAULT_DIRTY_MAE_PCT|r2_40_40|target-net-pct|stop-net-pct|target_pct|stop_pct|pnl_target_net_pct|hit_40|plus40|drawdown_before_plus40" scripts ghost-brain ghost-launcher configs PLANS docs | tee /tmp/label_contract_50_50_search.txt`

Pełny dump: `/tmp/label_contract_50_50_search.txt`

### Lista plików i decyzja

- **changed**
  - `scripts/gatekeeper_outcome_labeler.py`
  - `scripts/v3_p37_outcome_label_v2.py`
  - `scripts/build_selector_r2_market_paths.py`
  - `scripts/test_v3_p37_outcome_label_v2.py`
  - `scripts/test_selector_pipeline.py`

- **left as historical field name** (wyraźnie nazwane pola kontraktu historyczne, bez zmiany nazewnictwa)
  - `scripts/gatekeeper_outcome_labeler.py` (`hit_40_before_stop`, `hit_40`, `min_return_before_40_pct`)
  - `scripts/v3_p37_outcome_label_v2.py` (`drawdown_before_plus40`, `hit_40_before_stop`)
  - `scripts/v3_p37_price_path_fetcher.py` (`plus40`, `drawdown_before_plus40`, `hit_40`)
  - `scripts/test_v3_p37_price_path_fetcher.py`
  - `scripts/gatekeeper_40pct_validation.py`
  - `scripts/gatekeeper_policy_replay_grid.py`
  - `scripts/v3_outcome_quality_report.py`
  - `scripts/test_v3_outcome_quality_report.py`
  - `scripts/audit_selector_business_target_rate.py` (tam, gdzie odwołuje się do field-name kompatybilności)

- **left as documentation/archive**
  - `docs/ADR/ADR-0118-gatekeeper-plus40-validation-and-rollout-governance.md`
  - `PLANS/PLAN_SELECTOR_DATASET_V2_PHASE0_TO_PHASE4_20260601.md`
  - `PLANS/AUDYT/RAPORT_P3_6_FEATURE_SEPARATION_AUDIT_R10_R11_R13_20260518.md`
  - `PLANS/AUDYT/MANIFEST_P3_7_BASELINE_DATASET_R10_R11_R13_20260518.md`
  - `PLANS/AUDYT/RAPORT_P3_6_FINAL_CLOSURE_20260518.md`
  - `PLANS/AUDYT/RAPORT_P3_6_COMBINED_R10_R11_R13_CALIBRATION_20260518.md`
  - `PLANS/AUDYT/RAPORT_OPERACYJNY_P3_5_V3_OUTCOME_QUALITY_R10_20260516.md`
  - `PLANS/AUDYT/RAPORT_OPERACYJNY_P3_5_V3_OUTCOME_QUALITY_R11_20260516.md`
  - `PLANS/AUDYT/RAPORT_P3_5_V3_PRIMARY_ONLY_OUTCOME_QUALITY_20260516.md`
  - `PLANS/AUDYT/RAPORT_P3_6_V3_SHADOW_CALIBRATION_R10_R11_20260517.md`
  - `PLANS/PLAN_P3_6_SAMPLE_EXPANSION_R12_GOVERNANCE_20260517.md`
  - `PLANS/PLAN_P3_6_V3_SHADOW_ONLY_CALIBRATION_20260517.md`
  - `PLANS/PLAN_P3_7_FEATURE_REDESIGN_AND_LIFECYCLE_LABELS_20260518.md`
  - `PLANS/PLAN_P3_7_TRUTH_SOURCE_ACQUISITION_20260518.md`

- **not relevant / intentionally not changed in this PR**
  - `configs/rollout/ghost_brain_*.toml` – wartości `tp_phase*` są parametrami exit live, nie kontraktu labelingu R2.
  - `ghost-brain/ghost_brain_config.toml` – podobnie: TP/stopy to parametry live policy, oddzielne od label contract.
  - `scripts/build_selector_phase3_r2only.py` – `target-net-pct`/`stop-net-pct` defaultowane do `40.0` dla legacy-phase3 flow; nie było to wymagane przez zakres (nie dotyczy `R2` market-path).
  - `scripts/build_selector_all_decision_counterfactual_outcome.py` – domyślne `30.0` (odrębny eksperyment/budowa 30-30), poza wymaganym kontraktem.
  - `scripts/build_selector_phase2.py` – zawiera profil `r2_40_40_60s_v1` w starszej ścieżce.
  - `scripts/build_selector_training_view.py`, `scripts/build_selector_dataset.py`, `scripts/build_selector_accepted_lifecycle.py`, `scripts/selector_pipeline_common.py` – nie mają domyślnych wartości kontraktu w miejscu ryzykownym; wartości przekazywane jawnie.
  - `scripts/run_selector_phase0_validation.sh` – odzwierciedla historyczne runbooki, wymaga celowych migracji runbookowych, nie zmian w samym kontrakcie.

## Sprawdzenie modułów zakazanych (niezmienione)
Nie ruszono:
- `ghost-launcher/src/components/post_buy_runtime.rs` (`live_exit_take_profit_pct`, `live_exit_stop_loss_pct`)
- `ghost-brain/src/guardian/post_buy/engine.rs` (`ShadowSimpleExitThresholds`)
- żadnej ścieżki live execution, send path, decyzji Gatekeepera (`policy`, `gk`, `execution/live`), ani `XGBoost` skryptów.

## Walidacja
Wykonane komendy:
- `python3 scripts/gatekeeper_outcome_labeler.py --help`
- `python3 scripts/v3_p37_outcome_label_v2.py --help`
- `python3 scripts/build_selector_r2_market_paths.py --help`
- `python3 -m py_compile scripts/gatekeeper_outcome_labeler.py scripts/v3_p37_outcome_label_v2.py scripts/build_selector_r2_market_paths.py scripts/selector_pipeline_common.py`
- `python3 -m unittest scripts.test_v3_p37_outcome_label_v2 scripts.test_selector_pipeline.SelectorPipelineTests.test_r1_target_stop_nonpositive_excluded_and_gray_cases scripts.test_selector_pipeline.SelectorPipelineTests.test_r2_market_paths_profile_uses_configured_contract`

Wyniki:
- `--help` oraz `py_compile`: `exit 0`
- `unittest`: `Ran 10 tests in 0.008s`, `OK`
- `git diff --check` (po zmianach tego PR i powyższych testach): brak whitespace/merge errorów.

## Instrukcja uruchomień dla nowych runów
- `gatekeeper_outcome_labeler.py`:
  - domyślnie teraz `--target-pct 50 --stop-pct 50`
  - albo jawnie: `--target-pct 50 --stop-pct 50`
- `v3_p37_outcome_label_v2.py`:
  - domyślnie teraz `--target-pct 50 --stop-pct 50 --dirty-mae-pct -50`
  - albo jawnie z takim zakresem
- `build_selector_r2_market_paths.py`:
  - obowiązkowo: `--target-net-pct 50 --stop-net-pct 50`
  - `--horizon-ms` zgodnie z eksperymentem (np. `60000` dla 60s)
  - nowy profil manifestu: `r2_50_50_60s_v1`

## Werdykt końcowy
**BUSINESS_LABEL_CONTRACT_50_50_READY**

