# RAPORT: alpha_31100 PR-A0 R47 post-hoc proof

Data: 2026-06-25

Status: `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0 / NO RUNTIME PROMOTION`

Score: `alpha_31100_score_pr_a0_diagnostic`

## Scope

Ten raport wykonuje minimalny PR-A0 proof na scalonym R47/31100 ledgerze.

Non-goals:

- brak runtime alpha,
- brak sidecara,
- brak schema freeze,
- brak master ledger,
- brak ML/XGBoost,
- brak zmian Gatekeeper/V2.5/V3,
- brak zmian `v25_confidence`,
- brak progow z HTML w runtime.

## Werdykt

Final decision: `REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0`.

`alpha_31100_score_pr_a0_diagnostic` nie pokazal wystarczajacej samodzielnej incremental value ponad primary baseline `selector_shadow_score`.

Glowny wzorzec:

- alpha obniza `StopLoss` rate wzgledem `selector_shadow_score`,
- ale robi to glownie przez przesuniecie selekcji w `TimeStop`,
- traci `Target` rate,
- pogarsza albo nie poprawia `avg/median final_pnl_pct` na konserwatywnych top-K,
- swapped-in alpha rows maja gorszy target mix niz swapped-out selector rows.

Combined diagnostic (`rank(selector_shadow_score) + rank(alpha)`) jest ciekawy tylko badawczo:

- `top_1%` i `top_10%` wygladaja lepiej niz sam selector,
- ale `top_2%` i `top_5%` sa gorsze,
- dlatego combined nie jest jeszcze stabilnym dowodem i nie moze byc runtime inputem.

Decyzja PR-A0: nie promowac alpha do runtime. Nie tworzyc runtime integration, sidecara ani zmian `v25`/V3/Gatekeeper. Nastepny sensowny krok, jesli temat kontynuowac, to tylko falsyfikacja wariantow score offline: TimeStop bucket analysis, combined diagnostic anomaly review, ablation family, polarity sanity, rank-normalization variants i chronological repeat na kolejnym runie.

## Dokladne wejscia

Katalog roboczy:

```text
/tmp/gho_alpha_31100_pr_a0.nEnLoY
```

Trzy logi zlinkowane do katalogu `/tmp` dla `scripts/zbiory.py`:

```text
/tmp/gho_alpha_31100_pr_a0.nEnLoY/gatekeeper_v2_decisions.jsonl
-> /root/Gho/logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/gatekeeper_v2_decisions.jsonl

/tmp/gho_alpha_31100_pr_a0.nEnLoY/shadow_lifecycle.jsonl
-> /root/Gho/logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl

/tmp/gho_alpha_31100_pr_a0.nEnLoY/probe_shadow_lifecycle.jsonl
-> /root/Gho/logs/shadow_run/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl
```

Baseline sidecar uzyty do primary baseline:

```text
/root/Gho/logs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051/selector_shadow_score_v1.jsonl
```

Scalenie:

```text
python3 scripts/zbiory.py 0.01 -0.01 --directory /tmp/gho_alpha_31100_pr_a0.nEnLoY
```

Wynik:

- `3948` merged rows,
- `zbior_A.jsonl = 387`,
- `zbior_B.jsonl = 3561`,
- `zbior_N.jsonl = 0`.

## Consistency gates

Ledger nie miesza roznych konfiguracji poza R47/R38 maxwait31100.

| Field | Unique value | Count |
| --- | --- | ---: |
| `run_id` | `shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1` | 3948 |
| `session_id` | `r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1` | 3948 |
| `brain_config_path` | `/root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml` | 3948 |
| `brain_config_hash` | `9c02f4c4f92fb9934948c699da77550dd9704e2d9f9dd1ea3c18efb8690b2e89` | 3948 |
| `config_hash` | `8b506cc2b631260ea2f828e5fe1dc15b58c79efa2e4ce7a3cca675e057d87051` | 3948 |
| `decision_plane` | `legacy_live` | 3948 |
| `gatekeeper_version` | `v2.2` | 3948 |
| `observation_window_ms` | `31100` | 3948 |
| `observation_duration_ms` | `31100` | 3948 |
| `max_wait_time_ms` | `31100` | 3948 |

Time scope:

- UTC day: `2026-06-24`,
- run span: `13.0709 h`.

Outcome ledger:

| Outcome | Count |
| --- | ---: |
| `Target` | 280 |
| `StopLoss` | 391 |
| `TimeStop` | 3277 |

Overall:

- avg `final_pnl_pct`: `-7.9056`,
- median `final_pnl_pct`: `-2.0353`,
- total `final_pnl_pct`: `-31211.2850`,
- total `net_pnl_sol`: `-2.184789951`,
- `estimated_costs_sol` total: `0.0`.

## Alpha definition

`alpha_31100_score_pr_a0_diagnostic` jest:

- offline-only,
- deterministic,
- threshold-free,
- no ML,
- no XGBoost,
- no outcome inputs,
- no selector/confidence/verdict inputs,
- no config threshold inputs,
- no id/join/timestamp as score inputs.

Mechanika:

1. Dla kazdego allowlist field liczony jest percentile rank w calym PR-A0 ledgerze.
2. Kierunek `+` oznacza wyzsza wartosc lepsza.
3. Kierunek `-` oznacza nizsza wartosc lepsza.
4. Kazda rodzina dostaje srednia swoich dostepnych field ranks.
5. Final alpha score = srednia dostepnych family scores.
6. Brak rodziny daje status `degraded_missing_*`, nie `0.0`.

Allowlist:

| Family | Fields | Direction |
| --- | --- | --- |
| traction/momentum | `bonding_progress_pct`, `current_market_cap_sol`, `price_change_ratio`, `buy_count`, `total_volume_sol` | higher better |
| buy_pressure | `sol_buy_ratio`, `buy_ratio` | higher better |
| organicity | `unique_ratio`, `interval_cv` | higher better |
| concentration_toxicity | `hhi`, `top3_volume_pct`, `max_single_sell_impact_pct_observed`, `max_single_tx_price_impact_pct_observed` | lower better |
| dev_toxicity | `dev_tx_ratio`, `dev_volume_ratio`, `dev_has_sold` | lower/false better |
| execution_toxicity | `burst_ratio`, `jito_tip_intensity`, `compute_unit_cluster_dominance`, `avg_cpi_depth_50tx` | lower better |
| cross_pool_sybil | `cpv_other_pool_activity`, `signer_cross_pool_velocity` | lower better |
| temporal | `delta_buy_count_*` higher, `delta_jito_tip_intensity_*` lower, `delta_signer_cross_pool_velocity_*` lower | mixed |

`volume_cv` zostal celowo pominiety w alpha PR-A0, bo discovery polarity jest niejednoznaczne miedzy przekrojami.

## Alpha validity and missingness

| Status | Count |
| --- | ---: |
| `valid_all_families` | 3221 |
| `degraded_missing_cross_pool_sybil` | 322 |
| `degraded_missing_organicity_cross_pool_sybil_temporal` | 319 |
| `degraded_missing_cross_pool_sybil_temporal` | 44 |
| `degraded_missing_temporal` | 42 |

Family count:

| Family scores available | Count |
| ---: | ---: |
| 8 | 3221 |
| 7 | 364 |
| 6 | 44 |
| 5 | 319 |

Missing family counts:

| Missing family | Count |
| --- | ---: |
| `cross_pool_sybil` | 685 |
| `temporal` | 405 |
| `organicity` | 319 |

Selected field coverage:

| Field | Coverage |
| --- | ---: |
| core traction/buy/dev fields | 100.00% |
| `unique_ratio`, `interval_cv`, `hhi`, `top3_volume_pct` | 91.92% |
| `jito_tip_intensity` | 71.78% |
| `compute_unit_cluster_dominance` | 60.26% |
| `cpv_other_pool_activity`, `signer_cross_pool_velocity` | 82.65% |
| `delta_buy_count_1s_to_2s`, `delta_buy_count_1s_to_3s` | 89.74% |
| `delta_signer_cross_pool_velocity_1s_to_2s` | 55.78% |
| `delta_signer_cross_pool_velocity_1s_to_3s` | 54.69% |

## Top-K comparison

Primary baseline: `selector_shadow_score`.

Secondary baseline: `selector_soft_score` as bucket/gate baseline.

Combined diagnostic: average of percentile ranks of `selector_shadow_score` and `alpha_31100_score_pr_a0_diagnostic`; analysis only, no runtime claim.

