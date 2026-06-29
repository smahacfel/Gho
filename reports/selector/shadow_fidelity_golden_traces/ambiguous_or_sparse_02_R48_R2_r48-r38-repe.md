# Golden trace: ambiguous_or_sparse 2

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 7dN4Wqdr8YFWvsb7AbYvXhzX33kbXP3AeJmA3iegzdSS
- base_mint: FAaYN6TA7W2U2sZGewXeddsjWK7NJV5PUHFFAkxKpump
- entry_ts_ms: 1782411121555
- entry_price: 1.5123841226292415e-08
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782411121555 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782411128761 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782411149330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-60.93225714285715 |
| lifecycle position_closed | 1782411149331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-60.93225714285714 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 7201 | path_bps | pnl_bps=-1533 |
| path point | 7726 | path_bps | pnl_bps=-3163 |
| path point | 8232 | path_bps | pnl_bps=-3939 |
| path point | 8796 | path_bps | pnl_bps=-2851 |
| path point | 10275 | path_bps | pnl_bps=-2851 |
| path point | 10557 | path_bps | pnl_bps=-1845 |
| path point | 11777 | path_bps | pnl_bps=-1845 |
| path point | 12303 | path_bps | pnl_bps=-673 |
| path point | 13722 | path_bps | pnl_bps=-2672 |
| path point | 14776 | path_bps | pnl_bps=-2672 |
| path point | 15776 | path_bps | pnl_bps=-2672 |
| path point | 16144 | path_bps | pnl_bps=-647 |
| path point | 17276 | path_bps | pnl_bps=-647 |
| path point | 17462 | path_bps | pnl_bps=-1158 |
| path point | 18166 | path_bps | pnl_bps=-1029 |
| path point | 19275 | path_bps | pnl_bps=-1029 |
| path point | 20276 | path_bps | pnl_bps=-1029 |
| path point | 21776 | path_bps | pnl_bps=-1029 |
| path point | 22776 | path_bps | pnl_bps=-1029 |
| path point | 23776 | path_bps | pnl_bps=-1029 |
| path point | 24095 | path_bps | pnl_bps=-1332 |
| path point | 24661 | path_bps | pnl_bps=2446 |
| path point | 25599 | path_bps | pnl_bps=-2844 |
| path point | 26776 | path_bps | pnl_bps=-2844 |
| path point | 27232 | path_bps | pnl_bps=-4753 |
| path point | 27687 | path_bps | pnl_bps=-6052 |
| path point | 28276 | path_bps | pnl_bps=-6183 |
| path point | 29276 | path_bps | pnl_bps=-6183 |
| path point | 30280 | path_bps | pnl_bps=-6183 |
| path point | 31304 | path_bps | pnl_bps=-6200 |
| path point | 32776 | path_bps | pnl_bps=-6200 |
| path point | 33776 | path_bps | pnl_bps=-6200 |
| path point | 35275 | path_bps | pnl_bps=-6200 |
| path point | 36276 | path_bps | pnl_bps=-6200 |
| path point | 37276 | path_bps | pnl_bps=-6200 |
| path point | 38276 | path_bps | pnl_bps=-6200 |
| path point | 39276 | path_bps | pnl_bps=-6200 |
| path point | 40276 | path_bps | pnl_bps=-6200 |
| path point | 41775 | path_bps | pnl_bps=-6200 |
| path omitted |  | path_bps | 68 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-6460; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.5123841226292415e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
