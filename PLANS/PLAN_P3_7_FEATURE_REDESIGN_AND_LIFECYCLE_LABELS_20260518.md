# PLAN P3.7 - Feature Redesign + Lifecycle-Aware Outcome Model

Data: 2026-05-18

Status: **APPROVED FOR EXECUTION PLANNING / NO P2 / NO LIVE**

## 1. Executive summary

P3.7 jest nowym etapem po formalnym zamknieciu P3.6. Nie kontynuuje
threshold-level calibration obecnej rodziny V3 i nie probuje ratowac obecnych
progów przez kolejne strojenie.

Cel P3.7:

```text
Sprawdzic, czy V3 moze stac sie selektorem dopiero po poprawieniu prawdy
o wyniku: Outcome Label v2, execution feasibility join, temporal split,
feature prototype i swiezy holdout tylko po pre-registered hypothesis.
```

Podzial:

- **Phase A - Truth Layer**
  - dataset manifest,
  - Outcome Label v2,
  - execution feasibility join,
  - relabel R10/R11/R13,
  - temporal split baseline.
- **Phase B - Feature Redesign**
  - replay-only feature prototypes,
  - feature candidate gate,
  - fresh holdout po zapisanej hipotezie,
  - warunkowe MaterializedFeatureSet extension tylko po offline edge.

P3.7 nie autoryzuje:

- P2,
- live,
- R12 calibrated candidate,
- threshold tuningu,
- FSC active gate/ranking,
- runtime feature extension,
- progow z `analiza_porownawcza.py`.

## 2. Governance rules

Obowiazkowe reguly P3.7:

1. Outcome Label v2 przed feature conclusions.
2. Execution feasibility join przed BUY-quality claims.
3. Temporal split jest warunkiem, nie tylko raportem.
4. Combined-only evidence jest niewystarczajace.
5. Fresh holdout tylko po zapisanej hipotezie z metrykami i failure condition.
6. Current V3 zostaje risk shield/audit layer/negative control, nie selector candidate.
7. `analiza_porownawcza.py` jest appendix/hypothesis tool.
8. Sekcje Youden/L1/scoring/ready-rule/causal discovery sa `appendix_only`.
9. No runtime thresholds z legacy analyzer bez full replay ablation, temporal split i holdout.
10. P3.7 ma formalny kill criterion: jesli po truth layer, feature prototypes i holdout nie ma stabilnego edge, V3 selector line zostaje zamkniety.

## 3. Source of truth and constraints

Dokumenty zrodlowe:

- `PLANS/AUDYT/RAPORT_P3_6_FINAL_CLOSURE_20260518.md`
- `docs/ADR/ADR-0133-v3-p37-feature-redesign-lifecycle-labels.md`
- `PLANS/AUDYT/RAPORT_P3_6_DECISION_GATE_REVIEW_R10_R11_R13_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_6_FEATURE_SEPARATION_AUDIT_R10_R11_R13_20260518.md`
- `PLANS/AUDYT/RAPORT_P3_6_COMBINED_R10_R11_R13_CALIBRATION_20260518.md`

Baseline dataset:

- R10,
- R11,
- R13.

Nie wolno:

- modyfikowac historycznych artefaktow R10/R11/R13,
- uzywac FSC jako active hard gate/ranking pod single-stream constraint,
- traktowac `status=ok`, full replay lub duzej liczby rows jako edge,
- uzywac combined R10/R11/R13 jako jedynej walidacji,
- uznac market +40 za true good entry bez feasibility.

## 4. Phase A - Truth Layer

### 4.1 P3.7.0 - Closure commit and plan registration

Cel:

Zamknac formalnie P3.6 i zarejestrowac P3.7 jako osobny etap.

Zakres:

- zacommitowac i wypchnac, jesli jeszcze nie jest committed:
  - `PLANS/AUDYT/RAPORT_P3_6_FINAL_CLOSURE_20260518.md`
  - `docs/ADR/ADR-0133-v3-p37-feature-redesign-lifecycle-labels.md`
  - `PLANS/PLAN_P3_7_FEATURE_REDESIGN_AND_LIFECYCLE_LABELS_20260518.md`

Acceptance:

- `git diff --check`
- closure mowi jasno: P3.6 nie zawiodlo operacyjnie; P3.6 sfalsyfikowalo current selector family.
- stare untracked configi P3.2 r4-r6 nie sa stagingowane.

### 4.2 P3.7.1 - Dataset Manifest R10/R11/R13

Cel:

Utworzyc jedno audytowalne zrodlo prawdy dla danych wejsciowych P3.7.

Nowe artefakty:

- `PLANS/AUDYT/MANIFEST_P3_7_BASELINE_DATASET_R10_R11_R13_20260518.md`
- `logs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only/reports/p3_7_dataset_manifest.json`

Manifest per run musi zawierac:

- run name,
- rollout namespace,
- config path,
- decision log path,
- label v1 path,
- threshold hits path,
- lifecycle log path, jesli istnieje,
- shadow entry log path, jesli istnieje,
- replay report path,
- calibration report path,
- feature separation report path,
- row count,
- label count,
- strict replay status,
- known/good/bad/neutral/unknown counts,
- `decision_log_sha256`,
- `label_v1_sha256`,
- `threshold_hits_sha256`,
- `replay_report_sha256`,
- `config_hash`,
- `policy_hash`,
- `git_head`,
- `generated_at`.

Acceptance:

- JSON manifest jest obowiazkowy i czytelny przez przyszle skrypty.
- Wszystkie wskazane pliki istnieja albo maja explicit `missing` plus powod.
- Historyczne R10/R11/R13 pozostaja immutable baseline.
- Brak mieszania R13 sample expansion z candidate run.

### 4.3 P3.7.2 - Outcome Label v2

Cel:

Zastapic grube `+40 before stop` bogatsza klasyfikacja jakosci okazji.

Nowy skrypt:

- `scripts/v3_p37_outcome_label_v2.py`

Test:

- `scripts/test_v3_p37_outcome_label_v2.py`

Input:

- `--decisions <gatekeeper_v2_decisions.jsonl>`
- `--threshold-hits <threshold_hits_or_price_path.jsonl>`
- `--output <label_v2.jsonl>`
- opcjonalnie `--lifecycle <shadow_lifecycle.jsonl>`
- opcjonalnie `--shadow-entry <shadow_entries.jsonl>`

Twardy warunek price/lifecycle:

- MFE/MAE/time-to-MFE wymagaja price path lub lifecycle series.
- Jesli sciezka ceny/lifecycle nie istnieje, nie wolno wypelniac wartosci zerami.
- Jesli sciezka ceny/lifecycle nie istnieje, nie wolno inferowac MFE/MAE z samego OK/NOK.
- Brak sciezki = `unknown_reason` albo degraded class; nie `good_clean`.

Output fields:

- `label_v2_schema_version`
- `ab_record_id`
- `pool_id`
- `base_mint`
- `entry_price`
- `entry_price_source`
- `entry_price_confidence`
- `mfe_pct_10s`
- `mfe_pct_30s`
- `mfe_pct_60s`
- `mae_pct_10s`
- `mae_pct_30s`
- `mae_pct_60s`
- `time_to_mfe_ms`
- `time_to_mae_ms`
- `hit_plus_20`
- `hit_plus_40`
- `hit_plus_60`
- `hit_stop_20`
- `hit_stop_40`
- `survived_10s`
- `survived_30s`
- `survived_60s`
- `drawdown_before_plus40`
- `exit_feasible`
- `label_quality`
- `unknown_reason`

Rozdzielone osie:

- `market_outcome_class`
  - `good_clean`
  - `good_dirty`
  - `bad_clean`
  - `bad_dirty`
  - `neutral_clean`
  - `unknown`
- `execution_quality_class`
  - wypelniane przez P3.7.3
- `decision_quality_class`
  - wyliczane po feasibility join

Reguly klasyfikacji:

- `good_clean`: hit +40, acceptable MAE, usable entry price, usable label quality, not execution-infeasible.
- `good_dirty`: hit +40, ale high MAE, weak confidence, dirty path albo degraded execution feasibility.
- `bad_clean`: early death/rug/stop/adverse move z usable label quality.
- `bad_dirty`: adverse/bad path z degradacja label quality.
- `neutral_clean`: no target i no severe adverse przy usable label quality.
- `unknown`: missing price, missing entry, stale/ambiguous join, insufficient lifecycle data.

