# ADR-8D: R18C BCV2 Handoff Regression Guard

Status: Accepted
Typ: Regression guard / auditability hardening
Data: 2026-06-07
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `main`
Commit/PR: PR-R18C-REGRESSION-GUARD, base `73c782e`
Zakres: selector shadow/simcov route materialization guardrails only
Dotknięte moduły/pliki:
- `ghost-launcher/src/oracle_runtime.rs`
- `scripts/audit_selector_buy_simulation_coverage.py`
- `scripts/ci_assert_selector_regression_gates.py`
- `scripts/test_selector_pipeline.py`
- `tests/fixtures/selector/r18c_bcv2_handoff_regression/*`
Powiązane runy/logi/raporty:
- `shadow-burnin-v3-selector-dataset-r18c-bcv2-handoff-canonicalization-smoke`
- `reports/selector/shadow-burnin-v3-selector-dataset-r18c-bcv2-handoff-canonicalization-smoke/buy_simulation_coverage_audit_v1.json`
Poziom ryzyka: Medium, because the guarded path is Solana shadow simulation route materialization; the PR itself is test/audit only and does not change active execution.

## 1. Przygotowanie i działania wstępne

Plan początkowy:
Add a scoped regression guard after R18C: fixture, CI assert script, final handoff canonicalization test, forbidden marker gates, config guard, and ADR. Do not add features. Do not change Gatekeeper, send path, Custom(6024), provider, slippage, or success tuning.

Rzeczywisty przebieg:
The implementation was constrained to offline checks and regression assertions. A minimal fixture was added to represent the R18C successful fallback handoff state. A CI script was added to assert coverage and forbidden-marker conditions from existing audit/log artifacts. The Rust test surface was extended with an explicit stale-primary-reason non-fatal handoff regression case.

Odchylenia od planu:
The existing BUY simulation coverage audit did not expose the raw attempted row count. A small reporting-only field, `shadow_simulation_attempted_rows`, was added so the new guard does not have to infer exact counts from percentages.

## 2. Wykorzystane skills/sub-agenci

Nazwa: `ghost-execution`
Powód użycia: The task touches Ghost shadow/live separation, route materialization diagnostics, and decision/audit evidence.
Zakres użycia: Validate that the new guard remains offline and does not promote evidence into execution.
Wynik: Guard report explicitly states active execution, send path, and Gatekeeper remain unchanged.
Ograniczenia: No runtime behavior was changed by this PR.

Nazwa: `solana-pumpfun-architect`
Powód użycia: The regression target is Pump.fun legacy buy account layout after the BCV2 tail upgrade.
Zakres użycia: Keep BCV2 as protocol-derived meta-only and keep ordinary bonding curve as load-required state.
Wynik: Config guard checks that BCV2 meta-only is not accidentally applied to normal `bonding_curve`.
Ograniczenia: This does not tune slippage, provider retry, or Custom(6024) behavior.

Nazwa: `rust-master`
Powód użycia: The regression test touches Rust runtime helper code and must preserve deterministic shadow-only behavior.
Zakres użycia: Add a narrow Rust test without broad refactor or async behavior changes.
Wynik: Final fallback handoff is tested as deterministic canonicalization.
Ograniczenia: The test is unit-level, not a replacement for smoke runtime proof.

## 3. Opis problemu - 3W2H

What:
R18C fixed a route handoff bug where stale primary-route diagnostics such as `primary_route_bcv2_missing` could remain fatal after a valid final fallback manifest already contained a protocol-derived BCV2 tail.

Where:
The affected area is selector shadow/simcov route materialization around final fallback handoff diagnostics and BUY simulation coverage audit.

Why it matters:
If this regresses, Ghost can again classify a valid final legacy buy manifest as not executable, reducing BUY simulation attempt coverage and hiding a protocol-layout-valid route behind stale diagnostics.

How observed:
The R18B/R18C sequence showed rows with `bonding_curve_v2_present=true`, final fallback manifest selected, but stale primary BCV2 failure markers still blocking route executability. R18C resolved the handoff by recalculating final manifest status from the final account set.

How many / scale:
R18C smoke reached full attempt coverage for its BUY denominator. This guard prevents the specific handoff regression class from re-entering silently.

