# ADR-8D: R30 NLN Shadow RPC Only and IWIM Veto Gate Disabled

Status: Done
Typ: Config rollout / runtime restart
Data: 2026-06-14
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: local working tree, HEAD 4a4e6e4
Zakres: R30 continuation with dedicated shadow-simulation RPC routing
Dotkniete moduly/pliki:
- .env
- configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml
- reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_nln_shadow_rpc_iwim_off_20260614T225718Z/runtime.log
- logs/shadow_run/shadow-burnin-v3-r30-fsc-lookback-window-canary/
- logs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary/
- logs/nln_capture/shadow-burnin-v3-r30-fsc-lookback-window-canary/
Poziom ryzyka: Medium

## 1. Przygotowanie i działania wstępne

Plan poczatkowy:
- Nie uzywac NLN jako uniwersalnego RPC dla calego runtime.
- Zachowac stabilny main trigger/seer HTTP RPC na Alchemy.
- Skierowac NLN do dedykowanej sciezki shadow simulation przez GHOST_TRIGGER_SHADOW_RPC_URL.
- Wylaczyc aktywny post-Gatekeeper IWIM Veto Gate.
- Uruchomic R30 w tmux i wykonac smoke-check.

Rzeczywisty przebieg:
- Zweryfikowano konfiguracje R30 i osobne pola `trigger.rpc_url` oraz `trigger.shadow_run.shadow_rpc_url`.
- Usunieto z aktywnego namespace R30 pozostalosci po nieudanym preflight dRPC, przenoszac je do `backups/r30-drpc-preflight-20260614T225624Z/`.
- Zmieniono `.env` tak, aby `GHOST_SEER_RPC_ENDPOINT` i `GHOST_TRIGGER_RPC_URL` wskazywaly Alchemy, a `GHOST_TRIGGER_SHADOW_RPC_URL` wskazywal `https://rpc.nln.clr3.org`.
- Wylaczono `[iwim_veto_gate].enabled`.
- Preflight przeszedl.
- R30 uruchomiono w tmux jako `gho-r30`.

Odchylenia od planu:
- Brak. Zmiana pozostala w warstwie konfiguracji; binarium nie bylo przebudowywane.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Zmiana dotyka shadow/live boundary, Gatekeeper BUY path i IWIM post-gate veto.
Zakres uzycia: Potwierdzenie, ze shadow evidence nie jest live inclusion oraz ze IWIM jest etapem po Gatekeeper BUY.
Wynik: Zmieniono tylko aktywny `[iwim_veto_gate]`; nie ruszano Gatekeeper policy, SSOT ani verdict taxonomy.
Ograniczenia: Nie wykonywano dlugiej walidacji coverage, tylko smoke-check po starcie.

Nazwa: trading-systems
Powod uzycia: Zmiana dotyka execution orchestration i runtime restart.
Zakres uzycia: Zachowanie rozdzialu main RPC, shadow RPC, replay/audit artefaktow i fail-closed obserwacji.
Wynik: Run pozostawiono w `execution_mode=Shadow`, `entry_mode=shadow_only`.
Ograniczenia: Nie oceniano skutecznosci ekonomicznej, tylko zdrowie runtime.

## 3. Opis problemu - 3W2H

What:
- Poprzednie proby zmienialy aktywny endpoint RPC globalnie, co moglo mieszac wymagania main runtime, trigger oraz shadow simulation.
- Uzytkownik wymagal dedykowanego uzycia NLN w formie wlasciwej dla shadow simulation oraz tymczasowego wylaczenia IWIM.

Where:
- `.env`
- `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- `configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml`

Why it matters:
- Shadow simulation moze korzystac z innego RPC niz glowny trigger/seer HTTP path.
- Uzycie jednego endpointu dla wszystkich funkcji utrudnia diagnoze i moze powodowac regresje coverage przez limity/provider behavior.
- IWIM nie jest obecnie potrzebny, wiec powinien nie dokladac RPC/fetch latency ani veto po Gatekeeper BUY.

How observed:
- R30 rollout config ma `trigger.rpc_url = "${GHOST_TRIGGER_RPC_URL}"` oraz `trigger.shadow_run.shadow_rpc_url = "${GHOST_TRIGGER_SHADOW_RPC_URL}"`.
- Startup log po restarcie potwierdzil `IWIM Veto Gate CONFIG: enabled=false` oraz `iwim_veto: OFF`.

How many / scale:
- Dotknieto 2 pliki konfiguracyjne.
- Smoke-check objal okolo 60 sekund po restarcie.

Evidence:
- Preflight PASS: `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight`
- Runtime report: `reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_nln_shadow_rpc_iwim_off_20260614T225718Z/runtime.log`
- tmux session: `gho-r30`

## 4. Przyczyna źródłowa

Root cause:
- Endpoint NLN byl wczesniej rozwazany jako calosciowy RPC endpoint, podczas gdy konfiguracja R30 umozliwia precyzyjne rozdzielenie main RPC i shadow simulation RPC.

Mechanizm bledu:
- Globalna podmiana `GHOST_TRIGGER_RPC_URL` / `GHOST_SEER_RPC_ENDPOINT` zmienia wiecej niz sama sciezke shadow simulation.

Miejsce:
- Warstwa `.env` i interpolacja w `shadow-burnin-v3-r30-fsc-lookback-window-canary.toml`.

Skutek:
- Provider-specific problemy glownego RPC moga wygladac jak regresja shadow simulation coverage.

Dowod:
- R30 config ma osobne pole `trigger.shadow_run.shadow_rpc_url`.
- Preflight po rozdzieleniu endpointow przeszedl z main RPC Alchemy.

Odrzucone hipotezy:
- Nie trzeba zmieniac binarium; wymagane pola konfiguracyjne juz istnieja.
- Nie trzeba zmieniac Gatekeeper thresholds.
- Nie trzeba wycinac starej sekcji `[iwim]`; aktywny post-Gatekeeper modul startupowy to `[iwim_veto_gate]`.

## 5. Strategia naprawy

Przyjeta strategia:
- Zostawic main HTTP RPC na stabilnym Alchemy.
- Uzyc NLN tylko jako shadow simulation RPC.
- Zachowac NLN Program Streams bez zmian.
- Wylaczyc IWIM Veto Gate przez config, bez zmian w kodzie.

Zakres ingerencji:
- Minimalna zmiana `.env`.
- Minimalna zmiana `[iwim_veto_gate].enabled`.

Czego nie zmieniano:
- Gatekeeper policy.
- Gatekeeper thresholds.
- FSC config.
- Trigger execution mode.
- Shadow probe config.
- Kod Rust.

Ryzyka:
- R30 restart zaczyna FSC coverage window od nowa po wyczyszczeniu/backupie artefaktow.
- Jednominutowy smoke-check nie dowodzi dlugookresowej poprawy coverage.
- NLN jako shadow RPC nadal moze miec provider-specific ograniczenia widoczne dopiero przy wiekszej liczbie symulacji.

Odrzucone alternatywy:
- Globalne przelaczenie calego runtime na NLN.
- Usuniecie sekcji `[iwim]` lub zmian starych wag scoringowych IWIM.
- Zmiany w kodzie autoryzacji RPC.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `.env`
- Co zmieniono: `GHOST_SEER_RPC_ENDPOINT` i `GHOST_TRIGGER_RPC_URL` ustawiono na Alchemy; `GHOST_TRIGGER_SHADOW_RPC_URL` ustawiono na `https://rpc.nln.clr3.org`.
- Dlaczego: NLN ma byc uzyty dedykowanie dla shadow simulation, a nie dla calego runtime HTTP RPC.
- Efekt: Preflight pokazal main `rpc=https://solana-mainnet.g.alchemy.com/<redacted>`, a shadow runtime korzysta z osobnego `shadow_rpc_url`.

