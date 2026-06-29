# Golden trace: winning 4

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: BzFDKHSuxTBTXSNNuZm7AB3gwDJx7zY6P9HCkZahedht
- base_mint: 4g5DHYCLENFRoDT8MJbRNGnRtp4NZ3hpHCUNjvD2HTAg
- entry_ts_ms: 1782406355960
- entry_price: 6.853268592808158e-08
- result: target
- logged_result: target
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406355960 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406362935 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406365330 | shadow_lifecycle.jsonl | close_reason=None; pnl=64.33248571428571 |
| lifecycle position_closed | 1782406365331 | shadow_lifecycle.jsonl | close_reason=Target; pnl=64.33248571428572 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 6744 | path_bps | pnl_bps=2352 |
| path point | 7148 | path_bps | pnl_bps=3933 |
| path point | 8354 | path_bps | pnl_bps=3686 |
| path point | 8509 | path_bps | pnl_bps=5032 |
| path point | 9195 | path_bps | pnl_bps=6602 |
| path point | 9814 | path_bps | pnl_bps=8251 |
| path point | 10371 | path_bps | pnl_bps=9974 |
| path point | 11371 | path_bps | pnl_bps=9974 |
| path point | 11870 | path_bps | pnl_bps=11778 |
| path point | 12871 | path_bps | pnl_bps=11778 |
| path point | 13370 | path_bps | pnl_bps=11295 |
| path point | 14371 | path_bps | pnl_bps=11295 |
| path point | 14713 | path_bps | pnl_bps=11173 |
| path point | 15370 | path_bps | pnl_bps=10711 |
| path point | 16371 | path_bps | pnl_bps=10711 |
| path point | 17054 | path_bps | pnl_bps=9941 |
| path point | 17764 | path_bps | pnl_bps=9284 |
| path point | 18055 | path_bps | pnl_bps=8064 |
| path point | 19216 | path_bps | pnl_bps=6904 |
| path point | 20371 | path_bps | pnl_bps=6904 |
| path point | 20627 | path_bps | pnl_bps=6818 |
| path point | 21371 | path_bps | pnl_bps=6982 |
| path point | 22871 | path_bps | pnl_bps=6982 |
| path point | 23558 | path_bps | pnl_bps=5756 |
| path point | 24323 | path_bps | pnl_bps=4658 |
| path point | 25370 | path_bps | pnl_bps=4658 |
| path point | 26370 | path_bps | pnl_bps=4708 |
| path point | 26791 | path_bps | pnl_bps=3339 |
| path point | 27217 | path_bps | pnl_bps=4561 |
| path point | 28370 | path_bps | pnl_bps=2421 |
| path point | 29371 | path_bps | pnl_bps=2421 |
| path point | 30870 | path_bps | pnl_bps=2421 |
| path point | 31543 | path_bps | pnl_bps=2561 |
| path point | 32871 | path_bps | pnl_bps=2561 |
| path point | 33371 | path_bps | pnl_bps=2522 |
| path point | 34871 | path_bps | pnl_bps=2522 |
| path point | 36371 | path_bps | pnl_bps=2522 |
| path point | 37371 | path_bps | pnl_bps=2522 |
| path point | 38370 | path_bps | pnl_bps=1645 |
| path omitted |  | path_bps | 77 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-5920; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `6.853268592808158e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
