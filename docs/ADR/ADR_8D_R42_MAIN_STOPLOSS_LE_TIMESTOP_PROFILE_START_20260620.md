# ADR-8D: R42 Main StopLoss <= TimeStop Profile Start

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout threshold change, selector soft-score profile start, shadow-only runtime evidence
Data: 2026-06-20
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy R42 scope/profile na bazie R41 z profilem MAIN dobranym pod `StopLoss <= TimeStop << Target`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r42_main_stoploss_le_timestop_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1.toml`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`
- `docs/ADR/ADR_8D_R42_MAIN_STOPLOSS_LE_TIMESTOP_PROFILE_START_20260620.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T211602Z/`
- `reports/selector/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260620T211602Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1/`
- `datasets/events/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Utworzyc osobny R42 scope/profile po R41, bez mieszania nowych decyzji z artefaktami R41, i uruchomic shadow-only run dla profilu MAIN wybranego pod priorytet:

`StopLoss <= TimeStop << Target`

Profil MAIN:
- `selector_soft_score.min_buy_score = 12`
- `min_unique_ratio = 0.60`
- `max_top3_volume_pct = 0.45`
- `min_price_change_ratio = 1.10`
- `max_dev_volume_ratio = 0.10`
- `reject_on_dev_sell = true`

R41 median/core floors zostaly zachowane:
- `min_market_cap_sol = 30.0`
- `min_tx_count = 22`
- `min_buy_count = 12`
- `min_total_volume_sol = 18.0`
- `min_bonding_progress_pct = 38.0`
- `max_wait_time_ms = 31100`

## 2. Opis problemu - 3W2H

What:
R41 potwierdzil, ze `score=12` daje bardziej selektywny zestaw BUY, ale nadal trzeba bylo zmniejszyc udzial StopLoss i TimeStop. Analiza R41/R40+R41 wskazala profil MAIN oparty o `unique_ratio`, `top3_volume_pct`, `price_change_ratio` oraz dev filters.

Where:
- Brain profile:
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r42_main_stoploss_le_timestop_maxwait31100_fsc_off.toml`
- Rollout wrapper/scope:
  - `configs/rollout/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1.toml`
  - `shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1`
- Gatekeeper lower price bound:
  - `ghost-launcher/src/components/gatekeeper_policy.rs`
  - `ghost-launcher/src/components/gatekeeper.rs`

Why:
Same wpisanie `min_price_change_ratio = 1.10` do configu nie wystarczalo, bo aktywna polityka sprawdzala w kilku miejscach tylko gorny limit `max_price_change_ratio`. R42 wymagal, aby lower bound byl realnie egzekwowany przez aktywna sciezke Gatekeepera.

How:
Skopiowano R41 do nowego R42 scope/profile, ustawiono progi MAIN i dodano minimalne sprawdzenie `price_change_ratio >= min_price_change_ratio` w aktywnych sciezkach policy/runtime przed istniejacym upper-bound checkiem. Dodatkowo naprawiono testowe initializery `GatekeeperDecision`, ktore blokowaly static guard po dodaniu pola `selector_soft_score` w strukturze.

How many:
Zmiana obejmuje dwa nowe pliki config, dwa pliki Rust w aktywnej sciezce Gatekeepera, jeden testowy plik regresyjny oraz jeden ADR. Nie zmieniano TX buildera, sendera, execution mode, p37 probe mechanics, schema JSONL ani live/shadow boundary.

## 3. Przeprowadzone akcje

Zmiana 1: nowy brain profile R42
- Bazowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- Nowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r42_main_stoploss_le_timestop_maxwait31100_fsc_off.toml`
- Najwazniejsze progi:
  - `min_unique_ratio = 0.60`
  - `max_top3_volume_pct = 0.45`
  - `min_price_change_ratio = 1.10`
  - `max_dev_volume_ratio = 0.10`
  - `reject_on_dev_sell = true`
  - `gatekeeper_v2.selector_soft_score.min_buy_score = 12`
  - `gatekeeper_v2.selector_soft_score.unique_ratio_gte = 0.60`
  - `gatekeeper_v2.selector_soft_score.top3_volume_pct_lt = 0.45`

Zmiana 2: nowy rollout wrapper/scope R42
- Bazowy plik: `configs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1.toml`
- Nowy plik: `configs/rollout/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1.toml`
- Nowy metrics port: `9130`, aby nie kolidowac z nadal aktywnym R41 na `9129`.
- `entry_mode = "shadow_only"` i `execution_mode = "shadow"` pozostaly bez zmian.

Zmiana 3: egzekwowanie lower bound `min_price_change_ratio`
- Dodano lower-bound check w `ghost-launcher/src/components/gatekeeper_policy.rs`.
- Dodano analogiczny lower-bound check w `ghost-launcher/src/components/gatekeeper.rs`.
- Zmiana korzysta z istniejacego pola configu, bez nowego schema field.

Zmiana 4: test-only drift fix
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`
- Dodano `SelectorSoftScoreDiagnostics::default()` do lokalnych initializerow `GatekeeperDecision`.
- Cel: odblokowac static guard; brak zmiany produkcyjnego runtime behavior.

