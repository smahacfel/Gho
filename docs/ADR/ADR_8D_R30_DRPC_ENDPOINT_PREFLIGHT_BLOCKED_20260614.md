# ADR-8D: R30 dRPC endpoint preflight blocked by provider freetier policy

Status: BLOCKED / runtime not active
Typ: rollout operation / RPC endpoint validation
Data: 2026-06-14
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: 4a4e6e4 / none
Zakres: zatrzymanie R30 Alchemy, snapshot statystyk, przelaczenie HTTP RPC na dRPC i proba restartu R30
Dotkniete moduly/pliki:
- .env
- backups/r30-drpc-restart-20260614T221922Z/
Powiazane runy/logi/raporty:
- reports/selector/shadow-burnin-v3-r30-fsc-lookback-window-canary/r30_alchemy_start_20260614T211808Z/runtime.log
- configs/rollout/shadow-burnin-v3-r30-fsc-lookback-window-canary.toml
- configs/rollout/ghost_brain_selector_dataset_sampler_r30_fsc_lookback_1800.toml
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrobic aktualny check R30 na Alchemy, zatrzymac R30, przelaczyc HTTP RPC endpointy na dRPC, wykonac preflight i wznowic R30 jako `gho-r30`.

Rzeczywisty przebieg:
Zebrano snapshot statystyk aktywnego R30 na Alchemy, zatrzymano `gho-r30`, potwierdzono brak procesu `ghost-launcher`, zmieniono trzy HTTP RPC endpointy w `.env` na dRPC, zarchiwizowano dotychczasowe artefakty R30 do backupu, poniewaz `append=false` wymaga czystych sciezek, a nastepnie wykonano preflight.

Odchylenia od planu:
R30 nie zostal wznowiony na dRPC, bo endpoint zwrocil `400 Bad Request` / JSON-RPC error `code=35` dla `getVersion` i `getSlot`: `method is not available on freetier, please upgrade to paid tier`.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: operacja dotyka aktywnego shadow-only R30, DecisionLogger/JSONL artefaktow, shadow simulation coverage i FSC evidence.
Zakres uzycia: zachowanie shadow/live boundary, snapshot statystyk, czysta izolacja dRPC window przez backup artefaktow.
Wynik: R30 zostal zatrzymany i dRPC restart zostal zablokowany przed produkcja mylacych artefaktow.
Ograniczenia: nie uruchomiono runtime smoke na dRPC, bo preflight nie przeszedl.

Nazwa: trading-systems
Powod uzycia: endpoint RPC jest czescia execution/simulation feasibility proof.
Zakres uzycia: wymaganie zielonego preflight przed startem, klasyfikacja providera jako blokera.
Wynik: runtime nie wystartowal na endpointcie, ktory nie obsluguje wymaganych metod JSON-RPC.
Ograniczenia: nie oceniano alternatywnych endpointow.

## 3. Opis problemu -- 3W2H

What:
dRPC endpoint nie przechodzi podstawowych metod wymaganych przez preflight i Solana RPC client.

Where:
`.env`:
- `GHOST_SEER_RPC_ENDPOINT`
- `GHOST_TRIGGER_RPC_URL`
- `GHOST_TRIGGER_SHADOW_RPC_URL`

Why it matters:
Shadow simulation wymaga sprawnego HTTP RPC do preflightu, przygotowania requestow i `simulateTransaction`. Endpoint blokujacy `getVersion`/`getSlot` nie moze byc uzyty do wiarygodnego R30.

How observed:
`ghost-launcher --preflight` zwrocil FAIL na `trigger.rpc_url` i `trigger.balance`. Manualny `curl` do `getVersion` i `getSlot` zwrocil ten sam blad providera.

How many / scale:
1 preflight FAIL po przelaczeniu na dRPC. 3 manualne probes (`getVersion`, `getVersion/`, `getSlot`) zwrocily `400` / `code=35`.

Evidence:
- `trigger.rpc_url: rpc getVersion failed for https://lb.drpc.live/<redacted>`
- `trigger.balance: ... HTTP status client error (400 Bad Request)`
- `{"message":"method is not available on freetier, please upgrade to paid tier","code":35}`

## 4. Przyczyna zrodlowa

Root cause:
Podany endpoint dRPC dziala w trybie/freetier, ktory nie udostepnia wymaganych metod Solana JSON-RPC.

Mechanizm bledu:
Solana RPC client podczas `get_balance` wykonuje cluster version query. Provider zwraca `400 Bad Request`, zanim runtime moze przejsc preflight.

Miejsce:
`https://lb.drpc.live/solana/AhWd_wz6dU5ZrZMMIytbOGPs6TcKaD0R8ZoFVjewFaCJ`

Skutek:
R30 pozostaje zatrzymany; dRPC window nie wystartowal.

Dowod:
Manualny `curl` bezposrednio na endpoint zwraca JSON-RPC error `code=35` dla `getVersion` i `getSlot`.

