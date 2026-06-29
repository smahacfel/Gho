# Golden trace: ambiguous_or_sparse 3

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 496acLcZWGMw2CvpdriV3Pr7L2G8HB95Bx3XkSicBzDn
- base_mint: FtvSZp9CekKVsfF7AzcmfptqhE69kznQLgrvrEkipump
- entry_ts_ms: 1782411459877
- entry_price: 1.6341583859173357e-07
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782411459877 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782411466502 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782411491330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-63.35541428571428 |
| lifecycle position_closed | 1782411491331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-63.35541428571428 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 6536 | path_bps | pnl_bps=-974 |
| path point | 6896 | path_bps | pnl_bps=-935 |
| path point | 7409 | path_bps | pnl_bps=-865 |
| path point | 7768 | path_bps | pnl_bps=-818 |
| path point | 8017 | path_bps | pnl_bps=-621 |
| path point | 8761 | path_bps | pnl_bps=-1043 |
| path point | 9792 | path_bps | pnl_bps=-1933 |
| path point | 10449 | path_bps | pnl_bps=654 |
| path point | 10920 | path_bps | pnl_bps=55 |
| path point | 11831 | path_bps | pnl_bps=-109 |
| path point | 12411 | path_bps | pnl_bps=-79 |
| path point | 12881 | path_bps | pnl_bps=23 |
| path point | 13390 | path_bps | pnl_bps=-51 |
| path point | 13907 | path_bps | pnl_bps=-258 |
| path point | 14300 | path_bps | pnl_bps=-193 |
| path point | 14683 | path_bps | pnl_bps=-636 |
| path point | 15210 | path_bps | pnl_bps=-696 |
| path point | 16309 | path_bps | pnl_bps=-391 |
| path point | 16639 | path_bps | pnl_bps=-238 |
| path point | 17320 | path_bps | pnl_bps=-78 |
| path point | 18195 | path_bps | pnl_bps=-151 |
| path point | 18836 | path_bps | pnl_bps=-40 |
| path point | 19917 | path_bps | pnl_bps=-47 |
| path point | 20954 | path_bps | pnl_bps=-68 |
| path point | 21382 | path_bps | pnl_bps=-458 |
| path point | 21879 | path_bps | pnl_bps=-679 |
| path point | 22356 | path_bps | pnl_bps=-2491 |
| path point | 22783 | path_bps | pnl_bps=-2787 |
| path point | 23330 | path_bps | pnl_bps=-2904 |
| path point | 23875 | path_bps | pnl_bps=-4253 |
| path point | 24392 | path_bps | pnl_bps=-4329 |
| path point | 25327 | path_bps | pnl_bps=-4911 |
| path point | 25878 | path_bps | pnl_bps=-4940 |
| path point | 26908 | path_bps | pnl_bps=-4997 |
| path point | 27122 | path_bps | pnl_bps=-5026 |
| path point | 28387 | path_bps | pnl_bps=-5059 |
| path point | 28552 | path_bps | pnl_bps=-5100 |
| path point | 29344 | path_bps | pnl_bps=-5348 |
| path point | 29862 | path_bps | pnl_bps=-5727 |
| path omitted |  | path_bps | 86 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7769; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.6341583859173357e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
