# RAPORT: alpha_31100 PR-A0 R47 inventory

Data: 2026-06-25

Status: `READ-ONLY INVENTORY COMPLETE / READY FOR PR-A0 POST-HOC PROOF`

Zakres: minimalny, decision-neutral inventory dla przyszlego proofu `alpha_31100_score_pr_a0_diagnostic`.

Non-goals:

- brak runtime alpha,
- brak sidecara,
- brak zmian Gatekeeper/V2.5/V3,
- brak schema freeze,
- brak master ledger,
- brak treningu ML/XGBoost,
- brak kopiowania progow z HTML/Segment Lab do runtime.

## Werdykt inventory

R47 ma wystarczajace artefakty do pierwszego decision-neutral PR-A0 proofu:

- run/config identity,
- observation window `31100 ms`,
- outcome coverage dla BUY/shadow BUY,
- join keys decision -> lifecycle/outcome,
- decision-time feature fields,
- baseline ranking fields.

Poprzedni blocker wyniknal z patrzenia tylko na standardowe `logs/shadow_run/*` i `seer_runtime_coverage_audit.jsonl`. Wlasciwy R47 feature-bearing source jest tutaj:

```text
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl
```

Scalenie z lifecycle wykonuje istniejacy skrypt:

```text
scripts/zbiory.py 0.01 -0.01
```

Skrypt wymaga, aby `gatekeeper_v2_decisions.jsonl`, `shadow_lifecycle.jsonl` i `probe_shadow_lifecycle.jsonl` byly w jednym katalogu. Uruchomienie wykonano w izolowanym katalogu `/tmp/gho_alpha_31100_pr_a0.nEnLoY` na symlinkach do logow, bez modyfikowania repo.

## Sprawdzone katalogi i pliki

### Analiza/discovery

```text
analiza/0_cv_wyniki.csv
analiza/1_statystyki_rozkladow.csv
analiza/2_pelna_analiza_cech.csv
analiza/3_temporal_wyniki.csv
analiza/4_odrzucone_pola_leakage.csv
analiza/analizadodatnievsujemne.html
analiza/analizatop175.html
analiza/analizatop600.html
analiza/raport.html
analiza/zbiorcze temporals.txt
```

Wnioski:

- `analiza/analizadodatnievsujemne.html` wskazuje R47 natural A/B: `A=294`, `B=2789`.
- `analiza/analizatop175.html` wskazuje R47 target-vs-stop/top slice: `A=175`, `B=175`.
- `analiza/raport.html` i `analiza/analizatop600.html` wskazuja starsze/niewystarczajaco R47-oznaczone zbiory `31` i `31/top600`.
- `analiza/2_pelna_analiza_cech.csv` ma summary feature importance/distribution, nie raw rekordy.
- `analiza/4_odrzucone_pola_leakage.csv` ma leakage blacklist summary, nie raw rekordy.
- Discovery HTML/CSV moga sluzyc jako audit material, ale nie jako zrodlo progow runtime ani jako surowy PR-A0 ledger.

### R47 operational logs

Run:

```text
shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1
```

Sprawdzone pliki:

```text
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1-buys.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/shadow_entries.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_selection.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_entries.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_skips.jsonl
logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_transport.jsonl
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/seer_runtime_coverage_audit.jsonl
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl
logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/selector_shadow_score_v1.jsonl
```

Liczby rekordow:

| Plik | Rekordy valid | Bad JSON | Rola |
| --- | ---: | ---: | --- |
| `*-buys.jsonl` | 3349 | 1 | BUY decision/dispatch evidence |
| `shadow_entries.jsonl` | 3317 | 0 | shadow BUY entry rows |
| `shadow_lifecycle.jsonl` | 9375 | 1 | shadow lifecycle/outcomes |
| `probe_selection.jsonl` | 1119 | 0 | selected probe rows |
| `probe_shadow_entries.jsonl` | 1061 | 0 | probe shadow entries |
| `probe_shadow_lifecycle.jsonl` | 1962 | 0 | probe outcomes |
| `probe_skips.jsonl` | 10229 | 0 | unselected probe/candidate rows |
| `seer_runtime_coverage_audit.jsonl` | 1357 | 0 | Seer coverage audit, not Gatekeeper 31.1s feature ledger |
| `gatekeeper_v2_decisions.jsonl` | 11299 | 0 | feature-bearing R47 decision rows |
| `selector_shadow_score_v1.jsonl` | 11299 | 0 | decision-neutral selector baseline sidecar |

