# ADR-8D: R46 Temporal Discovery TimeStop V2 Long Run

Status: IMPLEMENTED / RUNTIME_START_PENDING_AT_CREATION
Typ: ADR-8D / rollout profile / long shadow telemetry run
Data: 2026-06-22
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: cleanup R45, reports selector cleanup, nowy profil R46 do zbierania danych z TimeStop V2 observe-only
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r46_temporal_discovery_maxwait42000_timestop_v2_observe_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R46_TEMPORAL_DISCOVERY_TIMESTOP_V2_LONG_RUN_20260622.md`
- runtime cleanup artefaktow w `reports/selector/`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Zatrzymac R45, oczyscic `reports/selector/` do danych R44/R45, przygotowac R46 jako dlugi run zbierajacy dane dla podstawowych delt, PnL checkpointow oraz TimeStop V2 telemetry.

Zalozenia:
- R46 jest profilem shadow/telemetrycznym, nie produkcyjnym profilem precyzji.
- Progi bazowe pochodza z `configs/rollout/ghost_brain_selector_dataset_sampler_r38_threshold_probe_maxwait31100_fsc_off.toml`.
- TimeStop V2 pozostaje `observe_only`; nie zmienia terminalnego close reason.
- `selector_soft_score` ma rejestrowac punktacje, ale nie blokowac decyzji.
- FSC pozostaje OFF jak w zrodle R38.

## 2. Opis problemu - 3W2H

What:
Potrzebny jest dluzszy, bardziej zbierajacy profil R46, ktory nie tnie populacji przez selector soft score, ale zachowuje jawna punktacje 12 warunkow i emituje TimeStop V2 windows.

Where:
- brain rollout config z progami Gatekeepera, selector score i post-buy guardian
- wrapper rollout z namespace, portami, logami i reportami
- `reports/selector/` jako katalog wynikow selektora

Why:
Dotychczasowe runy byly zbyt waskie albo zbyt lokalne dla oceny separowalnosci tokenow po temporalnych deltach i checkpointach PnL. R46 ma zbierac szerszy dataset pod analize, zamiast probowac od razu maksymalizowac Target/StopLoss/TimeStop.

How:
- R45 zostaje ubity przed startem R46.
- `reports/selector/` zostaje oczyszczony z danych innych niz R44/R45.
- Brain config R46 zostaje utworzony z R38 threshold-probe maxwait31100 FSC OFF.
- `max_wait_time_ms` zostaje zmienione z `31100` na `42000`.
- Phase 1 minima zostaja zmienione na:
  - `min_tx_count = 12`
  - `min_unique_signers = 9`
  - `min_buy_count = 8`
- `selector_soft_score` zostaje ustawiony jako `policy = "log_only"` z progami 1/12, aby punktacja byla rejestrowana bez gatingu.
- TimeStop V2 zostaje wlaczony w trybie `observe_only`.

How many:
Zmiana dotyka tylko nowych plikow rolloutowych, cleanupu artefaktow i ADR. Nie zmienia kodu runtime, `MaterializedFeatureSet`, Gatekeeper policy, TX buildera, sendera ani live execution.

## 3. Przyczyna zrodlowa

Root cause:
Do analizy separowalnosci dobrych i zlych tokenow potrzeba danych z dluzszego i mniej selektywnego telemetrycznego profilu. Wczesniejsze runy byly uzyteczne diagnostycznie, ale nie dawaly wystarczajacej populacji do badania trendow w deltach, PnL checkpointach i TimeStop V2 windows.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac shadow-only / telemetry-only charakter.
- Nie zmieniac kodu ani aktywnych kontraktow SSOT.
- Ograniczyc cleanup do `reports/selector/`, zgodnie z dyspozycja.
- Wykorzystac R38 jako zrodlo progow bazowych, z trzema wskazanymi korektami Phase 1 i dluzszym `max_wait_time_ms`.
- Ustawic selector score jako log-only, bo celem jest rejestracja liczby spelnionych warunkow, a nie blokowanie decyzji.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: cleanup procesu R45
- R45 zostal zatrzymany przed przygotowaniem R46.

Zmiana 2: cleanup `reports/selector/`
- Usunieto wpisy inne niz R44/R45.
- Po starcie R46 katalog moze zawierac R46 jako nowy, aktualny artefakt.

Zmiana 3: brain config R46
- Nowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r46_temporal_discovery_maxwait42000_timestop_v2_observe_fsc_off.toml`.
- `max_wait_time_ms = 42000`.
- `min_tx_count = 12`.
- `min_unique_signers = 9`.
- `min_buy_count = 8`.
- `selector_soft_score.policy = "log_only"`.
- `selector_soft_score.min_candidate_score = 1`.
- `selector_soft_score.min_buy_score = 1`.
- `post_buy_guardian.time_stop_v2.mode = "observe_only"`.

Zmiana 4: wrapper R46
- Nowy plik: `configs/rollout/shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`.
- Nowy scope:
  - `shadow-burnin-v3-r46-temporal-discovery-maxwait42000-timestop-v2-observe-target50-stop50-fsc-off-r1`
- Porty:
  - metrics `9134`
  - gui backend `8834`

## 6. Walidacja

Walidacja wymagana przed uznaniem startu:
- TOML parse dla brain i wrapper.
- stale-name scan dla R46 paths.
- `git diff --check`.
- sprawdzenie portow `9134` / `8834`.
- launcher start bez runtime timeoutu.
- potwierdzenie tmux session i procesu runtime.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: selector score mylnie potraktowany jako gate.
- Zabezpieczenie: `policy = "log_only"` i komentarz wrappera jawnie opisuje, ze score rejestruje punktacje bez blokowania BUY eligibility.

Ryzyko 2: pomieszanie artefaktow.
- Zabezpieczenie: osobny namespace R46 i cleanup `reports/selector/` przed startem.

Ryzyko 3: TimeStop V2 zmienia wynik runa.
- Zabezpieczenie: `mode = "observe_only"`.

Ryzyko 4: za szeroka interpretacja danych.
- Zabezpieczenie: R46 jest runem discovery/dataset, nie dowodem produkcyjnego edge.

## 8. Decyzja

R46 zostaje przygotowany jako shadow-only temporal-discovery long run z `max_wait_time_ms = 42000`, Phase 1 minima `12/9/8`, selector soft score w trybie log-only 1/12 oraz TimeStop V2 observe-only. Po walidacji profil zostaje uruchomiony jako dlugi run zbierajacy dane.