Odrzucone hipotezy:
- Bledny R30 config: odrzucone, preflight doszedl do RPC checks i wszystkie sciezki/config checks przeszly.
- Problem trailing slash: odrzucone, wariant z `/` zwrocil ten sam error.
- Problem NLN Program Streams: odrzucone, gRPC probe przeszedl, a blokada dotyczy HTTP RPC.

## 5. Strategia naprawy

Przyjeta strategia:
Nie startowac runtime po czerwonym preflight. Zachowac dRPC w `.env` zgodnie z ostatnia dyspozycja, ale pozostawic R30 nieaktywny do czasu decyzji o innym endpointcie lub platnym/odblokowanym dRPC.

Zakres ingerencji:
Zmieniono `.env`, zatrzymano proces R30, zarchiwizowano artefakty Alchemy window.

Czego nie zmieniano:
- Nie zmieniano R30 rollout configu.
- Nie zmieniano R30 brain configu.
- Nie zmieniano kodu.
- Nie przywrocono Alchemy bez jawnej dyspozycji.

Ryzyka:
R30 nie dziala obecnie. Artefakty Alchemy window sa w backupie; nowe R30 sciezki sa czyste i gotowe na restart po poprawnym endpointcie.

Odrzucone alternatywy:
Start mimo czerwonego preflightu odrzucono, bo endpoint nie udostepnia nawet `getVersion`/`getSlot`.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: runtime operation
- Co zmieniono: zatrzymano `gho-r30`.
- Dlaczego: user poprosil o zatrzymanie R30 przed przelaczeniem RPC.
- Efekt: brak aktywnego `tmux` i brak procesu `ghost-launcher`.

Zmiana 2:
- Plik/modul: `.env`
- Co zmieniono: `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_SHADOW_RPC_URL` ustawiono na dRPC.
- Dlaczego: user wskazal nowy endpoint do testu.
- Efekt: preflight probowal uzyc dRPC.

Zmiana 3:
- Plik/modul: `backups/r30-drpc-restart-20260614T221922Z/`
- Co zmieniono: przeniesiono dotychczasowe R30 runtime artefakty z fazy Alchemy.
- Dlaczego: `append=false` i `require_unique_namespace=true` wymagaja czystego namespace; mieszanie Alchemy+dRPC utrudniloby porownanie.
- Efekt: R30 sciezki artefaktow sa czyste, a Alchemy evidence zachowane.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| R30 process stop | `tmux ls`, `pgrep -af ghost-launcher` | brak aktywnego procesu | PASS | `no server running` |
| Alchemy stats snapshot | JSONL/runtime parsers | statystyki zebrane | PASS | active shadow entries 221/237 simulated |
| Clean namespace | `mv ... backups/r30-drpc-restart-20260614T221922Z` | artefakty przeniesione | PASS | backup dir |
| dRPC preflight | `target/release/ghost-launcher --config ... --preflight` | provider 400 / code 35 | FAIL | `method is not available on freetier` |
| Manual dRPC getVersion | `curl ... getVersion` | provider 400 / code 35 | FAIL | direct JSON-RPC response |
| Manual dRPC getSlot | `curl ... getSlot` | provider 400 / code 35 | FAIL | direct JSON-RPC response |

Wniosek walidacyjny:
dRPC endpoint w podanej formie nie nadaje sie do R30, bo blokuje podstawowe Solana JSON-RPC methods wymagane przez preflight i runtime.

Ograniczenia walidacji:
Nie sprawdzano platnego/alternatywnego dRPC endpointu ani dodatkowych headerow, bo user podal konkretny URL jako endpoint.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: preflight gate
- Co zabezpiecza: brak startu na endpointcie, ktory nie obsluguje wymaganych metod RPC.
- Kiedy sie aktywuje: przed runtime start.
- Jak przetestowano: preflight zatrzymal restart.
- Co pozostaje poza zakresem: automatyczne wykrywanie tieru providera.

Guardrail 2:
- Typ: artifact isolation
- Co zabezpiecza: brak zmieszania Alchemy i dRPC evidence w tych samych JSONL.
- Kiedy sie aktywuje: przed restartem z nowym endpointem przy `append=false`.
- Jak przetestowano: stare sciezki przeniesiono do backupu.
- Co pozostaje poza zakresem: automatyczne porownywanie providerow.

## Otwarte ryzyka / follow-up

- R30 obecnie nie dziala.
- `.env` wskazuje dRPC endpoint, ktory nie przechodzi preflight.
- Do wznowienia R30 potrzebny jest endpoint RPC, ktory pozwala co najmniej na `getVersion`, `getBalance`, `getSlot` i `simulateTransaction`.
- Alchemy window statystyki sa zachowane w `backups/r30-drpc-restart-20260614T221922Z/` oraz runtime logu w `reports/selector/.../r30_alchemy_start_20260614T211808Z/runtime.log`.
