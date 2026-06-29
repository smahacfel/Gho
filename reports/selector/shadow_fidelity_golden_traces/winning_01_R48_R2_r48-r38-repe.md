# Golden trace: winning 1

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: Anrf95XME5himwW1keyjdytLBxg2Hb6qQZGzQoxqGGLr
- base_mint: Hhvm4ZSafSAvFSjizTbaqqSTQjC5F7RNMBHBLUhbpump
- entry_ts_ms: 1782405997083
- entry_price: 1.0773730282763113e-07
- result: target
- logged_result: target
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405997083 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406005168 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406082831 | shadow_lifecycle.jsonl | close_reason=None; pnl=65.34931428571429 |
| lifecycle position_closed | 1782406082832 | shadow_lifecycle.jsonl | close_reason=Target; pnl=65.34931428571429 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 7595 | path_bps | pnl_bps=-225 |
| path point | 8166 | path_bps | pnl_bps=-194 |
| path point | 9119 | path_bps | pnl_bps=-88 |
| path point | 10248 | path_bps | pnl_bps=-88 |
| path point | 11308 | path_bps | pnl_bps=-20 |
| path point | 12084 | path_bps | pnl_bps=81 |
| path point | 12403 | path_bps | pnl_bps=148 |
| path point | 12955 | path_bps | pnl_bps=832 |
| path point | 13716 | path_bps | pnl_bps=1568 |
| path point | 14748 | path_bps | pnl_bps=1568 |
| path point | 15748 | path_bps | pnl_bps=1576 |
| path point | 16748 | path_bps | pnl_bps=1563 |
| path point | 17234 | path_bps | pnl_bps=1509 |
| path point | 18240 | path_bps | pnl_bps=1507 |
| path point | 19080 | path_bps | pnl_bps=399 |
| path point | 20254 | path_bps | pnl_bps=399 |
| path point | 21207 | path_bps | pnl_bps=543 |
| path point | 22220 | path_bps | pnl_bps=412 |
| path point | 23246 | path_bps | pnl_bps=525 |
| path point | 24572 | path_bps | pnl_bps=709 |
| path point | 25219 | path_bps | pnl_bps=823 |
| path point | 26248 | path_bps | pnl_bps=839 |
| path point | 26495 | path_bps | pnl_bps=897 |
| path point | 27178 | path_bps | pnl_bps=1010 |
| path point | 27589 | path_bps | pnl_bps=1049 |
| path point | 28587 | path_bps | pnl_bps=1120 |
| path point | 29452 | path_bps | pnl_bps=1816 |
| path point | 30003 | path_bps | pnl_bps=2180 |
| path point | 31077 | path_bps | pnl_bps=2127 |
| path point | 31747 | path_bps | pnl_bps=2045 |
| path point | 31919 | path_bps | pnl_bps=2016 |
| path point | 33157 | path_bps | pnl_bps=1945 |
| path point | 33805 | path_bps | pnl_bps=2040 |
| path point | 34737 | path_bps | pnl_bps=2074 |
| path point | 35211 | path_bps | pnl_bps=2118 |
| path point | 36248 | path_bps | pnl_bps=2114 |
| path point | 36660 | path_bps | pnl_bps=2179 |
| path point | 37391 | path_bps | pnl_bps=2256 |
| path point | 38470 | path_bps | pnl_bps=2262 |
| path omitted |  | path_bps | 95 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7400; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.0773730282763113e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
