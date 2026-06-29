# Golden trace: timeout 2

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 6cAJoMZmpf6hLgYbGCQzPNpQcxmAFAxt6UToRxNU1Lon
- base_mint: EkfopKRL6GwyjiDqyJfKvkre8CHwLPyejh6A2oG8pump
- entry_ts_ms: 1782405892225
- entry_price: 3.00270077654535e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405892225 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405896184 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782405922330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-2.0338857142857143 |
| lifecycle position_closed | 1782405922331 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-2.0338857142857143 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 4106 | path_bps | pnl_bps=-102 |
| path point | 5106 | path_bps | pnl_bps=-102 |
| path point | 6606 | path_bps | pnl_bps=-102 |
| path point | 7606 | path_bps | pnl_bps=-102 |
| path point | 9105 | path_bps | pnl_bps=-102 |
| path point | 10106 | path_bps | pnl_bps=-102 |
| path point | 11606 | path_bps | pnl_bps=-102 |
| path point | 12606 | path_bps | pnl_bps=-102 |
| path point | 13606 | path_bps | pnl_bps=-102 |
| path point | 14611 | path_bps | pnl_bps=-102 |
| path point | 16106 | path_bps | pnl_bps=-102 |
| path point | 17605 | path_bps | pnl_bps=-102 |
| path point | 18606 | path_bps | pnl_bps=-102 |
| path point | 19606 | path_bps | pnl_bps=-102 |
| path point | 20606 | path_bps | pnl_bps=-102 |
| path point | 21606 | path_bps | pnl_bps=-102 |
| path point | 23106 | path_bps | pnl_bps=-102 |
| path point | 24106 | path_bps | pnl_bps=-102 |
| path point | 25605 | path_bps | pnl_bps=-102 |
| path point | 26606 | path_bps | pnl_bps=-102 |
| path point | 28108 | path_bps | pnl_bps=-102 |
| path point | 29605 | path_bps | pnl_bps=-102 |
| path point | 30605 | path_bps | pnl_bps=-102 |
| path point | 31605 | path_bps | pnl_bps=-102 |
| path point | 32606 | path_bps | pnl_bps=-102 |
| path point | 33606 | path_bps | pnl_bps=-102 |
| path point | 34606 | path_bps | pnl_bps=-102 |
| path point | 35106 | path_bps | pnl_bps=-138 |
| path point | 36106 | path_bps | pnl_bps=-138 |
| path point | 37606 | path_bps | pnl_bps=-138 |
| path point | 39106 | path_bps | pnl_bps=-138 |
| path point | 40106 | path_bps | pnl_bps=-138 |
| path point | 41606 | path_bps | pnl_bps=-138 |
| path point | 43106 | path_bps | pnl_bps=-138 |
| path point | 44605 | path_bps | pnl_bps=-138 |
| path point | 45605 | path_bps | pnl_bps=-138 |
| path point | 46605 | path_bps | pnl_bps=-138 |
| path point | 47606 | path_bps | pnl_bps=-138 |
| path point | 49105 | path_bps | pnl_bps=-138 |
| path omitted |  | path_bps | 62 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-302; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `3.00270077654535e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
