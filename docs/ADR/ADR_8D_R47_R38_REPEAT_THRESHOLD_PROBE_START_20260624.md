# ADR-8D: R47 R38 Repeat Threshold Probe Start

Status: IMPLEMENTED / RUNTIME_START_PENDING_AT_CREATION
Typ: ADR-8D / rollout profile / shadow-only repeat run
Data: 2026-06-24
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy wrapper R47 bazujacy na wskazanym R38 brain configu, start przez selector lifecycle launcher
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R47_R38_REPEAT_THRESHOLD_PROBE_START_20260624.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Utworzyc R47 jako swieza powtorke R38 threshold-probe runu, z osobnym namespace i artefaktami, a nastepnie uruchomic go zgodnie z `docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md`.

Zalozenia:
- R47 ma bazowac na `/root/Gho/configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`.
- Brain config R38 nie jest kopiowany ani modyfikowany.
- R47 zmienia tylko wrapper/runtime namespace, sciezki artefaktow, run/session id oraz porty.
- Run pozostaje `execution_mode = "shadow"` i `entry_mode = "shadow_only"`.
- Nie zmieniamy Gatekeeper policy, `MaterializedFeatureSet`, TX buildera, sendera ani live execution.

## 2. Opis problemu - 3W2H

What:
Potrzebna jest powtorka R38 z nowym scope R47, aby zebrac swieze artefakty po cleanupie dysku.

Where:
- wrapper rollout w `configs/rollout/`
- runtime artefakty w `logs/shadow_run/`, `logs/rollout/`, `datasets/events/`, `data/rollout/`
- launcher proof w `reports/selector/`

Why:
R38 lifecycle output wygladal kompletniej jako active shadow lifecycle baseline. R47 ma powtorzyc te same brain semantics w nowym, izolowanym runie.

How:
- Utworzono nowy wrapper `shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml`.
- `ghost_brain_config_path` wskazuje na oryginalny R38 brain config.
- Wszystkie artefakty wrappera zawieraja R47 scope.
- Port metrics ustawiono na `9135`, GUI backend na `8835`.
- Start nalezy wykonac przez `scripts/start_selector_lifecycle_run.py`.

How many:
Zmiana dodaje tylko nowy wrapper i ADR. Nie zmienia kodu runtime ani brain configu.

## 3. Przyczyna zrodlowa

Root cause:
Potrzeba nowego runu discovery wymaga swiezej przestrzeni artefaktow, ale bez zmiany sprawdzonej semantyki R38. Kopiowanie albo tunowanie brain configu nie jest potrzebne do powtorki.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac R38 brain config jako zrodlo progow.
- Izolowac R47 przez nowy wrapper scope.
- Zachowac shadow-only boundary.
- Uruchomic przez selector lifecycle launcher, nie przez reczny tmux.
- Po starcie sprawdzic tylko liveness i zostawic run.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: wrapper R47
- Nowy plik: `configs/rollout/shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1.toml`.
- Scope:
  - `shadow-burnin-v3-r47-r38-repeat-threshold-probe-target50-stop50-fsc-off-r1`
- Brain config:
  - `../../configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`
- Porty:
  - metrics `9135`
  - gui backend `8835`

Zmiana 2: ADR-8D
- Nowy plik: `docs/ADR/ADR_8D_R47_R38_REPEAT_THRESHOLD_PROBE_START_20260624.md`.

## 6. Walidacja

Walidacja wymagana przed startem:
- TOML parse wrappera.
- stale-name scan dla sciezek R47.
- potwierdzenie wolnych portow `9135` / `8835`.
- `git diff --check` dla nowych plikow.
- start przez `scripts/start_selector_lifecycle_run.py`.
- po starcie tylko liveness check: `tmux has-session`, szybki log/process check.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: przypadkowa zmiana semantyki R38.
- Zabezpieczenie: wrapper wskazuje na oryginalny R38 brain config; nie tworzono kopii brain configu.

Ryzyko 2: pomieszanie artefaktow z R38/R46.
- Zabezpieczenie: wszystkie runtime paths zawieraja R47 scope.

Ryzyko 3: manualny start bez lifecycle launcher proof.
- Zabezpieczenie: runbook wymaga `scripts/start_selector_lifecycle_run.py`; manualny tmux start jest odrzucony.

Ryzyko 4: shadow/live boundary drift.
- Zabezpieczenie: wrapper utrzymuje `execution_mode = "shadow"` i `entry_mode = "shadow_only"`.

## 8. Decyzja

R47 zostaje utworzony jako swieza powtorka R38 threshold-probe runu z nowym namespace i artefaktami. Start ma byc wykonany launcherem lifecycle; po potwierdzeniu, ze run zyje, nalezy pozostawic go w tle bez dalszego audytu.
