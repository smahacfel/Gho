# ADR-8D: R43 Smoke R41-Loose Profile Start

Status: IMPLEMENTED / RUNTIME_LEFT_RUNNING
Typ: ADR-8D / rollout smoke start, shadow-only runtime evidence
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy R43 smoke scope/profile skopiowany z R41, bez zmiany progow decyzyjnych
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r43_smoke_r41_loose_maxwait31100_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R43_SMOKE_R41_LOOSE_PROFILE_START_20260621.md`

Runtime artifacts:
- `reports/selector/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T133251Z/`
- `reports/selector/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T133251Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`
- `reports/selector/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T133251Z/runtime.log`
- `logs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/`
- `logs/shadow_run/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/`
- `datasets/events/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `/Gho/docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w poprzednich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Uruchomic izolowany R43 smoke run na profilu zblizonym do R41 po rekompilacji projektu, aby sprawdzic runtime start, event ingest, shadow-only path oraz zapis nowych jawnych pol timeline w `shadow_lifecycle.jsonl`.

Profil:
- bazowy brain profile: R41 score12 timeout-median filters
- `selector_soft_score.min_buy_score = 12`
- `max_wait_time_ms = 31100`
- `min_tx_count = 22`
- `min_buy_count = 12`
- `min_total_volume_sol = 18.0`
- `min_bonding_progress_pct = 38.0`
- `min_market_cap_sol = 30.0`
- `post_buy_guardian.target_threshold = 50.0`
- `post_buy_guardian.stoploss_threshold = 50.0`
- `post_buy_guardian.wait_for_timestop = 30000`
- FSC v2 pozostaje disabled

## 2. Opis problemu - 3W2H

What:
Potrzebny byl szybki R43 smoke po zabiciu poprzedniego procesu i rekompilacji, z zachowaniem rozdzielenia artefaktow od R41/R42.

Where:
- Brain profile:
  - `configs/rollout/ghost_brain_selector_dataset_sampler_r43_smoke_r41_loose_maxwait31100_fsc_off.toml`
- Rollout wrapper/scope:
  - `configs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1.toml`
  - `shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1`

Why:
R43 mial potwierdzic, ze po rekompilacji runtime startuje, przyjmuje stream events, zostaje shadow-only i zapisuje audytowalne lifecycle timeline fields w nowych terminalnych rows.

How:
Skopiowano R41 wrapper i brain profile do R43, wykonano tylko mechaniczne podmiany scope/path/session/port. Nie zmieniano progow Gatekeepera ani progow post-buy wzgledem R41. Uruchomiono `scripts/start_selector_lifecycle_run.py` z event canary 120s, `--allow-zero-buy-lifecycle-proof`, `--skip-static-tests`, `--min-free-gb 20` oraz `--runtime-timeout-seconds 7200`.

How many:
Zmiana obejmuje dwa nowe pliki config i jeden ADR. Nie zmieniano kodu Rust, TX buildera, sendera, Gatekeeper policy, DecisionLogger schema ani shadow/live boundary.

## 3. Przeprowadzone akcje

Zmiana 1: nowy brain profile R43
- Bazowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r41_score12_median_timeout_filters_maxwait31100_fsc_off.toml`
- Nowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r43_smoke_r41_loose_maxwait31100_fsc_off.toml`
- Brain config hash z runtime: `ec1637066abd1bf98a3b35344a272424ac84acae71ce32cdef0e8285821b9d70`

Zmiana 2: nowy rollout wrapper/scope R43
- Bazowy plik: `configs/rollout/shadow-burnin-v3-r41-score12-timeout-median-target50-stop50-fsc-off-r1.toml`
- Nowy plik: `configs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1.toml`
- Metrics port: `9131`
- GUI backend port: `8831`
- `entry_mode = "shadow_only"` i `execution_mode = "shadow"` pozostaly bez zmian.

Zmiana 3: launcher smoke
- Scope: `shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1`
- Tmux session: `r43-smoke-r41-loose`
- Runtime cap: `7200s`
- Launcher report: `reports/selector/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T133251Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json`

## 4. Walidacja

| Walidacja | Wynik | Status |
|---|---|---|
| R43 TOML parse | oba pliki R43 parsowalne przez `tomllib` | PASS |
| R43 scope isolation | aktywne sciezki wrappera wskazuja R43 scope | PASS |
| Shadow/live boundary | `entry_mode = shadow_only`, `execution_mode = shadow` | PASS |
| Metrics port | `0.0.0.0:9131` wolny w preflight | PASS |
| Static guard | `guard_restore_shadow_lifecycle.py --skip-tests --skip-runtime` | PASS |
| Preflight | `cargo run -p ghost-launcher --bin ghost-launcher -- --config ... --preflight` | PASS |
| Event canary | `Candidate=16`, `NewPoolDetected=21`, `PoolTransaction=575`, `diag_account_update_relay_delta=1207` | PASS |
| Launcher result | `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED` | PASS |
| Runtime tmux | `r43-smoke-r41-loose` aktywny | PASS |
| Shadow lifecycle terminal rows | po starcie pojawily sie `exit_filled` i `position_closed` | PASS |
| Timeline fields in terminal rows | non-null `entry_simulation_rpc_slot`, `entry_market_anchor_slot`, `entry_landed_slot`, `exit_sample_slot`, `exit_market_anchor_slot`, `exit_reason_evaluation_ts_ms`, `exit_landed_slot` | PASS |

## 5. Aktualny runtime proof po starcie

Stan z kontroli po event canary:
- tmux: `r43-smoke-r41-loose`
- runtime: `/root/Gho/target/release/ghost-launcher --config /root/Gho/configs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1.toml`
- runtime timeout wrapper: `timeout 7200s`
- launcher status: `PASS`
- launcher claim: `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- launcher run_state: `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`

