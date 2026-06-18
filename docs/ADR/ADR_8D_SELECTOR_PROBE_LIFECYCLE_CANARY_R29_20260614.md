# ADR-8D: Selector R29 probe lifecycle canary false fail

Status: Implemented
Typ: Runtime guard / lifecycle proof repair
Data: 2026-06-14
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: local changes, not committed
Zakres: selector lifecycle canary, probe-plane lifecycle proof, reporter validation
Dotkniete moduly/pliki:
- scripts/check_selector_lifecycle_canary.py
- scripts/guard_restore_shadow_lifecycle.py
- scripts/test_selector_lifecycle_run_guard.py
- scripts/test_guard_restore_shadow_lifecycle.py
- docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md
Powiazane runy/logi/raporty:
- reports/selector/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/run_lifecycle_guard_20260614T131811Z/RUN_LIFECYCLE_LAUNCHER_REPORT.json
- reports/selector/shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000/manual_probe_lifecycle_canary_after_fix_20260614T1402Z/RUN_LIFECYCLE_CANARY_PROOF.json
- reports/selector/shadow-burnin-v3-r28-all-decision-counterfactual-30-30-maxwait4000/run_lifecycle_guard_20260614T_r28/RUN_LIFECYCLE_LAUNCHER_REPORT.json
Poziom ryzyka: medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zweryfikowac, dlaczego R29 zostal zabity przez launcher mimo dzialajacego ingest/probe evidence, a nastepnie naprawic lifecycle simulation proof bez zmiany Gatekeeper policy, TX buildera, FSC policy ani shadow/live boundary.

Rzeczywisty przebieg:
Sprawdzono finalny R29 launcher report. Run zakonczyl sie `FAIL_LIFECYCLE_PROOF`, `RUN_KILLED_AFTER_FAILED_CANARY`; event canary mial PASS, ale aktywny BUY shadow plane mial zero `shadow_buys`, `shadow_entries` i `shadow_lifecycle`. R29 mial jednoczesnie poprawny `probe_shadow_lifecycle` z resolved canonical truth. Naprawiono canary tak, aby dla `[p37_shadow_probe]` akceptowal probe-plane lifecycle proof, gdy aktywny BUY shadow proof nie wystapil w oknie canary.

Odchylenia od planu:
Podczas walidacji R28 wykryto dodatkowo, ze canary czytal caly appended log do pamieci i mogl dostac `MemoryError` na dlugich runach. Zmieniono liczenie markerow logow na streaming line-by-line.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Ghost runtime, shadow-only lifecycle, DecisionLogger/proof contract.
Zakres uzycia: rozdzielenie aktywnego BUY shadow proof od counterfactual probe proof, ochrona shadow/live boundary.
Wynik: naprawa guard/canary bez zmian runtime execution.
Ograniczenia: nie uruchamiano nowego live/runtime runu po poprawce.

Nazwa: solana-pumpfun-architect
Powod uzycia: lifecycle simulation i shadow execution artifacts dotykaja Solana execution proof.
Zakres uzycia: klasyfikacja jako shadow/probe evidence, bez zmian TX buildera.
Wynik: potwierdzono brak zmian w transakcyjnej sciezce wykonania.
Ograniczenia: brak nowego on-chain/live proof; walidacja jest artifact/offline.

Nazwa: rust-master
Powod uzycia: runtime guard i bounded memory behavior.
Zakres uzycia: naprawa memory-heavy log marker scan.
Wynik: marker scan dziala streamingowo z offsetu baseline.
Ograniczenia: zmiana dotyczy Python guard, nie Rust hot path.

## 3. Opis problemu - 3W2H

What:
`start_selector_lifecycle_run.py` zabil R29, bo `check_selector_lifecycle_canary.py` wymagal aktywnego BUY shadow lifecycle proof nawet dla all-decision counterfactual profile z `[p37_shadow_probe]`.

Where:
`scripts/check_selector_lifecycle_canary.py`, lifecycle phase.

Why it matters:
Launcher moze falszywie ubic zdrowy all-decision/probe run, mimo ze counterfactual lifecycle simulation produkuje resolved canonical truth rows.

How observed:
R29 final report: event canary PASS, lifecycle canary FAIL, zero active BUY artifacts. R29 probe coverage: `probe_shadow_lifecycle` zawieral resolved `position_closed` i `exit_filled` rows.

