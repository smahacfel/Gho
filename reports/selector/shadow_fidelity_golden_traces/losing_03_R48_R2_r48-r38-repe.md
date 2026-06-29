# Golden trace: losing 3

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: GwB1SDGrfBD6TtRE33SKAnDv5TJRauXMUKW4gCai9X9D
- base_mint: BXar6zDqoykC4Cyfn92QB7LBqV7PdBMvLhFhesUgpump
- entry_ts_ms: 1782406222785
- entry_price: 1.1004057258870274e-07
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406222785 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406229953 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406324830 | shadow_lifecycle.jsonl | close_reason=None; pnl=-72.08998571428572 |
| lifecycle position_closed | 1782406324831 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-72.08998571428572 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 6839 | path_bps | pnl_bps=-2264 |
| path point | 8046 | path_bps | pnl_bps=-2264 |
| path point | 9251 | path_bps | pnl_bps=-1972 |
| path point | 9872 | path_bps | pnl_bps=-1525 |
| path point | 11045 | path_bps | pnl_bps=-1525 |
| path point | 11696 | path_bps | pnl_bps=-1841 |
| path point | 13045 | path_bps | pnl_bps=-1841 |
| path point | 14038 | path_bps | pnl_bps=-1899 |
| path point | 15045 | path_bps | pnl_bps=-1899 |
| path point | 16025 | path_bps | pnl_bps=-1998 |
| path point | 17045 | path_bps | pnl_bps=-1998 |
| path point | 17803 | path_bps | pnl_bps=-1454 |
| path point | 18995 | path_bps | pnl_bps=-811 |
| path point | 20046 | path_bps | pnl_bps=-811 |
| path point | 21545 | path_bps | pnl_bps=-811 |
| path point | 22334 | path_bps | pnl_bps=-2232 |
| path point | 23546 | path_bps | pnl_bps=-2232 |
| path point | 25046 | path_bps | pnl_bps=-2232 |
| path point | 26545 | path_bps | pnl_bps=-2232 |
| path point | 27546 | path_bps | pnl_bps=-2232 |
| path point | 29046 | path_bps | pnl_bps=-2232 |
| path point | 29948 | path_bps | pnl_bps=-1645 |
| path point | 31046 | path_bps | pnl_bps=-1645 |
| path point | 32046 | path_bps | pnl_bps=-1645 |
| path point | 32173 | path_bps | pnl_bps=-1031 |
| path point | 32680 | path_bps | pnl_bps=-812 |
| path point | 33502 | path_bps | pnl_bps=-646 |
| path point | 33985 | path_bps | pnl_bps=-691 |
| path point | 35045 | path_bps | pnl_bps=-691 |
| path point | 35231 | path_bps | pnl_bps=-754 |
| path point | 36337 | path_bps | pnl_bps=-2783 |
| path point | 36709 | path_bps | pnl_bps=-2834 |
| path point | 38046 | path_bps | pnl_bps=-2834 |
| path point | 38910 | path_bps | pnl_bps=-2015 |
| path point | 40046 | path_bps | pnl_bps=-2015 |
| path point | 40536 | path_bps | pnl_bps=-2382 |
| path point | 41010 | path_bps | pnl_bps=-2464 |
| path point | 42045 | path_bps | pnl_bps=-2456 |
| path point | 43046 | path_bps | pnl_bps=-2456 |
| path omitted |  | path_bps | 75 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-7449; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `1.1004057258870274e-07` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