First smoke lifecycle snapshot:
- `shadow_entries.jsonl`: 3 rows
- `shadow_lifecycle.jsonl`: 7 rows
- `shadow_lifecycle.record_type`: `shadow_dispatch=3`, `exit_filled=2`, `position_closed=2`
- terminal close reasons observed: `StopLoss=1`, `TimeStop=1`
- `probe_shadow_entries.jsonl` / `probe_shadow_lifecycle.jsonl`: not present yet at snapshot time

## 6. Ryzyka i zabezpieczenia

Ryzyko 1: miejsce na dysku.
- Obserwacja: launcher wystartowal z `free_gb=23.53` i `--min-free-gb 20`, po build/preflight `df` pokazywal okolo `17G` wolnego.
- Zabezpieczenie: R43 smoke uruchomiono z runtime cap `7200s`; przy dluzszym runie trzeba monitorowac miejsce.

Ryzyko 2: zero-BUY allowance w launcherze.
- Obserwacja: formalny PASS launchera jest event-ingest proof, nie klasyczny lifecycle proof.
- Dodatkowy runtime proof: po czasie pojawily sie juz BUY/shadow lifecycle terminal rows z nowymi timeline fields.

Ryzyko 3: R43 bazuje na R41.
- Konsekwencja: to smoke profilu R41-like, nie nowa rekomendacja selekcyjna i nie claim o edge.

Ryzyko 4: shadow/live boundary.
- Zabezpieczenie: wrapper zachowuje shadow-only ustawienia, nie zmieniono sendera ani TX buildera.

## 7. Decyzja

R43 smoke zostal utworzony i uruchomiony jako osobny shadow-only run:
- scope: `shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1`
- tmux: `r43-smoke-r41-loose`
- brain config: `configs/rollout/ghost_brain_selector_dataset_sampler_r43_smoke_r41_loose_maxwait31100_fsc_off.toml`
- wrapper config: `configs/rollout/shadow-burnin-v3-r43-smoke-r41-loose-target50-stop50-fsc-off-r1.toml`

Run pozostaje aktywny i ma runtime cap 7200s od startu tmux. Kolejne analizy powinny filtrowac po scope R43 i brain config hash `ec1637066abd1bf98a3b35344a272424ac84acae71ce32cdef0e8285821b9d70`, aby nie mieszac materialu z R41/R42.