| ordering | K | n | target_rate | stop_rate | timestop_rate | avg_pnl_pct | median_pnl_pct | total_pnl_pct | total_net_sol | max_loss_streak |
| --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| selector_shadow | 0.5% | 20 | 0.4000 | 0.3500 | 0.2500 | -4.9280 | 0.4935 | -98.5609 | -0.0069 | 3 |
| selector_shadow | 1% | 39 | 0.3077 | 0.5128 | 0.1795 | -16.4837 | -50.6181 | -642.8628 | -0.0450 | 4 |
| selector_shadow | 2% | 79 | 0.3544 | 0.4557 | 0.1899 | -9.8079 | -28.0337 | -774.8224 | -0.0542 | 7 |
| selector_shadow | 5% | 197 | 0.3299 | 0.4162 | 0.2538 | -11.6029 | -31.7683 | -2285.7640 | -0.1600 | 10 |
| selector_shadow | 10% | 395 | 0.2835 | 0.3772 | 0.3392 | -14.4790 | -31.4402 | -5719.2201 | -0.4003 | 17 |
| selector_shadow | 20% | 790 | 0.2177 | 0.3304 | 0.4519 | -15.3752 | -22.5045 | -12146.4274 | -0.8502 | 21 |
| alpha | 0.5% | 20 | 0.2500 | 0.0500 | 0.7000 | -13.9945 | -29.7506 | -279.8892 | -0.0196 | 5 |
| alpha | 1% | 39 | 0.2564 | 0.2564 | 0.4872 | -18.3065 | -31.4402 | -713.9520 | -0.0500 | 9 |
| alpha | 2% | 79 | 0.2532 | 0.2785 | 0.4684 | -17.0269 | -30.3805 | -1345.1222 | -0.0942 | 8 |
| alpha | 5% | 197 | 0.2487 | 0.2944 | 0.4569 | -14.5220 | -25.6285 | -2860.8433 | -0.2003 | 15 |
| alpha | 10% | 395 | 0.2228 | 0.2911 | 0.4861 | -14.1603 | -21.0341 | -5593.3141 | -0.3915 | 13 |
| alpha | 20% | 790 | 0.2076 | 0.2506 | 0.5418 | -12.8038 | -18.7022 | -10115.0114 | -0.7081 | 16 |
| combined_diag | 0.5% | 20 | 0.3500 | 0.4500 | 0.2000 | -14.3017 | -26.1226 | -286.0349 | -0.0200 | 5 |
| combined_diag | 1% | 39 | 0.4359 | 0.3846 | 0.1795 | -4.8108 | 6.9849 | -187.6198 | -0.0131 | 4 |
| combined_diag | 2% | 79 | 0.2911 | 0.4304 | 0.2785 | -19.6654 | -35.3935 | -1553.5654 | -0.1087 | 11 |
| combined_diag | 5% | 197 | 0.2995 | 0.3756 | 0.3249 | -15.2870 | -34.0462 | -3011.5434 | -0.2108 | 10 |
| combined_diag | 10% | 395 | 0.3165 | 0.3620 | 0.3215 | -12.0482 | -31.4402 | -4759.0565 | -0.3331 | 14 |
| combined_diag | 20% | 790 | 0.2392 | 0.2861 | 0.4747 | -13.5999 | -26.7810 | -10743.9207 | -0.7521 | 11 |
| selector_soft_bucket | 5% | 197 | 0.1827 | 0.1371 | 0.6802 | -8.0073 | -8.5449 | -1577.4400 | -0.1104 | 13 |
| chrono_latest | 5% | 197 | 0.0609 | 0.0660 | 0.8731 | -8.3399 | -2.2093 | -1642.9548 | -0.1150 | 38 |

Interpretacja:

- Alpha standalone nie przebija `selector_shadow_score` na target rate ani top-K PnL.
- Alpha standalone ma nizszy StopLoss rate, ale z wysokim TimeStop rate.
- `selector_soft_score` bucket wyglada lagodniej na PnL, ale ma niski target rate i bardzo duzo TimeStop; traktowac jako gate/bucket, nie primary ranking.
- Chronological latest jest tylko sanity baseline i nie separuje targetow.

## Swapped-in / swapped-out: alpha vs selector_shadow

`swapped_in` = rekordy w top-K alpha, ktorych nie bylo w top-K `selector_shadow_score`.

`swapped_out` = rekordy usuniete z top-K `selector_shadow_score` przez alpha.