### PR-A0 merged ledger via zbiory.py

Uruchomienie:

```text
python3 scripts/zbiory.py 0.01 -0.01 --directory /tmp/gho_alpha_31100_pr_a0.nEnLoY
```

Wejscia:

```text
gatekeeper_v2_decisions.jsonl
shadow_lifecycle.jsonl
probe_shadow_lifecycle.jsonl
```

Wynik:

| Metric | Value |
| --- | ---: |
| Lifecycle scanned | 11338 |
| Lifecycle unique `mint_id` kept | 3948 |
| Lifecycle duplicate `mint_id` skipped | 4020 |
| Lifecycle better-version replacements | 3949 |
| Lifecycle without `final_pnl_pct` skipped | 3369 |
| Lifecycle JSON errors skipped | 1 |
| Decisions scanned | 11299 |
| Decisions outside lifecycle mint set skipped | 7351 |
| Merged records | 3948 |
| Lifecycle without matching decision | 0 |
| `zbior_A.jsonl` (`final_pnl_pct >= 0.01`) | 387 |
| `zbior_B.jsonl` (`final_pnl_pct <= -0.01`) | 3561 |
| `zbior_N.jsonl` | 0 |

Merged rows:

- `3948/3948` maja `observation_window_ms = 31100`,
- `3948/3948` maja `observation_duration_ms = 31100`,
- `3948/3948` maja `max_wait_time_ms = 31100`,
- `3948/3948` maja `run_id = shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1`,
- `3948/3948` maja `decision_plane = legacy_live`,
- `3948/3948` maja `base_mint == _merged_mint_id`,
- `3948/3948` maja terminalny `record_type = position_closed`.

Outcome mix po podziale `0.01 -0.01`:

- A: `Target=280`, `TimeStop=107`,
- B: `TimeStop=3170`, `StopLoss=391`.

### R47 event stream

Sprawdzony katalog:

```text
datasets/events/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/
```

Wynik skanu top-level event payload:

| Event type | Count |
| --- | ---: |
| `PoolTransaction` | 1151402 |
| `NewPoolDetected` | 11320 |
| `Candidate` | 11299 |
| `PositionOpened` | 3018 |
| `PositionClosed` | 3013 |
| `ExitSubmitted` | 3003 |
| `ExitFilled` | 3003 |

Ten strumien jest potencjalnie wystarczajacy do odtworzenia czesci cech, ale sam nie zawiera gotowych materialized feature fields ani `selector_soft_score`. Odtwarzanie cech z event streamu byloby osobnym etapem, nie PR-A0 inventory.

### Lokalny raw-like plik poza R47

```text
scripts/zbior_A.jsonl
```

Wynik:

- `198` rekordow,
- `run_id = shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1`,
- `observation_window_ms = 42000`,
- zawiera feature-bearing pola i outcome,
- nie jest R47/31100.

Ten plik potwierdza, jaki ksztalt raw feature-bearing recordow jest potrzebny, ale nie moze byc uzyty jako R47/31100 PR-A0 evidence.

## Potwierdzenie observation window

Konfiguracja:

```text
configs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml
configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml
```

Dowody:

- rollout config wskazuje brain config `ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`,
- brain config ma `max_wait_time_ms = 31100`,
- runtime log zawiera synchronizacje Gatekeeper alias: `observation_window_ms 1780 -> 31100`,
- `probe_selection.jsonl`: `observation_end_ts_ms - observation_start_ts_ms = 31100` dla `1119/1119` rekordow,
- `probe_skips.jsonl`: `observation_end_ts_ms - observation_start_ts_ms = 31100` dla `10229/10229` rekordow,
- R47 HTML reports pokazuja `observation_duration_ms = 31100` dla R47 natural i R47 top175.

Uwaga: `seer_runtime_coverage_audit.jsonl` ma `window_ms=2000` w coverage audit. To nie jest Gatekeeper observation window dla tego PR-A0 proofu.

## Kohorty

### Primary candidate: shadow BUY

