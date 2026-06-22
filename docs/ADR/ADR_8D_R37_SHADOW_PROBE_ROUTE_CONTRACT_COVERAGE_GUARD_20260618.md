# ADR-8D: R37 shadow-probe route contract coverage guard

Status: IMPLEMENTED / TARGETED_TESTS_PASS / RUNTIME_ARTIFACT_DIAGNOSIS_COMPLETE
Typ: ADR-8D / shadow-probe runtime guard / audit tooling
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: not committed at report time
Zakres: P37 counterfactual shadow-probe account-route prechecks and simulation coverage audit guard
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- `docs/ADR/ADR_8D_R37_SHADOW_PROBE_ROUTE_CONTRACT_COVERAGE_GUARD_20260618.md`

Powiazane runy/logi/raporty:
- R37 config:
  `configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml`
- R37 probe selection:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_selection.jsonl`
- R37 probe skips:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_skips.jsonl`
- R37 probe transport:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_transport.jsonl`
- R37 probe entries:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_entries.jsonl`
- R37 probe lifecycle:
  `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/probe_shadow_lifecycle.jsonl`
- Local diagnostic output:
  `/tmp/r37_p37_audit_after_patch_current_artifacts.json`
  `/tmp/r37_p37_audit_after_patch_current_artifacts.md`

Uwaga o szablonie:
Literalna sciezka `docs/ADR/ADR_8D_SZABLON.md` nie istnieje w tym checkoutcie. Dokument zachowuje lokalny format ADR-8D uzywany w repo i sekcje wymagane przez uzytkownika.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Ustalic, dlaczego R37 shadow-probe simulation coverage raportuje wartosci okolo 85-88%, ponizej minimalnego oczekiwania, oraz czy bardzo wysoki `probe_skipped` jest naturalnym efektem precheckow, czy bledem po naszej stronie.

Rzeczywisty przebieg:
- Sprawdzono aktywny R37 runtime i artefakty `probe_*`.
- Policzono unikalne decyzje, selected probes, skips, transport, entries, lifecycle closed i inflight.
- Rozbito `probe_skips` wedlug `precheck_failure_reason`.
- Rozbito `probe_transport` wedlug `execution_outcome`, `error_class`, `simulation_error_custom_code` i statusu creator-vault.
- Potwierdzono, ze surowy lifecycle coverage byl zanizany przez pozycje jeszcze w locie.
- Potwierdzono realna regresje: `Custom(2006)` z `creator_vault_source_not_authoritative` trafial do transport/simulation zamiast zostac sklasyfikowany jako fail-closed precheck skip.

Odchylenia od planu:
Po diagnozie wprowadzono minimalna naprawe runtime shadow-probe path oraz guard w skrypcie audytowym. Nie restartowano R37 w ramach tej zmiany; aktywny proces nadal pracuje na starej binarce do czasu osobnego rebuild/restartu.

## 2. Wykorzystane skills/sub-agenci

Nazwa:
`ghost-execution`

Powod uzycia:
Zmiana dotyka shadow/live separation, DecisionLogger/probe artifacts oraz fail-closed account-route contract dla P37 counterfactual probe.

Zakres uzycia:
Klasyfikacja aktywnej sciezki jako shadow-only probe, ochrona Gatekeeper policy i send path przed przypadkowa zmiana.

Wynik:
Naprawa zostala ograniczona do P37 shadow-probe precheckow i audytu artefaktow. Gatekeeper policy, execution policy i send path nie zostaly zmienione.

Ograniczenia:
Skill nie zastapil runtime reproofu po rebuildzie. Nowe artefakty musza zostac potwierdzone po restarcie runu na nowej binarce.

Nazwa:
`solana-pumpfun-architect`

Powod uzycia:
Problem dotyczy bezpiecznej symulacji transakcji pump.fun i rozroznienia miedzy route-ready account set a nieautorytatywnym creator-vault.

Zakres uzycia:
Ocena, czy brak pewnego route/account setu powinien byc symulowany, czy fail-closed przed transportem.

