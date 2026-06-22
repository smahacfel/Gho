# ADR-8D: R40 Selector Min Buy Score 8 Restart

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout threshold change, selector soft-score restart, shadow-only runtime evidence
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: zmiana `gatekeeper_v2.selector_soft_score.min_buy_score` z `6` na `8` w R40 i restart tego samego profile/scope
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- `docs/ADR/ADR_8D_R40_SELECTOR_MIN_BUY_SCORE_8_RESTART_20260620.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T091643Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T091712Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T091712Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zaostrzyc BUY threshold selector soft-score w R40: `score >= 8` zamiast `score >= 6`, bez zmiany listy 12 regul, progow pojedynczych regul, wag, shadow/live boundary ani scope.

Wymagania:
- Zmienic warunek minimum soft score z `6` na `8`.
- Uruchomic ponownie run na tym samym profilu i scope.
- Proces zostawic w tle.
- Nie czyscic artefaktow i nie kasowac danych bez jawnej decyzji uzytkownika.

## 2. Opis problemu - 3W2H

What:
Poprzedni restart R40 dzialal z `min_buy_score=6`. Uzytkownik zdecydowal o dalszym zaostrzeniu BUY gate do `min_buy_score=8`.

Where:
- R40 brain config: `[gatekeeper_v2.selector_soft_score]`.
- Ten sam rollout wrapper i scope:
  - `shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1`.

Why:
Prog 6/12 nadal moze byc zbyt permissive dla aktualnego testu selektora. Zmiana na 8/12 pozwala sprawdzic bardziej selektywny BUY gate bez powrotu do hard-AND pojedynczych metryk.

How:
Zmieniono tylko `min_buy_score = 8`, zatrzymano poprzednia sesje `r40-selector-soft-score` i uruchomiono ten sam wrapper przez selector lifecycle launcher.

How many:
Zmiana dotyczy jednego pola configu i jednego runtime restartu. Poniewaz scope zostal zachowany, stare artefakty w tym samym scope zawieraja okresy z `min_buy_score=3` oraz `min_buy_score=6`; swiezy run nalezy rozpoznawac po launcher timestampie `20260620T091712Z`, nowym katalogu decyzji/hashu profilu albo po `evidence_policy_context.selector_soft_score_min_buy_score=8`.

## 3. Przeprowadzone akcje

Zmiana 1: selector threshold
- Plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- Bylo:
  - `min_buy_score = 6`
- Jest:
  - `min_buy_score = 8`

Nie zmieniono:
- `min_candidate_score = 2`
- `policy = "buy_gate"`
- `missing_metric_policy = "no_point"`
- `allow_degraded_cpv = true`
- `allow_carried_temporal_deltas = true`
- progow 12 regul
- wag 12 regul
- `strict_metric_threshold_gate_enabled = false`
- shadow-only execution settings

Zmiana 2: stop poprzedniego R40
- Sesja zatrzymana:
  - `tmux kill-session -t r40-selector-soft-score`
- Po zatrzymaniu:
  - `tmux ls` nie znajdowal sesji,
  - `pgrep` nie znajdowal procesu R40.

Zmiana 3: pierwszy restart - zatrzymany przez storage guard
- Komenda standardowa:
  - `python3 scripts/start_selector_lifecycle_run.py --root /root/Gho --scope shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1 --config configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml --tmux-session r40-selector-soft-score --event-canary-seconds 120 --allow-zero-buy-lifecycle-proof --skip-static-tests`
- Wynik:
  - `SELECTOR_LIFECYCLE_RUN_START_FAIL:INCONCLUSIVE_ENV_OR_CONFIG`
  - `status=INCONCLUSIVE_ENV_OR_CONFIG`
- Przyczyna:
  - storage gate: `free_gb=28.07 < min_free_gb=35.00`
- Runtime:
  - nie zostal wystartowany.

Zmiana 4: drugi restart - jawnie obnizony storage guard
- Komenda:
  - `python3 scripts/start_selector_lifecycle_run.py --root /root/Gho --scope shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1 --config configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml --tmux-session r40-selector-soft-score --event-canary-seconds 120 --allow-zero-buy-lifecycle-proof --skip-static-tests --min-free-gb 25`
- Uzasadnienie:
  - na filesystemie bylo ok. 28 GB wolnego miejsca,
  - uzytkownik oczekiwal restartu,
  - nie wykonano kasowania artefaktow ani cleanupu.
- Launcher report:
  - status `PASS`
  - claim `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
  - run_state `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`
- Tmux session:
  - `r40-selector-soft-score`
- Runtime process:
  - `/root/Gho/target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml`

