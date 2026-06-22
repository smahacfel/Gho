# ADR-8D: R45 R42 Main MaxWait21100 TimeStop V2 Observe Start

Status: IMPLEMENTED / RUNTIME_START_PENDING_AT_CREATION
Typ: ADR-8D / rollout profile / TimeStop V2 observe-only telemetry
Data: 2026-06-21
Autor/Agent: Codex
Repo/branch: `/root/Gho`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: nowy profil R45 oparty o progi R42, z krotszym oknem obserwacji i TimeStop V2 observe-only
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `configs/rollout/ghost_brain_selector_dataset_sampler_r45_r42_main_maxwait21100_timestop_v2_observe_fsc_off.toml`
- `configs/rollout/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- `docs/ADR/ADR_8D_R45_R42_MAIN_MAXWAIT21100_TIMESTOP_V2_OBSERVE_START_20260621.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D uzyty w ostatnich raportach.

## 1. Przygotowanie i dzialania wstepne

Cel:
Odpalic nowy run R45, ktory zachowuje konserwatywne progi metryk z R42, skraca `max_wait_time_ms` z `31100` do `21100` i zbiera TimeStop V2 tylko telemetrycznie.

Zalozenia:
- R45 jest kopia R42 MAIN threshold profile, nie kopia R44.
- R44 nie byl traktowany jako zrodlo progow metryk, bo byl profilem TimeStop V2 smoke/observe i nie zachowywal wszystkich R42 filtracji.
- TimeStop V2 pozostaje `observe_only` i nie zamyka pozycji.
- FSC pozostaje OFF zgodnie z R42 lineage.

## 2. Opis problemu - 3W2H

What:
Potrzebny jest osobny run z krotszym oknem obserwacji dla selektora, aby sprawdzic, jak TimeStop V2 telemetry zachowuje sie przy szybszym terminalnym oknie decyzyjnym.

Where:
- brain rollout config z Gatekeeper/profile thresholds
- launcher rollout wrapper z log paths, probe paths, metrics ports i scope
- lifecycle JSONL `shadow_lifecycle.jsonl` oraz `probe_shadow_lifecycle.jsonl`

Why:
R44 potwierdzil mechanike TimeStop V2, ale bazowal na konkretnym profilu i dalo sie go interpretowac tylko lokalnie. R45 ma testowac V2 observe-only na konserwatywnym R42-like profilu z krotszym `max_wait_time_ms`.

How:
- Skopiowano brain config R42.
- Zmieniono `max_wait_time_ms = 21100`.
- Dodano `[post_buy_guardian.time_stop_v2]` w trybie `observe_only`.
- Skopiowano wrapper R42.
- Nadano nowy scope R45, nowe log paths i porty `9133` / `8833`.

How many:
Zmiana dotyka tylko nowych plikow rolloutowych i ADR. Nie zmienia kodu runtime, Gatekeeper policy, `MaterializedFeatureSet`, TX buildera, sendera ani live execution.

## 3. Przyczyna zrodlowa

Root cause:
Dotychczasowy material V2 pochodzi z jednego konkretnego profilu i nie powinien byc uogolniany jako dowod strategiczny dla innych configow. Potrzebny jest oddzielny, czysty profil R45.

## 4. Strategia naprawy

Przyjeta strategia:
- Zachowac R42 progi metryk jako SSOT dla tego runa.
- Zmienic tylko okno `max_wait_time_ms` i wlaczyc V2 telemetry.
- Uzyc nowego namespace, aby uniknac mieszania artefaktow R42/R44/R45.
- Zachowac `observe_only`, aby nie zmieniac finalnych close reason.

## 5. Przeprowadzone akcje naprawcze

Zmiana 1: brain config R45
- Nowy plik: `configs/rollout/ghost_brain_selector_dataset_sampler_r45_r42_main_maxwait21100_timestop_v2_observe_fsc_off.toml`.
- `max_wait_time_ms = 21100`.
- Dodano `[post_buy_guardian.time_stop_v2]` z:
  - `enabled = true`
  - `mode = "observe_only"`
  - `first_check_ms = 3000`
  - `window_ms = 4000`
  - `failed_windows_to_signal = 3`
  - `min_age_before_signal_ms = 11000`

Zmiana 2: wrapper R45
- Nowy plik: `configs/rollout/shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`.
- Nowy scope i osobne artefakty:
  - `shadow-burnin-v3-r45-r42-main-maxwait21100-timestop-v2-observe-target50-stop50-fsc-off-r1`
- Porty:
  - metrics `9133`
  - gui backend `8833`

## 6. Walidacja

Walidacja wymagana przed uznaniem runa:
- TOML parse dla brain i wrapper.
- stale-name scan dla R45 paths.
- launcher static guard.
- launcher preflight.
- event canary.
- runtime proof, ze `time_stop_v2_window` pojawia sie w clean delta albo lifecycle validator.

## 7. Ryzyka i zabezpieczenia

Ryzyko 1: overfit do R42/R45.
- Zabezpieczenie: R45 jest tylko telemetryczna walidacja. Nie uogolniac progow bez kolejnych profili/runow.

Ryzyko 2: pomieszanie artefaktow.
- Zabezpieczenie: nowy namespace R45 i nowe sciezki logow.

Ryzyko 3: przypadkowe aktywne zamykanie przez V2.
- Zabezpieczenie: `mode = "observe_only"`.

Ryzyko 4: port conflict.
- Zabezpieczenie: R45 uzywa portow `9133` i `8833`.

## 8. Decyzja

R45 zostaje przygotowany jako konserwatywny R42-derived profile z krotszym `max_wait_time_ms = 21100` i TimeStop V2 observe-only telemetry. Run nalezy odpalic launcherem lifecycle, a statystyki interpretowac tylko w ramach tego profilu.
