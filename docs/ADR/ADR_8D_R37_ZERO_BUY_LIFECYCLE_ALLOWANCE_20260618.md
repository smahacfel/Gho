# ADR-8D: R37 zero-BUY lifecycle allowance for strict probe runs

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_REPROOF_PASS
Typ: ADR-8D / launcher contract / shadow probe operations
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, local working tree
Commit/PR: not committed at report time
Zakres: `start_selector_lifecycle_run.py` zero-BUY handling for strict threshold probes and data-collection runs
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `scripts/start_selector_lifecycle_run.py`
- `scripts/test_selector_lifecycle_run_guard.py`
- `docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md`
- `docs/ADR/ADR_8D_R37_ZERO_BUY_LIFECYCLE_ALLOWANCE_20260618.md`

Powiazane runy/logi/raporty:
- R37 failed launcher report:
  `reports/selector/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260618T121843Z/RUN_LIFECYCLE_LAUNCHER_REPORT.md`
- R37 decision file inspected:
  `logs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/decisions/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/v2.2/legacy_live/e50df85f7880fd9115b8d1c09a6fc1c71777328988346ff456fccec5802ca955/gatekeeper_v2_decisions.jsonl`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo i sekcje wymagane przez uzytkownika.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zdiagnozowac, dlaczego restart R37 nie dopisuje nowych klasycznych `shadow_buys` / `shadow_lifecycle` rows mimo aktywnego decision logu.

Rzeczywisty przebieg:
- Sprawdzono aktywne sesje `tmux` i procesy.
- Sprawdzono ostatni raport launchera R37.
- Potwierdzono, ze event canary przeszedl, ale launcher zabil run po braku klasycznego BUY lifecycle proof.
- Potwierdzono, ze R37 decision log mial decyzje `REJECT_HARD_FAIL` / `TIMEOUT_*`, bez BUY rows w canary window.
- Potwierdzono, ze probe simulation artifacts istnieja osobno, ale klasyczne BUY lifecycle artifacts nie musza rosnac dla strict probe.

Odchylenia od planu:
Zamiast globalnie usunac lifecycle proof dla wszystkich runow, wprowadzono jawny tryb `--allow-zero-buy-lifecycle-proof`, aby nie oslabic zwyklych lifecycle-capable selector runs.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
`ghost-execution`

Powod uzycia:
Zmiana dotyka kontraktu startu runu, shadow/lifecycle evidence i rozdzielenia event-ingest proof od BUY lifecycle proof.

Zakres uzycia:
SSOT/shadow-live separation, DecisionLogger/audit boundary, launcher operational contract.

Wynik:
Zmiana zostala ograniczona do launchera i dokumentacji. Runtime, Gatekeeper policy, execution i send path nie zostaly zmienione.

Ograniczenia:
Skill nie zastapil runtime reproofu. Nowy tryb wymaga potwierdzenia na R37/R38 po starcie.

Specjalisci logiczni:
- Primary: Decision Logging Replay Analyst
- Supporting: Config Rollout Safety Reviewer

## 3. Opis problemu - 3W2H

What:
Launcher traktowal brak klasycznego BUY lifecycle proof jako fatalny blad dla kazdego selector runu.

Where:
`scripts/start_selector_lifecycle_run.py`, etap po `event_canary`, petla `RUNNING_AWAITING_LIFECYCLE_PROOF`.

Why it matters:
Strict threshold probe moze poprawnie dzialac i emitowac decyzje/probe artifacts, ale nie wygenerowac zadnego klasycznego BUY w canary window. W takim przypadku zabicie runu przez warunek `shadow_buys_delta=0` niszczy data-collection zamiast wykrywac awarie.

How observed:
R37 event canary pokazal dodatnie delty `Candidate`, `NewPoolDetected`, `PoolTransaction`, `DIAG_ACCOUNT_UPDATE_RELAY`, ale lifecycle proof mial `shadow_buys_delta <= 0`, `shadow_entries_delta <= 0`, `shadow_lifecycle_delta <= 0`.

How many / scale:
Dotyczy wszystkich strict threshold probes, w ktorych zero BUYs w pierwszym oknie jest dopuszczalnym wynikiem polityki.

Evidence:
R37 decision log mial nowe decyzje, ale bez BUY rows. Poprzedni launcher report zakonczyl `FAIL_LIFECYCLE_PROOF` i `RUN_KILLED_AFTER_FAILED_CANARY`.

## 4. Przyczyna zrodlowa

Root cause:
Launcher mial jeden kontrakt sukcesu: event proof + klasyczny BUY lifecycle proof.

Mechanizm bledu:
Po udanym event canary launcher zawsze przechodzil do petli lifecycle proof i zabijal `tmux`, jezeli w timeout nie pojawily sie klasyczne BUY/lifecycle rows.

Miejsce:
`scripts/start_selector_lifecycle_run.py`, blok po `event_result["exit_code"] == 0`.

Skutek:
Strict probe R37 byl zatrzymywany mimo zdrowego event ingest i mimo tego, ze zero BUYs moglo byc naturalnym wynikiem hard thresholds.

Dowod:
R37: decision rows rosly, event deltas byly dodatnie, a klasyczne BUY lifecycle artifacts pozostaly zerowe w canary window.

