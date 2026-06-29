# Golden trace: timeout 1

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 2KNcCaTtG6htM2x8RvBNFzzBGozAVLTSHbSEnZABiB7y
- base_mint: 7mfGVbDkaxkU12STCuB6fzUESKdNqZHRQnx5rpfGpump
- entry_ts_ms: 1782405885467
- entry_price: 1.5756512872961546e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405885467 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405892450 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782405938333 | shadow_lifecycle.jsonl | close_reason=None; pnl=-49.358557142857144 |
| lifecycle position_closed | 1782405938333 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-49.358557142857144 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 3442 | path_bps | pnl_bps=-4754 |
| path point | 7363 | path_bps | pnl_bps=-4754 |
| path point | 8364 | path_bps | pnl_bps=-4754 |
| path point | 9864 | path_bps | pnl_bps=-4754 |
| path point | 10864 | path_bps | pnl_bps=-4770 |
| path point | 11864 | path_bps | pnl_bps=-4770 |
| path point | 13364 | path_bps | pnl_bps=-4770 |
| path point | 14364 | path_bps | pnl_bps=-4770 |
| path point | 15863 | path_bps | pnl_bps=-4770 |
| path point | 16864 | path_bps | pnl_bps=-4770 |
| path point | 18364 | path_bps | pnl_bps=-4770 |
| path point | 19364 | path_bps | pnl_bps=-4770 |
| path point | 20364 | path_bps | pnl_bps=-4770 |
| path point | 21369 | path_bps | pnl_bps=-4770 |
| path point | 22555 | path_bps | pnl_bps=-4883 |
| path point | 23863 | path_bps | pnl_bps=-4883 |
| path point | 24864 | path_bps | pnl_bps=-4883 |
| path point | 26364 | path_bps | pnl_bps=-4883 |
| path point | 27364 | path_bps | pnl_bps=-4883 |
| path point | 28364 | path_bps | pnl_bps=-4883 |
| path point | 29864 | path_bps | pnl_bps=-4883 |
| path point | 30864 | path_bps | pnl_bps=-4883 |
| path point | 32363 | path_bps | pnl_bps=-4883 |
| path point | 33364 | path_bps | pnl_bps=-4883 |
| path point | 34866 | path_bps | pnl_bps=-4883 |
| path point | 36363 | path_bps | pnl_bps=-4883 |
| path point | 37363 | path_bps | pnl_bps=-4883 |
| path point | 38363 | path_bps | pnl_bps=-4883 |
| path point | 39364 | path_bps | pnl_bps=-4883 |
| path point | 40364 | path_bps | pnl_bps=-4883 |
| path point | 41364 | path_bps | pnl_bps=-4883 |
| path point | 42364 | path_bps | pnl_bps=-4883 |
| path point | 43364 | path_bps | pnl_bps=-4883 |
| path point | 44364 | path_bps | pnl_bps=-4883 |
| path point | 45864 | path_bps | pnl_bps=-4883 |
| path point | 46864 | path_bps | pnl_bps=-4883 |
| path point | 48364 | path_bps | pnl_bps=-4883 |
| path point | 49864 | path_bps | pnl_bps=-4883 |
| path point | 51363 | path_bps | pnl_bps=-4883 |
| path omitted |  | path_bps | 59 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-4883; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.5756512872961546e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
