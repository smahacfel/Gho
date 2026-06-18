# ADR-8D: Przygotowanie i uruchomienie profilu R32 (shadow, FSC OFF, maxwait 15000ms)

Status: DRAFT
Typ: Operacyjne
Data: 2026-06-16
Repo/branch: codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: nie dotyczy (praca operacyjna)
Zakres: Konfiguracje rollout + uruchomienie runa
Dotknięte moduły/pliki:
- `configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml` (utworzony na podstawie R31)
- `docs/ADR/ADR_8D_R32_PREP_AND_LAUNCH_20260616.md` (ten wpis)
Powiązane runy/logi/raporty: `R32` (scope `shadow-burnin-v3-r32-maxwait15000-fsc-off-r1`)
Poziom ryzyka: Niski

## 1. Przygotowanie i działania wstępne

Plan początkowy:
- Przygotować nowy profil R32 na bazie R31 bez modyfikacji strategii Gatekeepera.
- Uruchomić runa przez `scripts/start_selector_lifecycle_run.py` (proceduralnie) w `tmux` z nazwą sesji `gho-r32`.

Rzeczywisty przebieg:
- Skopiowano konfigurację R31 do nowej ścieżki:
  - `configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml`
- Zmieniono identyfikatory runa/scope z `r31-maxwait15000-fsc-off-r4` na `r32-maxwait15000-fsc-off-r1`.
- Pozostawiono ten sam `ghost_brain_config_path`:
  - `../../configs/rollout/ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml`
- Uruchomiono runa komendą:
  - `python3 scripts/start_selector_lifecycle_run.py --scope shadow-burnin-v3-r32-maxwait15000-fsc-off-r1 --config configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml --tmux-session gho-r32 --min-free-gb 35 --event-canary-seconds 900 --lifecycle-proof-timeout-seconds 3600 --min-reporter-rows 1`
- W ramach cyklu bezpieczeństwa startu uruchomiono:
  - static guard
  - preflight
  - uruchomienie runtime w `tmux`
- W tym momencie proces startowy oczekuje końca fazy `event_canary` (15 min), więc raport uruchomieniowy jeszcze nie został zamknięty jako PASS.

Odchylenia od planu:
- Żadne merytoryczne.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
- Brak dedykowanej aktywnej sub-agentowej interwencji merytorycznej (zmiana operacyjna oparta o istniejącą procedurę startu).
Powód użycia:
- Utrzymanie zgodności z dotychczasowym procesem lifecycle i ograniczenie ryzyka zmian runtime.
Zakres użycia:
- Jedynie pliki konfiguracyjne uruchomienia i uruchomienie procesu.
Wynik:
- Run uruchomiony w tle, brak zmian w kodzie produkcyjnym.
Ograniczenia:
- Raport launcher jest jeszcze w trakcie fazy canary i nie zakończony finalnie.

## 3. Opis problemu — 3W2H

What:
- Potrzebne było szybkie przygotowanie nowego profilu R32 na bazie R31 i uruchomienie go zgodnie z procedurą lifecycle.
Where:
- W repozytorium `/root/Gho`, konfiguracje rollout i tmux session.
Why it matters:
- Zapewnienie ciągłego pokrycia eksperymentu shadow bez wpływu na decyzje Gatekeeper (brak zmian strategii).
How observed:
- Sprawdzenie działania polega na obecności aktywnej sesji `tmux gho-r32`, katalogu artefaktów runa i logach startupowych.
How many / scale:
- Jeden run R32 uruchomiony jako kontynuacja dotychczasowych ustawień R31.
Evidence:
- `tmux ls` zwraca `gho-r32`
- logi startu zawierają `CONFIG` i `Gatekeeper V2 config loaded ...` dla `r31`-shared brain.

## 4. Przyczyna źródłowa

Root cause:
- brak
Mechanizm błędu:
- brak
Miejsce:
- brak
Skutek:
- brak
Dowód:
- brak
Odrzucone hipotezy:
- Nie zmieniano ustawień Gatekeepera ani ścieżek `ghost_brain`; tylko nowy wrapper R32.

## 5. Strategia naprawy

Przyjęta strategia:
- Replikacja działającego profilu R31 i zmiana wyłącznie identyfikatorów runa/scope.
Zakres ingerencji:
- Jeden plik konfiguracyjny runa.
Zakresu nie zmieniano:
- `ghost_brain_selector_dataset_sampler_r31...toml`
- logika runtime, transporty, strategie Gatekeeper i progi decyzyjne.
Ryzyka:
- R32 nie przejdzie jeszcze do finalnego stanu `PASS` w raportach do czasu zakończenia `event_canary`.
Odrzucone alternatywy:
- Ręczne odpalenie przez `tmux` bez lifecycle guard (odrzucone z uwagi na procedurę repo).

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `configs/rollout/shadow-burnin-v3-r32-maxwait15000-fsc-off.toml`
- Co zmieniono: skopiowanie `r31` i podmiana suffixu/identyfikatorów na R32.
- Dlaczego: zachowanie tożsamości parametrów i odseparowanie artefaktów.
- Efekt: nowe artefakty R32 będą zapisywane do dedykowanych ścieżek.

Zmiana 2:
- Plik/moduł: `scripts/start_selector_lifecycle_run.py` (niezmieniany)
- Co zmieniono: nie.
- Dlaczego: uruchomienie runa wyłącznie przez istniejącą ścieżkę procedury.
- Efekt: run uruchomiony pod lifecycle kontrolą; `tmux gho-r32` aktywny.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Kontrola istnienia sesji tmux | `tmux ls` | aktywna sesja `gho-r32` | PASS | lista sesji tmux |
| Uruchomienie lifecycle runa | `python3 scripts/start_selector_lifecycle_run.py --scope ...` | proces aktywny, etap preflight i start runtime wykonany | PASS (in progress) | katalog `reports/selector/shadow-burnin-v3-r32-maxwait15000-fsc-off-r1/run_lifecycle_guard_20260616T203749Z` |
| Sprawdzenie ścieżek runa | `logs`/`datasets`/`reports` zgodne ze scope `r32...-r1` | PASS | aktywne ścieżki w configu i logach |

Wniosek walidacyjny:
- R32 wystartował zgodnie z procedurą i pracuje w tle; finalna walidacja lifecycle (event/lifecycle canary) zależy od dalszego cyklu oczekiwania.

Ograniczenia walidacji:
- Nie dotarłem do finalnego `RUN_LIFECYCLE_LAUNCHER_REPORT.json` (w trakcie fazy canary).

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Rozdzielenie artefaktów przez oddzielny scope
- Co zabezpiecza: brak kolizji plików z R31 przy jednoczesnych uruchomieniach.
- Kiedy się aktywuje: od samego startu runa.
- Jak przetestowano: plik konfiguracyjny zawiera `...-r32...`.
- Co pozostaje poza zakresem: optymalizacja metryk jakości runa.

Guardrail 2:
- Typ: Lifecycle launcher
- Co zabezpiecza: brak ręcznych odpalonych runów omijających preflight.
- Kiedy się aktywuje: każde przyszłe uruchomienie runa w tym workflow.
- Jak przetestowano: uruchomienie odbyło się przez `start_selector_lifecycle_run.py`.
- Co pozostaje poza zakresem: wynik końcowy canary.

## Otwarte ryzyka / follow-up

- Po 15 minutach od startu skrypt powinien zakończyć kanarkę eventową i przejść do `lifecycle` (albo zamknąć run przy fail).
- Po uzyskaniu finalnego reportu można przygotować raport statusowy dla R32 (coverage, 429 itp.) jeśli będzie taka potrzeba.
