# ADR-8D: R31 restore NLN Program Streams route-evidence coverage

Status: wykonane / runtime active
Typ: rollout config repair
Data: 2026-06-15
Repo/branch: `/root/Gho`, `codex/gatekeeper-edge-policy-redesign-r1`
Commit/PR: brak
Zakres: R31 runtime profile only
Dotknięte moduły/pliki:
- `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- `docs/ADR/ADR_8D_R31_RESTORE_NLN_PROGRAM_STREAMS_COVERAGE_20260615.md`
Powiązane runy/logi/raporty:
- `reports/selector/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/run_repair_20260615T155458Z/runtime.log`
- `logs/shadow_run/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4/`
Poziom ryzyka: medium

## 1. Przygotowanie i działania wstępne

Plan początkowy:
Przywrocic R31 NLN Program Streams bez reaktywowania FSC decision/capture i uruchomic czysty pomiar coverage.

Rzeczywisty przebieg:
Zatrzymano wadliwy `gho-r31` oparty o namespace `shadow-burnin-v3-r31-maxwait15000-fsc-off-r3`, poprawiono runtime config, uruchomiono nowy tmux `gho-r31` na tym samym configu R31 z czystym namespace `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`.

Odchylenia od planu:
Uzyto `r4`, poniewaz `r3` mial juz skazone liczniki po okresie z `seer.program_streams.enabled=false`. Bez czystego namespace globalny wskaznik `active_entries/active_buys` mieszalby stan przed i po naprawie.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powod uzycia:
Interpretacja i naprawa shadow-only runtime/config path: Seer, NLN Program Streams, FSC off, shadow evidence coverage.
Zakres uzycia:
Utrzymanie rozdzialu shadow/live, FSC decision off, route-evidence streams on.
Wynik:
Naprawa ograniczona do R31 rollout config.
Ograniczenia:
Nie prowadzono pelnego audytu kodu shadow handoff/simulation success; zweryfikowano tylko krytyczny blad coverage `active_entries/active_buys`.

## 3. Opis problemu - 3W2H

What:
R31 mial wylaczone `seer.program_streams.enabled`, co usunelo dwa NLN Program Streams uzywane jako route/account evidence lane.

Where:
`configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`, sekcja `[seer.program_streams]`.

Why it matters:
Po wylaczeniu Program Streams coverage aktywnej sciezki BUY do shadow entries spadl do poziomu `199/602 = 33.1%` w poprzednim pomiarze `r3`.

How observed:
User zwrocil uwage na regresje coverage i na to, ze dwa NLN Program Streams nie sa tym samym co FSC raw gRPC capture.

How many / scale:
Wadliwy pomiar `r3`: `active_entries/active_buys = 199/602 = 33.1%`.
Po naprawie `r4` po okolo minucie: `active_entries/active_buys = 35/35 = 100.0%`.

Evidence:
- `runtime.log`: start dwoch topics `solana.pump_fun.buy`, `solana.pump_fun.buy_exact_sol_in`
- `runtime.log`: first message received dla obu topics
- `logs/shadow_run/...-r4`: `shadow_entries.jsonl` i `...-buys.jsonl`

## 4. Przyczyna zrodlowa

Root cause:
Zbyt szerokie potraktowanie `seer.program_streams` jako czesci FSC disable.

Mechanizm bledu:
`fsc_v2.capture_enabled=false`, `fsc_v2.decision_enabled=false` i `fsc_v2.hard_reject_enabled=false` wylaczaja FSC policy/capture, ale `seer.program_streams.enabled=false` wylacza rowniez route-evidence streams potrzebne do materializacji/coverage shadow execution evidence.

Miejsce:
`[seer.program_streams] enabled = false` w R31 `r3`.

Skutek:
Duza czesc BUY-side rows nie dostawala odpowiadajacego aktywnego `shadow_entries` record.

Dowod:
Po przywroceniu `program_streams.enabled=true` i czystym `r4`, coverage `active_entries/active_buys` wrocil do `100.0%` na pierwszej probce runtime.

Odrzucone hipotezy:
- Gatekeeper thresholds: nie zmieniano progow.
- FSC hard reject: pozostaje wylaczone.
- Live execution: pozostaje shadow-only.

## 5. Strategia naprawy

Przyjeta strategia:
Przywrocic dokladnie dwa NLN Program Streams, zachowac FSC disabled, nie wlaczac durable raw artifact writer.

Zakres ingerencji:
Tylko R31 runtime config i dokumentacyjny ADR.

Czego nie zmieniano:
- `ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml`
- progi Gatekeeper
- `fsc_v2.*`
- shadow/live mode
- RPC endpoint
- binarium

Ryzyka:
Kod loguje nazwe "FSC capture lane" dla Program Streams lane, mimo ze w tym profilu pracuje ona jako route-evidence lane bez artifact writer. Nazwa logu jest mylaca operacyjnie.

Odrzucone alternatywy:
- Nie wlaczono `funding_lane_mode=full_chain`.
- Nie wlaczono `artifact_capture_enabled=true`.
- Nie zostawiono pomiaru na starym `r3`, bo licznik byl juz skazony.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- Co zmieniono: `seer.program_streams.enabled=false -> true`
- Dlaczego: przywrocenie dwoch NLN route-evidence topics.
- Efekt: runtime uruchomil `solana.pump_fun.buy` i `solana.pump_fun.buy_exact_sol_in`.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- Co zmieniono: usunieto `artifact_capture_dir`, pozostawiono `artifact_capture_enabled=false`
- Dlaczego: unikniecie reaktywacji raw durable capture directory dla R31.
- Efekt: `has_artifact_writer=false`, brak katalogu `logs/nln_capture/shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`.

Zmiana 3:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml`
- Co zmieniono: scope/log paths z `r3` na `r4`
- Dlaczego: czysty pomiar po naprawie bez mieszania z wadliwym `r3`.
- Efekt: nowy runtime namespace `shadow-burnin-v3-r31-maxwait15000-fsc-off-r4`.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Config syntax whitespace | `git diff --check -- configs/rollout/shadow-burnin-v3-r31-maxwait15000-fsc-off.toml configs/rollout/ghost_brain_selector_dataset_sampler_r31_maxwait15000_fsc_off.toml` | brak outputu | PASS | shell check 2026-06-15 |
| Runtime process | `tmux ls`, `ps ... ghost-launcher` | `gho-r31`, PID `168624` | PASS | process check po starcie |
| Program Streams start | grep runtime log | `started_topic_count=2`, topics `buy`, `buy_exact_sol_in` | PASS | `runtime.log` lines 164, 450, 932 |
| FSC decision/capture off | config grep | `capture_enabled=false`, `decision_enabled=false`, `hard_reject_enabled=false`, `funding_lane_mode=disabled` | PASS | config grep |
| Raw artifact writer off | runtime log + find | `has_artifact_writer=false`; brak `logs/nln_capture/...r4` | PASS | runtime log first messages, `find logs/nln_capture ...r4` empty |
| Active coverage | runtime JSONL count | `active_entries/active_buys = 35/35 = 100.0%` | PASS | `logs/shadow_run/...r4` after ~60s |
| Probe coverage | runtime JSONL count | `probe_transport/probe_selection = 36/37 = 97.3%` | PASS | `logs/shadow_run/...r4` after ~60s |