Wynik:
Nieautorytatywne konto creator-vault oraz niekompletny route contract sa teraz pre-simulation skip, nie transport failure.

Ograniczenia:
Nie zmieniano samego TX buildera ani semantyki on-chain simulation.

Nazwa:
`rust-master`

Powod uzycia:
Zmiana dotyka asynchronicznego runtime path i musi nie wprowadzac blokad, retry loopow ani zmiany ownership boundary.

Zakres uzycia:
Waski patch w istniejacych funkcjach precheck/dispatch; brak nowych zadan, kolejek lub blokujacych operacji.

Wynik:
Zachowano fail-closed return path i istniejacy zapis JSONL artefaktow.

Ograniczenia:
Nie przeprowadzono pelnego test suite repo; wykonano targeted tests.

## 3. Opis problemu - 3W2H

What:
R37 raportowal shadow-probe simulation coverage okolo 85-88%, a `probe_skipped` dominowal liczebnie nad decyzjami.

Where:
P37 counterfactual shadow probe:
- selection/skips/transport/lifecycle artifacts w `logs/shadow_run/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1/`
- runtime precheck/dispatch path w `ghost-launcher/src/oracle_runtime.rs`
- audit script `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`

Why it matters:
Probe ma sluzyc do symulacji wszystkich decyzji, w tym REJECT/TIMEOUT. Jezeli niepewne account routes sa wysylane do symulacji, transport errors zanieczyszczaja coverage i maskuja prawdziwa przyczyne dropu.

How observed:
Na aktualnych R37 artefaktach audyt wykazal:
- `selected_simulation_coverage_excluding_inflight = 0.926829`
- `custom_2006_creator_vault_source_not_authoritative_rows = 74`
- `simulation_coverage_guard_status = fail`
- dominujace skip reasons: `creator_vault_source_not_authoritative`, `missing_execution_route_identity`, `no_executable_route_account_set`

How many / scale:
W lokalnym audycie R37:
- `probe_selected_unique_rows = 577`
- `probe_transport_unique_rows = 541`
- `probe_entry_unique_rows = 541`
- `probe_lifecycle_closed_unique_rows = 456`
- `probe_lifecycle_inflight_unique_rows = 85`
- `probe_simulated_ok_unique_rows = 462`
- `selected_simulation_denominator_excluding_inflight = 492`

Evidence:
`/tmp/r37_p37_audit_after_patch_current_artifacts.json`, sekcja `probe_entry_materialization`.

## 4. Przyczyna zrodlowa

Root cause:
Dwie rozne rzeczy byly mieszane w jednej metryce:
1. pozycje jeszcze w locie byly liczone jak brak lifecycle/simulation success;
2. fail-closed account-route failures, szczegolnie `creator_vault_source_not_authoritative`, mogly dojsc az do transport/simulation i wybuchnac jako `Custom(2006)`.

Mechanizm bledu:
P37 probe potrafil zbudowac selected route na podstawie niepelnego albo nieautorytatywnego creator/route contextu. Czesc takich przypadkow nie byla wycinana przed dispatch. Final manifest failure byl rowniez zapisywany jako `probe_transport` error zamiast `probe_skip`.

Miejsce:
- `p37_shadow_probe_derive_account_override_context_for_pool_with_mode`
- `maybe_handle_p37_shadow_probe_decision`
- `run_p37_shadow_probe_dispatch`
- `probe_entry_materialization`

Skutek:
Transport/simulation coverage byl zanizany przez blad, ktory powinien byc jawnie sklasyfikowany jako pre-sim skip. Operator widzial `Custom(2006)` jako simulation failure zamiast route/account readiness failure.

Dowod:
R37 audit:
- `custom_2006_creator_vault_source_not_authoritative_rows = 74`
- `simulation_coverage_guard_reasons = ["custom_2006_creator_vault_source_not_authoritative_gt_0"]`
- coverage po odjeciu inflight: `92.6829%`