Odrzucone hipotezy:
- Bledna sciezka decision logu: odrzucone, plik decision log mial nowe rows.
- Brak event ingest: odrzucone, event canary mial dodatnie delty.
- Brak probe simulation w ogole: odrzucone, probe artifacts istnialy osobno.

## 5. Strategia naprawy

Przyjeta strategia:
Dodac jawny opt-in tryb `--allow-zero-buy-lifecycle-proof`, ktory po PASS event canary zostawia run zywy bez oczekiwania na klasyczny BUY lifecycle proof.

Zakres ingerencji:
- Python launcher.
- Testy launchera.
- Runbook operacyjny.
- ADR-8D.

Czego nie zmieniano:
- Gatekeeper policy.
- Strict thresholds.
- Rust runtime.
- Shadow probe simulation.
- Klasyczny lifecycle canary.
- `check_selector_lifecycle_canary.py` definicja pelnego proof.
- Execution/send path.

Ryzyka:
- Operator moze uzyc flagi dla runu, ktory faktycznie powinien miec klasyczny BUY lifecycle proof.
- Raporty moglyby zostac zle zinterpretowane jako pelny lifecycle PASS.

Odrzucone alternatywy:
- Globalne usuniecie lifecycle proof: odrzucone, bo oslabiloby lifecycle-capable selector run contract.
- Zmiana definicji lifecycle canary w `check_selector_lifecycle_canary.py`: odrzucone, bo canary nadal poprawnie reprezentuje klasyczny BUY lifecycle proof.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `scripts/start_selector_lifecycle_run.py`
- Co zmieniono: dodano `--allow-zero-buy-lifecycle-proof`.
- Dlaczego: strict probe/data collection run moze miec zero BUYs jako dopuszczalny wynik.
- Efekt: po PASS event canary launcher moze zostawic run w `tmux` bez czekania na klasyczne `shadow_buys_delta > 0`.

Zmiana 2:
- Plik/modul: `scripts/start_selector_lifecycle_run.py`
- Co zmieniono: dodano osobny `run_state` i claim:
  `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED` oraz
  `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`.
- Dlaczego: raport nie moze klamac, ze mamy `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`.
- Efekt: event-only PASS jest audytowalnie odroznialny od pelnego lifecycle proof.

Zmiana 3:
- Plik/modul: `docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md`
- Co zmieniono: opisano tryb strict probe / data collection zero-BUY.
- Dlaczego: operator musi wiedziec, ze to nie jest klasyczny BUY lifecycle proof.
- Efekt: runbook rozdziela lifecycle-capable runs od zero-BUY probes.

Zmiana 4:
- Plik/modul: `scripts/test_selector_lifecycle_run_guard.py`
- Co zmieniono: dodano test parsera flagi i test claim/report dla event-only PASS.
- Dlaczego: zabezpieczenie przed regresja, w ktorej raport znowu udawalby pelny lifecycle proof.
- Efekt: narrow unit coverage dla nowego kontraktu.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Build | `python3 -m py_compile scripts/start_selector_lifecycle_run.py scripts/test_selector_lifecycle_run_guard.py` | syntax OK | PASS | command exit 0 |
| Unit | `python3 -m unittest scripts.test_selector_lifecycle_run_guard -v` | 11 tests OK | PASS | command exit 0 |
| Diff check | `git diff --check` | no whitespace errors | PASS | command exit 0 |
| Runtime smoke | R37 launcher with `--allow-zero-buy-lifecycle-proof` | event canary PASS, run left running | PASS | `RUN_LIFECYCLE_LAUNCHER_REPORT.json`: `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED` |

Wniosek walidacyjny:
Zmiana jest kodowo zaimplementowana, ma targeted test coverage i zostala potwierdzona przez ponowny start R37 w trybie `--allow-zero-buy-lifecycle-proof`.

Ograniczenia walidacji:
Test jednostkowy potwierdza kontrakt raportowania. Krotki R37 smoke potwierdzil event ingest i pozostawienie runu w `tmux`; dluzsza stabilnosc oraz dalszy wzrost probe artifacts pozostaja do monitoringu operacyjnego.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: opt-in CLI flag
- Co zabezpiecza: normalne lifecycle-capable selector runs nadal wymagaja klasycznego BUY lifecycle proof.
- Kiedy sie aktywuje: tylko przy `--allow-zero-buy-lifecycle-proof`.
- Jak przetestowano: test parsera flagi i test claim/report.
- Co pozostaje poza zakresem: zla decyzja operatora o uzyciu flagi dla runu, ktory powinien byc lifecycle-validation.

Guardrail 2:
- Typ: distinct claim/run_state
- Co zabezpiecza: brak falszywego claimu `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`.
- Kiedy sie aktywuje: PASS w zero-BUY mode.
- Jak przetestowano: unit test `test_launcher_zero_buy_lifecycle_allowance_has_distinct_pass_claim`.
- Co pozostaje poza zakresem: downstream raporty, ktore ignoruja claim i patrza tylko na `status=PASS`.

## Otwarte ryzyka / follow-up

- Monitorowac R37 po dluzszym oknie, czy decision rows i probe simulation artifacts dalej rosna.
- W pozniejszych raportach nie traktowac tego trybu jako dowodu klasycznego BUY lifecycle.
