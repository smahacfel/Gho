# ADR-8D: R41 Selector Score12 Timeout Median Filters Start

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout threshold change, selector soft-score profile start, shadow-only runtime evidence
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy R41 scope/profile na bazie R40 z `selector_soft_score.min_buy_score=12` oraz core floors z R40 Timeout snapshot
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R41_SELECTOR_SCORE12_TIMEOUT_MEDIAN_FILTERS_START_20260620.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T160533Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T160533Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Utworzyc osobny R41 scope/profile po R40, bez mieszania nowych decyzji z artefaktami R40, i uruchomic shadow-only run z ostrzejsza selekcja.

Wymagania:
- Zmienic `gatekeeper_v2.selector_soft_score.min_buy_score` z `8` na `12`.
- Ustawic core floors na podstawie R40 Timeout snapshot:
  - `min_market_cap_sol = 30.0`
  - `min_tx_count = 22`
  - `min_buy_count = 12`
  - `min_total_volume_sol = 18.0`
  - `min_bonding_progress_pct = 38.0`
- Wykluczyc `total_unique_buyers` z configu, bo pole `unique_buyers` bylo zerowe w analizowanym log surface.
- Reszte progow, execution, probe, route-evidence i shadow behavior zostawic jak w R40.

## 2. Opis problemu - 3W2H

What:
R40 pokazal, ze `score=12` ma lepszy outcome profile niz `8-11`, ale nadal duzo pozycji konczylo sie Timeout. R41 testuje pelny `12/12` BUY gate plus minimalne core floors dobrane z median/mean Timeout population, aby ograniczyc najslabszy tail.

Where:
- Brain profile:
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- Rollout wrapper/scope:
  - `configs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1.toml`
  - `shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1`

Why:
`score>=8` bylo zbyt szerokie. R41 ma sprawdzic, czy `score=12` z prostymi floorami dla market/flow/bonding ograniczy Timeout-heavy BUY set bez zmiany execution path.

How:
Skopiowano R40 do nowego R41 scope/profile i zmieniono tylko wskazane config thresholds oraz nazwy artifact roots. R41 uruchomiono przez `scripts/start_selector_lifecycle_run.py`.

How many:
Zmiana obejmuje dwa nowe pliki config i jeden ADR. Nie zmieniano kodu Rust, schemy JSONL, TX buildera, sendera, IWIM, PDD, V3 scoringu ani probe mechanics.

## 3. Przeprowadzone akcje

Zmiana 1: nowy brain profile R41
- Bazowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- Nowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- Zmiany:
  - `min_tx_count = 22`
  - `min_buy_count = 12`
  - `min_total_volume_sol = 18.0`
  - `min_bonding_progress_pct = 38.0`
  - `min_market_cap_sol = 30.0`
  - `gatekeeper_v2.selector_soft_score.min_buy_score = 12`

Zmiana 2: nowy rollout wrapper/scope R41
- Bazowy plik: `configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml`
- Nowy plik: `configs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1.toml`
- Nowe artifact roots:
  - `logs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`
  - `logs/shadow_run/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`
  - `datasets/events/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`
  - `data/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/`

Nie zmieniono:
- `entry_mode = "shadow_only"`
- `execution_mode = "shadow"`
- trigger shadow simulation settings
- p37 shadow probe policy, sample mode, builder mode, amount/slippage/wait settings
- FSC disabled state
- individual 12 selector rule thresholds and weights
- `strict_metric_threshold_gate_enabled = false`

## 4. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| R41 files created | new brain + rollout config | PASS |
| R41 scope isolation | all wrapper paths contain R41 scope | PASS |
| Initial dry-run | blocked before runtime by storage guard `14.54 GB < 25 GB` | EXPECTED |
| Start storage gate | explicit `--min-free-gb 10`, free `14.5377 GB` | PASS WITH RISK |
| Config contract | launcher report `PASS` | PASS |
| Scope contract | launcher report `PASS` | PASS |
| Preflight | exit code `0` | PASS |
| Event canary | `SELECTOR_EVENT_CANARY_PASS` | PASS |
| Runtime left running | `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED` | PASS |
| Fresh decision hash | `77bebf5886ad4086b938c9b0dbbd7a00096b092ba128ff04f759506da333b2be` | PASS |
| Fresh v2.2 records | 34/34 checked rows show new thresholds | PASS |

Fresh v2.2 decision proof at check time:
- `selector_soft_score_min_buy = 12`: 34/34
- `min_tx_count = 22`: 34/34
- `min_buy_count = 12`: 34/34
- `min_total_volume_sol = 18.0`: 34/34
- `min_market_cap_sol = 30.0`: 34/34
- `min_bonding_progress_pct = 38.0`: 34/34

Fresh event canary details:
- `NewPoolDetected` delta: 35
- `PoolTransaction` delta: 878
- `Candidate` delta: 27
- `diag_account_update_relay_delta`: 1754
- bad event json delta: 0

## 5. Ryzyka i zabezpieczenia

Ryzyko 1: storage pressure.
- Obserwacja: domyslny guard 25 GB zatrzymal dry-run; na dysku bylo ok. 14.54 GB wolne.
- Decyzja wykonawcza: start wykonano z jawnym `--min-free-gb 10`, bez kasowania artefaktow.
- Konsekwencja: dlugi R41 moze szybko wypelnic dysk. Run wymaga monitorowania wolnego miejsca albo osobnej decyzji o cleanup/archiwizacji starych artefaktow.

Ryzyko 2: `min_buy_score=12` moze znaczaco ograniczyc BUY count.
- Zabezpieczenie: uzyto `--allow-zero-buy-lifecycle-proof`, tak jak dla strict threshold/data-collection probe. Event canary PASS potwierdza ingest/run start; klasyczny BUY lifecycle proof nie jest wymagany do pozostawienia runa.

Ryzyko 3: nowe core floors sa policy change.
- Zabezpieczenie: zmiany sa izolowane do nowego R41 profile/scope i nie nadpisuja R40.

Ryzyko 4: `unique_buyers` zostalo pominiete.
- Uzasadnienie: R40 Timeout analysis pokazala `unique_buyers = 0` dla wszystkich sprawdzonych Timeout rows. To pole nie zostalo uzyte jako threshold.

Ryzyko 5: shadow/live boundary.
- Zabezpieczenie: wrapper zachowuje `entry_mode = "shadow_only"` oraz `execution_mode = "shadow"`; nie zmieniono sendera ani TX buildera.

## 6. Decyzja

R41 zostal utworzony i uruchomiony jako osobny shadow-only run:
- scope: `shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1`
- tmux: `r41-selector-score12-median`
- launcher report: `reports/selector/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T160533Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`

Run pozostaje aktywny. Kolejna analiza R41 powinna filtrowac po scope R41 i hashu `77bebf5886ad4086b938c9b0dbbd7a00096b092ba128ff04f759506da333b2be`, aby nie mieszac materialu z R40.