Odrzucone hipotezy:
- Sampling jako glowna przyczyna skipow: odrzucone, R37 ma `sample_modulus=1` i `sample_threshold=1`.
- Lifecycle coverage jako czysty simulation failure: odrzucone, znaczna czesc roznicy to `probe_lifecycle_inflight_unique_rows`.
- Gatekeeper policy jako przyczyna: odrzucone dla tej naprawy; probe-skips wynikaja z route/account readiness, nie z decyzji BUY/REJECT.

## 5. Strategia naprawy

Przyjeta strategia:
Przesunac niepewne route/account przypadki do fail-closed `probe_skips` przed symulacja oraz dodac audit guard, ktory failuje, jezeli `Custom(2006)` z `creator_vault_source_not_authoritative` ponownie pojawi sie w transport errors.

Zakres ingerencji:
- P37 shadow-probe precheck path.
- P37 dispatch final manifest failure classification.
- P37 audit metrics and tests.

Czego nie zmieniano:
- Gatekeeper policy.
- Strict threshold logic.
- BUY/REJECT/TIMEOUT verdict semantics.
- Trigger TX builder.
- Live execution.
- Send path.
- Shadow lifecycle target/stop logic.

Ryzyka:
- Wiecej rows moze trafiać do `probe_skips`, ale to jest oczekiwane fail-closed zachowanie przy braku decision-time-safe account route.
- Aktywny R37 run wymaga rebuild/restartu, zeby nowe artefakty odzwierciedlaly patch.

Odrzucone alternatywy:
- Obnizenie coverage threshold: odrzucone, maskowaloby realny `Custom(2006)` bug.
- Symulowanie mimo nieautorytatywnego creator-vault: odrzucone, to generuje fałszywe transport failures i nie jest decision-time safe.
- Zmiana Gatekeepera lub progow: poza zakresem.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: po materializacji account overrides dodano bezpieczna probę autoryzacji `detected_pool.creator` przez `p37_restore_legacy_buy_authorize_detected_pool_creator`, a potem ponowny route-contract marking.
- Dlaczego: legacy buy moze korzystac z detected creator tylko wtedy, gdy istnieje kompletny observed legacy account shape.
- Efekt: kompletne legacy routes nie sa niepotrzebnie degradowane, ale telemetry-only/incomplete creator nadal pozostaje fail-closed.

Zmiana 2:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: dodano `p37_shadow_probe_route_contract_precheck_failure`.
- Dlaczego: brak executable route account set musi byc pre-simulation skip, nie simulation failure.
- Efekt: route/account readiness failure dostaje jawny powod `no_executable_route_account_set:*`.

Zmiana 3:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: w `maybe_handle_p37_shadow_probe_decision` oraz `run_p37_shadow_probe_dispatch` dodano route-contract precheck.
- Dlaczego: blad trzeba lapac zarowno przed selected dispatch, jak i po final manifest validation.
- Efekt: niepoprawne route manifests sa zapisywane do `probe_skips`.

Zmiana 4:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: final selected route manifest failure nie zapisuje juz `probe_transport` error, tylko `probe_skips`.
- Dlaczego: to nie jest realna proba symulacji; to precondition failure.
- Efekt: transport coverage nie jest zanieczyszczany precheck failures.

Zmiana 5:
- Plik/modul: `scripts/v3_p37_mfs_lifecycle_join_key_audit.py`
- Co zmieniono: dodano unique probe counters, inflight-aware coverage denominator, `custom_2006_creator_vault_source_not_authoritative_rows` i `simulation_coverage_guard_status`.
- Dlaczego: coverage musi rozrozniać in-flight lifecycle od simulation failure oraz failowac na regresji `Custom(2006)`.
- Efekt: raport wskazuje realna przyczyne dropu zamiast jednego zbiorczego procentu.

