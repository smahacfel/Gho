# ADR-8D: R29 Gatekeeper tolerant thresholds restart

Status: wykonane, run pozostawiony aktywny
Typ: rollout/config threshold change
Data: 2026-06-14
Repo/branch: /root/Gho, codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: brak, zmiany lokalne
Zakres: R29 brain config Gatekeeper V2/V2.5/V3 thresholds oraz restart tego samego rollout profile
Dotkniete moduly/pliki:
- configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml
Powiazane runy/logi/raporty:
- configs/rollout/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000.toml
- reports/selector/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/r29_tolerant_restart_20260614T182951Z/runtime.log
- backups/r29-threshold-tolerant-restart-20260614T182910Z
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Przejrzec aktywny brain config R29 pod katem progow Gatekeepera, zidentyfikowac reject-driving thresholds, ustawic wartosci maksymalnie tolerancyjne dla dataset/sampler runu, zachowac ten sam rollout profile R29 i uruchomic go ponownie w tmux.

Rzeczywisty przebieg:
Zweryfikowano aktywne sekcje Gatekeepera w `ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`, porownano je z powodami rejectow z biezacych artefaktow R29, zmieniono progi V2/V2.5/V3, zatrzymano stary `tmux` R29, przeniesiono stare artefakty append-false namespace do backupu, wykonano release-binary preflight i uruchomiono R29 w `tmux`.

Odchylenia od planu:
Backup wymagal drugiego kroku, bo kilka aktywnych katalogow mialo identyczna nazwe bazowa. Nie kasowano danych; pozostale artefakty przeniesiono do typowanych podkatalogow backupu.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: zmiana dotyczy Gatekeeper threshold behavior, shadow/live boundary i decision logging rollout.
Zakres uzycia: zachowanie SSOT, shadow-only execution, reason-code/logging contracts i config safety.
Wynik: zmiany ograniczono do configu; kod, schema i live execution flags pozostaly bez zmian.
Ograniczenia: nie wykonywano dlugiej oceny statystycznej po restarcie.

Nazwa: trading-systems
Powod uzycia: maksymalnie tolerancyjne progi zwiekszaja activity/shadow BUY path i ryzyko nacisku na RPC.
Zakres uzycia: ocena residual risk dla shadow simulation i restartu.
Wynik: run pozostawiono aktywny, ale oznaczono RPC 429 jako ograniczenie walidacji.
Ograniczenia: nie zmieniano przepustowosci RPC, rate limitow ani execution orchestration.

## 3. Opis problemu - 3W2H

What:
R29 odrzucal wiekszosc pooli na progach Gatekeepera, glownie przez core/volume sanity, top3 volume hard-fail oraz dodatkowe Alpha/Prosperity/P6 minima.

Where:
`configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`, sekcje `[gatekeeper_v2]`, `[gatekeeper_v2.dow]`, `[gatekeeper_v2.tas]`, `[gatekeeper_v2.pdd]`, `[gatekeeper_v2.aps]`, `[gatekeeper_v3.early]`, `[gatekeeper_v3.normal]`, `[gatekeeper_v3.extended]`.

Why it matters:
R29 jest dataset/sampler runem; agresywne reject thresholds zawiezaja evidence universe i utrudniaja counterfactual sampling.

How observed:
Z biezacych logow R29 rejecty koncentrowaly sie w `CORE_FAIL: core1=true core2=false core3=true`, `HARD_FAIL: top3_vol`, `PROSPERITY_FAIL`, `ALPHA_FAIL` oraz pojedynczych P6/market-cap/avg-interval przypadkach.

How many / scale:
W probce przed zmiana dominowaly setki `REJECT_CORE_FAIL` i `REJECT_HARD_FAIL`; najwiekszy wzorzec fazowy to P4/core2 fail.

Evidence:
Lokalna analiza biezacych artefaktow R29 przed restartem wskazala m.in. `CORE_FAIL` jako najwieksza klase, dalej `HARD_FAIL top3_vol`, `REJECT_LOW_PROSPERITY` i `REJECT_LOW_ALPHA`.

## 4. Przyczyna zrodlowa

Root cause:
Dataset/sampler profile nadal zawieral aktywne progowe minima i hard capy odziedziczone z bardziej selekcyjnych konfiguracji.

Mechanizm bledu:
Przy szerokim sampling runie minima dla P4, P6, Alpha/Prosperity oraz top3 hard-fail ucinaly duza czesc pooli zanim evidence moglo zostac zebrane do offline analizy.

Miejsce:
Gatekeeper threshold fields w R29 brain config.

Skutek:
Nadmiar progowych REJECT zamiast szerokiego counterfactual collection.

Dowod:
Pre-change reject reason distribution wskazywal dominacje `CORE_FAIL`, `HARD_FAIL top3_vol`, `PROSPERITY_FAIL` i `ALPHA_FAIL`.

Odrzucone hipotezy:
Nie zmieniano kodu Gatekeepera, bo problem byl konfiguracja thresholdow, nie awaria parsera czy lifecycle launcher.

## 5. Strategia naprawy

Przyjeta strategia:
Neutralizowac progi przez wartosci minimalne `0.0`/`0` dla minimow i bardzo wysokie limity dla capow, zachowujac `max_wait_time_ms = 4000`, shadow-only execution i ten sam R29 rollout namespace.

Zakres ingerencji:
Tylko brain config R29 i runtime restart.