Acceptance:

- label v1 pozostaje nietkniety.
- label v2 jest additive/parallel.
- `good_clean` nie moze powstac bez usable price/lifecycle path.
- raportuje transition matrix v1 -> v2.
- brak post-hoc outcome fields w decision-time feature extraction.

### 4.4 P3.7.3 - Execution Feasibility Join

Cel:

Rozdzielic market outcome od executable opportunity.

Nowy skrypt:

- `scripts/v3_p37_lifecycle_join_report.py`

Test:

- `scripts/test_v3_p37_lifecycle_join_report.py`

Input:

- decision log,
- label v2,
- shadow entry log,
- shadow lifecycle log,
- optional shadow/onchain lifecycle report if available.

Output:

- `p3_7_execution_feasibility_join.jsonl`
- `p3_7_execution_feasibility_summary.json`
- `p3_7_execution_feasibility_summary.md`

Fields:

- `ab_record_id`
- `dispatch_expected`
- `shadow_dispatch_observed`
- `sim_status`
- `decision_to_sim_ms`
- `entry_materialized_at_ms`
- `decision_to_entry_materialization_ms`
- `quote_age_ms`
- `curve_age_ms`
- `simulation_success`
- `simulation_error_class`
- `compute_units`
- `shadow_entry_possible`
- `shadow_exit_possible`
- `no_dispatch_reason`
- `unknown_execution_status`
- `execution_quality_class`

Classes:

- `execution_feasible_clean`
- `execution_feasible_degraded`
- `execution_infeasible`
- `execution_unknown`
- `no_dispatch_expected`

Derived `decision_quality_class`:

- `good_executable`
- `good_not_executable`
- `bad_avoidable`
- `neutral`
- `unknown`

Rules:

- market-good + execution-infeasible != true good opportunity.
- unknown execution status is never success.
- REJECT/PENDING no-dispatch can be `no_dispatch_expected`, not execution failure.

Acceptance:

- join po `ab_record_id`,
- unmatched rows explicit,
- no live activation,
- no mutation of decision logs.

### 4.5 P3.7.4 - Re-label R10/R11/R13

Cel:

Utworzyc nowa baze analityczna P3.7.

Per run output:

- `logs/rollout/<run>/decisions/p3_7_label_v2.jsonl`
- `logs/rollout/<run>/reports/p3_7_label_v2_summary.json`
- `logs/rollout/<run>/reports/p3_7_execution_feasibility_summary.json`

Raport:

- `PLANS/AUDYT/RAPORT_P3_7_LABEL_V2_BASELINE_R10_R11_R13_20260518.md`

Raport musi zawierac:

- v1 good -> v2 good_clean/good_dirty/execution_infeasible/unknown,
- v1 bad -> v2 bad_clean/bad_dirty/unknown,
- v1 neutral -> v2 neutral_clean/unknown,
- label coverage,
- execution feasible ratio,
- MFE/MAE distributions,
- unknown reasons,
- R11 standalone,
- R13 standalone,
- combined only as secondary.

Acceptance:

- zadnych feature claims jeszcze,
- jesli duzo v1 good przechodzi do dirty/infeasible, Phase B musi to uwzglednic.

### 4.6 P3.7.5 - Temporal Split Baseline

Cel:

Nie dopuscic do combined-only overfitting.

Nowy skrypt:

- `scripts/v3_p37_temporal_split_report.py`

Output:

- `p3_7_temporal_split_report.json`
- `p3_7_temporal_split_report.md`

Required views:

- R11 standalone,
- R13 standalone,
- recent R11/R13,
- combined all as secondary.

Metrics:

- v2 class distribution,
- clean good/bad base rates,
- execution feasible share,
- MFE/MAE median/p75,
- time-to-MFE/time-to-MAE,
- drift R11 vs R13.

Acceptance:

- explicit `do_not_train_on_R13_then_validate_on_R13`,
- direction stability required for any candidate,
- candidate fails if direction differs between R11 and R13,
- candidate fails if effect exists only in combined,
- candidate fails if R13 standalone does not support it,
- candidate fails if confidence interval crosses zero in either R11 or R13.

## 5. Phase B - Feature Redesign

### 5.1 P3.7.6 - Replay-Only Feature Prototype

Cel:

Szukac nowych decision-time-safe feature families offline, bez runtime zmian.

Nowy skrypt:

- `scripts/v3_p37_feature_prototype_report.py`

Test:

- `scripts/test_v3_p37_feature_prototype_report.py`

Input:

- `--run name:config:label_v2:feasibility`
- `--feature-family trajectory|sell_absorption|entry_timing|buyer_continuity|risk_residual|all`
- `--output-dir <path>`
- `--json`
- `--markdown`

Fala 1:

- Trajectory Shape,
- Sell Absorption / Micro Pullback,
- Entry Timing / Too-Late.

Fala 2:

- Buyer Continuity,
- Risk Shield Residual.

Output:

- `feature_family_summary.json`
- `feature_family_summary.md`
- `per_feature_auc_overlap.json`
- `interaction_candidates.json`
- `sample_size_warnings.json`
- `leakage_risk_report.json`

Leakage taxonomy per feature:

- `decision_time_safe`
- `decision_time_degraded`
- `post_decision_observation`
- `post_hoc_label_derived`
- `execution_post_hoc`
- `forbidden`

Only `decision_time_safe` can enter candidate gate. `decision_time_degraded`
may be diagnostic only.

Candidate feature families:

#### Trajectory Shape

- `burst_decay_ratio`
- `second_wave_ratio`
- `pullback_recovery_score`
- `volume_followthrough_score`
- `buy_pressure_persistence`
- `interval_stability_after_burst`
- `hhi_recovery_or_decay`
- `top3_decay_after_t0`

#### Sell Absorption / Micro Pullback

- `first_sell_time_ms`
- `first_sell_impact_pct`
- `buy_recovery_after_first_sell`
- `sell_cluster_absorption`
- `post_sell_buy_ratio`
- `post_sell_new_signer_ratio`

#### Entry Timing / Too-Late

- `entry_drift_slope`
- `drift_since_t0_per_ms`
- `time_above_drift_cap`
- `late_edge_decay`
- `curve_progress_velocity`

#### Buyer Continuity

- `new_signer_quality_t2`
- `repeat_buyer_quality`
- `buyer_retention_without_concentration`
- `fresh_buyer_followthrough`
- `micro_whale_followthrough`

#### Risk Shield Residual

- `risk_shield_reason_family`
- `risk_shield_confidence`
- `risk_shield_evidence_quality`
- `risk_shield_subtrigger_count`
- `risk_shield_subtrigger_diversity`

Acceptance:

- primary comparison: `good_clean` vs `bad_clean` on `execution_feasible_clean`,
- `good_dirty`, `neutral_clean`, `unknown`, `execution_infeasible` reported separately,
- R11 and R13 standalone required,
- small samples marked `hypothesis_only`,
- no FSC active ranking,
- no outcome-derived features.

### 5.2 P3.7.7 - Feature Candidate Gate

Cel:

Zdecydowac, czy istnieje jakikolwiek kandydat do dalszego offline scoringu.

Candidate can proceed only if:

- enough clean samples or explicit `hypothesis_only`,
- direction stable in R11 and R13,
- R13 standalone supports effect,
- AUC/overlap improvement over P3.6 baseline is predeclared and material,
- bootstrap CI does not cross zero for key deltas,
- no leakage risk,
- execution-feasible subset preserves effect,
- feature can be represented in `MaterializedFeatureSet`.

Negative controls:

- compare to current V3 risk shield baseline,
- compare to random selector with same selected count,
- compare to active V2/V2.5 baseline where available.

If no candidate passes:

- write negative result report,
- do not run holdout,
- prepare selector-line closure or deeper redesign recommendation.

### 5.3 P3.7.8 - Pre-Registered Holdout

Cel:

Nie robic kolejnego blind runu.

Holdout allowed only after a feature candidate passes P3.7.7.

Before run, write hypothesis:

- candidate feature family,
- expected direction,
- target label subset,
- primary metric,
- minimum sample size,
- MAE/feasibility constraints,
- failure condition.

Holdout namespace:

- `shadow-burnin-v3-p37-holdout-r14-primary-only`

Holdout config:

- primary-only,
- shadow-only,
- `funding_lane_mode="disabled"`,
- V3 replay payload enabled,
- V3 promotion disabled,
- no active V2/V2.5 change,
- no IWIM change,
- no live sender change.

