# ADR-8D: R40 Selector Min Buy Score 6 Restart

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout threshold change, selector soft-score restart, shadow-only runtime evidence
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: zmiana `gatekeeper_v2.selector_soft_score.min_buy_score` z `3` na `6` w R40 i restart tego samego profile/scope
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- `docs/ADR/ADR_8D_R40_SELECTOR_MIN_BUY_SCORE_6_RESTART_20260620.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T033610Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T033610Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zaostrzyc BUY threshold selector soft-score w R40: `score >= 6` zamiast `score >= 3`, bez zmiany listy 12 regul, progow pojedynczych regul, wag, shadow/live boundary ani scope.

Wymagania:
- Zmienic warunek minimum soft score z `3` na `6`.
- Uruchomic ponownie run na tym samym profilu i scope.
- Proces zostawic w tle.

## 2. Opis problemu - 3W2H

What:
Pierwszy R40 wystartowal z `min_buy_score=3`. Uzytkownik zdecydowal, ze BUY should require stronger soft-score support, czyli `min_buy_score=6`.

Where:
- R40 brain config: `[gatekeeper_v2.selector_soft_score]`.
- Ten sam rollout wrapper i scope:
  - `shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1`.

Why:
Przy progu 3/12 selector jest bardzo permissive. Zmiana na 6/12 pozwala sprawdzic mocniejszy warunek BUY, dalej jako soft-score aggregate, a nie hard-AND pojedynczych metryk.

How:
Zmieniono tylko `min_buy_score = 6`, zatrzymano stary tmux runtime i uruchomiono ponownie ten sam wrapper przez selector lifecycle launcher.

How many:
Zmiana dotyczy jednego pola configu i jednego runtime restartu. Poniewaz scope zostal zachowany, stare artefakty w tym samym scope zawieraja jeszcze wpisy z `min_buy_score=3`; swiezy run nalezy rozpoznawac po nowym launcher report i nowym brain config hash.

## 3. Przeprowadzone akcje

Zmiana 1: selector threshold
- Plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- Bylo:
  - `min_buy_score = 3`
- Jest:
  - `min_buy_score = 6`

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

Zmiana 3: restart tego samego scope
- Komenda:
  - `python3 scripts/start_selector_lifecycle_run.py --root /root/Gho --scope shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1 --config configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml --tmux-session r40-selector-soft-score --event-canary-seconds 120 --allow-zero-buy-lifecycle-proof --skip-static-tests`
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
| Config threshold | `min_buy_score = 6` | PASS |
| Old runtime stopped | old tmux/session gone before restart | PASS |
| Launcher config contract | PASS | PASS |
| Launcher scope contract | PASS | PASS |
| Static guard | PASS | PASS |
| Preflight | PASS | PASS |
| Event canary | `SELECTOR_EVENT_CANARY_PASS` | PASS |
| Runtime left running | tmux `r40-selector-soft-score` alive | PASS |
| Runtime process alive | release `ghost-launcher --config ...r40...` alive | PASS |
| Fresh decision proof | `selector_soft_score_min_buy=6` visible in new decision artifact | PASS |
| Fresh policy context proof | `selector_soft_score_min_buy_score=6` visible in evidence policy context | PASS |

Event canary details after restart:
- `NewPoolDetected` delta: 15
- `PoolTransaction` delta: 939
- `Candidate` delta: 10
- `diag_account_update_relay_delta`: 1894
- bad event json delta: 0

Fresh runtime proof:
- New launcher output dir:
  - `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T033610Z/`
- Fresh R40 brain config hash in decision artifact:
  - `e23c9bc99408dc7738ad9f5e7bdc0d84e30251eeb7e7d159adefafdd80408be6`
- New BUY record proof includes:
  - `selector_soft_score_min_buy: 6`
  - `selector_soft_score_buy_passed: true`
  - `evidence_policy_context.selector_soft_score_min_buy_score: 6`

## 5. Ryzyka i zabezpieczenia

Ryzyko 1: stare i nowe artefakty sa w tym samym scope.
- Kiedy: analiza offline czyta caly scope R40 bez rozroznienia config hash albo czasu restartu.
- Zabezpieczenie: nowy launcher report ma timestamp `20260620T033610Z`, a nowe decyzje maja brain config hash `e23c9bc...`. Stare artefakty z pierwszego R40 moga nadal zawierac `min_buy_score=3`.

Ryzyko 2: zmiana progu zostanie potraktowana jak nowa semantyka metryk.
- Zabezpieczenie: zmieniono tylko `min_buy_score`; wszystkie reguly, progi reguly, wagi i missing/degraded semantics zostaly bez zmian.

Ryzyko 3: live/shadow boundary naruszony podczas restartu.
- Zabezpieczenie: wrapper nadal wskazuje `execution_mode="shadow"` i `entry_mode="shadow_only"`; launcher config contract przeszedl.

Ryzyko 4: run zatrzymany przez brak klasycznego BUY lifecycle w canary.
- Zabezpieczenie: uzyto `--allow-zero-buy-lifecycle-proof`, bo ten run jest selector/data-collection proofem.

## 6. Decyzja

R40 zostal przestawiony na `min_buy_score=6` i ponownie uruchomiony na tym samym profilu oraz scope. Proces pozostaje aktywny w tmux `r40-selector-soft-score`. Dalsza analiza R40 powinna filtrowac artefakty po nowym launcher timestampie albo brain config hash, aby nie mieszac okresu `min_buy_score=3` z okresem `min_buy_score=6`.