Zmiana 6:
- Plik/modul: `scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py`
- Co zmieniono: dodano asercje dla PASS coverage guard oraz FAIL przy `Custom(2006)` + `creator_vault_source_not_authoritative`.
- Dlaczego: zabezpieczenie przed powrotem tej regresji w audycie.
- Efekt: unit tests obejmuja krytyczny negative case.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| Python compile | `python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py` | syntax OK | PASS | command exit 0 |
| Unit | `python3 -m unittest scripts.test_v3_p37_mfs_lifecycle_join_key_audit -v` | 41 tests OK | PASS | command exit 0 |
| Rust targeted | `cargo test -p ghost-launcher p37_shadow_probe_creator_vault -- --nocapture` | 3 tests OK | PASS | command exit 0 |
| Rust targeted | `cargo test -p ghost-launcher legacy_buy_non_authoritative_creator_is_not_executable -- --nocapture` | 1 test OK | PASS | command exit 0 |
| Rust targeted | `cargo test -p ghost-launcher restore_legacy_buy_detected_pool_creator_recovery_requires_complete_observed_shape -- --nocapture` | 1 test OK | PASS | command exit 0 |
| Runtime artifact audit | `python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py --config configs/rollout/shadow-burnin-v3-r37-threshold-probe-target50-stop50-fsc-off-r1.toml --output-json /tmp/r37_p37_audit_after_patch_current_artifacts.json --output-md /tmp/r37_p37_audit_after_patch_current_artifacts.md` | completed, guard FAIL on old artifacts due `Custom(2006)` | PASS as diagnosis | `/tmp/r37_p37_audit_after_patch_current_artifacts.json` |

Wniosek walidacyjny:
Kodowo naprawiono klasyfikacje route/account failures przed symulacja i dodano guard, ktory odroznia realny simulation coverage od lifecycle in-flight. Aktualne stare artefakty R37 nadal pokazuja `Custom(2006)`, co potwierdza diagnoze i bedzie wymagalo rebuild/restartu runu do runtime reproofu.

Ograniczenia walidacji:
- Nie wykonano pelnego test suite.
- Nie zrestartowano aktywnego R37 procesu na nowej binarce.
- `probe_skipped` pozostanie wysoki, jezeli dla REJECT/TIMEOUT brakuje authoritative execution route evidence. To jest oczekiwane fail-closed zachowanie, ale wymaga osobnej pracy nad coverage route evidence, jezeli celem jest wyzszy procent symulowanych rejectow.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: runtime fail-closed precheck
- Co zabezpiecza: nieautorytatywne lub niekompletne route/account sety nie trafiaja do simulation transport.
- Kiedy sie aktywuje: przed P37 probe dispatch i po final selected route manifest validation.
- Jak przetestowano: targeted Rust tests dla non-authoritative creator i complete observed legacy shape.
- Co pozostaje poza zakresem: poprawa samej dostepnosci authoritative route evidence dla odrzuconych tokenow.

Guardrail 2:
- Typ: audit metric guard
- Co zabezpiecza: powrot `Custom(2006)` z `creator_vault_source_not_authoritative` w transport errors.
- Kiedy sie aktywuje: przy uruchomieniu `v3_p37_mfs_lifecycle_join_key_audit.py`.
- Jak przetestowano: Python unit test dla negative case.
- Co pozostaje poza zakresem: automatyczne zatrzymanie runtime; guard jest raportowy.

Guardrail 3:
- Typ: inflight-aware denominator
- Co zabezpiecza: nie liczy jeszcze otwartych lifecycle pozycji jako failure coverage.
- Kiedy sie aktywuje: w sekcji `probe_entry_materialization`.
- Jak przetestowano: Python unit test coverage guard.
- Co pozostaje poza zakresem: metryki biznesowego outcome po zamknieciu pozycji.

## Otwarte ryzyka / follow-up

- Rebuild i restart R37/R38 na nowej binarce, a potem ponownie uruchomic P37 audit.
- Acceptance po reproofie:
  - `custom_2006_creator_vault_source_not_authoritative_rows = 0`
  - `selected_simulation_coverage_excluding_inflight >= 0.90` lub wyzszy operator-defined threshold
  - `probe_skips` rozbite glownie na jawne route/account evidence gaps, nie transport errors.
- Jezeli `probe_skipped` nadal jest operacyjnie zbyt wysokie, osobny PR powinien poprawic materializacje authoritative execution route evidence dla REJECT/TIMEOUT, bez rozluzniania fail-closed simulation contract.
