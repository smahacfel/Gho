# Golden trace: losing 2

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 6Xu7BRUyqsToy7DmCokuUyYmvVVy3KcQmnFg9ZEBes3x
- base_mint: 96idzK5Qi5NtUHKcBqzvT145kMsQCrdqwdLeQXbApump
- entry_ts_ms: 1782406220573
- entry_price: 1.6354941479841294e-08
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406220573 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406229409 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406232330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-64.63937142857142 |
| lifecycle position_closed | 1782406232331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-64.63937142857142 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 8365 | path_bps | pnl_bps=-3732 |
| path point | 9718 | path_bps | pnl_bps=-2560 |
| path point | 10073 | path_bps | pnl_bps=-3747 |
| path point | 10658 | path_bps | pnl_bps=-5842 |
| path point | 11200 | path_bps | pnl_bps=-5717 |
| path point | 11611 | path_bps | pnl_bps=-6427 |
| path point | 12229 | path_bps | pnl_bps=-6375 |
| path point | 12753 | path_bps | pnl_bps=-6242 |
| path point | 13104 | path_bps | pnl_bps=-6160 |
| path point | 13572 | path_bps | pnl_bps=-6115 |
| path point | 14157 | path_bps | pnl_bps=-5878 |
| path point | 14754 | path_bps | pnl_bps=-5669 |
| path point | 15210 | path_bps | pnl_bps=-5450 |
| path point | 15739 | path_bps | pnl_bps=-5338 |
| path point | 16153 | path_bps | pnl_bps=-5250 |
| path point | 16660 | path_bps | pnl_bps=-5205 |
| path point | 17198 | path_bps | pnl_bps=-5059 |
| path point | 17757 | path_bps | pnl_bps=-8229 |
| path point | 18757 | path_bps | pnl_bps=-8245 |
| path point | 20054 | path_bps | pnl_bps=-8313 |
| path point | 21258 | path_bps | pnl_bps=-8313 |
| path point | 22258 | path_bps | pnl_bps=-8313 |
| path point | 23757 | path_bps | pnl_bps=-8313 |
| path point | 24758 | path_bps | pnl_bps=-8313 |
| path point | 25758 | path_bps | pnl_bps=-8313 |
| path point | 26690 | path_bps | pnl_bps=-7878 |
| path point | 27758 | path_bps | pnl_bps=-7878 |
| path point | 29258 | path_bps | pnl_bps=-7878 |
| path point | 30258 | path_bps | pnl_bps=-7878 |
| path point | 31258 | path_bps | pnl_bps=-7878 |
| path point | 32758 | path_bps | pnl_bps=-7878 |
| path point | 33758 | path_bps | pnl_bps=-7878 |
| path point | 34758 | path_bps | pnl_bps=-7878 |
| path point | 35758 | path_bps | pnl_bps=-7878 |
| path point | 37257 | path_bps | pnl_bps=-7878 |
| path point | 38258 | path_bps | pnl_bps=-7878 |
| path point | 39758 | path_bps | pnl_bps=-7878 |
| path point | 41258 | path_bps | pnl_bps=-7878 |
| path point | 42258 | path_bps | pnl_bps=-7878 |
| path omitted |  | path_bps | 67 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7878; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.6354941479841294e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