Evidence:
R18C audit artifacts showed `not_executable_route_rows=0`, successful attempted coverage for the smoke scope, and no critical legacy AccountNotFound-style markers. This ADR does not commit runtime logs or datasets.

## 4. Przyczyna źródłowa

Root cause:
The route handoff path could preserve a stale primary-route reason as fatal after selecting a final fallback route manifest.

Mechanizm błędu:
The primary candidate reason `primary_route_bcv2_missing` was not always separated from final manifest validation. A final manifest with valid BCV2 protocol tail should be evaluated through `BCV2_META_READY_BY_PROTOCOL_SCHEMA` and `BCV2_LOAD_NOT_REQUIRED`, not through primary route state.

Miejsce:
`ghost-launcher/src/oracle_runtime.rs`, final selected fallback route handoff diagnostics and validation.

Skutek:
Rows could end as `no_executable_route_account_set` or similar not-executable states despite having a valid final BCV2 tail.

Dowód:
R18C regression test covers a selected fallback route with a stale `primary_route_bcv2_missing` reason and asserts that it remains diagnostic, not fatal.

Odrzucone hipotezy:
- AccountNotFound / ephemeral payer regression: not observed in R18C.
- Custom(2006) creator vault regression: not the guarded class.
- Provider/slippage/Custom(6024) errors: explicitly out of scope for this PR.

## 5. Strategia naprawy

Przyjęta strategia:
Add regression gates, not new runtime behavior. Preserve R18C final handoff semantics by making forbidden markers executable in CI against a minimal fixture and real smoke audit artifacts.

Zakres ingerencji:
Offline audit gate, minimal fixture, exact-count coverage checks, Rust handoff regression test, ADR.

Czego nie zmieniano:
- Active execution path
- Send path
- Gatekeeper thresholds or policy
- Slippage behavior
- Provider retry behavior
- Custom(6024) / Custom(6002) taxonomy or fixes

Ryzyka:
The guard only catches logged/serialized evidence available to the script. A future runtime regression that fails to emit any handoff evidence would need an additional launcher or audit contract.

Odrzucone alternatywy:
- Strict runtime guard in launcher: rejected for this PR because the user asked for regression guard only, not new runtime feature behavior.
- Reuse Program Stream evidence to unlock execution: rejected because the path must stay shadow/simcov and can_unlock_execution must remain false.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1:
- Plik/moduł: `scripts/ci_assert_selector_regression_gates.py`
- Co zmieniono: Added an offline CI/assert script for R18C regression gates.
- Dlaczego: To fail fast on forbidden markers and exact-count coverage regressions.
- Efekt: The script reports PASS/FAIL, raw counts, forbidden marker counts, and claim boundaries.

Zmiana 2:
- Plik/moduł: `tests/fixtures/selector/r18c_bcv2_handoff_regression/*`
- Co zmieniono: Added a minimal passing R18C BCV2 fallback handoff fixture.
- Dlaczego: To make the CI gate testable without committing runtime logs/datasets.
- Efekt: The fixture captures the expected final handoff state: attempted rows equal BUY rows, not-executable rows zero, BCV2 meta-only only for `bonding_curve_v2`.

Zmiana 3:
- Plik/moduł: `scripts/test_selector_pipeline.py`
- Co zmieniono: Added positive and negative tests for the regression gate.
- Dlaczego: To ensure the script rejects stale fatal BCV2 reasons, missing route kind, BCV2 RPC precheck, normal bonding curve meta-only drift, and `can_unlock_execution=true`.
- Efekt: The guard is covered by unit tests and fixture-driven assertions.

Zmiana 4:
- Plik/moduł: `ghost-launcher/src/oracle_runtime.rs`
- Co zmieniono: Added a focused Rust regression test for stale primary BCV2 reason non-fatal final handoff.
- Dlaczego: To protect the canonicalization invariant in the code path itself.
- Efekt: The test asserts final fallback handoff keeps `primary_route_bcv2_missing` as stale diagnostic only.

Zmiana 5:
- Plik/moduł: `scripts/audit_selector_buy_simulation_coverage.py`
- Co zmieniono: Added `shadow_simulation_attempted_rows` to the metrics output.
- Dlaczego: Exact-count guards should not reconstruct attempted rows from floating-point percentages.
- Efekt: Coverage gates can report exact attempted row counts.

## 7. Walidacja działań naprawczych

