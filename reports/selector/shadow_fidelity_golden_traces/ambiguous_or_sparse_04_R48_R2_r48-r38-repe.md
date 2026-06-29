# Golden trace: ambiguous_or_sparse 4

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: Av1kJ8qyp2Lu2wT9XNhxdg5Agvj1mzXvsdzHTfiBoTBj
- base_mint: 31bsWtJidPzUaEVTJCzwtzRVqL3ufFRU8s6qTrPmpump
- entry_ts_ms: 1782412932703
- entry_price: 2.3247251284926922e-07
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782412932703 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782412940870 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782412954331 | shadow_lifecycle.jsonl | close_reason=None; pnl=-61.224914285714284 |
| lifecycle position_closed | 1782412954331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-61.224914285714284 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 7888 | path_bps | pnl_bps=-3577 |
| path point | 8952 | path_bps | pnl_bps=-3575 |
| path point | 9874 | path_bps | pnl_bps=-4399 |
| path point | 10540 | path_bps | pnl_bps=-4342 |
| path point | 11097 | path_bps | pnl_bps=-3010 |
| path point | 12088 | path_bps | pnl_bps=-3154 |
| path point | 12398 | path_bps | pnl_bps=-1521 |
| path point | 13452 | path_bps | pnl_bps=-2045 |
| path point | 14022 | path_bps | pnl_bps=-3204 |
| path point | 14524 | path_bps | pnl_bps=-3644 |
| path point | 15528 | path_bps | pnl_bps=-3590 |
| path point | 16038 | path_bps | pnl_bps=-3988 |
| path point | 16524 | path_bps | pnl_bps=-4595 |
| path point | 16933 | path_bps | pnl_bps=-3583 |
| path point | 17382 | path_bps | pnl_bps=-4219 |
| path point | 18064 | path_bps | pnl_bps=-3530 |
| path point | 18512 | path_bps | pnl_bps=-5193 |
| path point | 19034 | path_bps | pnl_bps=-4559 |
| path point | 20093 | path_bps | pnl_bps=-4532 |
| path point | 21314 | path_bps | pnl_bps=-6083 |
| path point | 22127 | path_bps | pnl_bps=-7267 |
| path point | 22393 | path_bps | pnl_bps=-7565 |
| path point | 23048 | path_bps | pnl_bps=-7412 |
| path point | 23627 | path_bps | pnl_bps=-6945 |
| path point | 24023 | path_bps | pnl_bps=-6113 |
| path point | 24525 | path_bps | pnl_bps=-6614 |
| path point | 25031 | path_bps | pnl_bps=-6094 |
| path point | 25628 | path_bps | pnl_bps=-7780 |
| path point | 27084 | path_bps | pnl_bps=-7753 |
| path point | 28088 | path_bps | pnl_bps=-6871 |
| path point | 28628 | path_bps | pnl_bps=-6784 |
| path point | 29128 | path_bps | pnl_bps=-6084 |
| path point | 29571 | path_bps | pnl_bps=-6969 |
| path point | 30013 | path_bps | pnl_bps=-7876 |
| path point | 30616 | path_bps | pnl_bps=-8883 |
| path point | 31628 | path_bps | pnl_bps=-8878 |
| path point | 31739 | path_bps | pnl_bps=-8791 |
| path point | 32919 | path_bps | pnl_bps=-8576 |
| path point | 33601 | path_bps | pnl_bps=-8492 |
| path omitted |  | path_bps | 118 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-1681; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `2.3247251284926922e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
