# ADR-8D: R30 FSC lookback canary config and RPC preflight blocker

Status: OPEN / runtime start blocked by RPC preflight
Typ: rollout configuration / operational validation
Data: 2026-06-14
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: 4a4e6e4 / none
Zakres: przygotowanie R30 na profilu R27b FSC lookback 1800
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml
- configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml
Powiazane runy/logi/raporty:
- configs/rollout/ghost_brain_selector_dataset_sampler_r27b_fsc_lookback_1800.toml
- configs/rollout/shadow-burnin-v3-r27b-fsc-lookback-window-canary.toml
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Utworzyc R30 zgodnie z procedura rolloutowa, opierajac brain config na identycznym profilu jak `ghost_brain_selector_dataset_sampler_r27b_fsc_lookback_1800.toml`, wykonac preflight i wystartowac runtime w `tmux`.

Rzeczywisty przebieg:
Utworzono brain config R30 jako kopie byte-for-byte profilu R27b. Utworzono osobny rollout config R30 na bazie R27b, zmieniajac tylko identyfikatory runu, namespace i sciezki artefaktow na R30. Wykonano dwa preflighty release oraz manualne JSON-RPC probes.

Odchylenia od planu:
Runtime nie zostal uruchomiony, poniewaz aktualny HTTP RPC endpoint z `.env` (`https://rpc.nln.clr3.org`) nie przeszedl preflightu: `getVersion` i balance fetch timeoutowaly.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: rollout dotyka Ghost shadow runtime, DecisionLogger/JSONL artefaktow i shadow/live boundary.
Zakres uzycia: zachowanie shadow-only, osobne namespace/sciezki, brak mieszania artefaktow R27b/R30.
Wynik: R30 rollout config izoluje artefakty i wskazuje na R30 brain profile.
Ograniczenia: nie diagnozowano jeszcze regresji R29 shadow simulation coverage.

Nazwa: trading-systems
Powod uzycia: preflight runtime chroni przed startem z niedzialajacym RPC w systemie zbierajacym evidence.
Zakres uzycia: decyzja operacyjna, zeby nie startowac runu po czerwonym RPC preflight.
Wynik: run nie zostal uruchomiony na uszkodzonym transporcie.
Ograniczenia: brak alternatywnego RPC nie zostal samowolnie podstawiony.

## 3. Opis problemu -- 3W2H

What:
R30 config zostal przygotowany, ale runtime start zostal zablokowany przez nieudany RPC preflight.

Where:
`target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight`

Why it matters:
Start shadow simulation bez sprawnego HTTP RPC moglby natychmiast zaniyc coverage i wytworzyc mylace artefakty porownawcze dla R30.

How observed:
Dwa preflighty wykazaly ten sam blad `trigger.rpc_url`; manualne `curl` do `getVersion` i `getSlot` rowniez timeoutowaly.

How many / scale:
2/2 preflighty nie przeszly; 2/2 manualne probes timeoutowaly po 10 sekundach.

Evidence:
- `trigger.rpc_url: rpc getVersion failed for https://rpc.nln.clr3.org`
- `trigger.balance: failed to fetch balance over trigger.rpc_url ... operation timed out`
- manualny `curl getVersion`: `Operation timed out after 10001 milliseconds with 0 bytes received`
- manualny `curl getSlot`: `Operation timed out after 10002 milliseconds with 0 bytes received`

## 4. Przyczyna zrodlowa

Root cause:
Aktualny HTTP RPC endpoint NLN w `.env` nie odpowiadal na JSON-RPC w czasie preflightu.

Mechanizm bledu:
Preflight wymaga `getVersion` i pobrania balance przez `trigger.rpc_url`; oba wywolania timeoutowaly.

Miejsce:
`https://rpc.nln.clr3.org` uzywany przez:
- `GHOST_SEER_RPC_ENDPOINT`
- `GHOST_TRIGGER_RPC_URL`
- `GHOST_TRIGGER_SHADOW_RPC_URL`

Skutek:
R30 nie zostal uruchomiony, mimo ze config lokalny i gRPC probe sa poprawne.

Dowod:
Preflight pokazal poprawne zaladowanie configu R30, Gatekeeper `min_tx=3 min_unique=2 min_buy=2 max_wait_ms=10000`, poprawne sciezki R30 i PASS dla gRPC, ale FAIL dla HTTP RPC.