Holdout success requires:

- clean BUY precision `>= 0.68`,
- Wilson confidence interval reported,
- selected rows sample meaningful,
- MAE controlled,
- execution-feasible ratio acceptable,
- stable vs R11/R13,
- no post-hoc metric switching.

Failure:

- if direction breaks, candidate is killed,
- if sample too small, candidate is killed or remains `hypothesis_only`,
- if edge disappears on execution-feasible clean subset, candidate is killed,
- if MAE/exit feasibility invalidates economics, candidate is killed.

### 5.4 P3.7.9 - Conditional MaterializedFeatureSet Extension

Cel:

Runtime feature extension only after holdout-supported offline edge.

Separate ADR/plan required before implementation.

Possible structs:

- `TrajectoryShapeFeatures`
- `SellAbsorptionFeatures`
- `EntryTimingFeatures`
- `BuyerContinuityFeatures`
- `RiskShieldResidualFeatures`

Rules:

- additive only,
- `#[serde(default)]`,
- `Option<T>` / `skip_serializing_if`,
- materialized at SSOT boundary,
- no mutable state reads inside policy,
- replay hash/payload compatibility documented,
- old JSONL/TOML compatibility tests.

Acceptance:

- no implementation without separate ADR/plan,
- no P2,
- no live,
- no active V2/V2.5 regression.

### 5.5 P3.7.10 - Conditional Offline Candidate Evaluator

Cel:

Evaluate P3.7 candidate only after label v2, feasibility, feature prototype and
holdout evidence.

Possible script:

- `scripts/v3_p37_candidate_offline_report.py`

Metrics:

- `selected_good_clean`
- `selected_bad_clean`
- `selected_good_dirty`
- `selected_neutral_clean`
- `selected_unknown`
- `selected_execution_infeasible`
- `precision_clean`
- Wilson CI
- `bad_selected_rate`
- MFE/MAE distributions
- execution feasible ratio
- confidence bucket quality
- R11/R13/holdout stability
- negative controls

No P2/live implication without separate ADR.

## 6. Test plan

Core checks:

```bash
python3 -m py_compile scripts/v3_p37_outcome_label_v2.py
python3 -m py_compile scripts/v3_p37_lifecycle_join_report.py
python3 -m py_compile scripts/v3_p37_temporal_split_report.py
python3 -m py_compile scripts/v3_p37_feature_prototype_report.py
python3 -m unittest scripts/test_v3_p37_outcome_label_v2.py -v
python3 -m unittest scripts/test_v3_p37_lifecycle_join_report.py -v
python3 -m unittest scripts/test_v3_p37_feature_prototype_report.py -v
git diff --check
```

Required scenarios:

- missing price path -> no `good_clean`,
- missing lifecycle path -> explicit `unknown_reason`,
- hit +40 with high MAE -> `good_dirty`,
- severe adverse before target -> bad/adverse class,
- market-good + execution-infeasible -> not true good opportunity,
- unknown execution status is not success,
- label v1 unchanged,
- label v2 joins by `ab_record_id`,
- temporal split reports R11/R13 separately,
- feature prototype rejects post-hoc/leaky fields,
- legacy analyzer threshold sections marked `appendix_only`.

## 7. Formal kill block

If after Outcome Label v2, execution feasibility join, temporal split, feature
prototype and one pre-registered holdout:

- no candidate reaches clean BUY precision target,
- selected sample is too small,
- direction is unstable across R11/R13/holdout,
- execution-feasible clean subset does not preserve edge,
- MAE/exit feasibility invalidates economics,

then V3 selector line is closed.

Current V3 remains:

- risk shield,
- audit infrastructure,
- reason-code telemetry,
- replay/outcome tooling,
- negative control.

## 8. Assumptions and defaults

- P3.6 closure and ADR-0133 are governance prerequisites for P3.7.
- R10/R11/R13 are the immutable P3.7 baseline dataset.
- Phase A must complete before Phase B feature claims.
- No runtime feature extension, P2, live or threshold tuning is authorized by this plan.
- Fresh holdout is forbidden until a hypothesis is pre-registered.
- If no stable label-v2/feasibility/temporal-holdout edge appears, V3 selector line is closed.
