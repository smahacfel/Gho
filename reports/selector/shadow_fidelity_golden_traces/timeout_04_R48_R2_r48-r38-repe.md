# Golden trace: timeout 4

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: GcKfiLvNDFSULs2FH37QHsrPESyqG3KyicvJ3mDLWgmk
- base_mint: HghTGv1U6sMS8UoV753DAiZttsxuCBp2fEESYbwtpump
- entry_ts_ms: 1782405897367
- entry_price: 5.2137100876179666e-08
- result: timeout
- logged_result: timeout
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782405897367 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782405905689 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406134831 | shadow_lifecycle.jsonl | close_reason=None; pnl=-45.3992 |
| lifecycle position_closed | 1782406134831 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-45.3992 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 7980 | path_bps | pnl_bps=-1964 |
| path point | 9042 | path_bps | pnl_bps=-2252 |
| path point | 10444 | path_bps | pnl_bps=-926 |
| path point | 10982 | path_bps | pnl_bps=-259 |
| path point | 12462 | path_bps | pnl_bps=-134 |
| path point | 12924 | path_bps | pnl_bps=-795 |
| path point | 13261 | path_bps | pnl_bps=-702 |
| path point | 13702 | path_bps | pnl_bps=-819 |
| path point | 14416 | path_bps | pnl_bps=-749 |
| path point | 15132 | path_bps | pnl_bps=-47 |
| path point | 16464 | path_bps | pnl_bps=-47 |
| path point | 17432 | path_bps | pnl_bps=-557 |
| path point | 18463 | path_bps | pnl_bps=-569 |
| path point | 19417 | path_bps | pnl_bps=1918 |
| path point | 19892 | path_bps | pnl_bps=2055 |
| path point | 20779 | path_bps | pnl_bps=1040 |
| path point | 21604 | path_bps | pnl_bps=1075 |
| path point | 22966 | path_bps | pnl_bps=1087 |
| path point | 23045 | path_bps | pnl_bps=1601 |
| path point | 24190 | path_bps | pnl_bps=1652 |
| path point | 24524 | path_bps | pnl_bps=-702 |
| path point | 25964 | path_bps | pnl_bps=-702 |
| path point | 26283 | path_bps | pnl_bps=-630 |
| path point | 27363 | path_bps | pnl_bps=-584 |
| path point | 28310 | path_bps | pnl_bps=-1685 |
| path point | 29464 | path_bps | pnl_bps=-1685 |
| path point | 30189 | path_bps | pnl_bps=-1710 |
| path point | 31464 | path_bps | pnl_bps=-1710 |
| path point | 32464 | path_bps | pnl_bps=-1710 |
| path point | 32959 | path_bps | pnl_bps=-3336 |
| path point | 33348 | path_bps | pnl_bps=-3420 |
| path point | 33724 | path_bps | pnl_bps=-2612 |
| path point | 34121 | path_bps | pnl_bps=-1983 |
| path point | 35464 | path_bps | pnl_bps=-1983 |
| path point | 35690 | path_bps | pnl_bps=-2063 |
| path point | 36964 | path_bps | pnl_bps=-2063 |
| path point | 37353 | path_bps | pnl_bps=-2020 |
| path point | 38464 | path_bps | pnl_bps=-2020 |
| path point | 38595 | path_bps | pnl_bps=-2090 |
| path omitted |  | path_bps | 90 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=2784; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `5.2137100876179666e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
