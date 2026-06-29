# Golden trace: losing 5

- scope: R48/R2
- run_id: shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- session_id: r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2
- pool_id: BVtTRVMb94VMRsiQvJRzPintpHFeN2Nu9Se46VJKu2MF
- base_mint: 8vdLPYYLtsBdxBWWsrLfR6eQhrQrQFzyQ8vEx8Pvpump
- entry_ts_ms: 1782406400076
- entry_price: 7.255804458971738e-08
- result: stop
- logged_result: stop
- result_quality: OK

## Chronologia

| step | ts_or_age_ms | evidence | notes |
| --- | ---: | --- | --- |
| shadow entry | 1782406400076 | shadow_exit_replay_v1.entry_price | price source is shadow/synthetic, not live landed fill |
| lifecycle shadow_dispatch | 1782406409170 | shadow_lifecycle.jsonl | close_reason=None; pnl=None |
| lifecycle exit_filled | 1782406432830 | shadow_lifecycle.jsonl | close_reason=None; pnl=-60.75122857142857 |
| lifecycle position_closed | 1782406432831 | shadow_lifecycle.jsonl | close_reason=StopLoss; pnl=-60.75122857142857 |
| path point | 0 | path_bps | pnl_bps=0 |
| path point | 8777 | path_bps | pnl_bps=813 |
| path point | 9183 | path_bps | pnl_bps=866 |
| path point | 9628 | path_bps | pnl_bps=909 |
| path point | 10558 | path_bps | pnl_bps=856 |
| path point | 11142 | path_bps | pnl_bps=825 |
| path point | 12364 | path_bps | pnl_bps=908 |
| path point | 13200 | path_bps | pnl_bps=950 |
| path point | 13367 | path_bps | pnl_bps=1006 |
| path point | 14217 | path_bps | pnl_bps=1173 |
| path point | 15669 | path_bps | pnl_bps=1186 |
| path point | 16150 | path_bps | pnl_bps=1267 |
| path point | 16715 | path_bps | pnl_bps=1356 |
| path point | 17755 | path_bps | pnl_bps=1366 |
| path point | 18276 | path_bps | pnl_bps=1428 |
| path point | 19665 | path_bps | pnl_bps=1527 |
| path point | 19820 | path_bps | pnl_bps=1448 |
| path point | 20414 | path_bps | pnl_bps=1539 |
| path point | 21082 | path_bps | pnl_bps=1614 |
| path point | 21968 | path_bps | pnl_bps=1684 |
| path point | 22688 | path_bps | pnl_bps=1865 |
| path point | 23755 | path_bps | pnl_bps=1865 |
| path point | 24730 | path_bps | pnl_bps=1948 |
| path point | 25735 | path_bps | pnl_bps=2073 |
| path point | 26562 | path_bps | pnl_bps=2131 |
| path point | 27267 | path_bps | pnl_bps=2204 |
| path point | 28042 | path_bps | pnl_bps=2240 |
| path point | 28883 | path_bps | pnl_bps=2284 |
| path point | 30050 | path_bps | pnl_bps=2470 |
| path point | 31254 | path_bps | pnl_bps=2478 |
| path point | 31479 | path_bps | pnl_bps=2572 |
| path point | 31932 | path_bps | pnl_bps=2665 |
| path point | 32575 | path_bps | pnl_bps=-6035 |
| path point | 33257 | path_bps | pnl_bps=-6062 |
| path point | 34754 | path_bps | pnl_bps=-6086 |
| path point | 36259 | path_bps | pnl_bps=-6126 |
| path point | 37755 | path_bps | pnl_bps=-6126 |
| path point | 38755 | path_bps | pnl_bps=-6126 |
| path point | 39755 | path_bps | pnl_bps=-6126 |
| path point | 40755 | path_bps | pnl_bps=-6126 |
| path omitted |  | path_bps | 69 additional points omitted from trace view |
| replay close | 120000 | shadow_exit_replay_v1 | last_pnl_bps=-6127; quality=clean; truncated=False |

## Odpowiedzi audytowe

- Co shadow uwazal za cene: `7.255804458971738e-08` plus post-entry path_bps.
- Skad cena: shadow_exit_replay_v1.entry_price i runtime shadow entry/lifecycle, nie potwierdzony live fill.
- Czego live potrzebowalby do tej ceny: quote/min_out, submit timestamp, landing slot, slippage, fees and failure/no-fill status.
- Niezamodelowane: latency/slippage/own-impact/failed landing; szczegoly w `reports/selector/shadow_fidelity_live_equivalence_gap.csv`.
- Czy trace jest wiarygodny: OK dla offline path research; nie live-equivalent.
