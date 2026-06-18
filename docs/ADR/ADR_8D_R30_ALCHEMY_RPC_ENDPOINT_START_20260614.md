# ADR-8D: R30 start after switching HTTP RPC to Alchemy

Status: DONE / runtime active
Typ: rollout operation / RPC endpoint change
Data: 2026-06-14
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: 4a4e6e4 / none
Zakres: przelaczenie HTTP RPC endpointow na Alchemy i uruchomienie R30
Dotkniete moduly/pliki:
- .env
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml
- configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml
- reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_alchemy_start_20260614T211808Z/runtime.log
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zmienic HTTP RPC endpointy na wskazany endpoint Alchemy, ponowic R30 preflight i jesli przejdzie uruchomic R30 w `tmux`.

Rzeczywisty przebieg:
Zmieniono trzy zmienne RPC w `.env`: `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_SHADOW_RPC_URL`. `GHOST_RPC_AUTH_*` pozostawiono bez zmian, poniewaz kod stosuje ten auth tylko dla hostow NLN RPC. Preflight R30 przeszedl i runtime zostal uruchomiony w `tmux` jako `gho-r30`.

Odchylenia od planu:
Brak. Start wykonano dopiero po zielonym preflight.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: zmiana endpointu runtime dotyka shadow execution, FSC capture i DecisionLogger artefaktow.
Zakres uzycia: zachowanie shadow-only, sprawdzenie preflight i runtime smoke.
Wynik: R30 wystartowal na R30 configu bez zmiany Gatekeeper/FSC profilu.
Ograniczenia: smoke trwal okolo minuty; nie jest to pelna ocena coverage.

Nazwa: trading-systems
Powod uzycia: endpoint RPC jest elementem runtime execution/simulation integrity.
Zakres uzycia: wymaganie zielonego preflight przed startem oraz kontrola 429/timeoutow po starcie.
Wynik: `429=0`, `rpc_timeout=0` w pierwszym smoke.
Ograniczenia: dlugookresowe zachowanie providera wymaga dalszej obserwacji.

## 3. Opis problemu -- 3W2H

What:
Poprzedni start R30 byl zablokowany przez timeouty HTTP RPC NLN. Endpoint zostal zmieniony na Alchemy i preflight przeszedl.

Where:
`.env` oraz runtime R30:
`configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml`

Why it matters:
Shadow simulation coverage i lifecycle evidence zalezy od sprawnego HTTP RPC. Start z timeoutujacym RPC produkowalby zafalszowane dane porownawcze.

How observed:
Preflight po zmianie endpointu zwrocil `getVersion=4.0.2`, balance OK i `all runtime checks passed`.

How many / scale:
1 preflight PASS po zmianie na Alchemy. 1 runtime start PASS. Smoke okolo 60 sekund.

Evidence:
- `trigger.rpc_url: jsonrpc getVersion=4.0.2`
- `trigger.balance: 0.047172000 SOL >= 0.007200000 SOL reserve+trade budget`
- `preflight: all runtime checks passed`
- `tmux ls` pokazal `gho-r30`
- smoke: `too_many_requests=0`, `rpc_timeout=0`, `nln_first_message=2`

## 4. Przyczyna zrodlowa

Root cause:
NLN HTTP RPC timeoutowal w poprzednim preflight. Po przelaczeniu na Alchemy HTTP RPC preflight przeszedl.

Mechanizm bledu:
Runtime potrzebuje HTTP RPC do trigger/shadow RPC i preflightu balance. NLN HTTP RPC nie odpowiadal, Alchemy odpowiada.

Miejsce:
`.env` endpointy HTTP RPC.

Skutek:
R30 zostal uruchomiony dopiero po zmianie endpointu i zielonym preflight.

Dowod:
Preflight po zmianie na Alchemy zakonczyl sie PASS i runtime log zaczal zbierac ingest oraz FSC gate updates.

