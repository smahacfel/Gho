# ADR-8D: R40 Selector Soft Score Profile Start

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout profile, selector soft-score runtime start, shadow-only evidence run
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `bbe06d4`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: przygotowanie i uruchomienie R40 jako shadow-only selector-soft-score validation run
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R40_SELECTOR_SOFT_SCORE_PROFILE_START_20260620.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T032219Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T032219Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Utworzyc nowy profil R40, analogiczny do najnowszego R38 `maxwait31100`, ale z oddzielnym namespace i output paths. R40 ma realizowac aktywna polityke `selector_soft_score.buy_gate` z progami zapisanymi w brain configu.

Założenia:
- Projekt byl juz przebudowany przez uzytkownika.
- Run ma byc shadow-only.
- Run ma zostac pozostawiony w tle.
- Nie nalezy modyfikowac live execution, sendera, TX builderow ani thresholdow po starcie.

## 2. Opis problemu - 3W2H

What:
Po implementacji selector soft-score potrzebny byl swiezy runtime proof na nowym scope, z nowymi artefaktami, bez mieszania z R37/R38.

Where:
- rollout wrapper config,
- ghost brain sampler config,
- selector lifecycle launcher,
- R40 logs/reports.

Why:
Stare artefakty R37/R38 mieszaly poprzednie binarki, poprzednie konfiguracje i rozne etapy poprawek. R40 daje czysty punkt startowy dla oceny soft-score polityki.

How:
Skopiowano R38 `maxwait31100` jako baze R40, wykonano mechaniczna izolacje identyfikatorow `r38/R38 -> r40/R40` i uruchomiono przez `scripts/start_selector_lifecycle_run.py`.

How many:
Utworzono dwa nowe pliki konfiguracyjne i jeden nowy ADR. Runtime artifacts sa generowane przez dzialajacy proces.

## 3. Strategia naprawy / startu

Przyjeta strategia:
- Uzyc R38 jako bazy, bo byl najnowszym profilem z 31100 ms observation window i selector-soft-score configiem.
- Zachowac FSC off.
- Zachowac `strict_metric_threshold_gate_enabled=false`.
- Zachowac `selector_soft_score.enabled=true`, `policy="buy_gate"`, `min_candidate_score=2`, `min_buy_score=3`.
- Zachowac wszystkie 12 progow i wagi 1.
- Nadac osobny scope:
  - `shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1`.
- Start wykonac przez selector lifecycle launcher z `--allow-zero-buy-lifecycle-proof`, zeby event-ingest proof zostawil run w tle bez wymogu klasycznego BUY lifecycle w pierwszych minutach.

Non-goals:
- Brak runtime threshold tuning po starcie.
- Brak live execution.
- Brak zmian w materializacji, Gatekeeper policy code, Seer, TX builderach lub shadow senderze.
- Brak kasowania lub rotacji starych artefaktow.

## 4. Przeprowadzone akcje

Zmiana 1: brain config R40
- Plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r40_threshold_probe_maxwait31100_fsc_off.toml`
- Baza: R38 maxwait31100.
- Aktywny selector:
  - `enabled=true`
  - `policy="buy_gate"`
  - `min_candidate_score=2`
  - `min_buy_score=3`
  - `missing_metric_policy="no_point"`
  - `allow_degraded_cpv=true`
  - `allow_carried_temporal_deltas=true`
- Strict metric hard gate:
  - `strict_metric_threshold_gate_enabled=false`

Zmiana 2: rollout wrapper R40
- Plik: `configs/rollout/shadow-burnin-v3-r40-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `ghost_brain_config_path` wskazuje nowy R40 brain config.
- Wszystkie decision/shadow/dataset/WAL/log paths wskazuja R40 scope.
- `execution.execution_mode="shadow"` i `trigger.entry_mode="shadow_only"`.

Zmiana 3: runtime start
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

## 5. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| R40 config files absent before creation | both absent | PASS |
| R40 scope isolation | no `r38/R38` residue in new R40 configs | PASS |
| Storage gate | free_gb 44.43 >= 35.0 | PASS |
| Launcher config contract | PASS | PASS |
| Launcher scope contract | PASS | PASS |
| Static guard | PASS | PASS |
| Preflight | PASS | PASS |
| Event canary | `SELECTOR_EVENT_CANARY_PASS` | PASS |
| Runtime left running | tmux `r40-selector-soft-score` alive | PASS |
| Runtime process alive | release `ghost-launcher --config ...r40...` alive | PASS |
| First selector verdicts visible | `REJECT_SELECTOR_BELOW_BUY`, `REJECT_SELECTOR_NOT_CANDIDATE` | PASS |
| First selector diagnostics visible | `selector_soft_score` payload in decision/probe artifacts | PASS |

Event canary details:
- `NewPoolDetected` delta: 22
- `PoolTransaction` delta: 961
- `Candidate` delta: 15
- `diag_account_update_relay_delta`: 1910
- bad event json delta: 0

Early runtime artifacts:
- `shadow_entries.jsonl`: present
- `shadow_lifecycle.jsonl`: present
- `probe_skips.jsonl`: present
- `gatekeeper_v2_decisions.jsonl`: present for legacy live and v25 shadow planes

## 6. Ryzyka i zabezpieczenia

Ryzyko 1: R40 pomylony ze starym R38.
- Zabezpieczenie: osobne pliki config, osobny namespace, osobne log/dataset/report paths, brak `r38/R38` residue w nowych configach.

Ryzyko 2: live execution przypadkowo wlaczone.
- Zabezpieczenie: wrapper utrzymuje `execution_mode="shadow"` i `entry_mode="shadow_only"`; lifecycle launcher config contract przeszedl.

Ryzyko 3: run ubity przez brak BUY lifecycle w pierwszych minutach.
- Zabezpieczenie: uzyto `--allow-zero-buy-lifecycle-proof`; event canary PASS zostawia runtime w tmux.

Ryzyko 4: selector soft-score nie jest aktywny.
- Zabezpieczenie: pierwsze runtime decyzje zawieraja `REJECT_SELECTOR_BELOW_BUY` i `REJECT_SELECTOR_NOT_CANDIDATE` oraz `selector_soft_score` diagnostics.

Ryzyko 5: runtime dziala na innej binarce niz oczekiwano.
- Zabezpieczenie: launcher raportuje runtime binary `/root/Gho/target/release/ghost-launcher`, mtime `2026-06-20T02:55:28.511363+00:00`. `build_release_before_start=false`, bo projekt byl przebudowany przez uzytkownika przed startem.

## 7. Decyzja

R40 zostal przygotowany i uruchomiony jako shadow-only selector-soft-score validation run. Proces pozostaje aktywny w tmux `r40-selector-soft-score`; dalsza ocena wymaga zebrania wiekszej probki i pozniejszego audytu artefaktow R40.
