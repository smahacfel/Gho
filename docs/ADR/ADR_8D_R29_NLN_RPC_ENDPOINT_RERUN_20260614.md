# ADR-8D: R29 NLN RPC endpoint rerun

Status: wykonane, run pozostawiony aktywny
Typ: rollout/runtime config endpoint change
Data: 2026-06-14
Repo/branch: /root/Gho, codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: brak, zmiany lokalne
Zakres: podmiana RPC endpointu R29 z Alchemy na NLN header-auth RPC oraz restart kontynuacji R29
Dotkniete moduly/pliki:
- .env
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000.toml
- configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml
- reports/selector/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/r29_nln_rpc_restart_20260614T194704Z/runtime.log
- backups/r29-nln-rpc-restart-20260614T194430Z
- backups/r29-nln-rpc-restart-missing-nln-key-20260614T194606Z
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zatrzymac aktywny R29, podmienic runtime RPC endpointy na `https://rpc.nln.clr3.org`, zachowac auth przez `x-api-key`, wykonac preflight i uruchomic kontynuacje R29.

Rzeczywisty przebieg:
Zweryfikowano, ze launcher i Seer obsluguja header-auth RPC przez `GHOST_RPC_AUTH_HEADER` i `GHOST_RPC_AUTH_TOKEN`. Zatrzymano stary run, zmieniono `.env`, wykonano preflight, wykonano krotki start kontrolny, wykryto brak aliasu `NLN_API_KEY` dla Program Streams FSC capture, zatrzymano start kontrolny, dodano aliasy `NLN_API_KEY` i `GHOST_NLN_API_KEY`, powtorzono preflight i uruchomiono finalna kontynuacje R29.

Odchylenia od planu:
Pierwszy preflight po dodaniu aliasow mial chwilowy timeout `getVersion`/gRPC probe; reczny `curl` z `x-api-key` potwierdzil dzialanie `getSlot` i `getVersion`, a drugi preflight przeszedl.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: zmiana dotyczy aktywnego shadow rollout, Seer/RPC config, FSC capture i runtime restart.
Zakres uzycia: zachowanie shadow-only boundary, R29 namespace, Gatekeeper 3/2/2 i artifact isolation.
Wynik: endpoint podmieniony bez zmiany kodu i bez zmiany decyzji Gatekeeper.
Ograniczenia: nie wykonano dlugiego burn-in po restarcie.

Nazwa: trading-systems
Powod uzycia: endpoint RPC wplywa na shadow simulation, IWIM, account fetches i rate-limit behavior.
Zakres uzycia: smoke liveness, `429` count i residual risk.
Wynik: w poczatkowym smoke finalnego runu `429_count = 0`.
Ograniczenia: dalsze limity/timeouty moga ujawnic sie w dluzszym oknie.

## 3. Opis problemu - 3W2H

What:
Alchemy RPC generowal `429 Too Many Requests` przy R29 po poluzowaniu progow i restartach.

Where:
Runtime RPC endpointy w `.env`: `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_SHADOW_RPC_URL`.

Why it matters:
Shadow BUY path i IWIM/account probes sa RPC-heavy. Rate limiting degraduje evidence i powoduje shadow path failures.

How observed:
Poprzednie runtime logi R29 mialy setki wpisow `429 Too Many Requests`, glownie przy shadow simulation, `getTokenAccountBalance`, payer account/balance fetch.

How many / scale:
Przed zmiana endpointu obserwowano m.in. 265 wpisow `429` w jednym z odczytow starego runtime logu oraz dalszy wzrost w kolejnych restartach.

Evidence:
Nowy preflight pokazal `RPC HTTP auth configured for header-auth RPC endpoints header=x-api-key` i `trigger.rpc_url ... via https://rpc.nln.clr3.org`.

## 4. Przyczyna zrodlowa

Root cause:
Poprzedni endpoint RPC byl throughput/rate-limit bottleneckiem dla obecnego R29 shadow workload.

Mechanizm bledu:
Wiele rownoleglych shadow checks/probes trafialo w rate limit providera, co skutkowalo HTTP 429 i failami w BUY path.

Miejsce:
Runtime RPC endpoint variables w `.env`.

Skutek:
Degradacja shadow simulation/account probe evidence.

Dowod:
Stare runtime logi zawieraly `HTTP status client error (429 Too Many Requests)`.

Odrzucone hipotezy:
Nie byl to problem skladni endpointu NLN; reczny `curl` z `x-api-key` potwierdzil `getSlot` i `getVersion`.

## 5. Strategia naprawy

