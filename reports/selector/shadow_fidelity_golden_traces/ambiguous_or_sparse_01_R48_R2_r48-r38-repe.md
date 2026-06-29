# Golden trace: ambiguous_or_sparse 1

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 33RSdTeehkEYfS5xrjZKfH3Vhjaqprkbn1q2Wp6JUwqb
- base_mint: 5T5aTVRr9Nop9PtQUh1LvgYeKiaxi5Uhq9qmpRuXpump
- entry_ts_ms: 1782407503251
- entry_price: 9.419348562760073e-08
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782407503251 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782407513159 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782407533330 | shadow_lifecycle.jsonl | close_reason=None; pnl=-69.70102857142857 |
| lifecycle position_closed | 1782407533331 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-69.70102857142857 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 9858 | path_bps | pnl_bps=-863 |
| path point | 11080 | path_bps | pnl_bps=-863 |
| path point | 12394 | path_bps | pnl_bps=-824 |
| path point | 12927 | path_bps | pnl_bps=-661 |
| path point | 13335 | path_bps | pnl_bps=-587 |
| path point | 15080 | path_bps | pnl_bps=-602 |
| path point | 15554 | path_bps | pnl_bps=-559 |
| path point | 16505 | path_bps | pnl_bps=-516 |
| path point | 17580 | path_bps | pnl_bps=-510 |
| path point | 18428 | path_bps | pnl_bps=-448 |
| path point | 18941 | path_bps | pnl_bps=-331 |
| path point | 19478 | path_bps | pnl_bps=-171 |
| path point | 19991 | path_bps | pnl_bps=-113 |
| path point | 21040 | path_bps | pnl_bps=-72 |
| path point | 22080 | path_bps | pnl_bps=110 |
| path point | 22548 | path_bps | pnl_bps=161 |
| path point | 23580 | path_bps | pnl_bps=161 |
| path point | 23977 | path_bps | pnl_bps=310 |
| path point | 24325 | path_bps | pnl_bps=382 |
| path point | 24787 | path_bps | pnl_bps=411 |
| path point | 26028 | path_bps | pnl_bps=602 |
| path point | 26383 | path_bps | pnl_bps=682 |
| path point | 27035 | path_bps | pnl_bps=753 |
| path point | 27517 | path_bps | pnl_bps=951 |
| path point | 28957 | path_bps | pnl_bps=988 |
| path point | 30047 | path_bps | pnl_bps=-6939 |
| path point | 31080 | path_bps | pnl_bps=-6960 |
| path point | 32579 | path_bps | pnl_bps=-6961 |
| path point | 34080 | path_bps | pnl_bps=-6964 |
| path point | 35579 | path_bps | pnl_bps=-6964 |
| path point | 37579 | path_bps | pnl_bps=-6969 |
| path point | 38580 | path_bps | pnl_bps=-6969 |
| path point | 39580 | path_bps | pnl_bps=-6969 |
| path point | 41079 | path_bps | pnl_bps=-6970 |
| path point | 42080 | path_bps | pnl_bps=-6970 |
| path point | 43580 | path_bps | pnl_bps=-6970 |
| path point | 45079 | path_bps | pnl_bps=-6970 |
| path point | 46079 | path_bps | pnl_bps=-6970 |
| path point | 47080 | path_bps | pnl_bps=-6970 |
| path omitted |  | path_bps | 63 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-6955; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `9.419348562760073e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
