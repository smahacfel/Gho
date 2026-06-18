# ADR-8D: Shadow probe legacy fallback creator-vault authority guard

Status: WIP / implemented locally, runtime proof pending
Typ: Correctness fix / shadow probe anti-regression guard
Data: 2026-06-18
Autor/Agent: Codex
Repo/branch: /root/Gho / codex/gatekeeper-edge-policy-redesign-r1
Commit/PR: local changes on top of 4a4e6e4; no commit / no PR
Zakres: Counterfactual shadow probe route handoff for selected legacy-buy fallback
Dotkniete moduly/pliki:
- ghost-launcher/src/oracle_runtime.rs
- docs/ADR/ADR_8D_SHADOW_PROBE_LEGACY_FALLBACK_CREATOR_VAULT_AUTHORITY_GUARD_20260618.md
Powiazane runy/logi/raporty:
- R34 scope: shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_transport.jsonl
- logs/shadow_run/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/probe_skips.jsonl
- logs/rollout/shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1/system.log
Poziom ryzyka: Medium

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zdiagnozowac regresje coverage shadow simulation od strony NLN Program Streams, a jezeli NLN nie jest glowna przyczyna, rozbic `probe_transport` i `shadow_entries` na konkretne klasy bledow.

Rzeczywisty przebieg:
R34 pokazal, ze NLN Program Streams nie sa w pelni zdrowe: `solana.pump_fun.buy` zakonczyl sie `Subscribe request failed`, a `solana.pump_fun.buy_exact_sol_in` dostal first message. Jednoczesnie klasa bledow w `probe_transport.jsonl` byla inna i lokalna dla shadow simulation: `account_pda_constraint_error` z `creator_vault_source_not_authoritative` oraz `actual_expected_mismatch` na `legacy_buy` po `selected_legacy_buy_final_manifest_validated`.

Odchylenia od planu:
Naprawa nie dotyka NLN streams, bo nie byly glownym mechanizmem tej konkretnej regresji probe coverage. NLN pozostaje osobnym problemem ingest coverage.

## 2. Wykorzystane skills/sub-agenci

Nazwa: ghost-execution
Powod uzycia: Zmiana dotyka shadow-only probe lifecycle, DecisionLogger/probe artifacts i granicy shadow/live.
Zakres uzycia: Ograniczenie naprawy do `counterfactual_shadow_probe` route handoff bez zmiany Gatekeeper policy, scoringu, execution/send path ani live behavior.
Wynik: Zachowano shadow-only separation i fail-closed semantics.
Ograniczenia: Skill nie potwierdza zewnetrznej zdrowotnosci NLN.

Nazwa: solana-pumpfun-architect
Powod uzycia: Blad byl zwiazany z kontami pump.fun legacy-buy i seed/creator-vault authority.
Zakres uzycia: Rozdzielenie nieautorytatywnego `detected_pool.creator` od autorytatywnego `creator_vault` przy fallbacku legacy-buy.
Wynik: Fallback nie powinien juz trafic do symulacji jako executable, gdy nie ma autorytatywnego creator-vault.
Ograniczenia: Nie usuwa realnych `quote_slippage_error` ani zewnetrznych provider errors.

Nazwa: rust-master
Powod uzycia: Zmiana jest w Rust runtime hot path i wymaga minimalnego, deterministycznego guardu.
Zakres uzycia: Dodano waski warunek i dwa testy jednostkowe zamiast refaktoru route resolvera.
Wynik: Targeted tests przechodza.
Ograniczenia: Runtime proof wymaga rebuilt binary i fresh/restarted run.

## 3. Opis problemu - 3W2H

What:
Counterfactual shadow probe oznaczal czesc legacy-buy fallbackow jako executable i probowal symulacji mimo braku autorytatywnego `creator_vault`.

Where:
`ghost-launcher/src/oracle_runtime.rs`, funkcja `p37_selected_legacy_buy_fallback_overrides()` oraz downstream `probe_transport.jsonl`.

Why it matters:
Takie rekordy nie powinny zuzywac proby symulacji i obnizac `counterfactual_shadow_probe_simulated` coverage. Powinny byc fail-closed jako niegotowe konto wykonawcze albo pozostac w skip/precheck class.

