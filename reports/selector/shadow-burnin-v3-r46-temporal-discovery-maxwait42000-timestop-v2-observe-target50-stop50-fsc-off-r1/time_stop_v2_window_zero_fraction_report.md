# TimeStop V2 Window Zero-Fraction Report

- input_paths: `/root/Gho/logs/shadow_run/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1/shadow_lifecycle.jsonl, /root/Gho/logs/shadow_run/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl`
- window_rows: `35732`
- positions: `2718`
- invalid_json_lines_skipped: `4`
- inferred_base_window_ms: `4000`
- tested_window_ms: `[4000, 8000, 12000, 16000, 20000, 24000, 28000, 32000]`
- thresholds: `zero_or_missing <= 0.30`, `missing <= 0.15`

## Status Mix

- status_counts: `{'stale_or_insufficient': 11689, 'weak': 18710, 'alive': 3653, 'heartbeat': 1680}`
- subreason_counts: `{'stale_or_missing_market_sample': 11667, 'low_vitality_no_meaningful_progress': 6567, 'alive_meaningful_progress': 3653, 'no_new_market_sample': 12143, 'micro_tx_heartbeat_no_price_progress': 1680, 'invalid_market_sample': 15, 'missing_market_sample': 7}`

## Metric Zero-Fraction

| metric | window_ms | chunks | present | zero_all | missing_all | zero_or_missing | median_abs | max_abs |
|---|---:|---:|---:|---:|---:|---:|---:|---:|
| total_tx_delta | 4000 | 35732 | 24058 | 34.0% | 32.7% | 66.7% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 4000 | 35732 | 24058 | 34.1% | 32.7% | 66.7% | 0.0000 | 166.4752 |
| price_delta_pct | 4000 | 35732 | 24058 | 34.1% | 32.7% | 66.7% | 0.0000 | 132.4617 |
| mcap_delta_pct | 4000 | 35732 | 24058 | 34.1% | 32.7% | 66.7% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 4000 | 35732 | 24058 | 48.5% | 32.7% | 81.1% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 4000 | 35732 | 11903 | 0.0% | 66.7% | 66.7% | 0.2524 | 166.4752 |
| total_tx_delta | 8000 | 18908 | 13332 | 29.9% | 29.5% | 59.4% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 8000 | 18908 | 13332 | 30.0% | 29.5% | 59.5% | 0.0000 | 166.4752 |
| price_delta_pct | 8000 | 18908 | 13332 | 30.0% | 29.5% | 59.5% | 0.0000 | 132.4617 |
| mcap_delta_pct | 8000 | 18908 | 13332 | 30.0% | 29.5% | 59.5% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 8000 | 18908 | 13332 | 44.2% | 29.5% | 73.7% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 8000 | 18908 | 7673 | 0.0% | 59.4% | 59.5% | 0.2524 | 166.4752 |
| total_tx_delta | 12000 | 13312 | 9571 | 27.8% | 28.1% | 55.9% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 12000 | 13312 | 9571 | 27.9% | 28.1% | 56.0% | 0.0000 | 166.4752 |
| price_delta_pct | 12000 | 13312 | 9571 | 27.9% | 28.1% | 56.0% | 0.0000 | 132.4617 |
| mcap_delta_pct | 12000 | 13312 | 9571 | 27.9% | 28.1% | 56.0% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 12000 | 13312 | 9571 | 41.6% | 28.1% | 69.7% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 12000 | 13312 | 5864 | 0.1% | 55.9% | 56.0% | 0.2524 | 166.4752 |
| total_tx_delta | 16000 | 9761 | 7269 | 25.0% | 25.5% | 50.6% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 16000 | 9761 | 7269 | 25.1% | 25.5% | 50.7% | 0.0000 | 166.4752 |
| price_delta_pct | 16000 | 9761 | 7269 | 25.1% | 25.5% | 50.7% | 0.0000 | 132.4617 |
| mcap_delta_pct | 16000 | 9761 | 7269 | 25.1% | 25.5% | 50.7% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 16000 | 9761 | 7269 | 39.3% | 25.5% | 64.8% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 16000 | 9761 | 4824 | 0.1% | 50.6% | 50.7% | 0.2524 | 166.4752 |
| total_tx_delta | 20000 | 8543 | 6297 | 25.6% | 26.3% | 51.9% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 20000 | 8543 | 6297 | 25.7% | 26.3% | 52.0% | 0.0000 | 166.4752 |
| price_delta_pct | 20000 | 8543 | 6297 | 25.7% | 26.3% | 52.0% | 0.0000 | 132.4617 |
| mcap_delta_pct | 20000 | 8543 | 6297 | 25.7% | 26.3% | 52.0% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 20000 | 8543 | 6297 | 38.9% | 26.3% | 65.1% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 20000 | 8543 | 4109 | 0.1% | 51.9% | 52.0% | 0.2524 | 166.4752 |
| total_tx_delta | 24000 | 7699 | 5597 | 25.7% | 27.3% | 53.0% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 24000 | 7699 | 5597 | 25.7% | 27.3% | 53.0% | 0.0000 | 166.4752 |
| price_delta_pct | 24000 | 7699 | 5597 | 25.7% | 27.3% | 53.0% | 0.0000 | 132.4617 |
| mcap_delta_pct | 24000 | 7699 | 5597 | 25.7% | 27.3% | 53.0% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 24000 | 7699 | 5597 | 38.2% | 27.3% | 65.5% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 24000 | 7699 | 3622 | 0.1% | 53.0% | 53.0% | 0.2524 | 166.4752 |
| total_tx_delta | 28000 | 5611 | 4421 | 21.0% | 21.2% | 42.2% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 28000 | 5611 | 4421 | 21.1% | 21.2% | 42.3% | 0.0000 | 166.4752 |
| price_delta_pct | 28000 | 5611 | 4421 | 21.1% | 21.2% | 42.3% | 0.0000 | 132.4617 |
| mcap_delta_pct | 28000 | 5611 | 4421 | 21.1% | 21.2% | 42.3% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 28000 | 5611 | 4421 | 35.9% | 21.2% | 57.1% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 28000 | 5611 | 3243 | 0.1% | 42.2% | 42.3% | 0.2524 | 166.4752 |
| total_tx_delta | 32000 | 5232 | 4079 | 21.5% | 22.0% | 43.6% | 0.0000 | 257.0000 |
| total_volume_sol_delta | 32000 | 5232 | 4079 | 21.7% | 22.0% | 43.7% | 0.0000 | 166.4752 |
| price_delta_pct | 32000 | 5232 | 4079 | 21.7% | 22.0% | 43.7% | 0.0000 | 132.4617 |
| mcap_delta_pct | 32000 | 5232 | 4079 | 21.7% | 22.0% | 43.7% | 0.0000 | 132.4617 |
| bonding_progress_delta_pct | 32000 | 5232 | 4079 | 35.9% | 22.0% | 57.9% | 0.0000 | 100.0000 |
| avg_volume_per_tx_sol_window | 32000 | 5232 | 2952 | 0.1% | 43.6% | 43.7% | 0.2524 | 166.4752 |

