# R49 TimeStop V2 Target/Stop Matrix Report

- scope: `shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1`
- mode: target/stop/max-hold matrix from exact `shadow_exit_replay_v1` levels
- targets_bps: `1000,1500,2000,3000,5000,6000,7500,10000`
- stops_bps: `-200,-300,-500,-700,-1000,-1500,-2000,-3000,-5000,-6000`
- max_hold_ms: `30000,60000,120000`
- matrix_rows: `240`
- positive_delta_rows: `155/240`
- recommendation: `TIMESTOP_V2_NO_ECONOMIC_BENEFIT`
- generated_at: `2026-06-26T11:05:41.293795+00:00`

## Artefakty uzyte jako wejscie

- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_exit_replay_v1.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/shadow_lifecycle.jsonl`
- `logs/shadow_run/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/probe_shadow_lifecycle.jsonl`

## Artefakty wynikowe

- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_target_stop_matrix_report_v1.json`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_target_stop_matrix_exit_v1.jsonl`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/TIME_STOP_V2_TARGET_STOP_MATRIX_COUNTERFACTUAL_REPORT.md`
- `reports/selector/shadow-burnin-v3-r49-r48-target60-stop60-exit-replay-timestop-v2-maxwait66000-r1/time_stop_v2_target_stop_matrix_summary_v1.csv`

## Top combinations by PnL delta

| target_bps | stop_bps | max_hold_ms | delta_sum_bps | delta_avg_bps | baseline_sum | tsv2_sum | targets_cut | stops_saved | beneficial | harmful | tsv2_exits |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 10000 | -6000 | 120000 | 134370 | 214.99 | -320463 | -186093 | 4 | 12 | 240 | 92 | 625 |
| 7500 | -6000 | 120000 | 128578 | 205.72 | -295243 | -166665 | 7 | 12 | 239 | 93 | 625 |
| 10000 | -5000 | 120000 | 125938 | 201.82 | -305733 | -179795 | 4 | 32 | 241 | 90 | 624 |
| 7500 | -5000 | 120000 | 120146 | 192.54 | -281513 | -161367 | 7 | 32 | 240 | 91 | 624 |
| 6000 | -6000 | 120000 | 118578 | 189.72 | -249695 | -131117 | 13 | 11 | 237 | 95 | 625 |
| 6000 | -5000 | 120000 | 111146 | 178.12 | -237965 | -126819 | 13 | 31 | 238 | 93 | 624 |
| 2000 | -6000 | 120000 | 109597 | 177.63 | -281423 | -171826 | 49 | 10 | 223 | 101 | 617 |
| 3000 | -6000 | 120000 | 106420 | 171.09 | -264460 | -158040 | 37 | 10 | 229 | 100 | 622 |
| 2000 | -5000 | 120000 | 103243 | 167.60 | -270771 | -167528 | 49 | 26 | 224 | 99 | 616 |
| 3000 | -5000 | 120000 | 100907 | 162.49 | -254649 | -153742 | 37 | 28 | 230 | 98 | 621 |
| 5000 | -6000 | 120000 | 94067 | 150.51 | -233545 | -139478 | 19 | 11 | 234 | 98 | 625 |
| 5000 | -5000 | 120000 | 87554 | 140.31 | -222734 | -135180 | 19 | 29 | 235 | 96 | 624 |
| 1000 | -6000 | 120000 | 87002 | 145.49 | -270112 | -183110 | 64 | 8 | 202 | 103 | 598 |
| 1000 | -5000 | 120000 | 84165 | 140.98 | -262977 | -178812 | 64 | 20 | 203 | 101 | 597 |
| 1500 | -6000 | 120000 | 77559 | 127.56 | -251627 | -174068 | 61 | 8 | 209 | 106 | 608 |

## Worst combinations by PnL delta

