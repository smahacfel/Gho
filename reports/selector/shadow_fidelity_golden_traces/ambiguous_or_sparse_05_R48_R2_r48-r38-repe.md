# Golden trace: ambiguous_or_sparse 5

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: EY89QvejfmtXSTAgCLRvzYe1ed6a9rrtfkyPwrsH1n73
- base_mint: BZdUqnEXBZ52ZDn4ddYJSff5U6zm3aU66o4T6JMkpump
- entry_ts_ms: 1782413055405
- entry_price: 4.687088455962824e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782413055405 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782413064324 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782413183333 | shadow_lifecycle.jsonl | close_reason=None; pnl=59.39372857142856 |
| lifecycle position_closed | 1782413183334 | shadow_lifecycle.jsonl | close_reason=Target; pnl=59.39372857142856 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 8858 | path_bps | pnl_bps=-1172 |
| path point | 9613 | path_bps | pnl_bps=-1064 |
| path point | 10437 | path_bps | pnl_bps=-980 |
| path point | 11925 | path_bps | pnl_bps=-964 |
| path point | 12021 | path_bps | pnl_bps=-907 |
| path point | 13332 | path_bps | pnl_bps=-871 |
| path point | 14125 | path_bps | pnl_bps=-758 |
| path point | 15426 | path_bps | pnl_bps=-758 |
| path point | 15734 | path_bps | pnl_bps=-669 |
| path point | 16560 | path_bps | pnl_bps=-599 |
| path point | 17380 | path_bps | pnl_bps=-512 |
| path point | 18250 | path_bps | pnl_bps=-411 |
| path point | 19169 | path_bps | pnl_bps=-295 |
| path point | 20426 | path_bps | pnl_bps=-295 |
| path point | 20514 | path_bps | pnl_bps=-395 |
| path point | 21324 | path_bps | pnl_bps=-321 |
| path point | 21931 | path_bps | pnl_bps=-211 |
| path point | 22473 | path_bps | pnl_bps=-138 |
| path point | 23125 | path_bps | pnl_bps=-75 |
| path point | 23882 | path_bps | pnl_bps=6 |
| path point | 24779 | path_bps | pnl_bps=183 |
| path point | 25576 | path_bps | pnl_bps=227 |
| path point | 26925 | path_bps | pnl_bps=227 |
| path point | 27012 | path_bps | pnl_bps=339 |
| path point | 27594 | path_bps | pnl_bps=460 |
| path point | 28238 | path_bps | pnl_bps=562 |
| path point | 28878 | path_bps | pnl_bps=624 |
| path point | 29638 | path_bps | pnl_bps=667 |
| path point | 30725 | path_bps | pnl_bps=681 |
| path point | 31926 | path_bps | pnl_bps=681 |
| path point | 32563 | path_bps | pnl_bps=-129 |
| path point | 33573 | path_bps | pnl_bps=-61 |
| path point | 34293 | path_bps | pnl_bps=4 |
| path point | 35425 | path_bps | pnl_bps=4 |
| path point | 36030 | path_bps | pnl_bps=55 |
| path point | 36727 | path_bps | pnl_bps=175 |
| path point | 37926 | path_bps | pnl_bps=175 |
| path point | 38642 | path_bps | pnl_bps=281 |
| path point | 39436 | path_bps | pnl_bps=344 |
| path omitted |  | path_bps | 112 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=4472; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `4.687088455962824e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