Wniosek walidacyjny:
Regresja `active_entries/active_buys` zostala naprawiona na swiezym R31 `r4`: coverage wrocil do `100.0%` na pierwszej probce po restarcie.

Ograniczenia walidacji:
Probka jest krotka. `shadow_simulated/active_buys = 28/35 = 80.0%` pokazuje osobny problem jakosci/sukcesu symulacji, glownie `shadow_handoff_transport_error`, ale nie jest to ta sama metryka co brak `active_entries`.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: config separation
- Co zabezpiecza: Program Streams route-evidence lane nie jest wylaczana razem z FSC decision/capture.
- Kiedy sie aktywuje: R31 runtime start.
- Jak przetestowano: runtime log pokazal dwa started topics i first message.
- Co pozostaje poza zakresem: mylace nazewnictwo logu "FSC capture lane".

Guardrail 2:
- Typ: artifact capture suppression
- Co zabezpiecza: brak durable raw capture directory dla R31.
- Kiedy sie aktywuje: `artifact_capture_enabled=false` oraz brak `artifact_capture_dir`.
- Jak przetestowano: `has_artifact_writer=false`, brak katalogu `logs/nln_capture/...r4`.
- Co pozostaje poza zakresem: stare abortowane katalogi `r31...`/`r2` nie sa usuwane.

## Otwarte ryzyka / follow-up

- Zweryfikowac po dluzszym okresie, czy `active_entries/active_buys` utrzymuje sie powyzej 90%.
- Osobno zdiagnozowac `shadow_simulated/active_buys`, bo po naprawie coverage wynosil `80.0%`, z `shadow_handoff_transport_error=5` na pierwszej probce.
- Rozwazyc zmiane nazwy logu `Seer: starting NLN Program Streams FSC capture lane`, bo myli Program Streams route-evidence lane z aktywnym FSC.
