# ADR-8D: HET-PM V2 PR B — uszczelnienie kontraktu promotion evidence po focused review

Status: ACCEPTED FOR PR #73 REMEDIATION / NO AUTHORITY CHANGE

Typ: ADR-8D / promotion evidence contract / analyzer-runtime provenance

Data: 2026-07-17

Repozytorium: `/root/Gho_dynamic_exit_v1_pr2b`

Gałąź: `agent/het-pm-v2-promotion-evidence`

Uwaga o szablonie: wskazany globalnie plik `/root/Gho/docs/ADR/ADR_8D_SZABLON.md`
nie istnieje w środowisku roboczym. Dokument zachowuje lokalny układ D1-D8 używany
przez ADR-8D w repozytorium.

## 1. Problem

Focused review PR #73 wykazał, że pierwotny promotion-evidence prerequisite był
poprawnie odseparowany od authority cutoveru, ale nie domykał kontraktu dowodowego.
Najważniejsze luki:

- `validate` sprawdzało spójność artefaktu wewnętrznie, ale nie rekalkulowało go z
  run manifestów i zhashowanych źródeł;
- ekonomia candidate była liczona globalnie po pierwszym `ExitAll`, więc Crash lub
  inny niepromowany gate mógł maskować Trailing;
- calibration runy mogły wpływać na validation metrics;
- manifest nie wiązał danych z konkretnym release binary, launcherem, canary proof
  i clean shutdown;
- metryka executable continuation mierzyła recurrence późniejszych kandydatów, nie
  rzeczywistą executable quote continuation path;
- terminal correlation była globalna po `comparison_id`, bez per-position/action
  zgodności;
- pozycje cenzorowane oraz admission/reservation gap nie były trwałym denominatorem
  Gate 1/Gate 4;
- diff-scoped Clippy zgłaszał diagnostyki w zmienionym zakresie PR.

Te braki nie tworzyły dual authority, ale mogły pozwolić na fałszywie wiarygodny
artefakt promocji.

## 2. Decyzja

PR #73 pozostaje wyłącznie prerequisite’em promotion evidence. Nie wprowadza
HET-PM V2 authority cutoveru, selective runtime hierarchy ani deploy migration.

Kontrakt evidence zostaje uszczelniony przez:

1. source-recomputing `validate`, który wymaga tych samych run manifestów co
   `evaluate` i porównuje canonical bytes bitowo z dostarczonym artefaktem;
2. gate-specific artifact i gate-specific economics dla Trailing, Vitality, Crash
   oraz safety gates;
3. fail-closed odrzucenie `run_role != "validation"` w promotion evaluation;
4. obowiązkowy `launcher_proof` z release binary SHA-256, commit SHA, config hash,
   canary/static guard proof, exact invocation i controlled shutdown result;
5. jawne `analysis_dependency_hashes` dla promotion toola i analyzera PR A;
6. realną executable continuation coverage z późniejszych executable return rows,
   a nie z późniejszego candidate recurrence;
7. exact terminal correlation po `run_id`, `position_id`, `epoch`,
   `writer_instance_id`, `source_snapshot_id` i `action_id`;
8. trwałe artefakty `position_censored_v1.jsonl` oraz admission reconciliation;
9. lokalne poprawki diff-scoped Clippy w zmienionym teście coordination metrics.

## 3. D1 — Dane i źródła prawdy

Promotion analyzer wymaga teraz następujących klas wejść:

- `brain_config`;
- `run_config`;
- `launcher_proof`;
- `comparison`;
- `writer_health`;
- `lifecycle`;
- `exit_replay`;
- `position_events`;
- `position_censored`;
- `admission`;
- `gatekeeper_buys`;
- `runtime_log`.

Jednostką ekonomicznego joinu pozostaje:

```text
(run_id, position_id, position_epoch)
```

Terminal join używa dodatkowo action/correlation identity. Admission join używa
identity handoffu i musi rozliczyć przejście:

```text
post_buy_submitted
→ handoff_accepted | handoff_rejected
→ monitoring_registered | typed_no_het_release | rejection_release
→ terminal_release | shutdown_release
```

Pozycje usunięte administracyjnie podczas controlled horizon są zapisywane jako
censored evidence, a nie znikają z denominatora.

## 4. D2 — Determinizm

`evaluate` tworzy canonical artifact z manifestów i kryteriów. `validate` nie jest
już artifact-only autoryzacją. W trybie PR B musi:

1. zweryfikować manifesty i hashe inputów;
2. ponownie wykonać evaluation;
3. wygenerować canonical JSON;
4. porównać bytes dostarczonego artefaktu z bytes rekalkulowanymi.

Artifact-only check pozostaje wyłącznie jako `validate-structure` i nie spełnia
promotion gate.

Artefakt utrwala:

- `gate_eligibility`;
- `analysis_dependency_hashes`;
- listę validation run IDs;
- listę launch cohort IDs;
- source input manifests;
- observed metrics i checks wynikające z rekalkulacji.

## 5. D3 — Coverage, gates i economics