## Recommendation

- overall_recommended_window_ms_for_key_metrics: `None`
- rule: `max of per-key-metric first acceptable windows; null means at least one key metric did not meet thresholds within tested multiples`

| metric | key_metric | recommended_window_ms | meets_threshold | reason |
|---|---:|---:|---:|---|
| total_tx_delta | True | - | False | no_candidate_window_met_thresholds |
| total_volume_sol_delta | True | - | False | no_candidate_window_met_thresholds |
| price_delta_pct | True | - | False | no_candidate_window_met_thresholds |
| mcap_delta_pct | True | - | False | no_candidate_window_met_thresholds |
| bonding_progress_delta_pct | True | - | False | no_candidate_window_met_thresholds |
| avg_volume_per_tx_sol_window | False | - | False | no_candidate_window_met_thresholds |

## Unavailable Expected Metrics

- `total_buyers_delta`: not emitted in TimeStop V2 window rows
- `unique_buyers_delta`: not emitted in TimeStop V2 window rows
- `total_unique_buyers_delta`: not emitted in TimeStop V2 window rows

## Interpretation Notes

- `zero_all` counts numeric zero deltas over all candidate chunks. Missing deltas are not silently treated as zero.
- Invalid JSONL lines are skipped by default and counted in `invalid_json_lines_skipped`; use `--strict-json` to fail fast.
- `zero_or_missing` is the stricter operational noise estimate: zero numeric deltas plus missing metric chunks.
- Candidate windows larger than the runtime cadence are synthetic groups of consecutive per-position windows.
- The script cannot validate a window smaller than the captured runtime cadence.