How observed:
R34 `probe_transport.jsonl` mial probe simulation errors:
- `account_pda_constraint_error`
- `creator_vault_source_not_authoritative`
- `creator_vault_mismatch_reason=actual_expected_mismatch`
- `buy_variant=legacy_buy`
- `selected_route_handoff_reason=selected_legacy_buy_final_manifest_validated`

How many / scale:
W biezacym R34 snapshot: 8 z 12 probe simulation errors to ta klasa. Probe simulated coverage wynosilo okolo `118 / 131 = 90.08%`, ponizej progu 92%.

Evidence:
- `probe_transport.jsonl`: 8 x `account_pda_constraint_error` z `creator_vault_source_not_authoritative`.
- `probe_skips.jsonl`: duza liczba rows juz klasyfikowana jako `creator_vault_source_not_authoritative`, co pokazuje, ze precheck zna te klase, ale selected fallback omijal ja dla czesci rekordow.
- NLN logs: `solana.pump_fun.buy` padl, ale probe failure class byla lokalna dla simulation account authority, nie dla NLN message ingest.

## 4. Przyczyna zrodlowa

Root cause:
Selected legacy-buy fallback dziedziczyl `creator_pubkey_source=detected_pool.creator` i nie wymuszal downgrade authority, gdy nie bylo autorytatywnego `creator_vault`.

Mechanizm bledu:
Fallback przechodzil jako `selected_legacy_buy_final_manifest_validated`, mimo ze creator identity nie wystarczala do poprawnego legacy-buy account setu. Symulacja Anchor odrzucala transakcje jako PDA/seed mismatch (`Custom(2006)`), a coverage spadal jako `counterfactual_shadow_probe_simulation_error`.

Miejsce:
`p37_selected_legacy_buy_fallback_overrides()`.

Skutek:
Niewykonalne legacy fallbacki trafialy do simulation transport zamiast fail-closed przed symulacja.

Dowod:
R34 `probe_transport.jsonl` laczy `selected_route_handoff_applied`, `selected_legacy_buy_final_manifest_validated`, `legacy_buy`, `creator_vault_source_not_authoritative` i `actual_expected_mismatch`.

Odrzucone hipotezy:
- NLN jest glowna przyczyna probe simulation drop: odrzucone dla tej klasy, bo blad jest w route/account authority przy simulation.
- FSC disabled jest przyczyna: odrzucone, R34 jest `fsc_off`, a `funding_status=unavailable` jest oczekiwane.
- Gatekeeper policy/progi sa przyczyna: odrzucone, blad wystepuje po decyzji, w counterfactual shadow probe route handoff.

## 5. Strategia naprawy

Przyjeta strategia:
W fallbacku legacy-buy, jezeli nie ma autorytatywnego `creator_vault`, a creator pochodzi z `detected_pool.creator`, oznaczyc `creator_pubkey_authoritative=false`. Dalej istniejacy contract checker ma fail-closed z `creator_vault_source_not_authoritative` zamiast wysylac taki request do symulacji.

Zakres ingerencji:
Jedna funkcja fallbacku i dwa testy jednostkowe.

Czego nie zmieniano:
- Gatekeeper policy
- decision thresholds
- scoring
- live execution
- send path
- Solana tx builder semantics
- DecisionLogger schema
- NLN stream config

Ryzyka:
Coverage liczony wzgledem `probe_selection` moze dalej wygladac nizej, jezeli metric traktuje precheck skip jako brak symulacji. To jest semantycznie poprawniejsze niz symulowanie niewykonalnego account setu, ale raport coverage musi rozdzielac: selected, transported, simulated, precheck_not_executable.

Odrzucone alternatywy:
- Podbijanie slippage albo ignorowanie Anchor error: odrzucone, bo maskowaloby realna niewykonalnosc.
- Uznanie `detected_pool.creator` za autorytatywne: odrzucone, bo dowod runtime pokazal actual/expected mismatch.
- Zmiana Gatekeepera albo probe sampling: poza zakresem tej regresji.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: W `p37_selected_legacy_buy_fallback_overrides()` dodano downgrade `creator_pubkey_authoritative=false`, gdy fallback nie ma autorytatywnego `creator_vault`, a creator source to `detected_pool.creator`.
- Dlaczego: Taki fallback nie powinien byc traktowany jako executable legacy-buy account set.
- Efekt: Contract checker moze fail-closed przed symulacja z `creator_vault_source_not_authoritative`.

