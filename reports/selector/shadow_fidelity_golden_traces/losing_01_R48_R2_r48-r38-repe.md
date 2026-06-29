# Golden trace: losing 1

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 2XQAW8hbQovHUDxQsdFASXQbGMpQMaxjMkjQBivREuSU
- base_mint: 9voQmGgYyfBwujp2mCUTB4HRDTKgtzBpbZeKpCcspump
- entry_ts_ms: 1782405875656
- entry_price: 1.2276541874447916e-07
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405875656 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405885335 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782405926330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-74.33845714285714 |
| lifecycle position_closed | 1782405926331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-74.33845714285714 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 9441 | path_bps | pnl_bps=1201 |
| path point | 10061 | path_bps | pnl_bps=1292 |
| path point | 11175 | path_bps | pnl_bps=1303 |
| path point | 11628 | path_bps | pnl_bps=1412 |
| path point | 12167 | path_bps | pnl_bps=1470 |
| path point | 13062 | path_bps | pnl_bps=1535 |
| path point | 13501 | path_bps | pnl_bps=1581 |
| path point | 13776 | path_bps | pnl_bps=1613 |
| path point | 14583 | path_bps | pnl_bps=1540 |
| path point | 15073 | path_bps | pnl_bps=1655 |
| path point | 15642 | path_bps | pnl_bps=1687 |
| path point | 15982 | path_bps | pnl_bps=1740 |
| path point | 16963 | path_bps | pnl_bps=1797 |
| path point | 17644 | path_bps | pnl_bps=1899 |
| path point | 17970 | path_bps | pnl_bps=1964 |
| path point | 18439 | path_bps | pnl_bps=2001 |
| path point | 19569 | path_bps | pnl_bps=1854 |
| path point | 20616 | path_bps | pnl_bps=1871 |
| path point | 21027 | path_bps | pnl_bps=1928 |
| path point | 21607 | path_bps | pnl_bps=1990 |
| path point | 22149 | path_bps | pnl_bps=2019 |
| path point | 22642 | path_bps | pnl_bps=2091 |
| path point | 23560 | path_bps | pnl_bps=2135 |
| path point | 24163 | path_bps | pnl_bps=2266 |
| path point | 25036 | path_bps | pnl_bps=2297 |
| path point | 26175 | path_bps | pnl_bps=2312 |
| path point | 26347 | path_bps | pnl_bps=2721 |
| path point | 27174 | path_bps | pnl_bps=4179 |
| path point | 27634 | path_bps | pnl_bps=4258 |
| path point | 28675 | path_bps | pnl_bps=4258 |
| path point | 29268 | path_bps | pnl_bps=4127 |
| path point | 30324 | path_bps | pnl_bps=4172 |
| path point | 31509 | path_bps | pnl_bps=4221 |
| path point | 31919 | path_bps | pnl_bps=4302 |
| path point | 33175 | path_bps | pnl_bps=4323 |
| path point | 33582 | path_bps | pnl_bps=4352 |
| path point | 34634 | path_bps | pnl_bps=4423 |
| path point | 34989 | path_bps | pnl_bps=4466 |
| path point | 35538 | path_bps | pnl_bps=4521 |
| path omitted |  | path_bps | 82 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7718; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.2276541874447916e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