Przyjeta strategia:
Podmienic wszystkie aktywne endpointy RPC uzywane przez R29 na `https://rpc.nln.clr3.org`, zachowac `x-api-key` auth i dodac brakujace aliasy NLN API key dla Program Streams.

Zakres ingerencji:
Tylko `.env`, backup artefaktow append-false i restart R29.

Czego nie zmieniano:
Nie zmieniano kodu Rust, rollout configu, brain configu, progow Gatekeeper, `max_wait_time_ms`, shadow/live flags ani nazwy R29.

Ryzyka:
NLN RPC ma plytsza historie niz deep archival RPC; runtime path powinien dzialac dla biezacych zapytan, ale dluzsze history lookups moga byc ograniczone.

Odrzucone alternatywy:
Nie wprowadzano dual-RPC split w kodzie, bo obecny runtime juz ma config/env support dla header-auth endpointu.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `.env`
- Co zmieniono: `GHOST_SEER_RPC_ENDPOINT`, `GHOST_TRIGGER_RPC_URL`, `GHOST_TRIGGER_SHADOW_RPC_URL` ustawiono na `https://rpc.nln.clr3.org`.
- Dlaczego: zastapienie Alchemy endpointu ograniczajacego R29 przez 429.
- Efekt: preflight laduje runtime RPC jako NLN RPC.

Zmiana 2:
- Plik/modul: `.env`
- Co zmieniono: dodano aliasy `NLN_API_KEY` i `GHOST_NLN_API_KEY` zgodne z istniejacym sekretem w `.env`.
- Dlaczego: Program Streams FSC capture oczekuje tych nazw env i przy ich braku startowal w stanie zdegradowanym.
- Efekt: finalny runtime log pokazal start NLN Program Streams FSC capture lane i first message events.

Zmiana 3:
- Plik/modul: runtime artifacts
- Co zmieniono: artefakty poprzednich aktywnych startow przeniesiono do backupow.
- Dlaczego: rollout profile ma `append=false` i `require_unique_namespace=true`.
- Efekt: finalny start R29 ma czyste aktywne sciezki.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Manual RPC getSlot | `curl` z `x-api-key` | JSON-RPC result slot | PASS | `https://rpc.nln.clr3.org` odpowiada |
| Manual RPC getVersion | `curl` z `x-api-key` | `solana-core=4.0.0` | PASS | endpoint wspiera metode |
| Preflight | `target/release/ghost-launcher --config ... --preflight` | all runtime checks passed | PASS | `trigger.rpc_url ... via https://rpc.nln.clr3.org` |
| RPC auth | runtime/preflight log | `RPC HTTP auth configured ... header=x-api-key` | PASS | auth header wlaczony |
| Gatekeeper config | preflight/runtime log | `min_tx=3 min_unique=2 min_buy=2` | PASS | progi zachowane |
| Program Streams FSC | runtime log | lane started + first message received | PASS | brak `NLN API key not found` w finalnym smoke |
| Runtime smoke | finalny `runtime.log` | `429_count=0` w pierwszym oknie | PASS | finalny smoke po starcie |

Wniosek walidacyjny:
R29 dziala na NLN RPC z header-auth i zachowanymi progami Gatekeeper 3/2/2. Program Streams FSC capture nie startuje juz z bledem braku API key.

Ograniczenia walidacji:
Smoke byl krotki. Dalszy runtime moze pokazac timeouty lub limity inne niz 429; NLN RPC ma ograniczona historie, co moze dotknac dluzszych lookupow.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: release preflight
- Co zabezpiecza: config load, auth RPC, keypair, gRPC probe, wallet balance i artifact paths.
- Kiedy sie aktywuje: przed startem runtime.
- Jak przetestowano: PASS po powtorzonym preflight.
- Co pozostaje poza zakresem: dlugookresowa jakosc endpointu.

Guardrail 2:
- Typ: live-output tmux launch via `tee`
- Co zabezpiecza: puste okno tmux mylone z brakiem procesu.
- Kiedy sie aktywuje: podczas startu R29.
- Jak przetestowano: `tmux capture-pane` pokazuje live logi.
- Co pozostaje poza zakresem: rotacja duzego `runtime.log`.

## Otwarte ryzyka / follow-up

- Monitorowac `429`, RPC timeouty oraz IWIM UNKNOWN/TIMEOUT w dluzszym oknie.
- Jesli historia 3000 blokow okaze sie niewystarczajaca dla IWIM/FSC lookup, potrzebny bedzie osobny deep-history fallback RPC.