Odrzucone hipotezy:
- Bledny R30 rollout config: odrzucone poprzednio przez preflight sciezek i brain parity.
- Brak Program Streams key: odrzucone, `nln_first_message=2`, `nln_missing_key=0`.
- Auth header koliduje z Alchemy: odrzucone na podstawie kodu, auth HTTP RPC stosuje sie tylko do hostow NLN.

## 5. Strategia naprawy

Przyjeta strategia:
Minimalna zmiana `.env`: tylko HTTP RPC endpointy, bez zmiany R30 configu i bez zmiany progow Gatekeeper/FSC.

Zakres ingerencji:
`.env` oraz start runtime w `tmux`.

Czego nie zmieniano:
- Nie zmieniano brain configu R30.
- Nie zmieniano rollout configu R30.
- Nie zmieniano kodu.
- Nie zmieniano Program Streams NLN key/env.

Ryzyka:
Alchemy moze miec limity 429 przy dluzszym runie. Smoke potwierdza tylko pierwsza minute bez 429 i timeoutow.

Odrzucone alternatywy:
Nie uruchamiano ponownie na NLN HTTP RPC, poniewaz preflight timeoutowal.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `.env`
- Co zmieniono: `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_SHADOW_RPC_URL` ustawiono na wskazany endpoint Alchemy.
- Dlaczego: NLN HTTP RPC blokowal R30 preflight timeoutami.
- Efekt: R30 preflight przeszedl.

Zmiana 2:
- Plik/modul: runtime operation
- Co zmieniono: uruchomiono `tmux new-session -d -s gho-r30 ... ghost-launcher --config ...`
- Dlaczego: po zielonym preflight mozna bylo wystartowac R30.
- Efekt: aktywny proces `ghost-launcher` PID `104478` i runtime log R30.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Preflight | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight` | all runtime checks passed | PASS | getVersion=4.0.2, balance OK |
| Runtime start | `tmux new-session -d -s gho-r30 ...` | sesja i proces aktywne | PASS | PID 104478 |
| Smoke 429 | runtime log po ok. 60s | `too_many_requests=0` | PASS | `runtime.log` |
| Smoke RPC timeout | runtime log po ok. 60s | `rpc_timeout=0` | PASS | `runtime.log` |
| Program Streams FSC | runtime log po ok. 60s | `nln_first_message=2`, `nln_missing_key=0` | PASS | `runtime.log` |
| Artifact smoke | R30 shadow log dir | shadow/probe JSONL powstaja | PASS | `shadow_entries=4`, `shadow_lifecycle=10`, `probe_selection=2`, `probe_skips=39` |

Wniosek walidacyjny:
R30 dziala w `tmux` na Alchemy HTTP RPC i w pierwszym smoke nie wykazuje 429 ani RPC timeoutow.

Ograniczenia walidacji:
Pierwsza minuta nie wystarcza do oceny docelowego shadow simulation coverage ani FSC attribution coverage.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: preflight gate
- Co zabezpiecza: brak startu z niedzialajacym HTTP RPC.
- Kiedy sie aktywuje: przed runtime start.
- Jak przetestowano: PASS po przelaczeniu na Alchemy.
- Co pozostaje poza zakresem: dlugookresowe limity providera.

Guardrail 2:
- Typ: runtime smoke
- Co zabezpiecza: wczesne wykrycie 429/timeoutow/braku Program Streams key.
- Kiedy sie aktywuje: pierwsze okno po starcie.
- Jak przetestowano: `429=0`, `rpc_timeout=0`, `nln_first_message=2`.
- Co pozostaje poza zakresem: analiza regresji coverage R29.

## Otwarte ryzyka / follow-up

- Monitorowac R30 dluzej pod katem Alchemy 429 i shadow simulation coverage.
- Osobno ustalic przyczyne regresji R29 coverage: provider problem vs route/materialization/precheck/slippage/seed mismatch.
- FSC lookback 1800 oznacza, ze `coverage_window_ready=false` jest oczekiwane na starcie, dopoki okno lookback sie nie napelni.