Globalny pierwszy `ExitAll` nie jest już reprezentatywną ekonomią promocji.
Analyzer wybiera pierwszy candidate per gate/reason per position. Niepromowany
Crash może pozostać kontrolą diagnostyczną, ale nie może spełnić sample minimum
Trailing ani usunąć późniejszego Trailinga z gate-specific analysis.

Kryteria zawierają osobne minima:

- `min_executable_trailing_candidate_positions`;
- `min_executable_trailing_matched_positions`;
- `min_vitality_candidate_positions`;
- `min_vitality_matched_positions`.

`counterfactual_executable_path_coverage` nie jest już gate metric. Zastępują ją:

- `candidate_executable_continuation_coverage`;
- `max_later_executable_upside_bps`;
- `max_later_executable_downside_bps`;
- `route_availability_after_candidate`.

Gate 4 failuje przy `candidate_bearing_censored_count > 0`, chyba że w przyszłym,
oddzielnie zamrożonym kontrakcie zostanie dodana formalna right-censor semantics.

## 6. D4 — Runtime i shutdown

Runtime nie zmienia V1 ownership. Dodane artefakty są observer/reconciliation
evidence:

- post-buy admission JSONL rozlicza handoff, monitoring registration i release;
- shadow terminal watcher emituje `terminal_release`;
- rejected handoff zwalnia lokalny slot przez typed `rejection_release`;
- accepted handoff bez terminal receivera zwalnia slot przez typed
  `typed_no_het_release`;
- administrative shutdown zapisuje `position_censored_v1.jsonl` z ostatnim znanym
  HET comparison/candidate identity.

Administracyjne cenzorowanie nadal nie tworzy fillu, PnL ani terminal disposition.

## 7. D5 — Degradacja i fail-closed

Promotion evaluation failuje, gdy:

- source manifests są nieobecne;
- artifact-only structure check jest mylony z validation;
- run ma `run_role != "validation"`;
- launcher proof nie zgadza się z configami, runtime logiem, SHA binary albo commit
  identity;
- Crash próbuje spełnić Trailing evidence;
- terminal ma tylko globalnie istniejący `comparison_id`, ale nie zgadza się per
  run/position/action/snapshot;
- istnieje candidate-bearing censored position;
- admission flow nie ma final/release reconciliation;
- required executable continuation coverage jest nieznany lub niewystarczający.

Brak danych jest klasyfikowany jako brak evidence, nie jako zero-loss ani Hold.

## 8. D6 — Deployment i rollback

Zmiany są ograniczone do shadow/evidence prerequisite. Rollback oznacza powrót do
poprzedniego promotion-evidence branch state albo do zaakceptowanego PR A. Nie ma
migracji live state i nie ma zmiany V1 terminal/apply/capacity authority.

Run `r2a` uruchomiony przed tą korektą nie spełnia finalnego, prospective,
pre-registered contractu. Może zostać użyty diagnostycznie/calibration, ale po
zamrożeniu poprawionego kontraktu wymagane są nowe validation runy.

## 9. D7 — Testy i dowody

Dodane lub rozszerzone testy obejmują:

- source-recomputing validate i artifact-only `validate-structure`;
- fałszywie spójny artefakt bez manifestów;
- zero Trailing candidates;
- Crash candidates próbujące spełnić Trailing minimum;
- wcześniejszy Crash nieusuwający późniejszego Trailinga;
- calibration manifest odrzucony przez promotion evaluation;
- exact terminal correlation mismatch;
- executable continuation odróżnione od later candidate recurrence;
- candidate-bearing censor denominator;
- admission reconciliation;
- administrative shutdown censor artifact;
- terminal watcher admission release artifact.

Weryfikacja lokalna wykonana dla tej korekty:

```text
python3 scripts/test_het_pm_v2_promotion_gate_v1.py
python3 scripts/test_selector_lifecycle_run_guard.py
python3 -m py_compile scripts/het_pm_v2_promotion_gate_v1.py scripts/start_selector_lifecycle_run.py scripts/test_het_pm_v2_promotion_gate_v1.py scripts/test_selector_lifecycle_run_guard.py
cargo test -p ghost-launcher components::post_buy_runtime::tests::shadow_terminal_watcher_writes_admission_terminal_release --lib
cargo test -p ghost-brain guardian::post_buy::engine::tests::administrative_shutdown_removes_all_positions_without_terminal_disposition --lib
cargo test -p ghost-core --test coordination_metrics_phase06
python3 scripts/guard_diff_scoped_clippy.py --base origin/main --head HEAD
git diff --check
```

## 10. D8 — Acceptance

Ta remediation jest kompletna, gdy:

- source-recomputing validation zastępuje artifact-only promotion authorization;
- gate-specific promotion nie może być spełnione przez niepromowany gate;
- calibration runs nie wpływają na validation Gate 1-5;
- launcher/binary/source provenance jest wymagane i hashowane;
- executable continuation mierzy quote path, nie candidate recurrence;
- terminal correlation jest exact per identity;
- censored/admission denominators są durable;
- diff-scoped Clippy i narrow runtime/analyzer tests są zielone.

Po spełnieniu powyższego można uruchomić nowe prospective validation runy bez
dalszego dostrajania progów. Authority cutover pozostaje poza tym PR.