| Walidacja | Komenda/run | Wynik | Status | Dowód |
|---|---|---|---|---|
| Build | `python3 -m py_compile scripts/ci_assert_selector_regression_gates.py scripts/audit_selector_buy_simulation_coverage.py scripts/test_selector_pipeline.py` | No syntax errors | PASS | Local command output |
| Unit | `python3 -m unittest ...test_selector_regression_gate_* -v` | 7 tests passed | PASS | Local command output |
| CLI guard | `python3 scripts/ci_assert_selector_regression_gates.py --scope r18c-bcv2-handoff-regression-fixture ... --json` | `status=PASS`, `attempted_rows=2`, `buy_rows=2`, `not_executable_route_rows=0` | PASS | Local command output |
| R18C artifact guard | `python3 scripts/ci_assert_selector_regression_gates.py --scope shadow-burnin-v3-selector-dataset-r18c-bcv2-handoff-canonicalization-smoke ... --json` | `status=PASS`, `attempted_rows=165`, `buy_rows=165`, `not_executable_route_rows=0`, target 95% = 157 | PASS | Local R18C artifact |
| Rust unit | `cargo test -p ghost-launcher selected_fallback_handoff --lib` | 3 tests passed, existing workspace warnings only | PASS | Local command output |
| Replay/simulation | R18C smoke scope | R18C was already used as runtime proof; this PR does not start a new run | PASS (prior evidence) | Local R18C audit artifact |
| Guard negative case | Fixture mutation tests | Missing route kind, fatal stale BCV2 reason, BCV2 RPC precheck, normal curve meta-only, `can_unlock_execution=true`, and coverage drop all failed as expected | PASS | Python unit tests |

Wniosek walidacyjny:
The guard is locally validated by unit tests, CLI execution, py_compile, and targeted Rust unit tests. R18C runtime proof is prior evidence; this PR does not start a new runtime smoke.

Ograniczenia walidacji:
This PR does not prove provider, slippage, Custom(6024), or success coverage behavior. It protects only route materialization / BCV2 handoff regression semantics.

## 8. Wdrożone zabezpieczenia antyregresyjne

Guardrail 1:
- Typ: Offline CI/assert script
- Co zabezpiecza: Forbidden R18C regression markers and coverage floor.
- Kiedy się aktywuje: When `scripts/ci_assert_selector_regression_gates.py` is run against an audit JSON and JSONL evidence.
- Jak przetestowano: Fixture positive and negative tests.
- Co pozostaje poza zakresem: It does not start runtime and cannot catch missing evidence emission unless the audit artifacts exist.

Guardrail 2:
- Typ: Minimal fixture
- Co zabezpiecza: Expected final fallback handoff shape after R18C.
- Kiedy się aktywuje: During Python regression tests.
- Jak przetestowano: The fixture must pass the guard; mutated fixture variants must fail.
- Co pozostaje poza zakresem: It is not a large replay dataset and does not include provider/slippage failures.

Guardrail 3:
- Typ: Rust unit regression test
- Co zabezpiecza: `primary_route_bcv2_missing` remains stale diagnostic only after valid final fallback handoff.
- Kiedy się aktywuje: Rust unit tests for `ghost-launcher`.
- Jak przetestowano: Test asserts selected route kind, BCV2 meta status, load status, stale reason, and empty fatal reason set.
- Co pozostaje poza zakresem: It does not simulate the Solana program.

Guardrail 4:
- Typ: Config/role guard in CI script
- Co zabezpiecza: BCV2 meta-only handling cannot drift onto ordinary `bonding_curve`.
- Kiedy się aktywuje: If JSONL evidence marks normal `bonding_curve` with `BCV2_LOAD_NOT_REQUIRED` or `BCV2_META_READY_BY_PROTOCOL_SCHEMA`.
- Jak przetestowano: Negative fixture mutation.
- Co pozostaje poza zakresem: It does not inspect every internal Rust state unless that state is serialized into evidence.

## Otwarte ryzyka / follow-up

- Add this script to the project CI or smoke runbook invocation once the team decides the exact CI profile.
- A launcher-level strict gate can be added later if the team wants runtime start/continue decisions to require this assert.
- Custom(6024), provider failures, slippage, and success tuning remain separate follow-up work and were intentionally not mixed into this guard.
