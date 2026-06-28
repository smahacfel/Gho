# PR-RUG-MARKUP-A0: Offline proof

Data: `2026-06-29`

Final verdict: `RUG_MARKUP_REJECTED_FOR_RUNTIME`

## Decyzja

NO R51. CLOSE TRADING EDGE SEARCH.

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`new_run_approval = false`

## Zakres i ograniczenia

To jest offline-only proof. Nie zmienia runtime, Gatekeepera, BUY/REJECT, selector runtime, `v25_confidence`, V3 promotion, TX builder/sender/Jito/live path, `shadow_close_only`, active close, sidecarow, `alpha_31100`, XGBoost ani zadnych progow runtime.

Skrypt nie uruchamia runu i nie wykonuje cleanupu. Surowe JSONL pozostaja lokalnym dowodem i nie sa przeznaczone do commita.

## Evidence inventory

Pierwszy wygenerowany artefakt:

`reports/selector/rug_markup_a0_evidence_inventory.csv`

| scope | has_gatekeeper_v2_decisions | has_materialized_feature_snapshot | has_shadow_exit_replay_v1 | full_evidence_for_rug_markup_a0 | blocking_reason |
| --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1 | False | False | True | False | missing_gatekeeper_v2_decisions_jsonl;missing_materialized_or_pre_entry_fields |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | True | True | True | True |  |

## Scope coverage

| scope | decision_rows | replay_rows | joined_records | exact_join_rate | unjoined_replay_rows | malformed_decision_rows | malformed_replay_rows |
| --- | --- | --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | 13405 | 3656 | 3633 | 0.9937089715536105 | 23 | 1 | 1 |

Full evidence scopes: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

Full evidence scope count: `1`

## Fixed classifier family

- `R0_BROAD`
- `R1_DEV_SIGNER_CONCENTRATION`
- `R2_BUY_BURST_MARKUP`
- `R3_SCAMBOT_COORDINATION`
- `R4_MARKUP_WITH_DUMP_RISK`

Klasyfikatory korzystaja tylko z decision-time/pre-entry fieldow z `gatekeeper_v2_decisions.jsonl` / `materialized_feature_snapshot`. `pool_id`, `base_mint` i identyfikatory sa uzyte tylko do joinu i sortowania chronologicznego, nie jako features.

## Fixed exit grid

- target_bps: `1000, 1500, 2000, 2500`
- stop_bps: `-300, -500, -700, -1000`
- max_hold_ms: `20000, 30000, 40000`
- costs: `100, 200`

Nie uzyto broad grid search, nowych masek ani strojenia R50.

## Best diagnostic rows

| scope | classifier | target_bps | stop_bps | max_hold_ms | retained_count | precision_cost100 | wilson_lower95_cost100 | cost100_sum_pnl_bps | cost200_sum_pnl_bps | cost100_median_pnl_bps | passes_single_scope_gates | acceptance_failures |
| --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- | --- |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2500 | -300 | 40000 | 983 | 0.12309257375381485 | 0.1040090443651834 | -71381 | -169681 | -347.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2500 | -300 | 30000 | 983 | 0.12614445574771108 | 0.10683401465229493 | -76546 | -174846 | -253.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2500 | -300 | 20000 | 983 | 0.13936927772126145 | 0.11911945653580146 | -80399 | -178699 | -211.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2000 | -300 | 40000 | 983 | 0.1271617497456765 | 0.10777654970217936 | -84059 | -182359 | -327.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2000 | -300 | 20000 | 983 | 0.14140386571719227 | 0.12101549890147119 | -86023 | -184323 | -211.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 2000 | -300 | 30000 | 983 | 0.13021363173957273 | 0.11060672766943573 | -90891 | -189191 | -241.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 1500 | -300 | 40000 | 983 | 0.14140386571719227 | 0.12101549890147119 | -98500 | -196800 | -267.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 1500 | -300 | 20000 | 983 | 0.14750762970498474 | 0.1267125872391917 | -100286 | -198586 | -211.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R4_MARKUP_WITH_DUMP_RISK | 1500 | -300 | 30000 | 983 | 0.14140386571719227 | 0.12101549890147119 | -103354 | -201654 | -228.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |
| shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1 | R2_BUY_BURST_MARKUP | 2500 | -300 | 40000 | 1214 | 0.11779242174629324 | 0.10085300264199212 | -105544 | -226944 | -242.0 | False | precision_cost100_lt_65pct;cost100_sum_not_positive;cost200_sum_not_positive_for_promising;median_cost100_negative;median_cost200_negative_for_promising;top5_tail_removed_negative |

## Acceptance

Hard gates dla `PROMISING` wymagaja tej samej fixed rule na co najmniej dwoch niezaleznych scope. W obecnym evidence set `1` requested scope ma pelny pre-entry + replay evidence.

Passing fixed rules across two scopes: `0`

Single-scope signal rows: `0`

Best rule: `R4_MARKUP_WITH_DUMP_RISK/2500/-300/40000` on `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`

## Leakage guard

Zakazane fieldy nie sa uzywane jako classifier inputs: final PnL, target/stop/timeout labels, future path after decision horizon, pool/mint/signer/dev wallet IDs oraz outcome-derived labels. `shadow_exit_replay_v1.path_bps` sluzy tylko do ewaluacji target/stop/hold i tail audit.

## Output files

- `reports/selector/rug_markup_a0_evidence_inventory.csv`
- `reports/selector/rug_markup_a0_summary.csv`
- `reports/selector/rug_markup_a0_cost_sensitivity.csv`
- `reports/selector/rug_markup_a0_stability.csv`
- `reports/selector/rug_markup_a0_tail_audit.csv`
- `reports/selector/rug_markup_a0_threshold_manifest.csv`
- `docs/ADR/ADR_8D_RUG_MARKUP_A0_RESULT_20260629.md`

## Runtime decision

Nie istnieje zgoda na runtime ani `shadow_close_only` z tego A0.