How many / scale:
R29: 12 probe transports, 12 probe entries, 20 probe lifecycle rows, 10 simulated transport rows, 10 resolved reporter rows po naprawie.

Evidence:
`manual_probe_lifecycle_canary_after_fix_20260614T1402Z/RUN_LIFECYCLE_CANARY_PROOF.json` ma `status=PASS`, `accepted_lifecycle_plane=probe`, `close_truth_coverage=10/10`, bad markers = 0.

## 4. Przyczyna zrodlowa

Root cause:
Lifecycle canary mial tylko jeden model proof: aktywny BUY shadow plane.

Mechanizm bledu:
Dla profilu all-decision counterfactual probe aktywne BUY rows moga nie wystapic w canary window. Mimo to probe plane moze miec pelna symulacje i resolved lifecycle. Stary canary ignorowal `probe_transport`, `probe_shadow_entries` i `probe_shadow_lifecycle`.

Miejsce:
`scripts/check_selector_lifecycle_canary.py::validate_lifecycle_canary()` oraz main lifecycle phase.

Skutek:
`FAIL_LIFECYCLE_PROOF` i automatyczne zabicie tmux R29 przez launcher.

Dowod:
R29 report: `shadow_buys_delta=0`, `shadow_lifecycle_delta=0`; po poprawce ten sam baseline i artifact set przechodzi przez `accepted_lifecycle_plane=probe`.

Odrzucone hipotezy:
- TX builder regression: odrzucone, bo probe shadow simulation i reporter dzialaja.
- FSC policy block: odrzucone jako bezposrednia przyczyna, bo `ShadowOnly` nie jest blokowany przez FSC authoritative buy gate.
- Gatekeeper threshold drift: poza zakresem, nie zmieniano policy ani progow.

## 5. Strategia naprawy

Przyjeta strategia:
Dodac probe-plane lifecycle proof jako drugi akceptowalny plane dla configow z `[p37_shadow_probe]`, bez luzowania shadow BUY proof i bez ignorowania bad markerow.

Zakres ingerencji:
Python guard/canary, reporter validation, tests, runbook.

Czego nie zmieniano:
- Ghost Rust runtime
- Gatekeeper policy/scoring/thresholds
- FSC decision/hard reject behavior
- TX builder / DirectBuyBuilder / Helius/live sender
- shadow/live boundary

Ryzyka:
Probe proof nie jest aktywnym Gatekeeper BUY dispatch. Raport jawnie zapisuje `accepted_lifecycle_plane=probe`, aby nie mylic go z BUY shadow proof.

Odrzucone alternatywy:
- Wydluzanie timeoutu launchera: nie naprawia falszywego wymagania BUY dla profilu probe.
- Tuning Gatekeepera w celu wymuszenia BUY: naruszalby policy scope.
- Ignorowanie lifecycle proof, gdy nie ma BUY: utraciloby symulacyjny dowod dla all-decision runu.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: scripts/check_selector_lifecycle_canary.py
- Co zmieniono: dodano wykrywanie probe paths, baseline line counts, probe transport/entry/lifecycle deltas, probe lifecycle validator i `accepted_lifecycle_plane`.
- Dlaczego: all-decision counterfactual run musi moc udowodnic lifecycle przez probe-plane.
- Efekt: R29 artifacts przechodza jako `SELECTOR_LIFECYCLE_CANARY_PASS` z `accepted_lifecycle_plane=probe`.

Zmiana 2:
- Plik/modul: scripts/check_selector_lifecycle_canary.py
- Co zmieniono: log marker scan zmieniono na streaming line-by-line od baseline offsetu.
- Dlaczego: pelny R28 recheck trafial w `MemoryError` przy czytaniu ogromnego appended loga.
- Efekt: canary nie laduje calych logow do pamieci.

Zmiana 3:
- Plik/modul: scripts/guard_restore_shadow_lifecycle.py
- Co zmieniono: `validate_reporter_rows()` dostal `require_gatekeeper_buy_context` z domyslnym `true`.
- Dlaczego: shadow BUY reporter nadal wymaga BUY context, ale probe reporter dla REJECT/TIMEOUT nie powinien.
- Efekt: kompatybilnosc domyslna zachowana; probe reporter moze przejsc bez `gatekeeper_buy_context_found`.