| K | overlap | swapped_in_target | swapped_in_stop | swapped_in_timestop | swapped_in_avg_pnl | swapped_out_target | swapped_out_stop | swapped_out_timestop | swapped_out_avg_pnl |
| ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| 0.5% | 0.0000 | 0.2500 | 0.0500 | 0.7000 | -13.9945 | 0.4000 | 0.3500 | 0.2500 | -4.9280 |
| 1% | 0.1282 | 0.2647 | 0.2059 | 0.5294 | -15.8683 | 0.3235 | 0.5000 | 0.1765 | -13.7774 |
| 2% | 0.1646 | 0.2121 | 0.2576 | 0.5303 | -19.1199 | 0.3333 | 0.4697 | 0.1970 | -10.4790 |
| 5% | 0.3655 | 0.2080 | 0.2160 | 0.5760 | -13.8576 | 0.3360 | 0.4080 | 0.2560 | -9.2570 |
| 10% | 0.4785 | 0.1553 | 0.2087 | 0.6359 | -12.0012 | 0.2718 | 0.3738 | 0.3544 | -12.6124 |
| 20% | 0.5430 | 0.1080 | 0.1053 | 0.7867 | -11.1523 | 0.1302 | 0.2798 | 0.5900 | -16.7795 |

Interpretacja:

- Alpha swaps reduce stops.
- But swapped-in rows are mostly TimeStop and have lower target rate than swapped-out selector rows.
- This is not a clean utility improvement; it is mostly a toxicity/downside filter that also filters out too many strong target candidates.

## Equal-count run segments

Run ma jeden UTC day, wiec dodatkowo podzielono go chronologicznie na cztery rowne segmenty `Q1..Q4`. W kazdym segmencie liczony jest equal-count top 5% (`K=49`).

| segment | ordering | n | K | target_rate | stop_rate | timestop_rate | avg_pnl_pct | total_pnl_pct | max_loss_streak |
| --- | --- | ---: | ---: | ---: | ---: | ---: | ---: | ---: | ---: |
| Q1 | selector_shadow | 987 | 49 | 0.2449 | 0.4286 | 0.3265 | -17.1024 | -838.0189 | 12 |
| Q1 | alpha | 987 | 49 | 0.2449 | 0.3061 | 0.4490 | -10.9762 | -537.8357 | 13 |
| Q1 | combined_diag | 987 | 49 | 0.2857 | 0.4490 | 0.2653 | -16.1588 | -791.7814 | 11 |
| Q2 | selector_shadow | 987 | 49 | 0.4490 | 0.3673 | 0.1837 | 0.4919 | 24.1055 | 4 |
| Q2 | alpha | 987 | 49 | 0.3061 | 0.3061 | 0.3878 | -8.5997 | -421.3865 | 11 |
| Q2 | combined_diag | 987 | 49 | 0.4082 | 0.3265 | 0.2653 | -1.6075 | -78.7680 | 9 |
| Q3 | selector_shadow | 987 | 49 | 0.3061 | 0.3673 | 0.3265 | -13.4102 | -657.1015 | 16 |
| Q3 | alpha | 987 | 49 | 0.2449 | 0.2245 | 0.5306 | -10.5216 | -515.5605 | 7 |
| Q3 | combined_diag | 987 | 49 | 0.3265 | 0.2653 | 0.4082 | -10.7600 | -527.2407 | 11 |
| Q4 | selector_shadow | 987 | 49 | 0.2653 | 0.4898 | 0.2449 | -21.4797 | -1052.5033 | 10 |
| Q4 | alpha | 987 | 49 | 0.2041 | 0.3061 | 0.4898 | -23.7631 | -1164.3921 | 7 |
| Q4 | combined_diag | 987 | 49 | 0.2449 | 0.4082 | 0.3469 | -24.2404 | -1187.7799 | 11 |

Segment interpretation:

- Alpha beats selector on avg/total PnL in Q1 and Q3.
- Selector beats alpha decisively in Q2 and Q4.
- Alpha target rate is never above selector in any segment top 5%.
- Alpha stop reduction is consistent, but it comes with materially higher TimeStop rate.

## Conclusion

Final PR-A0 status:

```text
REJECTED_AS_STANDALONE_RERANKER_FOR_PR_A0
```

PR-A0 falsifies the strongest simple claim:

```text
alpha_31100_score_pr_a0_diagnostic standalone > selector_shadow_score
```

That claim is not supported on R47.

Narrower claim that remains plausible:

```text
alpha_31100_score_pr_a0_diagnostic contains a toxicity/downside component
that may be useful as a veto/modulator after separate validation.
```

But this narrower claim is not enough for runtime integration. It requires a separate offline follow-up:

1. Family ablation: especially concentration/dev/execution/cross_pool vs momentum.
2. Polarity sanity: check whether some directions are inverted in R47.
3. Combined diagnostic stability: repeat on another chronological run.
4. Missingness sentinel: compare valid-only vs degraded records.
5. Equal-count utility on BUY-only and candidate-pass separately.

No runtime changes are recommended from PR-A0.