Odrzucone hipotezy:
- Bledny brain config R30: odrzucone, `cmp` potwierdzil kopie byte-for-byte R27b.
- Bledne sciezki artefaktow R30: odrzucone, preflight potwierdzil writable dirs.
- Brak gRPC dostepu: odrzucone, gRPC app probe przeszedl.

## 5. Strategia naprawy

Przyjeta strategia:
Przygotowac R30 configi, ale nie uruchamiac runtime bez zielonego preflightu RPC.

Zakres ingerencji:
Dodano nowe configi R30 i niniejszy ADR.

Czego nie zmieniano:
- Nie zmieniano R27b configu.
- Nie zmieniano R29 configu.
- Nie zmieniano `.env`.
- Nie zmieniano kodu runtime.
- Nie podstawiono alternatywnego RPC bez jawnej decyzji.

Ryzyka:
R30 pozostaje gotowy, ale nieaktywny. Start wymaga sprawnego NLN HTTP RPC albo jawnej decyzji o innym RPC endpointcie.

Odrzucone alternatywy:
Start mimo czerwonego preflightu zostal odrzucony, bo produkowalby artefakty z oczywiscie uszkodzonym transportem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml`
- Co zmieniono: dodano kopie R27b brain config.
- Dlaczego: R30 ma uzyc identycznego profilu progow/FSC jak R27b.
- Efekt: `brain_copy_identical=PASS`.

Zmiana 2:
- Plik/modul: `configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml`
- Co zmieniono: dodano rollout config R30 na bazie R27b z osobnym namespace, run_id, session_id i sciezkami artefaktow.
- Dlaczego: R30 nie moze zapisywac do artefaktow R27b.
- Efekt: preflight laduje R30 namespace i R30 output paths.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Brain parity | `cmp -s configs/rollout/ghost_brain_selector_dataset_sampler_r27b_fsc_lookback_1800.toml configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml` | byte-for-byte equal | PASS | `brain_copy_identical=PASS` |
| R30 path isolation | `rg ... shadow-burnin-v3-r30-fsc-lookback-window-canary.toml` | wszystkie sciezki R30 | PASS | namespace/run_id/session_id/log paths R30 |
| Preflight 1 | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight` | RPC timeout | FAIL | `trigger.rpc_url` i `trigger.balance` failed |
| Manual JSON-RPC | `curl ... getVersion/getSlot` | timeout | FAIL | 10s timeout, 0 bytes |
| Preflight 2 | `target/release/ghost-launcher --config configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml --preflight` | RPC timeout | FAIL | powtorzony ten sam blad |

Wniosek walidacyjny:
Config R30 jest przygotowany, ale runtime start jest zablokowany przez aktualna niedostepnosc/timeout HTTP RPC NLN.

Ograniczenia walidacji:
Nie wykonano runtime smoke, poniewaz preflight nie przeszedl.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: namespace isolation
- Co zabezpiecza: brak mieszania artefaktow R27b i R30.
- Kiedy sie aktywuje: kazdy zapis rollout/probe/shadow/lifecycle/events.
- Jak przetestowano: `rg` po rollout configu R30.
- Co pozostaje poza zakresem: zachowanie providera RPC po starcie.

Guardrail 2:
- Typ: release preflight gate
- Co zabezpiecza: runtime nie startuje z niedzialajacym HTTP RPC.
- Kiedy sie aktywuje: przed uruchomieniem `tmux`.
- Jak przetestowano: dwa preflighty wykryly timeout RPC.
- Co pozostaje poza zakresem: automatyczny fallback na innego providera RPC.

## Otwarte ryzyka / follow-up

- R30 wymaga ponowienia preflightu po powrocie NLN HTTP RPC albo jawnej decyzji o innym RPC endpointcie.
- Regresja R29 shadow simulation coverage nadal wymaga osobnej diagnostyki przyczyn: `network_provider_problem`, `too_much_sol_required`, `precheck_failed_not_dispatched`, `seed_mismatch_constraint_seeds`, `simulation_mismatch`.
- Wplyw zmiany RPC na NLN na shadow simulation coverage pozostaje hipoteza do sprawdzenia po uzyskaniu stabilnego punktu porownawczego.
