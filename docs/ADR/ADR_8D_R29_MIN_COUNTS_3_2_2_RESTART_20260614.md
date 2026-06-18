# ADR-8D: R29 min-count thresholds 3/2/2 restart

Status: wykonane, run pozostawiony aktywny
Typ: rollout/config threshold correction
Data: 2026-06-14
Repo/branch: /root/Gho, codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: brak, zmiany lokalne
Zakres: korekta Gatekeeper V2 quantity gate dla kontynuacji R29
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000.toml
- reports/selector/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/r29_mincounts_3_2_2_restart_20260614T190254Z/runtime.log
- backups/r29-mincounts-3-2-2-restart-20260614T190247Z
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zatrzymac aktywny R29, ustalic konkretny RPC endpoint uzyty przez run, ustawic Gatekeeper V2 quantity gate na `min_tx_count = 3`, `min_unique_signers = 2`, `min_buy_count = 2`, uruchomic kontynuacje R29.

Rzeczywisty przebieg:
Zatrzymano sesje `gho-r29`, potwierdzono brak procesu `ghost-launcher`, zweryfikowano endpoint RPC w runtime logu i `.env`, zmieniono trzy wskazane progi, przesunieto aktywne artefakty append-false do backupu, wykonano release preflight i wystartowano `gho-r29` ponownie.

Odchylenia od planu:
Brak. Preflight i krotki smoke wykonano przed pozostawieniem runu aktywnego.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: restart i config Gatekeepera dotykaja shadow runtime, decision thresholds i rollout safety.
Zakres uzycia: zachowanie shadow-only boundary, tego samego R29 namespace i config-driven Gatekeeper thresholds.
Wynik: zmiana ograniczona do trzech progow quantity gate oraz restartu.
Ograniczenia: nie wykonywano dlugiego burn-in po restarcie.

Nazwa: trading-systems
Powod uzycia: zmiana progow ma bezposredni wplyw na aktywnosc BUY/shadow path i nacisk na RPC.
Zakres uzycia: kontrola smoke po restarcie i liczby `429`.
Wynik: w pierwszym krotkim oknie po restarcie `429_count = 0`.
Ograniczenia: pozniejszy rate limit moze pojawic sie wraz z dalszym runtime.

## 3. Opis problemu - 3W2H

What:
Poprzedni tolerant restart mial zbyt niskie quantity gate `1/1/1`, co istotnie podbilo activity i RPC pressure.

Where:
`[gatekeeper_v2]` w `configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`.

Why it matters:
R29 ma pozostac dataset/sampler kontynuacja, ale bez natychmiastowego zalewania shadow BUY path przy minimalnych warunkach `1/1/1`.

How observed:
Aktywny run po tolerant thresholds generowal `429 Too Many Requests` na Alchemy RPC, glownie shadow simulation i ATA/token-balance probes.

How many / scale:
Przed zatrzymaniem w aktywnym runtime logu bylo 265 wpisow `429`.

Evidence:
Runtime log wskazywal `RPC URL: https://solana-mainnet.g.alchemy.com/v2/<redacted>` i `GATEKEEPER BUY PATH FAILED` / `ATA pre-submit fast probe failed` z HTTP 429.

## 4. Przyczyna zrodlowa

Root cause:
Gatekeeper V2 quantity gate ustawiony na `1/1/1` dopuszczal zbyt szeroki BUY/shadow simulation path.

Mechanizm bledu:
Nizsze minima zwiekszyly liczbe kandydatow wymagajacych RPC-heavy shadow checks.

Miejsce:
Linie quantity gate w `[gatekeeper_v2]`.

Skutek:
Nacisk na Alchemy RPC i `429 Too Many Requests` w shadow path.

Dowod:
Pre-change runtime log mial 265 wpisow `429`; po zmianie i krotkim smoke `429_count = 0`.

Odrzucone hipotezy:
Nie stwierdzono crasha procesu; problem nie byl lifecycle failure, tylko runtime pressure po poluzowaniu progow.

## 5. Strategia naprawy

Przyjeta strategia:
Przywrocic quantity gate do `3/2/2`, pozostawiajac reszte tolerancyjnych progow bez zmian, zgodnie z dyspozycja.

Zakres ingerencji:
Trzy pola w brain configu R29 oraz restart runu.

Czego nie zmieniano:
Nie zmieniano RPC endpointu, rollout configu, nazwy R29, `max_wait_time_ms`, kodu Rust, schema JSONL ani shadow/live flags.

Ryzyka:
R29 nadal uzywa tego samego Alchemy endpointu, wiec dalsze `429` sa mozliwe przy burstach.

Odrzucone alternatywy:
Nie zmieniano providerow RPC ani throttlingu, bo dyspozycja dotyczyla progow quantity gate i restartu.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`
- Co zmieniono: `min_tx_count = 3`, `min_unique_signers = 2`, `min_buy_count = 2`.
- Dlaczego: ograniczenie zbyt szerokiego shadow BUY path po tolerant thresholds.
- Efekt: preflight zaladowal `min_tx=3 min_unique=2 min_buy=2`.

Zmiana 2:
- Plik/modul: runtime artifacts
- Co zmieniono: aktywne artefakty zatrzymanego runu przeniesiono do `backups/r29-mincounts-3-2-2-restart-20260614T190247Z`.
- Dlaczego: aktywny rollout ma `append=false` i `require_unique_namespace=true`.
- Efekt: restart mogl utworzyc swieze aktywne sciezki R29.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Stop run | `tmux kill-session -t gho-r29` | brak sesji i brak `ghost-launcher` | PASS | `tmux ls` zwrocil brak servera |
| RPC endpoint | runtime log + `.env` | Alchemy Solana mainnet endpoint | PASS | `RPC URL` i `GHOST_TRIGGER_RPC_URL` wskazuja ten sam provider |
| TOML syntax | `python3 -c 'import tomllib; ...'` | `toml_ok` | PASS | brain config parsuje sie |
| Preflight | `target/release/ghost-launcher --config ... --preflight` | all runtime checks passed | PASS | `min_tx=3 min_unique=2 min_buy=2` |
| Runtime start | `tmux new-session -d -s gho-r29 ...` | sesja aktywna | PASS | `tmux ls` pokazuje `gho-r29` |
| Smoke 429 | aktywny `runtime.log` po krotkim smoke | `429_count = 0` | PASS | runtime log restartu |

Wniosek walidacyjny:
R29 zostal zatrzymany, skorygowany do `3/2/2`, przeszedl preflight i dziala ponownie jako `gho-r29`.

Ograniczenia walidacji:
Smoke byl krotki; dluzszy run moze ponownie ujawnic rate limiting, ale nie pojawil sie w pierwszym kontrolnym oknie po restarcie.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: release preflight
- Co zabezpiecza: zgodnosc configu i runtime dependencies.
- Kiedy sie aktywuje: przed startem.
- Jak przetestowano: PASS.
- Co pozostaje poza zakresem: dlugoterminowe limity RPC.

Guardrail 2:
- Typ: artifact isolation
- Co zabezpiecza: brak mieszania append-false artefaktow miedzy restartami R29.
- Kiedy sie aktywuje: przed restartem kontynuacji.
- Jak przetestowano: nowe artefakty powstaly po starcie.
- Co pozostaje poza zakresem: retencja backupow.

## Otwarte ryzyka / follow-up

- Endpoint RPC pozostal ten sam: Alchemy mainnet z `.env`; dalsze `429` sa mozliwe przy burstach.
- FSC coverage gate byl w warmupie w trakcie krotkiego smoke.
