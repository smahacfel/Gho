# PR-RCE-A0: Regime-Confirmed Entry offline proof

Data: `2026-06-29`

Status: `SPEC_AND_LOGGING_SURFACE_PREP / OFFLINE_ONLY / NO_RUNTIME`

## Cel

PR-RCE-A0 jest finalna, ograniczona proba edge-search po zamknieciu:

- ORG-A0,
- R48/R2 global exit matrix,
- TSV2 A1/A2/A3,
- EIX,
- RTP-A0,
- RUG-MARKUP-A0.

Nie kontynuujemy zadnej z tych sciezek. Nie robimy kolejnego strojenia masek, progow, gridow ani selector rerankera.

Nowa hipoteza:

> Edge, jesli istnieje, nie lezy w statycznej klasyfikacji poola ani w exit-only damage reduction. Moze wymagac pozniejszego wejscia dopiero po tym, jak pre-entry tape potwierdzi przejscie w kontrolowana kontynuacje.

To oznacza:

- pozniejsze wejscie,
- mniejszy target,
- krotszy i scislej kontrolowany horyzont,
- najpierw logging-only evidence,
- brak runtime decision.

## Twarde granice

RCE-A0:

- nie jest runtime,
- nie jest `shadow_close_only`,
- nie jest active close,
- nie jest kolejnym ORG/TSV2/RUG/RTP pass,
- nie zmienia BUY/REJECT,
- nie zmienia Gatekeeper policy,
- nie zmienia selector runtime,
- nie zmienia `v25_confidence`,
- nie zmienia V3 promotion,
- nie zmienia TX builder/sender/Jito/live path,
- nie uzywa `alpha_31100`,
- nie uzywa XGBoost,
- nie uzywa final PnL / target / stop / timeout jako input feature,
- nie uzywa sciezki po horyzoncie decyzji jako input feature,
- nie robi broad grid search,
- nie uruchamia nowego runu bez osobnej zgody.

## Wymaganie danych

RCE-A0 wymaga swiezej logging-only probki, chyba ze istniejace logi maja juz pelna decision-time surface:

- `gatekeeper_v2_decisions.jsonl`,
- `materialized_feature_snapshot` albo rownowazny embedded decision surface,
- `pre_entry_path_summary_v1`,
- `session_regime_snapshot_v1`,
- `shadow_exit_replay_v1.jsonl`,
- `shadow_lifecycle.jsonl`,
- `probe_shadow_lifecycle.jsonl`,
- launcher PASS report,
- `pre_run_manifest.json`,
- `post_run_manifest.json`.

Aktualne R49/R50/RUG evidence nie jest wystarczajace dla RCE, bo nie zawiera nowej powierzchni `pre_entry_path_summary_v1` / `session_regime_snapshot_v1`.

Jesli nie ma sponsor approval na jeden swiezy logging-only scope, trading edge search nalezy zamknac.

## Logging-only feature surface

Nowa powierzchnia jest addytywna w `MaterializedFeatureSet`, a wiec trafia do istniejacego `materialized_feature_snapshot` w `gatekeeper_v2_decisions.jsonl`.

Nie tworzony jest nowy sidecar.

### `pre_entry_path_summary_v1`

Decision-time safe fields:

- `pre_entry_ret_5s`
- `pre_entry_ret_10s`
- `pre_entry_ret_20s`
- `pre_entry_ret_30s`
- `pre_entry_ret_45s`
- `pre_entry_mfe_10s`
- `pre_entry_mfe_20s`
- `pre_entry_mfe_30s`
- `pre_entry_mfe_45s`
- `pre_entry_mae_10s`
- `pre_entry_mae_20s`
- `pre_entry_mae_30s`
- `pre_entry_mae_45s`
- `pullback_depth_bps`
- `reclaim_bps`
- `reclaim_fraction`
- `higher_low_count`
- `above_0bps_dwell_ms`
- `above_300bps_dwell_ms`
- `above_600bps_dwell_ms`

### `session_regime_snapshot_v1`

Decision-time safe fields:

- `same_ms_tx_ratio_recent`
- `same_ms_tx_ratio_decay`
- `burst_ratio_recent`
- `burst_ratio_decay`
- `unique_ratio_recent`
- `unique_ratio_drift`
- `top3_signer_volume_ratio_recent`
- `top3_signer_volume_ratio_drift`
- `buy_sell_ratio_recent`
- `session_pool_rate_5m`
- `session_pool_rate_10m`
- `session_followthrough_rate_10m_optional`
- `template_reason_code`
- `veto_reason_code`

`session_pool_rate_5m`, `session_pool_rate_10m` i `session_followthrough_rate_10m_optional` pozostaja nullable, dopoki nie ma bezpiecznego wlasciciela tych metryk w SSOT. Nie wolno ich proxy-rekonstruowac z outcome.

## Predeclared templates

Offline proof moze testowac tylko:

1. `T1_BREAKOUT_RETEST_RECLAIM`
2. `T2_STAIRSTEP_CONTINUATION`
3. `T3_HOT_SESSION_RECLAIM_WITH_TOXICITY_DECAY`

Nie wolno dodac T4/T5/T20 po zobaczeniu wynikow.

### T1 intent

- impuls cenowy,
- kontrolowany pullback,
- reclaim poprzedniego poziomu,
- brak ostrego pogorszenia koncentracji.

### T2 intent

- sekwencja higher lows / stair-step continuation,
- dodatni dwell nad wczesnymi poziomami,
- brak glebokiego MAE przed wejściem.

### T3 intent

- hot session context,
- reclaim po pullbacku,
- toxicity albo koncentracja nie pogarsza sie materialnie.

## Fixed exit grid offline

Tylko:

- `target_bps = 600, 900, 1200`,
- `stop_bps = -250, -400, -600`,
- `max_hold_ms = 10000, 20000, 30000`,
- `costs_bps = 100, 200`.

Brak broad grid search.

## R51 logging-only scope

Proponowany scope:

`shadow-burnin-v3-r51-rce-logging-only-target12-stop6-maxwait45000-r1`

Hard runtime boundaries:

- `entry_mode = "shadow_only"`,
- `execution_mode = "shadow"`,
- no active close,
- no `shadow_close_only`,
- no BUY/REJECT change,
- no Gatekeeper policy change,
- no selector runtime change,
- no `alpha_31100`,
- no XGBoost,
- no TX/Jito/live path change.

Allowed:

- observation/logging window do `45000 ms`,
- richer decision-time logging only,
- shadow-only evidence collection.

## Evidence retention

R51 nie moze pisac aktywnych logow do archive volume przez symlink.

Wymagane:

- archive volume read-only by default,
- active logs under local real dirs or explicitly configured non-archive active path,
- `pre_run_manifest.json`,
- `post_run_manifest.json`,
- cleanup only through `scripts/guard_rollout_evidence_cleanup.py`,
- no raw JSONL commit.

## Acceptance dla R51 single-scope

`RCE_PROMISING_SINGLE_SCOPE_ONLY` wymaga:

- `precision_cost100 >= 0.65`,
- `wilson_lower95 >= 0.60`,
- `cost100_sum_pnl_bps > 0`,
- `cost200_sum_pnl_bps >= 0` albo jawnie policzony near-flat friction gap,
- `median_cost100 >= 0`,
- wynik po usunieciu top 5% positive records `>= 0`,
- internal holdout passes,
- `selected_count >= 250`,
- no leakage.

## Immediate kill criteria

Zamknac bez runtime, jesli:

- brak pelnej evidence surface,
- najlepszy template precision `< 0.55`,
- `median_cost100 < 0`,
- wynik ginie po usunieciu top 5%,
- wymaga nowych template,
- wymaga szerszego gridu,
- wymaga runtime change,
- retention manifest missing.

## Decyzja

`runtime_approval = false`

`shadow_close_only_approval = false`

`active_close_approval = false`

`run_started = false`

Rekomendacja po tym kroku:

`GO_R51_LOGGING_ONLY` tylko jako osobno zatwierdzony, logging-only evidence collection. Bez zgody na ten jeden scope: `NO_GO_CLOSE_PROJECT`.
