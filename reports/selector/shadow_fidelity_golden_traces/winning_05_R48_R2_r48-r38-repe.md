# Golden trace: winning 5

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: FUMnCx5hGuuteZSw3bg2DmXq1L7csz6jwg88P3ddA68e
- base_mint: 2Dd7Zkidnd9UkrTd4m4YQh5yiMQTvUQNGd1Sk98gpump
- entry_ts_ms: 1782406389429
- entry_price: 2.831325292457681e-08
- result: target
- logged_result: target
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406389429 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406397482 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406398330 | shadow_lifecycle.jsonl | close_reason=None; pnl=67.2083 |
| lifecycle position_closed | 1782406398331 | shadow_lifecycle.jsonl | close_reason=Target; pnl=67.2083 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 7979 | path_bps | pnl_bps=4071 |
| path point | 8486 | path_bps | pnl_bps=6894 |
| path point | 9060 | path_bps | pnl_bps=2288 |
| path point | 9882 | path_bps | pnl_bps=1306 |
| path point | 10901 | path_bps | pnl_bps=154 |
| path point | 11151 | path_bps | pnl_bps=1076 |
| path point | 11901 | path_bps | pnl_bps=-4021 |
| path point | 12902 | path_bps | pnl_bps=-4021 |
| path point | 14023 | path_bps | pnl_bps=-2997 |
| path point | 14881 | path_bps | pnl_bps=-2262 |
| path point | 15347 | path_bps | pnl_bps=-2108 |
| path point | 15852 | path_bps | pnl_bps=-2965 |
| path point | 16315 | path_bps | pnl_bps=-4473 |
| path point | 16499 | path_bps | pnl_bps=-4360 |
| path point | 17331 | path_bps | pnl_bps=-4531 |
| path point | 17871 | path_bps | pnl_bps=-5411 |
| path point | 18260 | path_bps | pnl_bps=-6496 |
| path point | 18902 | path_bps | pnl_bps=-5348 |
| path point | 19350 | path_bps | pnl_bps=-4722 |
| path point | 20371 | path_bps | pnl_bps=-5188 |
| path point | 20889 | path_bps | pnl_bps=-3857 |
| path point | 21388 | path_bps | pnl_bps=-2070 |
| path point | 21851 | path_bps | pnl_bps=-2649 |
| path point | 22398 | path_bps | pnl_bps=-2495 |
| path point | 22840 | path_bps | pnl_bps=-3236 |
| path point | 23397 | path_bps | pnl_bps=-2327 |
| path point | 23837 | path_bps | pnl_bps=-1505 |
| path point | 24380 | path_bps | pnl_bps=-956 |
| path point | 24809 | path_bps | pnl_bps=-3352 |
| path point | 25263 | path_bps | pnl_bps=-309 |
| path point | 25706 | path_bps | pnl_bps=-124 |
| path point | 26304 | path_bps | pnl_bps=-6 |
| path point | 26848 | path_bps | pnl_bps=-4058 |
| path point | 27328 | path_bps | pnl_bps=-5076 |
| path point | 27877 | path_bps | pnl_bps=-5457 |
| path point | 28289 | path_bps | pnl_bps=-7120 |
| path point | 28846 | path_bps | pnl_bps=-6639 |
| path point | 29174 | path_bps | pnl_bps=-6537 |
| path point | 29744 | path_bps | pnl_bps=-6384 |
| path omitted |  | path_bps | 94 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-9091; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `2.831325292457681e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
