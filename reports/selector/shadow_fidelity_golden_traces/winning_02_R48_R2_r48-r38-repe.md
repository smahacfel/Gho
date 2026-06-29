# Golden trace: winning 2

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: 4Fjgj3Ntnd1NgTuWNtk1VfXNEZ5jDfCjvDWTynvQqJNW
- base_mint: A77nct5umYPyzGFfaDeRJ2WAX37iSo9E5U3YSdCKpump
- entry_ts_ms: 1782406026698
- entry_price: 4.639482032200359e-08
- result: target
- logged_result: target
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406026698 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406033501 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406056831 | shadow_lifecycle.jsonl | close_reason=None; pnl=-5.7916 |
| lifecycle position_closed | 1782406056831 | shadow_lifecycle.jsonl | close_reason=TimeStop; pnl=-5.7916 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 4193 | path_bps | pnl_bps=-483 |
| path point | 7133 | path_bps | pnl_bps=-483 |
| path point | 8133 | path_bps | pnl_bps=-483 |
| path point | 9633 | path_bps | pnl_bps=-483 |
| path point | 11133 | path_bps | pnl_bps=-483 |
| path point | 12632 | path_bps | pnl_bps=-483 |
| path point | 13633 | path_bps | pnl_bps=-483 |
| path point | 15132 | path_bps | pnl_bps=-483 |
| path point | 16132 | path_bps | pnl_bps=-483 |
| path point | 17134 | path_bps | pnl_bps=-483 |
| path point | 18633 | path_bps | pnl_bps=-483 |
| path point | 20133 | path_bps | pnl_bps=-483 |
| path point | 21633 | path_bps | pnl_bps=-483 |
| path point | 23133 | path_bps | pnl_bps=-483 |
| path point | 24633 | path_bps | pnl_bps=-483 |
| path point | 26133 | path_bps | pnl_bps=-483 |
| path point | 27133 | path_bps | pnl_bps=-483 |
| path point | 28133 | path_bps | pnl_bps=-483 |
| path point | 29632 | path_bps | pnl_bps=-483 |
| path point | 30633 | path_bps | pnl_bps=-483 |
| path point | 31633 | path_bps | pnl_bps=-483 |
| path point | 32633 | path_bps | pnl_bps=-483 |
| path point | 33633 | path_bps | pnl_bps=-483 |
| path point | 35133 | path_bps | pnl_bps=-483 |
| path point | 35276 | path_bps | pnl_bps=2364 |
| path point | 36632 | path_bps | pnl_bps=2364 |
| path point | 36828 | path_bps | pnl_bps=294 |
| path point | 38028 | path_bps | pnl_bps=-1313 |
| path point | 39132 | path_bps | pnl_bps=-1313 |
| path point | 39620 | path_bps | pnl_bps=136 |
| path point | 40633 | path_bps | pnl_bps=136 |
| path point | 41633 | path_bps | pnl_bps=136 |
| path point | 42633 | path_bps | pnl_bps=3042 |
| path point | 43633 | path_bps | pnl_bps=3042 |
| path point | 44133 | path_bps | pnl_bps=1194 |
| path point | 45632 | path_bps | pnl_bps=4621 |
| path point | 46632 | path_bps | pnl_bps=4621 |
| path point | 47132 | path_bps | pnl_bps=8700 |
| path point | 48133 | path_bps | pnl_bps=8700 |
| path omitted |  | path_bps | 69 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-8548; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `4.639482032200359e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
