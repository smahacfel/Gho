# Golden trace: timeout 3

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 3C1KUYdfLhvG9eYd3vHnZEr43wGc3ej3fG5msXMmNG14
- base_mint: 86mPHqPT3zsmBG7hMa5ipGzZTe2hKpFfvqRQmk4Ypump
- entry_ts_ms: 1782405894064
- entry_price: 3.3279867653078726e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405894064 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405900706 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782405947331 | shadow_lifecycle.jsonl | close_reason=None; pnl=-16.844685714285713 |
| lifecycle position_closed | 1782405947332 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-16.844685714285713 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 5774 | path_bps | pnl_bps=-1011 |
| path point | 7266 | path_bps | pnl_bps=-1011 |
| path point | 8267 | path_bps | pnl_bps=-1011 |
| path point | 9767 | path_bps | pnl_bps=-1011 |
| path point | 10767 | path_bps | pnl_bps=-1011 |
| path point | 11767 | path_bps | pnl_bps=-1011 |
| path point | 12772 | path_bps | pnl_bps=-1011 |
| path point | 14267 | path_bps | pnl_bps=-1011 |
| path point | 15766 | path_bps | pnl_bps=-1011 |
| path point | 16767 | path_bps | pnl_bps=-1011 |
| path point | 17767 | path_bps | pnl_bps=-1011 |
| path point | 18767 | path_bps | pnl_bps=-1011 |
| path point | 19767 | path_bps | pnl_bps=-1016 |
| path point | 21267 | path_bps | pnl_bps=-1016 |
| path point | 22267 | path_bps | pnl_bps=-1016 |
| path point | 22481 | path_bps | pnl_bps=-1599 |
| path point | 23766 | path_bps | pnl_bps=-1599 |
| path point | 24767 | path_bps | pnl_bps=-1599 |
| path point | 26269 | path_bps | pnl_bps=-1599 |
| path point | 27766 | path_bps | pnl_bps=-1599 |
| path point | 28766 | path_bps | pnl_bps=-1599 |
| path point | 29766 | path_bps | pnl_bps=-1599 |
| path point | 30767 | path_bps | pnl_bps=-1599 |
| path point | 31767 | path_bps | pnl_bps=-1599 |
| path point | 32767 | path_bps | pnl_bps=-1599 |
| path point | 33767 | path_bps | pnl_bps=-1599 |
| path point | 34767 | path_bps | pnl_bps=-1599 |
| path point | 35767 | path_bps | pnl_bps=-1599 |
| path point | 37267 | path_bps | pnl_bps=-1599 |
| path point | 38267 | path_bps | pnl_bps=-1599 |
| path point | 39767 | path_bps | pnl_bps=-1599 |
| path point | 41267 | path_bps | pnl_bps=-1599 |
| path point | 42766 | path_bps | pnl_bps=-1599 |
| path point | 43766 | path_bps | pnl_bps=-1599 |
| path point | 44766 | path_bps | pnl_bps=-1599 |
| path point | 45767 | path_bps | pnl_bps=-1599 |
| path point | 47266 | path_bps | pnl_bps=-1599 |
| path point | 48267 | path_bps | pnl_bps=-1599 |
| path point | 49267 | path_bps | pnl_bps=-1599 |
| path omitted |  | path_bps | 60 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-1599; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `3.3279867653078726e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