Czego nie zmieniano:
Nie zmieniano kodu Rust, schematow JSONL, DecisionLogger, FSC configu, rollout profile path, nazwy R29, execution mode, live execution flags ani `max_wait_time_ms`.

Ryzyka:
Maksymalnie tolerancyjne progi zwiekszaja liczbe shadow BUY/simulation attempts i moga powodowac rate limiting RPC.

Odrzucone alternatywy:
Nie wlaczano selekcyjnego middle-ground progu, bo dyspozycja wymagale ustawien maksymalnie tolerancyjnych.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`
- Co zmieniono: Gatekeeper V2 minima ilosciowe, P2/P3/P4/P6 capy i minima, Alpha/Prosperity/dev-unknown/hybrid thresholds ustawiono na tolerancyjne.
- Dlaczego: glowne rejecty pochodzily z P4/core2, top3 hard-fail, Alpha/Prosperity i P6.
- Efekt: preflight laduje konfiguracje z `min_tx=1 min_unique=1 min_buy=1 max_wait_ms=4000`.

Zmiana 2:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`
- Co zmieniono: Gatekeeper V2.5 DOW/TAS/PDD/APS thresholds zneutralizowano bez wlaczania disabled modulow.
- Dlaczego: gdyby modul zostal aktywowany przez config path, nie powinien ponownie blokowac sampler runu niskimi progami.
- Efekt: zachowano disabled flags, ale progi sa tolerancyjne.

Zmiana 3:
- Plik/modul: `configs/rollout/ghost_brain_selector_dataset_sampler_r29_maxwait4000.toml`
- Co zmieniono: Gatekeeper V3 early/normal/extended profile minima i hard-fail capy zneutralizowano.
- Dlaczego: R29 ma V3 shadow sidecar disabled/emit-only, ale profil nie powinien miec agresywnych progow w tym dataset runie.
- Efekt: V3 thresholds sa zgodne z tolerancyjnym charakterem R29.

Zmiana 4:
- Plik/modul: runtime artefacts
- Co zmieniono: stary R29 `tmux` zamknieto; aktywne append-false artefakty przeniesiono do `backups/r29-threshold-tolerant-restart-20260614T182910Z`; R29 uruchomiono ponownie w `tmux` jako `gho-r29`.
- Dlaczego: `append=false` i `require_unique_namespace=true` wymagaly czystych aktywnych sciezek.
- Efekt: nowy R29 dziala na tym samym rollout profile i poprawionym brain configu.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| TOML syntax | `python3 -c 'import tomllib; ...'` | `toml_ok` | PASS | config parsuje sie jako TOML |
| Release preflight | `target/release/ghost-launcher --config ... --preflight` | all runtime checks passed | PASS | log preflight z 2026-06-14T18:29:51Z |
| Config load | release preflight | `min_tx=1 min_unique=1 min_buy=1 max_wait_ms=4000` | PASS | launcher zaladowal poprawiony Gatekeeper V2 config |
| Runtime start | `tmux new-session -d -s gho-r29 ...` | sesja aktywna | PASS | `tmux ls` pokazuje `gho-r29` |
| Runtime smoke | ok. 60 sekund po starcie | artefakty rosna | PASS_WITH_WARN | `probe_selection=10`, `shadow_entries=40`, `shadow_lifecycle=62` po pierwszej minucie |

Wniosek walidacyjny:
R29 zostal uruchomiony z poprawionym configiem, proces dziala w `tmux`, ingest i artefakty sa aktywne. Walidacja potwierdza start i runtime liveness, nie pelna jakosc wielogodzinnego runu.

Ograniczenia walidacji:
W logu runtime po poluzowaniu progow pojawily sie `429 Too Many Requests` z RPC przy shadow simulation. Jest to oczekiwane ryzyko zwiekszonej liczby BUY-path attempts po ustawieniu progow maksymalnie tolerancyjnych, ale moze ograniczac jakosc shadow execution evidence.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: preflight release-binary
- Co zabezpiecza: bledy configu, brak dostepu do RPC/GRPC, niepoprawne sciezki artefaktow, brak shadow keypair, konflikt portu metrics.
- Kiedy sie aktywuje: przed startem runtime.
- Jak przetestowano: release preflight zakonczony PASS.
- Co pozostaje poza zakresem: rate limit RPC podczas wysokiej liczby shadow simulations.

Guardrail 2:
- Typ: append-false artifact isolation
- Co zabezpiecza: mieszanie starego i nowego R29 runu w aktywnym namespace.
- Kiedy sie aktywuje: przed restartem runu.
- Jak przetestowano: aktywne sciezki namespace przeniesiono do backupu, a nowe artefakty zaczely powstawac po starcie.
- Co pozostaje poza zakresem: stare backupy pozostaja na dysku i moga wymagac pozniejszej polityki retencji.

## Otwarte ryzyka / follow-up

- Tolerancyjne thresholds istotnie zwiekszaja activity i nacisk na RPC; pierwsza minuta pokazala `429 Too Many Requests` przy shadow simulation.
- FSC coverage gate byl jeszcze w warmupie podczas pierwszej minuty, wiec pelny BUY-gate behavior wymaga dluzszego runtime po zamknieciu warmupu.
- Nie wykonywano pelnego raportu lifecycle/canary po restarcie; wykonano preflight i minutowy smoke liveness zgodnie z pilna dyspozycja restartu.