Zmiana 2:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- Co zmieniono: `[iwim_veto_gate].enabled = false`.
- Dlaczego: Uzytkownik wymagal tymczasowego wylaczenia modulu IWIM.
- Efekt: Startup log potwierdzil `IWIM Veto Gate CONFIG: enabled=false` oraz `iwim_veto: OFF`.

Zmiana 3:
- Plik/modul: runtime/tmux
- Co zmieniono: R30 uruchomiono ponownie w `tmux` session `gho-r30`.
- Dlaczego: Run ma pozostac aktywny w tle.
- Efekt: Proces `ghost-launcher` dzialal po smoke-checku.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Preflight | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight` | All runtime checks passed | PASS | terminal output 2026-06-14T22:57:07Z |
| Main RPC | preflight `trigger.rpc_url` | `getVersion=4.0.2` via Alchemy | PASS | preflight output |
| Runtime start | `tmux new-session -d -s gho-r30 ...` | `ghost-launcher` active | PASS | PID 112464, tmux `gho-r30` |
| IWIM disabled | runtime log grep | `IWIM Veto Gate CONFIG: enabled=false`; `iwim_veto: OFF` | PASS | runtime.log lines near startup |
| HTTP 429 smoke | runtime log grep | `Too Many Requests=0`, `HTTP/status 429=0` | PASS | smoke-check after start |
| Shadow artefacts | wc over shadow files | shadow/probe entries and lifecycle files created | PASS | `logs/shadow_run/...` |
| FSC stream | runtime log grep | FSC gate updates present, `coverage_window_ready=false` during warmup | PASS with expected warmup | runtime.log |

Wniosek walidacyjny:
- R30 wystartowal zdrowo w trybie shadow-only z main RPC na Alchemy, NLN jako dedykowany shadow simulation RPC oraz IWIM Veto Gate disabled.

Ograniczenia walidacji:
- Smoke-check trwal okolo minute.
- `coverage_window_ready=true` nie wystapilo jeszcze, bo FSC coverage window po restarcie wymaga dluzszego warmupu.
- Nie przeprowadzono dlugiej analizy coverage ani ekonomicznej skutecznosci runu.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Config separation
- Co zabezpiecza: Main runtime RPC nie jest mylony z shadow simulation RPC.
- Kiedy sie aktywuje: Przy starcie R30 przez interpolacje `.env`.
- Jak przetestowano: Preflight pokazal main RPC Alchemy; runtime zostal uruchomiony z osobnym shadow RPC env var.
- Co pozostaje poza zakresem: Provider-side rate limits NLN przy dlugim obciazeniu.

Guardrail 2:
- Typ: Runtime startup confirmation
- Co zabezpiecza: IWIM Veto Gate nie wykonuje post-Gatekeeper fetch/veto.
- Kiedy sie aktywuje: Przy ladowaniu `ghost_brain_config`.
- Jak przetestowano: Runtime log pokazal `enabled=false` oraz `iwim_veto: OFF`.
- Co pozostaje poza zakresem: Stara sekcja `[iwim]` i historyczne wagi scoringowe, ktorych nie ruszano.

## Otwarte ryzyka / follow-up

- Sprawdzic R30 po dluzszym czasie, gdy FSC `coverage_window_ready=true`.
- Porownac shadow simulation coverage NLN-only-shadow z poprzednim Alchemy-only-shadow po zebraniu porownywalnej probki.
- Monitorowac, czy pojawiaja sie `Too Many Requests`, shadow transport errors albo materialization/slippage errors przy wiekszym wolumenie symulacji.
