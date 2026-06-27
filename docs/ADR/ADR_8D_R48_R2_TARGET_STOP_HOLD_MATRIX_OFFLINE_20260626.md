# ADR-8D: R48/R2 Target Stop Hold Offline Matrix

Status: IMPLEMENTED / TARGETED_TESTS_PASSED / OFFLINE_REPORT_GENERATED
Typ: ADR-8D / offline research tooling / shadow exit replay evaluation
Data: 2026-06-26
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `research/alpha-31100-validation-harness-v1`
HEAD podczas pracy: `f618d8e8ae09858cbcaf7a2efcd8eb1017927b49`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: offline-only matrix Target x StopLoss x max_hold for R48/R2 `shadow_exit_replay_v1`
Poziom ryzyka: LOW runtime risk / MEDIUM analytical risk

Dotkniete moduly/pliki:
- `scripts/shadow_exit_replay_eval.py`
- `scripts/test_shadow_exit_replay_eval.py`
- `docs/ADR/ADR_8D_R48_R2_TARGET_STOP_HOLD_MATRIX_OFFLINE_20260626.md`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_matrix_exact.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_matrix_exact.json`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_top20.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_stability.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/TARGET_STOP_HOLD_MATRIX_REPORT.md`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_cost_sensitivity.csv`
- `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/target_stop_hold_heatmap_*.csv`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje istniejacy lokalny format ADR-8D uzyty juz w repo.

## 1. Przygotowanie i dzialania wstepne

Cel:
Policzyc offline macierz `Target x StopLoss x max_hold` na rzeczywistych sciezkach cenowych R48/R2 zapisanych w `shadow_exit_replay_v1.jsonl`.

Twarde ograniczenia:
- offline-only,
- bez zmian Rust/runtime,
- bez zmian Gatekeepera, BUY/REJECT, v2.5, v3 i selectora,
- bez zmian lifecycle i aktywnego runu,
- bez uzycia lifecycle `close_reason`,
- bez filtrowania po `selector_shadow_score`,
- bez R48/R1,
- bez interpolowania barrier hitow z `path_bps`.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powod uzycia: ochrona shadow/live boundary, replay semantics i brak zmian aktywnego runtime.
Zakres uzycia: klasyfikacja zmiany jako offline-only i potwierdzenie, ze Gatekeeper/BUY/REJECT/lifecycle nie sa dotykane.
Wynik: zmiana pozostala w skrypcie offline i raportach.

Nazwa: `large-data-analytics`
Powod uzycia: kontrola jakosci event/replay datasetu, duplicate checks, common level grid, stabilnosc czasowa.
Zakres uzycia: snapshot discipline, data quality gate, tercyle chronologiczne, heatmap artifacts.

Nazwa: `statistical-research-engine`
Powod uzycia: stabilnosc, no-lookahead, unikanie wyboru zwyciezcy tylko po `sum_pnl_bps`.
Zakres uzycia: Pareto frontier, top-k wedlug avg/median/profit factor, cost sensitivity oddzielona od gross.

## 3. Opis problemu - 3W2H

What:
Dotychczasowy `scripts/shadow_exit_replay_eval.py` ocenial tylko `target_bps x stop_bps` bez wymiaru `max_hold_ms` i bez pelnej kontroli danych/snapshotu.

Where:
- `scripts/shadow_exit_replay_eval.py`
- R48/R2 `shadow_exit_replay_v1.jsonl`
- raporty pod `reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/`

Why it matters:
Aktywny lifecycle R48/R2 ma tylko jeden profil exit. `shadow_exit_replay_v1` pozwala offline przetestowac wiele alternatywnych exitow bez nowych rolloutow i bez zmiany runtime.

How observed:
R48/R2 aktywnie produkowal `shadow_exit_replay_v1.jsonl`. Snapshot zrobiono do `/tmp`, zeby nie liczyc na pliku, ktory moze dalej rosnac.

How many / scale:
- snapshot records: 3621,
- qualified records: 3600,
- matrix combinations: 1232,
- max_hold variants: 8,
- target levels: 14,
- stop levels: 11.

## 4. Przyczyna zrodlowa

Root cause:
Pierwsza wersja evaluator byla minimalnym narzedziem do target/stop replay i nie miala kontraktu dla max-hold, kosztow, tercyli, Pareto frontier ani szczegolowej kontroli danych.

Mechanizm ryzyka:
Bez max-hold i snapshot discipline wynik moglby mieszac rosnacy runtime log lub uzywac `last_pnl_bps` tam, gdzie wymagany jest ostatni punkt `path_bps <= max_hold_ms`.

## 5. Strategia naprawy

Przyjeta strategia:
Minimalnie rozszerzyc istniejacy `scripts/shadow_exit_replay_eval.py`, zamiast tworzyc nowy skrypt. Stary tryb CLI `--targets-bps/--stops-bps` zostal zachowany do kompatybilnosci i porownania `+6000/-6000/120000`.

Nowy tryb:
- `--matrix-output-dir`,
- common exact level grid,
- max-hold simulation,
- data controls,
- top20,
- stability terciles,
- Pareto frontier,
- cost sensitivity,
- Markdown report,
- heatmap CSV per `max_hold_ms`.

Semantyka:
- `first_hit_ms[target_bps]` i `first_hit_ms[stop_bps]` sa exact barrier hits,
- hit liczy sie tylko gdy `<= max_hold_ms`,
- tie target/stop w tej samej ms wygrywa STOP,
- TIMEOUT uzywa ostatniego `path_bps` punktu z `age_ms <= max_hold_ms`,
- brak punktu timeout nie jest zamieniany na zero,
- wyniki glowne sa gross, nie net.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: evaluator offline
- Dodano typ `ReplayRecord` i parser kontroli danych.
- Dodano `simulate_record()` dla exact Target/StopLoss/max-hold.
- Dodano `evaluate_matrix()` z metrykami wymaganymi przez zadanie.
- Zachowano funkcje `evaluate()` dla dotychczasowego trybu bez max-hold.

Zmiana 2: raporty
- Zapisano pelna macierz CSV/JSON.
- Zapisano top20, stability terciles i cost sensitivity.
- Zapisano heatmap CSV dla `avg_pnl_bps`, `median_pnl_bps`, `profit_factor`, `stop_rate`, `timeout_rate` osobno dla kazdego `max_hold_ms`.
- Zapisano `TARGET_STOP_HOLD_MATRIX_REPORT.md`.

Zmiana 3: testy
- Rozszerzono `scripts/test_shadow_exit_replay_eval.py` do 11 testow.
- Dodano przypadki Target przed Stop, Stop przed Target, tie STOP, TIMEOUT path-prev, brak timeout point != zero, degraded/unavailable exclusion, determinism i `+6000/-6000/120000` legacy parity.

## 7. Snapshot i wynik operacyjny

Snapshot:
- source: `logs/shadow_run/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2/shadow_exit_replay_v1.jsonl`
- snapshot: `/tmp/r48_r2_shadow_exit_replay_v1_20260626T035624Z.jsonl`
- timestamp UTC: `2026-06-26T03:56:24Z`
- record count: `3621`
- size bytes: `9141710`
- SHA-256: `ddae06c8ade85c9359fe4683cb997c1c86424bb9d4ba005d87a36148f1d12908`

Data controls:
- `quality_counts`: `clean=3600`, `unavailable=21`
- `truncated=false`: `3621`
- `horizon_ms=120000`: `3621`
- valid `path_bps`: `3621`
- duplicates key `(run_id, session_id, pool_id, base_mint, entry_ts_ms)`: `0`
- qualified records for main matrix: `3600`

Common exact levels:
`[-6000, -5000, -3000, -2000, -1500, -1000, -700, -500, -300, -200, -100, 100, 200, 300, 400, 500, 700, 1000, 1500, 2000, 3000, 5000, 6000, 7500, 10000]`

## 8. Walidacja dzialan

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| Evaluator unit tests | `python3 scripts/test_shadow_exit_replay_eval.py` | 11 tests passed | PASS |
| Python compile | `python3 -m py_compile scripts/shadow_exit_replay_eval.py scripts/test_shadow_exit_replay_eval.py` | passed | PASS |
| R48/R2 matrix run | `python3 scripts/shadow_exit_replay_eval.py --input /tmp/r48_r2_shadow_exit_replay_v1_20260626T035624Z.jsonl --matrix-output-dir reports/selector/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target60-stop60-exit-replay-r2 --snapshot-timestamp-utc 2026-06-26T03:56:24Z --roundtrip-cost-bps 0,50,100,150,200` | 1232 combinations generated | PASS |
| Legacy parity | built-in comparison for `+6000/-6000/120000` | counts and sum PnL match | PASS |

Legacy parity:
- legacy total: `3600`
- matrix eligible: `3600`
- target_count: `205`
- stop_count: `212`
- timeout_count: `3183`
- sum_pnl_bps: `-2546969`
- avg_pnl_bps: `-707.4913888888889`
- median_pnl_bps: `-220.0`

## 9. Ryzyka resztkowe

- Wynik jest gross replay PnL. Nie ma dowodu w rekordzie, ze obejmuje Pump.fun fee, priority fee, Jito tip lub live execution costs.
- TIMEOUT PnL uzywa kompresowanego `path_bps`, wiec jest `path_prev_timeout`, nie raw tick-by-tick.
- Top wynik wedlug avg ma bardzo wysoki stop rate i ujemna mediane; nie powinien byc wybierany jako zwyciezca tylko po `sum_pnl_bps`.
- Jest to analiza R48/R2 snapshot, nie dowod stabilnosci poza tym runem.

## 10. Scope out

Poza zakresem:
- TimeStop V2,
- alpha,
- XGBoost,
- selector scoring/filtering,
- Gatekeeper policy,
- BUY/REJECT,
- v2.5/v3,
- Rust/runtime,
- lifecycle close behavior,
- live execution,
- nowa telemetria runtime.

## 11. Decyzja

Przyjeto rozszerzenie istniejacego offline evaluatora jako najmniejsza bezpieczna zmiane. R48/R2 run nie zostal zatrzymany ani zmodyfikowany. Wyniki sa zapisane jako artefakty research-only i nie sa konsumowane przez aktywny runtime.
