# PLAN: alpha_31100 PR-A0 decision-neutral post-hoc proof

Data: 2026-06-25

Status: `PR-A0 PROOF COMPLETE / REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`

Score name: `alpha_31100_score_pr_a0_diagnostic`

Final PR-A0 decision: `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`.

Reason: standalone alpha reduces `StopLoss` rate, but sacrifices `Target` rate, increases `TimeStop` share and does not improve conservative top-K PnL versus `selector_shadow_score`.

## Cel

Minimalnie sprawdzic, czy `alpha_31100_score_pr_a0_diagnostic` wnosi incremental value ponad obecny selector/Gatekeeper baseline na tej samej kohorcie BUY/shadow BUY/candidate-pass.

To nie jest finalny alpha model, runtime score ani BUY trigger. To jest deterministic diagnostic reranker do pierwszego falsyfikacyjnego proofu.

## Scope

In scope:

- read-only inventory R47/31100,
- offline join decision -> feature rows -> lifecycle outcome,
- deterministic score computation only po potwierdzeniu feature-bearing data,
- head-to-head ranking comparison z baseline,
- equal-count per day/per run segment,
- raport sukces/porażka bez zmian runtime.

Out of scope:

- runtime alpha,
- sidecar,
- schema freeze,
- master ledger,
- train/test harness,
- ML training,
- XGBoost,
- zmiany Gatekeeper/V2.5/V3,
- zmiany `v25_confidence`,
- progi z HTML w runtime,
- nowy BUY/REJECT path.

## Required inputs przed liczeniem

Audit wolno uruchomic dopiero gdy inventory potwierdzi:

- join coverage,
- availability baseline fields,
- availability feature families,
- `observation_window_ms = 31100`,
- outcome coverage.

Aktualny R47 inventory potwierdza wszystkie wymagane wejscia po uzyciu wlasciwego decision source:

```text
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl
```

oraz:

```text
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
```

Scalenie:

```text
python3 scripts/zbiory.py 0.01 -0.01 --directory <katalog_z_trzema_plikami>
```

Wynik inventory: `3948` scalonych rekordow, `A=387`, `B=3561`, `N=0`.

## Kohorta

Primary cohort:

- merged R47 rows z `zbiory.py 0.01 -0.01`:
  - `gatekeeper_v2_decisions.jsonl`,
  - `shadow_lifecycle.jsonl`,
  - `probe_shadow_lifecycle.jsonl`,
  - join po `base_mint == mint_id`.

Warunek wejscia do metryk:

- rekord ma feature vector dostepny do `T0+31100ms`,
- rekord ma outcome lifecycle (`Target`, `StopLoss`, `TimeStop` albo jawny `outcome_missing`),
- rekord nie uzywa outcome/leakage fields jako inputu score.

Primary cohort inventory:

- `3948` merged rows,
- `3948/3948` z `observation_window_ms = 31100`,
- `3948/3948` z terminalnym `position_closed`,
- `3948/3948` z `final_pnl_pct`,
- `3003` BUY/BUY_EXTENDED,
- `945` TIMEOUT_PHASE1_INSUFFICIENT,
- outcome mix: `Target=280`, `StopLoss=391`, `TimeStop=3277`.

Secondary cohorts, jesli dane istnieja:

- probe shadow entries z `probe_shadow_entries.jsonl` + `probe_shadow_lifecycle.jsonl`,
- szersze candidate/pass rows z `probe_selection.jsonl` i `probe_skips.jsonl`, tylko jesli maja joinowalny outcome albo sa raportowane jako availability-only,
- selector-pass/candidate-pass, jesli feature ledger zawiera jawna semantyke selector pass/below-buy/not-candidate.

## Definicja score

`alpha_31100_score_pr_a0_diagnostic`:

- offline-only,
- deterministic,
- no training,
- no XGBoost,
- no learned weights,
- no HTML thresholds,
- no runtime hooks,
- output `0.0..1.0` plus `score_validity_status`.

Mechanika:

1. Uzyj tylko decision-time features dostepnych do cutoff `T0+31100ms`.
2. Przypisz pola do rodzin cech.
3. Dla kazdej rodziny policz normalized family subscore przez rank/percentile transform w obrebie run/day segmentu.
4. Final score to srednia dostepnych family subscores.
5. Jesli brakuje wymaganych rodzin, emituj `Unavailable(reason)` zamiast `0.0`.
6. Nie imputuj outcome ani selector verdictow.

Startowe rodziny cech:

| Family | Przykladowe candidate fields |
| --- | --- |
| traction/momentum | `bonding_progress_pct`, `current_market_cap_sol`, `price_change_ratio`, `buy_count`, `total_volume_sol` |
| buy_pressure | `sol_buy_ratio`, `buy_ratio` |
| organicity | `unique_ratio`, `interval_cv`, `volume_cv` |
| concentration_toxicity | `hhi`, `top3_volume_pct` albo `top3_signer_volume_ratio`, `max_single_sell_impact_pct_observed` |
| dev_toxicity | `dev_tx_ratio`, `dev_volume_ratio`, `dev_has_sold` as negative diagnostic flag only |
| execution_toxicity | `burst_ratio`, `jito_tip_intensity`, `compute_unit_cluster_dominance`, `avg_cpi_depth_50tx` |
| cross_pool_sybil | `cpv_other_pool_activity`, `signer_cross_pool_velocity` |
| temporal | safe deltas/rates dostepne przed `T0+31100ms` |

Polarity/direction musi byc jawnie zapisana przed liczeniem i nie moze byc optymalizowana pod wynik PR-A0. Jesli polarity jest niepewne, pole trafia do `needs_review` i nie wchodzi do pierwszego score.

## Forbidden inputs

Zakazane jako input score:

- `selector_soft_score`,
- Gatekeeper verdict/reason/confidence,
- `v25_confidence` i `v25_confidence_*`,
- `v3_shadow_confidence` i `v3_shadow_confidence_*`,
- `exit_*`,
- `final_*`,
- `pnl`,
- `profit`,
- `loss`,
- `target`,
- `stop`,
- `eval`,
- `simulation`,
- `result`,
- `future`,
- `after`,
- `entry_price`,
- `exit_price`,
- `entry_value_sol`,
- `exit_value_sol`,
- `sample_age_ms`,
- absolute timestamps,
- slots/finality,
- token id / record id / join key as model input,
- config thresholds/min/max fields,
- any field unavailable before `T0+31100ms`.

IDs, join keys and timestamps sa dozwolone tylko do joinu, dedup, cohorting i equal-count bucketing.

## Baseline

Primary baseline:

- `selector_shadow_score` z `selector_shadow_score_v1.jsonl`, join po `base_mint`.

Secondary baseline:

- `selector_soft_score` z `gatekeeper_v2_decisions.jsonl`.

Baseline variance gate:

- jesli `selector_shadow_score` albo `selector_soft_score` jest nieobecny, staly albo prawie staly w BUY cohort, nie traktowac go jako pelnego rankingu,
- raportowac go jako baseline gate/context,
- porownac alpha rowniez z naturalnym/current BUY ordering i dostepnym Gatekeeper order/confidence proxy, o ile proxy jest niestale.

Potwierdzenie R47:

- `selector_shadow_score`: `3948/3948` joined, zakres `0.11115574074396377..0.49136648348893336`, `3448` unikalnych wartosci.
- `selector_soft_score`: `3948/3948`, zakres `0..12`, `13` unikalnych wartosci.
- `soft_points`: stale `0`, nie ranking.
- `v25_confidence`: stale `0.0` tam, gdzie obecne, nie ranking.

Fallback baseline:

- `timestamp`/`observation_end_ts_ms`/row order jako current BUY ordering,
- reason-code strata,
- `v3_shadow_confidence` tylko jesli ma wariancje; w aktualnym R47 probe inventory jest stale `0.0`, wiec nie jest rankingiem.

## Equal-count per day / per segment

Dla kazdego UTC day z `decision_ts_ms`:

1. Wez ta sama liczbe kandydatow dla baseline i alpha.
2. Porownuj top-k w obrebie tego samego dnia.
3. Jesli run miesci sie praktycznie w jednym dniu, dodaj segmenty czasowe, np. 6h albo kwantyle po `decision_ts_ms`.
4. Nie porownuj alpha top-k z innym budzetem wejsc niz baseline.

## Join decision -> outcome

Klucze probowane w kolejnosci:

1. `candidate_id`,
2. `pool_id`,
3. `mint_id`,
4. `base_mint`,
5. `join_key` tylko jesli jest wspolny i nie jest uzywany jako feature.

Reguly:

- wiele lifecycle rows dla jednego kandydata agregowac do jednej finalnej outcome row,
- preferowac terminal `position_closed`,
- `exit_filled` moze uzupelniac ekonomie,
- brak terminal outcome oznaczac `outcome_missing`,
- nie imputowac PnL ani close reason.

## Metryki

Raportowac per day/per segment i lacznie:

- target/stop/timestop mix,
- avg PnL,
- median PnL,
- EV after costs, jesli `estimated_costs_sol`/model kosztow jest dostepny,
- overlap baseline top-k vs alpha top-k,
- swapped-in winners/losers,
- max consecutive losses w porzadku chronologicznym,
- count candidates,
- count unavailable score,
- baseline variance summary.

Nie uzywac F1 jako decyzji tradingowej.

## Warunek sukcesu

Sukces PR-A0 oznacza, ze na tej samej kohorcie i tym samym budzecie wejsc `alpha_31100_score_pr_a0_diagnostic` pokazuje marginalna wartosc ponad baseline:

- nizszy stop rate albo lepszy target/stop/timestop mix,
- nie gubi targetow szybciej niz usuwa stopy,
- dodatni albo wyraznie lepszy EV after costs na konserwatywnym top-k, jesli koszty sa dostepne,
- stabilnosc w segmentach czasowych,
- wynik nie jest wyjasniony przez missingness, IDs, outcome leakage ani baseline overlap.

## Warunek porazki

Sygnał nalezy uznac za obalony na poziomie PR-A0, jesli:

- poprawia discovery/balanced metryki, ale nie poprawia naturalnego BUY ledgeru,
- dziala tylko przez missingness/coverage,
- jest niemal tozsamy z selector ordering,
- usuwa targety razem ze stopami bez poprawy EV,
- wynik znika po reason-code strata albo segmentacji czasowej,
- wymaga oceniania odrzuconych pooli bez counterfactual execution proof.

## Expected outputs nastepnego offline proofu

- `PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_POSTHOC_PROOF_20260625.md`
- offline tables:
  - baseline variance,
  - feature family availability,
  - score availability,
  - equal-count top-k,
  - swapped-in/swapped-out,
  - outcome mix,
  - EV after costs, jesli koszt dostepny.

## Current blockers

Brak blockerow danych dla pierwszego PR-A0 post-hoc proofu. Pozostaja blokady zakresowe:

- scoring musi byc offline-only,
- scoring musi byc allowlist-only,
- outcome/config/id/leakage fields z merged rows musza byc jawnie wykluczone z inputu,
- `selector_shadow_score` i `selector_soft_score` sa baseline, nie input alpha,
- wynik proofu nie moze zmieniac runtime policy.

## Exact next step

Napisac i uruchomic minimalny offline PR-A0 proof, ktory:

1. Tworzy temp working directory z trzema logami albo uzywa wskazanego katalogu.
2. Uruchamia `scripts/zbiory.py 0.01 -0.01`.
3. Liczy `alpha_31100_score_pr_a0_diagnostic` tylko z allowlisty decision-time feature families.
4. Joinuje `selector_shadow_score_v1.jsonl` jako primary baseline.
5. Porownuje alpha vs baseline przez equal-count top-k per segment.
6. Generuje raport:

```text
PLANS/AUDYT/RAPORT_ALPHA_31100_PR_A0_R47_POSTHOC_PROOF_20260625.md
```

Nie tworzyc runtime source, sidecara, schem JSON ani modelu ML.