Zmiana 2:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: Dodano test `selected_legacy_buy_fallback_downgrades_detected_pool_creator_without_creator_vault`.
- Dlaczego: Chroni przed regresja, w ktorej `detected_pool.creator` bez creator-vault przechodzi jako executable.
- Efekt: Test wymaga fail-closed reason zaczynajacego sie od `creator_vault_source_not_authoritative:legacy_buy:`.

Zmiana 3:
- Plik/modul: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: Dodano test `selected_legacy_buy_fallback_keeps_authoritative_creator_vault_executable`.
- Dlaczego: Guard nie moze zepsuc poprawnej sciezki z autorytatywnym `creator_vault`.
- Efekt: Autorytatywny creator-vault nadal przechodzi bez contract failure.

## 7. Walidacja dzialan naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowod |
|---|---|---|---|---|
| NLN check | `grep -R "NLN Program Streams..." logs/rollout/.../system.log*` | `solana.pump_fun.buy` failed; `buy_exact_sol_in` first message received | WARN | R34 system.log lines 163, 312, 313, 799 |
| Coverage before runtime proof | `python3 scripts/runtime_probe_coverage.py --root /root/Gho --scope shadow-burnin-v3-r34-maxwait2999-target50-stop50-fsc-off-r1 --json` | BUY simulated 382/411 = 92.94%; probe simulated 118/131 = 90.08% | FAIL for probe | R34 current artifacts, old binary |
| Unit guard | `cargo test -p ghost-launcher selected_legacy_buy_fallback -- --nocapture` | 4 fallback tests passed | PASS | `selected_legacy_buy_fallback_*` tests ok |
| Formatting | `rustfmt --edition 2021 ghost-launcher/src/oracle_runtime.rs` | no formatting error | PASS | Command completed |
| Runtime proof | Fresh/restarted R34 on rebuilt binary | Not yet run | PENDING | Current R34 uses old binary |

Wniosek walidacyjny:
Kodowy root cause dla dominujacej klasy probe simulation drop zostal zidentyfikowany i unit-tested. Aktualny R34 nie jest jeszcze dowodem poprawy, bo dziala na binarce sprzed zmiany.

Ograniczenia walidacji:
Nie wykonano jeszcze release build i fresh smoke/proof run na poprawionej binarce. NLN `solana.pump_fun.buy` nadal wymaga osobnej diagnozy, ale nie jest glowna przyczyna tej klasy simulation failure.

## 8. Wdrozone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Runtime account-authority guard
- Co zabezpiecza: Legacy fallback bez autorytatywnego creator-vault nie trafia do symulacji jako executable.
- Kiedy sie aktywuje: Przy selected legacy-buy fallback handoff.
- Jak przetestowano: `selected_legacy_buy_fallback_downgrades_detected_pool_creator_without_creator_vault`.
- Co pozostaje poza zakresem: Realne slippage failures i provider/RPC errors.

Guardrail 2:
- Typ: Positive-path guard
- Co zabezpiecza: Poprawny legacy fallback z autorytatywnym creator-vault nadal jest executable.
- Kiedy sie aktywuje: Gdy `creator_vault_authoritative=true` i creator-vault jest obecny.
- Jak przetestowano: `selected_legacy_buy_fallback_keeps_authoritative_creator_vault_executable`.
- Co pozostaje poza zakresem: Runtime stream health.

## Otwarte ryzyka / follow-up

- Przebudowac release binary i uruchomic fresh/restarted smoke run, aby potwierdzic probe coverage na realnych artefaktach.
- Utrzymac osobna metryke coverage: selected -> transported -> simulated -> lifecycle, z rozbiciem precheck_not_executable vs simulation_error.
- Dodac przyszly guard smoke/CI: `account_pda_constraint_error + creator_vault_source_not_authoritative` w `probe_transport` musi byc 0 albo ponizej jawnego progu.
- Osobno ustalic, dlaczego NLN `solana.pump_fun.buy` konczy sie `Subscribe request failed`.
