# Golden trace: timeout 5

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: C1FbDLBb7FeYVXkaYCZMxdT3jQaDp43WaqQCTc7etSho
- base_mint: 66yPSt3YiUSNu9mfVjicamwYnhCJHvkEAyVRaNJ1pump
- entry_ts_ms: 1782405902066
- entry_price: 3.3492126381558253e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405902066 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405909653 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406018330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-17.37158571428572 |
| lifecycle position_closed | 1782406018331 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-17.37158571428572 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 4277 | path_bps | pnl_bps=-570 |
| path point | 7764 | path_bps | pnl_bps=-570 |
| path point | 8765 | path_bps | pnl_bps=-570 |
| path point | 9032 | path_bps | pnl_bps=-453 |
| path point | 9534 | path_bps | pnl_bps=-98 |
| path point | 10765 | path_bps | pnl_bps=-98 |
| path point | 11765 | path_bps | pnl_bps=-98 |
| path point | 13265 | path_bps | pnl_bps=-98 |
| path point | 14009 | path_bps | pnl_bps=81 |
| path point | 15264 | path_bps | pnl_bps=81 |
| path point | 16265 | path_bps | pnl_bps=81 |
| path point | 16482 | path_bps | pnl_bps=-98 |
| path point | 17764 | path_bps | pnl_bps=-98 |
| path point | 18770 | path_bps | pnl_bps=-98 |
| path point | 20252 | path_bps | pnl_bps=909 |
| path point | 20311 | path_bps | pnl_bps=1119 |
| path point | 21764 | path_bps | pnl_bps=1119 |
| path point | 22324 | path_bps | pnl_bps=1280 |
| path point | 23765 | path_bps | pnl_bps=1280 |
| path point | 24198 | path_bps | pnl_bps=1543 |
| path point | 25265 | path_bps | pnl_bps=1543 |
| path point | 26265 | path_bps | pnl_bps=1543 |
| path point | 27765 | path_bps | pnl_bps=1543 |
| path point | 29265 | path_bps | pnl_bps=1543 |
| path point | 30265 | path_bps | pnl_bps=1543 |
| path point | 31666 | path_bps | pnl_bps=1543 |
| path point | 32764 | path_bps | pnl_bps=1543 |
| path point | 33765 | path_bps | pnl_bps=1543 |
| path point | 35265 | path_bps | pnl_bps=1543 |
| path point | 36267 | path_bps | pnl_bps=1543 |
| path point | 37765 | path_bps | pnl_bps=1543 |
| path point | 39264 | path_bps | pnl_bps=1543 |
| path point | 40265 | path_bps | pnl_bps=1543 |
| path point | 41265 | path_bps | pnl_bps=1543 |
| path point | 42765 | path_bps | pnl_bps=1543 |
| path point | 43765 | path_bps | pnl_bps=1543 |
| path point | 45265 | path_bps | pnl_bps=1543 |
| path point | 46765 | path_bps | pnl_bps=1543 |
| path point | 48264 | path_bps | pnl_bps=1543 |
| path omitted |  | path_bps | 68 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-1652; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `3.3492126381558253e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