Zmiana 4:
- Plik/modul: docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md
- Co zmieniono: opisano `shadow` i `probe` accepted lifecycle planes.
- Dlaczego: runbook musi odpowiadac realnemu kontraktowi all-decision/probe.
- Efekt: operacyjna procedura nie sugeruje juz falszywie, ze kazdy all-decision run musi miec BUY shadow rows w canary window.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Python compile | `python3 -m py_compile scripts/check_selector_lifecycle_canary.py scripts/start_selector_lifecycle_run.py scripts/guard_restore_shadow_lifecycle.py scripts/test_selector_lifecycle_run_guard.py scripts/test_guard_restore_shadow_lifecycle.py` | no output | PASS | local command |
| Unit | `python3 -m unittest scripts/test_selector_lifecycle_run_guard.py scripts/test_guard_restore_shadow_lifecycle.py -v` | 23 tests OK | PASS | test output |
| R29 artifact proof | `python3 scripts/check_selector_lifecycle_canary.py ... --scope shadow-burnin-v3-r29-all-decision-counterfactual-30-30-maxwait4000 --phase lifecycle --json` | `SELECTOR_LIFECYCLE_CANARY_PASS`, `accepted_lifecycle_plane=probe` | PASS | `manual_probe_lifecycle_canary_after_fix_20260614T1402Z/RUN_LIFECYCLE_CANARY_PROOF.json` |
| Probe reporter | same R29 canary | `rows_written=10`, `close_truth_coverage=10/10`, `artifact_plane=probe` | PASS | reporter payload |
| Bad marker guard | same R29 canary | all bad marker counts = 0 | PASS | canary JSON |
| R28 long recheck | `check_selector_lifecycle_canary.py ... r28 ...` | no `MemoryError`; fails on real `AccountNotFound_delta=39` | EXPECTED FAIL | `manual_shadow_lifecycle_canary_after_fix_20260614T1402Z/RUN_LIFECYCLE_CANARY_PROOF.json` |

Wniosek walidacyjny:
R29 lifecycle simulation proof is repaired for the all-decision/probe profile. The old false-fail condition is removed without weakening bad-marker failure behavior.

Ograniczenia walidacji:
Nie uruchamiano nowego runtime/tmux runu po poprawce. Walidacja opiera sie na realnych artefaktach R29 i R28 oraz unit tests.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: unit test
- Co zabezpiecza: probe lifecycle canary PASS bez aktywnych BUY artifacts.
- Kiedy sie aktywuje: zmiany w `validate_probe_lifecycle_canary()`.
- Jak przetestowano: `test_probe_lifecycle_canary_passes_without_active_buy_artifacts`.
- Co pozostaje poza zakresem: jakosc selekcji probe i market-label correctness.

Guardrail 2:
- Typ: unit test
- Co zabezpiecza: bad marker nadal failuje probe lifecycle.
- Kiedy sie aktywuje: `AccountNotFound` lub inny bad marker w delta.
- Jak przetestowano: `test_probe_lifecycle_canary_fails_bad_marker_delta`.
- Co pozostaje poza zakresem: klasyfikacja nowych przyszlych markerow.

Guardrail 3:
- Typ: unit test
- Co zabezpiecza: reporter shadow-plane nadal wymaga Gatekeeper BUY context, probe-plane nie.
- Kiedy sie aktywuje: `validate_reporter_rows()`.
- Jak przetestowano: `test_reporter_requires_buy_context_by_default_but_not_for_probe_plane`.
- Co pozostaje poza zakresem: reporter business outcome semantics.

Guardrail 4:
- Typ: unit test
- Co zabezpiecza: log marker counting dziala z baseline offsetu bez czytania calego loga.
- Kiedy sie aktywuje: marker scan w canary.
- Jak przetestowano: `test_appended_log_marker_count_streams_from_baseline_offset`.
- Co pozostaje poza zakresem: ekstremalnie dlugie pojedyncze linie logow.

## Otwarte ryzyka / follow-up

- R29 run zostal juz zabity przez poprzedni launcher; poprawka zostala potwierdzona offline na jego artefaktach, ale nowy run wymaga nowego scope.
- R28 pelny post-fact recheck wykazuje `AccountNotFound_delta=39`; to nie blokuje naprawy R29 false-fail, ale nie powinno byc ignorowane przy ocenie dlugiego R28.
- Probe lifecycle proof nie jest rownoznaczny z aktywnym BUY shadow proof; raporty musza czytac `accepted_lifecycle_plane`.