Zrodla:

```text
shadow_entries.jsonl
shadow_lifecycle.jsonl
```

Join/outcome:

- entries: `3317`,
- lifecycle rows: `9375`,
- outcome-like rows: `6016`,
- join by `candidate_id`: `3013/3317 = 90.84%`,
- join by `pool_id`: `3013/3317 = 90.84%`,
- join by `mint_id`: `3013/3317 = 90.84%`,
- close reasons: `TimeStop=2344`, `StopLoss=391`, `Target=278`.

Ocena: primary cohort do post-hoc rankingu jest teraz potwierdzona przez `zbiory.py`: `3948` scalonych rekordow, w tym `3003` BUY/BUY_EXTENDED oraz `945` TIMEOUT_PHASE1_INSUFFICIENT z outcome lifecycle.

### Secondary candidate: probe shadow

Zrodla:

```text
probe_shadow_entries.jsonl
probe_shadow_lifecycle.jsonl
```

Join/outcome:

- entries: `1061`,
- lifecycle rows: `1962`,
- outcome-like rows: `1962`,
- join by `candidate_id`: `981/1061 = 92.46%`,
- close reasons: `TimeStop=979`, `Target=2`.

Ocena: uzyteczne jako secondary cohort, ale target/stop mix jest bardzo waski. Nie powinno byc primary proofem marginalnej wartosci alpha.

### Candidate/pass availability rows

Zrodla:

```text
probe_selection.jsonl
probe_skips.jsonl
```

Distribution:

- `probe_selection`: `BUY=42`, `TIMEOUT_PHASE1_INSUFFICIENT=1077`.
- `probe_skips`: `BUY=3310`, `TIMEOUT_PHASE1_INSUFFICIENT=4798`, `TIMEOUT_PHASE1_NO_DATA=1087`, `REJECT_SELECTOR_NOT_CANDIDATE=690`, `REJECT_SELECTOR_BELOW_BUY=344`.

Ocena: dobre do sprawdzania population/availability, ale nie maja kompletu outcome dla pominietych kandydatow.

## Baseline availability i wariancja

Baseline fields po wlasciwym joinie:

- `selector_soft_score`: obecny w `3948/3948`, dyskretny zakres `0..12`, `13` unikalnych wartosci.
- `selector_soft_score_candidate_passed`: obecny w `3948/3948`, `True=3632`, `False=316`.
- `selector_soft_score_buy_passed`: obecny w `3948/3948`, `True=3628`, `False=320`.
- `selector_shadow_score`: joinowalny z `selector_shadow_score_v1.jsonl` dla `3948/3948`, ciagly zakres `0.11115574074396377..0.49136648348893336`, `3448` unikalnych wartosci.
- `soft_points`: obecny w `3948/3948`, ale staly `0`, wiec nie jest rankingiem.
- `v25_confidence`: obecny tylko czesciowo i staly `0.0`, wiec nie jest rankingiem.
- `v3_shadow_confidence` w probe rows pozostaje context/gate, nie ranking.

Dystrybucja `selector_soft_score`:

| Score | Count |
| ---: | ---: |
| 12 | 960 |
| 11 | 648 |
| 10 | 398 |
| 9 | 297 |
| 8 | 213 |
| 7 | 148 |
| 6 | 127 |
| 5 | 148 |
| 4 | 362 |
| 3 | 327 |
| 2 | 4 |
| 1 | 13 |
| 0 | 303 |

Wniosek:

- Primary baseline dla PR-A0 moze byc `selector_shadow_score` z sidecara, bo ma wysoka wariancje i jawne claim boundaries (`changes_gatekeeper_decision=false`, `changes_execution=false`).
- Secondary baseline moze byc dyskretny `selector_soft_score`.
- `soft_points`, `v25_confidence` i stale confidence fields nie sa ranking baseline.

## Feature family availability

Feature family availability w scalonych `3948` R47/31100 rekordach:

| Family | Fields | Coverage |
| --- | --- | ---: |
| traction/momentum | `bonding_progress_pct`, `current_market_cap_sol`, `price_change_ratio`, `buy_count`, `total_volume_sol` | 100% |
| buy_pressure | `sol_buy_ratio`, `buy_ratio` | 100% |
| organicity | `volume_cv` | 100% |
| organicity | `unique_ratio`, `interval_cv` | 91.92% |
| concentration_toxicity | `max_single_sell_impact_pct_observed`, `max_single_tx_price_impact_pct_observed` | 100% |
| concentration_toxicity | `hhi`, `top3_volume_pct` | 91.92% |
| dev_toxicity | `dev_tx_ratio`, `dev_volume_ratio`, `dev_has_sold` | 100% |
| execution_toxicity | `burst_ratio` | 100% |
| execution_toxicity | `avg_cpi_depth_50tx` | 99.97% |
| execution_toxicity | `jito_tip_intensity` | 71.78% |
| execution_toxicity | `compute_unit_cluster_dominance` | 60.26% |
| cross_pool_sybil | `cpv_other_pool_activity`, `signer_cross_pool_velocity` | 82.65% |
| temporal | `delta_buy_count_1s_to_2s`, `delta_buy_count_1s_to_3s` | 89.74% |
| temporal | `delta_jito_tip_intensity_1s_to_2s` | 87.08% |
| temporal | `delta_jito_tip_intensity_1s_to_3s` | 84.65% |
| temporal | `delta_signer_cross_pool_velocity_1s_to_2s` | 55.78% |
| temporal | `delta_signer_cross_pool_velocity_1s_to_3s` | 54.69% |

Wniosek: R47/31100 feature family availability jest wystarczajaca do PR-A0 post-hoc proofu, z warunkiem jawnego `Unavailable(reason)` dla rodzin o nizszym pokryciu.

## Name-based leakage audit

`analiza/4_odrzucone_pola_leakage.csv` zawiera `436` odrzuconych pol. Klasy ryzyka potwierdzone po nazwach:

| Pattern | Przyklady |
| --- | --- |
| outcome/exit | `exit_price`, `exit_value_sol`, `total_exits` |
| final/pnl | `final_pnl`, `final_pnl_pct`, `gross_pnl_sol`, `net_pnl_sol` |
| eval | `eval_count`, `total_tx_evaluated`, `unique_signers_evaluated`, `decision_eval_snapshots` |
| timestamps | `timestamp`, `timestamp_ms`, `sample_timestamp_ms` |
| slot/finality | `entry_slot`, `sample_slot`, `curve_finality`, `curve_finality_is_finalized` |
| ids/join | `source_ab_record_id`, `ab_record_id`, `join_key`, `record_type` |
| confidence/output | `v25_confidence_*`, `v3_shadow_confidence_*` |

PR-A0 score nie moze uzywac tych pol jako inputu. `selector_soft_score`, verdicty i confidence moga byc tylko baseline/context, nigdy inputem do `alpha_31100_score_pr_a0_diagnostic`.

## Readiness gates

Status po korekcie inventory:

| Gate | Status | Evidence |
| --- | --- | --- |
| join coverage | PASS | `zbiory.py`: `3948` merged, `0` lifecycle unmatched |
| baseline fields availability | PASS | `selector_shadow_score` joined `3948/3948`, `selector_soft_score` `3948/3948` |
| baseline variance | PASS | `selector_shadow_score`: `3448` unique; `selector_soft_score`: `13` unique |
| feature family availability | PASS | core families present with coverage from `60.26%` to `100%` |
| observation window | PASS | merged rows `3948/3948` have `31100` |
| outcome coverage | PASS | merged rows `3948/3948` have `final_pnl_pct` and terminal `position_closed` |
| leakage/input separation | TODO before scoring | merged rows contain outcome/config/id/leakage fields; scoring implementation must use explicit allowlist |

Nastepny krok po inventory:

1. Nie generowac jeszcze runtime code ani sidecara.
2. Zbudowac offline PR-A0 proof script/report na `/tmp` albo w `PLANS/AUDYT` jako raport, uzywajac allowlisty decision-time feature fields.
3. Porownac `alpha_31100_score_pr_a0_diagnostic` z:
   - primary baseline `selector_shadow_score`,
   - secondary baseline `selector_soft_score`,
   - current/natural ordering po `timestamp`/`observation_end_ts_ms` jako sanity baseline.
4. Raportowac equal-count top-k oraz outcome/EV metrics bez uzycia F1 i bez progow z HTML.
