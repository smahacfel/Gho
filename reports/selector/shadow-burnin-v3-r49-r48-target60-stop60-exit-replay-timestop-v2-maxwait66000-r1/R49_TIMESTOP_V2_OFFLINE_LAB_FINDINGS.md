# R49 TimeStop V2 Offline Lab Report

- scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- mode: single policy counterfactual
- policy: `TARGET=+6000bps`, `STOP=-6000bps`, `max_hold_ms=120000`
- recommendation: `TIMESTOP_V2_COUNTERFACTUAL_PROMISING`
- generated_at: `2026-06-26T11:05:34.580476+00:00`

## Artefakty uzyte jako wejscie

- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl`

## Artefakty wynikowe

- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_single_policy_6000_-6000_120000_report_v1.json`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_single_policy_6000_-6000_120000_exit_v1.jsonl`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_SINGLE_POLICY_6000_-6000_120000_REPORT.md`

## Coverage

- simulated_positions: `768`
- positions_with_exit_replay: `648`
- positions_with_tsv2_windows: `768`
- positions_with_tsv2_windows_rate: `100.00%`
- candidate_positions: `753`
- candidate_positions_rate_over_windows: `98.05%`
- candidate_before_terminal: `631`
- candidate_before_terminal_rate: `83.80%`

## Actual terminal distribution

- STOP: `30`
- TARGET: `43`
- TIMEOUT: `690`
- UNKNOWN: `5`

## Candidate class distribution

- heartbeat_only_candidate: `3`
- no_candidate: `15`
- no_progress_with_volume_candidate: `68`
- stale_data_no_action: `6`
- weak_no_progress_candidate: `676`

## Counterfactual economics

- active_exit_eligible_positions: `625`
- saved_stop_count: `11`
- timeout_improved_count: `226`
- targets_cut_by_tsv2: `13`
- beneficial_exit_count: `237`
- harmful_exit_count: `95`
- neutral_exit_count: `293`
- delta_sum_bps: `118578`
- delta_avg_bps: `189.7248`
- delta_median_bps: `0.0`

## Single-policy matrix row

- baseline_target_count: `23`
- baseline_stop_count: `15`
- baseline_timeout_count: `610`
- baseline_sum_pnl_bps: `-249695`
- baseline_avg_pnl_bps: `-385.3317901234568`
- tsv2_exit_count: `625`
- tsv2_target_count: `10`
- tsv2_stop_count: `4`
- tsv2_timeout_count: `9`
- tsv2_sum_pnl_bps: `-131117`
- tsv2_avg_pnl_bps: `-202.34104938271605`
- pnl_delta_sum_bps: `118578`
- pnl_delta_avg_bps: `189.7248`

## Resurrection checks

- alive_within_4000ms_after_candidate: `{'alive_count': 30, 'alive_rate': 0.0398406374501992, 'candidate_rows': 753}`
- alive_within_8000ms_after_candidate: `{'alive_count': 54, 'alive_rate': 0.07171314741035857, 'candidate_rows': 753}`

## Wnioski

- TimeStop V2 ma bardzo wysokie coverage w pozycjach z oknami TSV2 i duzo kandydatow przed terminalem.
- Dla pojedynczej polityki `+6000/-6000/120000ms` counterfactual jest dodatni ekonomicznie i klasyfikowany jako `TIMESTOP_V2_COUNTERFACTUAL_PROMISING`.
- Glowna wartosc pochodzi z poprawy TIMEOUT/dead-flow oraz czesciowego ratowania STOP-ow.
- Ryzyko pozostaje materialne: TimeStop V2 tnie czesc pozniejszych TARGET-ow, dlatego to nie jest approval dla aktywnego exit.
- R49 nadal pracuje, wiec raport jest snapshotem, nie finalnym zamknieciem runu.
