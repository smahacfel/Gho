# Golden trace: losing 4

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 3Ca3nZtDi1awM6dkK1NEEhqp3HRGDgQRE1aS8eUxjjvr
- base_mint: 8LtH3uCJXXcp9Sjnsgsy7KEx52wREqzp4DUeohAVpump
- entry_ts_ms: 1782406308431
- entry_price: 1.0455640051265851e-07
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406308431 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406317189 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406391830 | shadow_lifecycle.jsonl | close_reason=None; pnl=-72.66194285714286 |
| lifecycle position_closed | 1782406391831 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-72.66194285714286 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 8506 | path_bps | pnl_bps=-1512 |
| path point | 9116 | path_bps | pnl_bps=-1569 |
| path point | 10399 | path_bps | pnl_bps=-1564 |
| path point | 11400 | path_bps | pnl_bps=-1584 |
| path point | 11489 | path_bps | pnl_bps=-1555 |
| path point | 12153 | path_bps | pnl_bps=-1517 |
| path point | 12646 | path_bps | pnl_bps=-1466 |
| path point | 12975 | path_bps | pnl_bps=-1531 |
| path point | 14381 | path_bps | pnl_bps=-1506 |
| path point | 15891 | path_bps | pnl_bps=-1462 |
| path point | 16272 | path_bps | pnl_bps=-1390 |
| path point | 17379 | path_bps | pnl_bps=-1335 |
| path point | 17937 | path_bps | pnl_bps=-1271 |
| path point | 19351 | path_bps | pnl_bps=-1185 |
| path point | 19805 | path_bps | pnl_bps=-1109 |
| path point | 20242 | path_bps | pnl_bps=-1081 |
| path point | 20720 | path_bps | pnl_bps=-994 |
| path point | 21128 | path_bps | pnl_bps=-945 |
| path point | 22724 | path_bps | pnl_bps=-397 |
| path point | 23889 | path_bps | pnl_bps=-335 |
| path point | 24883 | path_bps | pnl_bps=-237 |
| path point | 25239 | path_bps | pnl_bps=-183 |
| path point | 25879 | path_bps | pnl_bps=-68 |
| path point | 26779 | path_bps | pnl_bps=-34 |
| path point | 27387 | path_bps | pnl_bps=57 |
| path point | 28215 | path_bps | pnl_bps=86 |
| path point | 28774 | path_bps | pnl_bps=122 |
| path point | 28977 | path_bps | pnl_bps=162 |
| path point | 29724 | path_bps | pnl_bps=256 |
| path point | 30899 | path_bps | pnl_bps=256 |
| path point | 31303 | path_bps | pnl_bps=345 |
| path point | 32123 | path_bps | pnl_bps=461 |
| path point | 32845 | path_bps | pnl_bps=523 |
| path point | 33658 | path_bps | pnl_bps=560 |
| path point | 34900 | path_bps | pnl_bps=579 |
| path point | 35304 | path_bps | pnl_bps=552 |
| path point | 35820 | path_bps | pnl_bps=609 |
| path point | 36861 | path_bps | pnl_bps=656 |
| path point | 37365 | path_bps | pnl_bps=629 |
| path omitted |  | path_bps | 94 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7324; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.0455640051265851e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