## 4. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| Config threshold | `min_buy_score = 8` | PASS |
| Old runtime stopped | old tmux/session gone before restart | PASS |
| First launcher attempt | blocked by storage guard before runtime start | PASS/EXPECTED |
| Second launcher storage gate | `free_gb=28.0737`, `min_free_gb=25.0` | PASS |
| Launcher scope | `shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1` | PASS |
| Event canary | `SELECTOR_EVENT_CANARY_PASS` | PASS |
| Runtime left running | tmux `r40-selector-soft-score` alive | PASS |
| Runtime process alive | release `ghost-launcher --config ...r40...` alive | PASS |
| Fresh decision context proof | 41/41 latest v2.2 records have `selector_soft_score_min_buy_score=8` in `evidence_policy_context` | PASS |
| Fresh v25 decision context proof | 41/41 latest v2.5 records have `selector_soft_score_min_buy_score=8` in `evidence_policy_context` | PASS |
| Fresh BUY proof | latest legacy_live BUY file: 6/6 records have min score 8 and observed scores are only `8,10,11,12` | PASS |

Event canary details after restart:
- `NewPoolDetected` delta: 36
- `PoolTransaction` delta: 855
- `Candidate` delta: 31
- `diag_account_update_relay_delta`: 1746
- bad event json delta: 0

Fresh runtime proof:
- New launcher output dir:
  - `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T091712Z/`
- Latest decision artifact hash/segment:
  - `576cd4e8080d43bf2bdd4a58f6dc3ec1f4b481028c54d9972cd101d90001cba2`
- Fresh decision files checked:
  - `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/576cd4e8080d43bf2bdd4a58f6dc3ec1f4b481028c54d9972cd101d90001cba2/gatekeeper_v2_decisions.jsonl`
  - `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/v2.5/v25_shadow/576cd4e8080d43bf2bdd4a58f6dc3ec1f4b481028c54d9972cd101d90001cba2/gatekeeper_v2_decisions.jsonl`
  - `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/576cd4e8080d43bf2bdd4a58f6dc3ec1f4b481028c54d9972cd101d90001cba2/gatekeeper_v2_buys.jsonl`

Latest v25 decision score distribution at check time:
- score 0: 10
- score 1: 2
- score 2: 7
- score 3: 7
- score 4: 1
- score 5: 8
- score 7: 2
- score 8: 2
- score 9: 1
- score 12: 1

Latest legacy_live BUY score distribution at check time:
- score 8: 2
- score 10: 1
- score 11: 2
- score 12: 1

## 5. Ryzyka i zabezpieczenia

Ryzyko 1: stare i nowe artefakty sa w tym samym scope.
- Kiedy: analiza offline czyta caly scope R40 bez rozroznienia config hash, launcher timestamp albo `evidence_policy_context`.
- Zabezpieczenie: nowy launcher report ma timestamp `20260620T091712Z`, a nowe decyzje w sprawdzonym segmencie maja `selector_soft_score_min_buy_score=8`. Starsze segmenty R40 nadal moga zawierac `min_buy_score=3` i `min_buy_score=6`.

Ryzyko 2: zmiana progu zostanie potraktowana jak nowa semantyka metryk.
- Zabezpieczenie: zmieniono tylko `min_buy_score`; wszystkie reguly, progi reguly, wagi i missing/degraded semantics zostaly bez zmian.

Ryzyko 3: live/shadow boundary naruszony podczas restartu.
- Zabezpieczenie: restart wykonano tym samym rollout wrapperem R40; nie zmieniano ustawien execution/entry mode.

Ryzyko 4: storage guard zostal obnizony.
- Kiedy: dalszy dlugi run moze szybciej dojsc do limitu przestrzeni dyskowej.
- Zabezpieczenie: obnizenie bylo jawne i lokalne dla launchera (`--min-free-gb 25`), bez zmiany configu projektu i bez usuwania artefaktow. Ten fakt musi byc brany pod uwage przy dlugim runie.

Ryzyko 5: brak top-level pola progu.
- Obserwacja: w swiezych decyzjach prog jest widoczny w `evidence_policy_context.selector_soft_score_min_buy_score=8`; osobne top-level `selector_soft_score_min_buy_score` nie bylo emitowane w sprawdzonym materiale.
- Zabezpieczenie: aktualny dowod opiera sie na evidence policy context. Jesli downstream wymaga plaskiego pola top-level, to jest osobny temat kompatybilnosci/loggingu, nie zmiana progu.

## 6. Decyzja

R40 zostal przestawiony na `min_buy_score=8` i ponownie uruchomiony na tym samym profilu oraz scope. Proces pozostaje aktywny w tmux `r40-selector-soft-score`. Dalsza analiza R40 powinna filtrowac artefakty po launcher timestampie `20260620T091712Z`, segmencie `576cd4e8080d43bf2bdd4a58f6dc3ec1f4b481028c54d9972cd101d90001cba2` albo po `evidence_policy_context.selector_soft_score_min_buy_score=8`, aby nie mieszac okresow `min_buy_score=3`, `6` i `8`.