| target_bps | stop_bps | max_hold_ms | delta_sum_bps | delta_avg_bps | baseline_sum | tsv2_sum | targets_cut | stops_saved | beneficial | harmful | tsv2_exits |
|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|---:|
| 5000 | -200 | 120000 | -38138 | -76.12 | 42157 | 4019 | 10 | 139 | 174 | 53 | 501 |
| 10000 | -200 | 120000 | -36760 | -73.37 | 25264 | -11496 | 3 | 142 | 177 | 50 | 501 |
| 10000 | -200 | 60000 | -35347 | -70.55 | 37436 | 2089 | 2 | 100 | 133 | 57 | 501 |
| 5000 | -300 | 120000 | -34868 | -67.97 | 27290 | -7578 | 11 | 111 | 180 | 58 | 513 |
| 6000 | -200 | 120000 | -32336 | -64.54 | 41716 | 9380 | 7 | 141 | 176 | 51 | 501 |
| 10000 | -300 | 120000 | -30510 | -59.47 | 7017 | -23493 | 3 | 114 | 183 | 55 | 513 |
| 7500 | -200 | 60000 | -30347 | -60.57 | 35136 | 4789 | 2 | 100 | 133 | 57 | 501 |
| 7500 | -200 | 120000 | -29260 | -58.40 | 20464 | -8796 | 3 | 142 | 177 | 50 | 501 |
| 6000 | -200 | 60000 | -27548 | -54.99 | 40537 | 12989 | 4 | 100 | 133 | 57 | 501 |
| 10000 | -300 | 60000 | -27187 | -53.00 | 17679 | -9508 | 2 | 81 | 138 | 61 | 513 |
| 6000 | -300 | 120000 | -26186 | -51.04 | 23969 | -2217 | 7 | 113 | 182 | 56 | 513 |
| 3000 | -200 | 120000 | -24297 | -48.59 | 14825 | -9472 | 15 | 137 | 172 | 54 | 500 |
| 5000 | -200 | 60000 | -23618 | -47.14 | 29235 | 5617 | 5 | 100 | 133 | 57 | 501 |
| 7500 | -300 | 120000 | -23010 | -44.85 | 2317 | -20693 | 3 | 114 | 183 | 55 | 513 |
| 5000 | -500 | 120000 | -22633 | -42.30 | -1479 | -24112 | 11 | 103 | 192 | 64 | 535 |

## Average delta by target

| target_bps | avg_delta_sum_bps | positive_rows | rows |
|---:|---:|---:|---:|
| 1000 | 16764.20 | 23 | 30 |
| 1500 | 12252.90 | 22 | 30 |
| 2000 | 20092.03 | 24 | 30 |
| 3000 | 17396.47 | 23 | 30 |
| 5000 | 7158.53 | 18 | 30 |
| 6000 | 10486.53 | 19 | 30 |
| 7500 | 11122.53 | 14 | 30 |
| 10000 | 8307.27 | 12 | 30 |

## Average delta by stop

| stop_bps | avg_delta_sum_bps | positive_rows | rows |
|---:|---:|---:|---:|
| -6000 | 54847.88 | 24 | 24 |
| -5000 | 48401.62 | 24 | 24 |
| -3000 | 30632.67 | 24 | 24 |
| -2000 | 14608.79 | 20 | 24 |
| -1500 | 5234.00 | 16 | 24 |
| -1000 | 8913.17 | 21 | 24 |
| -700 | 2679.83 | 16 | 24 |
| -500 | -4442.21 | 10 | 24 |
| -300 | -12547.00 | 0 | 24 |
| -200 | -18853.17 | 0 | 24 |

## Wnioski

- TimeStop V2 dziala najlepiej przy szerokich stopach `-3000/-5000/-6000 bps`; wszystkie kombinacje dla `-5000/-6000` sa dodatnie na tym snapshotcie.
- Bardzo ciasne stop-lossy `-200/-300 bps` sa niekompatybilne z TimeStop V2 w tej probie i czesto pogarszaja PnL.
- Najlepsza kombinacja w snapshotcie: `TARGET=10000bps`, `STOP=-6000bps`, `HOLD=120000ms` z dodatnim `pnl_delta_sum_bps`.
- Konserwatywna rekomendacja calego matrix labu pozostaje `TIMESTOP_V2_NO_ECONOMIC_BENEFIT`, bo caly grid zawiera zle warianty i target-cut risk.
- Wynik jest hipoteza offline/counterfactual; nie nalezy go traktowac jako approval aktywnego exit bez finalnego R49 snapshotu i diagnostyki overflow panic.
