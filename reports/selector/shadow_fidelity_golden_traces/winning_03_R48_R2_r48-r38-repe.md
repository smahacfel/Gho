# Golden trace: winning 3

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 4VV1jVDirJkRHZz9SF2HiTsizGiEFqnuMUk3tsk1sqeg
- base_mint: BdD36259EFM5TJ69HXWahabWsw4FwSxNKWBzceNmpump
- entry_ts_ms: 1782406217313
- entry_price: 3.615420343644257e-08
- result: target
- logged_result: target
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406217313 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406223835 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406255331 | shadow_lifecycle.jsonl | close_reason=None; pnl=63.24042857142857 |
| lifecycle position_closed | 1782406255331 | shadow_lifecycle.jsonl | close_reason=Target; pnl=63.24042857142857 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 5612 | path_bps | pnl_bps=1035 |
| path point | 7017 | path_bps | pnl_bps=1035 |
| path point | 8017 | path_bps | pnl_bps=1035 |
| path point | 9018 | path_bps | pnl_bps=1035 |
| path point | 10018 | path_bps | pnl_bps=1035 |
| path point | 11018 | path_bps | pnl_bps=1035 |
| path point | 12018 | path_bps | pnl_bps=1035 |
| path point | 12847 | path_bps | pnl_bps=1998 |
| path point | 14017 | path_bps | pnl_bps=1998 |
| path point | 15017 | path_bps | pnl_bps=1998 |
| path point | 16018 | path_bps | pnl_bps=1998 |
| path point | 17018 | path_bps | pnl_bps=1998 |
| path point | 18024 | path_bps | pnl_bps=1998 |
| path point | 19518 | path_bps | pnl_bps=1998 |
| path point | 21017 | path_bps | pnl_bps=1998 |
| path point | 21509 | path_bps | pnl_bps=3025 |
| path point | 22517 | path_bps | pnl_bps=3025 |
| path point | 23518 | path_bps | pnl_bps=3025 |
| path point | 23810 | path_bps | pnl_bps=2815 |
| path point | 25017 | path_bps | pnl_bps=2815 |
| path point | 26018 | path_bps | pnl_bps=2815 |
| path point | 27517 | path_bps | pnl_bps=2815 |
| path point | 28518 | path_bps | pnl_bps=2815 |
| path point | 30017 | path_bps | pnl_bps=2815 |
| path point | 31018 | path_bps | pnl_bps=2815 |
| path point | 32518 | path_bps | pnl_bps=2815 |
| path point | 33518 | path_bps | pnl_bps=2815 |
| path point | 34518 | path_bps | pnl_bps=2815 |
| path point | 36018 | path_bps | pnl_bps=2815 |
| path point | 37018 | path_bps | pnl_bps=2815 |
| path point | 37367 | path_bps | pnl_bps=3292 |
| path point | 37950 | path_bps | pnl_bps=6493 |
| path point | 39018 | path_bps | pnl_bps=6493 |
| path point | 40517 | path_bps | pnl_bps=6493 |
| path point | 41518 | path_bps | pnl_bps=6493 |
| path point | 43001 | path_bps | pnl_bps=-1765 |
| path point | 43341 | path_bps | pnl_bps=-2244 |
| path point | 44518 | path_bps | pnl_bps=-2244 |
| path point | 45518 | path_bps | pnl_bps=-2244 |
| path omitted |  | path_bps | 64 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-2267; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `3.615420343644257e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