## 4. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| R42 TOML parse | oba pliki R42 parsowalne przez `tomllib` | PASS |
| R42 scope isolation | aktywne sciezki wrappera wskazuja R42 scope | PASS |
| Price lower bound wiring | `min_price_change_ratio` sprawdzany obok `max_price_change_ratio` w policy/runtime | PASS |
| Release build | `cargo build --release -p ghost-launcher` w launcherze | PASS |
| Static guard first attempt | blocked przez stare testowe initializery bez `selector_soft_score` | FAIL, FIXED |
| Targeted guard test after fix | `cargo test -q -p ghost-launcher components::trigger::shadow_run::tests::p5_precheck_failure_writes_not_dispatched_lifecycle_record` | PASS |
| Static guard second attempt | `RESTORE_PATH_STATIC_GUARD_PASS` | PASS |
| Preflight second attempt | runtime checks passed, metrics port 9130 free | PASS |
| Runtime tmux | `r42-main-stoploss-le-timestop` aktywny | PASS |
| Runtime logging | `runtime.log`, decisions, shadow/probe logs aktualizuja sie | PASS |

Uwaga:
Druga proba startu doszla do tmux runtime, ale po zerwaniu sesji wrapper nie zostawil top-level `RUN_LIFECYCLE_LAUNCHER_REPORT.json` w katalogu `run_lifecycle_guard_20260620T211602Z`. Runtime dziala niezaleznie w tmux i zapisuje artefakty.

## 5. Aktualny runtime proof po starcie

Stan z kontroli po disconnectcie:
- tmux: `r42-main-stoploss-le-timestop`
- PID runtime: aktywny `/root/Gho/target/release/ghost-launcher --config ...r42...toml`
- runtime log: aktywnie rosnacy
- R41 nadal dziala osobno; nie zostal zatrzymany ani zmieniony.

Snapshot artefaktow:
- `gatekeeper_v2_decisions.jsonl` legacy_live: ponad 1400 decyzji w pierwszym sprawdzeniu po starcie.
- `gatekeeper_v2_buys.jsonl`: 13 BUY.
- `shadow_entries.jsonl`: 13 entries.
- `shadow_lifecycle.jsonl`: pozycje BUY byly juz zamykane z `Target`, `StopLoss` i `TimeStop`.
- `probe_selection.jsonl` / `probe_transport.jsonl` / `probe_shadow_entries.jsonl`: proby byly aktywnie emitowane.

## 6. Ryzyka i zabezpieczenia

Ryzyko 1: brak finalnego raportu launchera dla drugiej proby.
- Obserwacja: static guard i preflight sa zapisane oraz PASS; runtime dziala w tmux.
- Konsekwencja: status R42 nalezy traktowac jako runtime-start-confirmed, ale nie jako launcher-report-confirmed dla top-level final JSON.

Ryzyko 2: R41 i R42 dzialaja rownolegle.
- Zabezpieczenie: R42 ma osobny scope, config paths, log roots, event dataset roots i metrics port `9130`.

Ryzyko 3: lower-bound price change jest aktywna policy change.
- Zabezpieczenie: wykorzystuje istniejacy config field i dziala tylko zgodnie z configiem profilu. Stare configi z domyslnym/luznym `min_price_change_ratio` nie sa celowo zaostrzane przez R42 wrapper.

Ryzyko 4: shadow/live boundary.
- Zabezpieczenie: wrapper zachowuje `entry_mode = "shadow_only"` oraz `execution_mode = "shadow"`; nie zmieniono sendera ani TX buildera.

Ryzyko 5: duzy wolumen logow.
- Obserwacja: R42 szybko generuje duze `runtime.log`, `oracle.log` i `system.log`.
- Konsekwencja: dlugi run wymaga monitorowania miejsca na dysku.

## 7. Decyzja

R42 zostal utworzony i uruchomiony jako osobny shadow-only run:
- scope: `shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1`
- tmux: `r42-main-stoploss-le-timestop`
- brain config: `configs/rollout/ghost_brain_selector_dataset_sampler_r42_main_stoploss_le_timestop_maxwait31100_fsc_off.toml`
- wrapper config: `configs/rollout/shadow-burnin-v3-r42-main-stoploss-le-timestop-target50-stop50-fsc-off-r1.toml`

Run pozostaje aktywny. Kolejna analiza R42 powinna filtrowac po scope R42 i brain config hash `74207c1c090336298ac77ba18488c5f67188411a92837f5297694cb6fd716670`, aby nie mieszac materialu z R41.
