# ADR-8D: R48 R38 Target24 Stop3 Rollout Profile

Status: IMPLEMENTED / RUNTIME_START_PENDING_AT_CREATION
Typ: ADR-8D / rollout profile / shadow-only lifecycle threshold experiment
Data: 2026-06-25
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy wrapper R48 bazujacy na R47/R38 31.1s profile, ze zmienionymi progami shadow lifecycle Target/StopLoss
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R48_R38_TARGET24_STOP3_ROLLOUT_PROFILE_20260625.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Utworzyc izolowany profil R48 na bazie wrappera R47/R38 i uruchomic nowy shadow-only run z tym samym R38 brain configiem, ale z ostrzejszymi progami post-buy lifecycle:

- Target: `+24%`
- StopLoss: `-3%`

Zalozenia:
- R48 ma bazowac na wrapperze `configs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml`.
- R48 nadal wskazuje na `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`.
- Nie kopiujemy ani nie zmieniamy brain configu.
- Nie zmieniamy Gatekeeper policy, selector thresholds, `MaterializedFeatureSet`, TX buildera, sendera ani live execution.
- Run pozostaje `execution_mode = "shadow"` i `entry_mode = "shadow_only"`.

## 2. Opis problemu - 3W2H

What:
Potrzebny jest nowy R48 run porownawczy, ktory utrzyma ten sam 31.1s broad sampler co R47, ale zmieni etykietowanie/close behavior shadow lifecycle z `+50%/-50%` na `+24%/-3%`.

Where:
- wrapper rollout w `configs/rollout/`
- runtime artefakty w `logs/shadow_run/`, `logs/rollout/`, `datasets/events/`, `data/rollout/`
- launcher proof w `reports/selector/`

Why:
Analiza R47 pokazala, ze duza czesc pozycji konczy sie jako ujemne TimeStop albo przegrzane StopLossy. R48 ma sprawdzic shadow lifecycle przy bardziej konserwatywnym loss-cut i nizszym target capture, bez ruszania decyzji BUY ani feature policy.

How:
- Utworzono nowy wrapper `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1.toml`.
- Zmieniono tylko namespace/run/session paths oraz launcher lifecycle thresholds:
  - `live_exit_take_profit_pct = 0.24`
  - `live_exit_stop_loss_pct = 0.03`
- `ghost_brain_config_path` nadal wskazuje na oryginalny R38 brain config.

How many:
Zmiana dodaje tylko nowy wrapper i ADR. Nie zmienia kodu runtime ani brain configu.

## 3. Przyczyna zrodlowa

Root cause:
R47 byl dataset samplerem z post-buy lifecycle `+50%/-50%`, co dobrze nadaje sie do szerokiej obserwacji, ale nie odpowiada aktualnej hipotezie o koniecznosci szybkiego ucinania strat i nizszego target capture.

Skutek:
Bez nowego wrappera nie da sie odpalic izolowanego R48 runu z innymi progami Target/StopLoss bez mieszania artefaktow z R47.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac R38 brain config jako zrodlo Gatekeeper/selector semantics.
- Izolowac R48 przez nowy wrapper scope.
- Zmienic tylko post-buy shadow lifecycle thresholds w launcherowym wrapperze.
- Zachowac shadow-only boundary i uruchomic przez selector lifecycle launcher.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: wrapper R48
- Nowy plik: `configs/rollout/shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1.toml`.
- Scope:
  - `shadow-burnin-v3-r48-r38-repeat-threshold-probe-target24-stop3-fsc-off-r1`
- Brain config:
  - `../../configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Lifecycle thresholds:
  - `live_exit_take_profit_pct = 0.24`
  - `live_exit_stop_loss_pct = 0.03`
- Porty pozostaja zgodne z R47:
  - metrics `9135`
  - gui backend `8835`

Zmiana 2: ADR-8D
- Nowy plik: `docs/ADR/ADR_8D_R48_R38_TARGET24_STOP3_ROLLOUT_PROFILE_20260625.md`.

## 6. Walidacja

Walidacja wymagana przed startem:
- TOML parse wrappera.
- stale-name scan dla sciezek R48.
- potwierdzenie, ze progi to `0.24` i `0.03`.
- `git diff --check` dla nowych plikow.
- start przez `scripts/start_selector_lifecycle_run.py`.
- po starcie liveness check: `tmux has-session`, szybki log/process check.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: przypadkowa zmiana semantyki Gatekeeper/selector.
- Zabezpieczenie: wrapper wskazuje na oryginalny R38 brain config; nie tworzono ani nie modyfikowano brain configu.

Ryzyko 2: pomieszanie artefaktow z R47.
- Zabezpieczenie: wszystkie runtime paths zawieraja R48 scope `target24-stop3`.

Ryzyko 3: bledna interpretacja StopLoss znaku.
- Zabezpieczenie: launcher fields `live_exit_take_profit_pct` i `live_exit_stop_loss_pct` sa fraction magnitude. `0.03` oznacza `-3%` stop loss threshold w runtime lifecycle.

Ryzyko 4: shadow/live boundary drift.
- Zabezpieczenie: wrapper utrzymuje `execution_mode = "shadow"` i `entry_mode = "shadow_only"`. Nie wlacza live execution.

Ryzyko 5: runtime lifecycle proof moze potrwac dluzej albo nie przejsc.
- Zabezpieczenie: start ma byc wykonany lifecycle launcherem, ktory zapisuje raport PASS/FAIL i w razie failure zabija sesje tmux.

## 8. Decyzja

R48 zostaje utworzony jako izolowana powtorka R47/R38 31.1s sampler z nowymi shadow lifecycle progami `+24%/-3%`. Jest to eksperyment shadow-only dotyczacy close/outcome surface, a nie zmiana runtime BUY policy.
