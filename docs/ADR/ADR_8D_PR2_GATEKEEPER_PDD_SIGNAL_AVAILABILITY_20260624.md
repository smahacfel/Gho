# ADR-8D: PR2 Gatekeeper PDD signal availability status

Status: IMPLEMENTED / TARGETED_VALIDATION_COMPLETED_WITH_EXTERNAL_BASELINE_FAILURE
Typ: ADR-8D / gatekeeper PDD availability semantics and JSONL compatibility
Data: 2026-06-24
Autor/Agent: Codex
Repo/branch: `/root/Gho`, `backup/pre-refactor-evidence-contract-20260619`
HEAD podczas pracy: `6ad066c`
Commit/PR: local working tree, not committed at ADR creation time
Zakres: implementacja PR2 z `PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`; lokalny PDD-only model availability dla spike/ramping/flash bez globalnego `Signal<T>`
Poziom ryzyka: MEDIUM

Dotkniete moduly/pliki:
- `ghost-launcher/src/components/gatekeeper_pdd_sequence.rs`
- `ghost-launcher/src/components/gatekeeper_pdd.rs`
- `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/tests/gatekeeper_v25_regression.rs`

Powiazane plany:
- `PLANS/PLAN_GATEKEEPER_V2_POLICY_SSOT_AND_AVAILABILITY_20260624.md`

Uwaga o szablonie:
Literalna sciezka z globalnej instrukcji, `docs/ADR/ADR_8D_SZABLON.md`, nie istnieje w tym checkoutcie. Ten dokument zachowuje lokalny format ADR-8D zastosowany w `ADR_8D_PR1_GATEKEEPER_HARD_FAIL_SSOT_PARITY_20260624.md`.

## 1. Przygotowanie i dzialania wstepne

Plan poczatkowy:
Zrealizowac PR2 z planu Gatekeeper policy SSOT and availability, czyli usunac niebezpieczne `unknown as false` dla PDD sequence signals bez przebudowy calego modelu typow i bez naruszania kompatybilnych bool fields w logach oraz replay payloads.

Rzeczywisty przebieg:
- Potwierdzono, ze PR2 dotyka lokalnego kontraktu PDD sequence diagnostics, APS regime detection oraz buy-log JSONL.
- Zachowano stary bool surface:
  - `spike_detected`,
  - `ramping_detected`,
  - `flash_crash_risk`,
  - odpowiadajace pola JSONL.
- Dodano jawny status availability jako nowy kontrakt semantyczny.
- Nie wprowadzono globalnego `Signal<T>`.
- Nie zmieniono thresholdow, configu rolloutowego, typed verdict taxonomy ani live/shadow behavior.

## 2. Wykorzystane skills i routing

Uzyte skills:
- `ghost-execution`: Gatekeeper SSOT, policy boundary, shadow/live separation, replay/audit semantics.
- `rust-master`: lokalny typ Rust, minimalne API, testy i kompatybilnosc serde.
- `trading-systems`: ryzyko traktowania braku danych jako czystego false oraz skutki dla regime/confidence.

Zaladowane dokumenty specjalistyczne:
- `docs/agents/gatekeeper-policy-auditor.md`
- `docs/agents/ssot-feature-materialization-guardian.md`
- `docs/agents/decision-logging-replay-analyst.md`

Powod:
PR2 zmienia kontrakt interpretacji evidence w Gatekeeperze, statusy availability w materialized feature path, diagnostyke APS oraz buy-log JSONL. Nie bylo potrzeby ladowania dokumentow Solana execution ani ingest, bo PR2 nie dotyka sendera, transakcji live, parserow ani Yellowstone/Geyser routing.

## 3. Opis problemu - 3W2H

What:
Brak `tx_segment_sequence` albo brak mozliwosci oceny PDD sequence signal byl materializowany jako `false` w starych polach bool. To zacieralo roznice miedzy:
- sprawdzono i sygnalu nie bylo,
- sygnal nie byl wymagany,
- sygnal byl wymagany, ale dane byly niedostepne.

Where:
- `gatekeeper_pdd_sequence.rs`
- `gatekeeper_pdd.rs`
- feature-driven assessment path w `gatekeeper_policy.rs`
- APS regime detection w `gatekeeper_adaptive_prosperity.rs`
- buy-log export w `gatekeeper.rs`
- `GatekeeperBuyLog` w `ghost-brain/src/oracle/decision_logger.rs`

Why it matters:
W gatekeeperze brak danych nie moze obnizac ryzyka tak samo jak clean negative. `false` jako jedyny nosnik informacji byl niewystarczajacy, bo downstream mogl uznac missing PDD sequence za dowod braku spike/ramping/flash. To oslabia auditability i moze z czasem doprowadzic do confidence/regime driftu.

How observed:
Plan PR2 wymagal lokalnego modelu availability i rozroznienia `Available + detected=false`, `Unavailable(reason) + detected=false` oraz `NotApplicable + detected=false`. Aktualny kod nie mial per-signal statusow w PDD diagnostics ani w JSONL.

How many / scale:
Zmiana jest lokalna do PDD sequence signals, APS diagnostics i buy-log export. Blast radius obejmuje aktywny Gatekeeper V2/V2.5 diagnostic/evidence path, ale nie zmienia progow decyzyjnych ani execution handoff.

## 4. Przyczyna zrodlowa

Root cause:
Historyczne bool fields w PDD diagnostics byly jednoczesnie:
- aliasem kompatybilnosciowym dla logow,
- noznikiem wyniku detekcji,
- implicit fallbackiem przy braku sekwencji.

Taki model nie mial miejsca na status availability. W efekcie `false` moglo znaczyc zarowno "checked clean", jak i "nie mozemy ocenic".

## 5. Strategia naprawy

Przyjeta strategia:
- Dodac lokalny PDD-only model:
  - `PddSequenceUnavailableReason`,
  - `PddSignalStatus`,
  - `PddSignalObservation`.
- Zachowac stare bool fields jako kompatybilnosciowe aliasy.
- W materialized feature path ustawic status per signal na podstawie obecnosci i jakosci `tx_segment_sequence`.
- Dla APS przekazywac `PddSignalObservation`, nie golego boola.
- W JSONL dodac opcjonalne status/reason fields, bez usuwania starych bool fields.

Granice:
- Brak globalnego `Signal<T>`.
- Brak zmian thresholdow.
- Brak zmian active hard-fail policy.
- Brak zmian live/shadow execution.
- Brak destrukcyjnej migracji DecisionLogger schema.

## 6. Przeprowadzone akcje naprawcze

Zmiana 1: lokalny model availability
- Plik: `ghost-launcher/src/components/gatekeeper_pdd_sequence.rs`
- Dodano:
  - `PddSequenceUnavailableReason::{MissingSequence, InsufficientDuration, InsufficientTxPerSegment, FlashCrashUnavailable}`,
  - `PddSignalStatus::{NotApplicable, Available, Unavailable(...)}`,
  - `PddSignalObservation`.
- Stabilne serial labels:
  - `not_applicable`,
  - `available`,
  - `unavailable`.
- Stabilne reasons:
  - `missing_sequence`,
  - `insufficient_duration`,
  - `insufficient_tx_per_segment`,
  - `pdd_flash_crash_unavailable`.

Zmiana 2: PDD diagnostics rozroznia bool alias od availability
- Plik: `ghost-launcher/src/components/gatekeeper_pdd.rs`
- `PddDiagnostics` zachowuje stare bool fields.
- Dodano per-signal observations:
  - `spike_signal`,
  - `ramping_signal`,
  - `flash_crash_signal`.
- `PddDiagnostics::not_run()` ustawia nowe statusy na `NotApplicable`.

Zmiana 3: feature/materialized path nie udaje clean false
- Plik: `ghost-launcher/src/components/gatekeeper_policy.rs`
- Dodano helper materializacji PDD sequence statusow z `MaterializedFeatureSet.tx_segment_sequence`.
- Missing sequence daje `Unavailable(MissingSequence)`.
- Za krotka sekwencja daje `Unavailable(InsufficientDuration)`.
- Niespelniony min tx per segment daje `Unavailable(InsufficientTxPerSegment)`.
- Flash crash pozostaje jawnie niedostepny przez `Unavailable(FlashCrashUnavailable)` i reason `pdd_flash_crash_unavailable`.
- Evaluable spike/ramping daje `Available` i realny wynik detekcji.

Zmiana 4: APS konsumuje observation, nie bool
- Plik: `ghost-launcher/src/components/gatekeeper_adaptive_prosperity.rs`
- `evaluate_aps()` i `detect_regime()` przyjmuja `PddSignalObservation`.
- High-volatility regime moze zostac podbite przez PDD spike tylko gdy `status == Available` i `detected == true`.
- `Unavailable(_)` i `NotApplicable` sa zachowane jako rozne diagnostics.
- Disabled APS nie nadpisuje PDD spike availability na `NotApplicable`; zachowuje status wejscia w diagnostyce.

Zmiana 5: JSONL/buy-log dostal additive fields
- Pliki:
  - `ghost-launcher/src/components/gatekeeper.rs`
  - `ghost-brain/src/oracle/decision_logger.rs`
- `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` zwiekszono do `31`.
- Dodano opcjonalne pola:
  - `pdd_spike_signal_status`,
  - `pdd_spike_unavailable_reason`,
  - `pdd_ramping_signal_status`,
  - `pdd_ramping_unavailable_reason`,
  - `pdd_flash_crash_signal_status`,
  - `pdd_flash_crash_unavailable_reason`.
- Stare bool fields pozostaly bez zmiany nazw i semantyki kompatybilnosciowej.

## 7. Walidacja dzialan naprawczych

### Targeted validation

| Walidacja | Komenda | Wynik | Status |
|---|---|---|---|
| PDD sequence unit surface | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher gatekeeper_pdd_sequence -- --nocapture` | 11 passed | PASS |
| APS diagnostics and regime tests | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher gatekeeper_adaptive_prosperity -- --nocapture` | 10 passed | PASS |
| PR2/P1 regression subset | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_v25_regression p1_ -- --nocapture` | 9 passed | PASS |
| Gatekeeper policy tests | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test gatekeeper_policy_tests -- --nocapture` | 44 passed | PASS |
| DecisionLogger JSON serialization | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain test_serialization_to_json -- --nocapture` | 1 passed | PASS |
| DecisionLogger legacy deserialization | `CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-brain test_gatekeeper_buy_log_v19_without_v3_fields_deserializes -- --nocapture` | 1 passed | PASS |

### Szerszy baseline check

Komenda:

```bash
CARGO_TARGET_DIR=/tmp/gho-codex-target cargo test -p ghost-launcher --test session_lifecycle_tests -- --nocapture
```

Wynik:
- 25 passed,
- 1 failed: `filtered_transfer_does_not_unlock_fsc_when_stream_is_only_health_ready`.

Powod failure:
Test oczekuje `FSC_ROLLING_STATE_UNAVAILABLE_REASON` w `features.sybil_resistance.degraded_reasons`, a aktualny wynik tego nie zawiera.

Ocena zakresu:
- Failure odtwarza sie deterministycznie takze po uruchomieniu samego testu.
- Dotyczy FSC rolling-state degraded reason w session/TxIntelligence materialization.
- PR2 nie dotyka `session_lifecycle_tests.rs`, FSC rolling-state materializacji ani `ghost-core/src/tx_intelligence/types.rs`.
- Nie zostal naprawiony w tym PR2, bo bylby osobnym zakresem poza PDD sequence availability.

## 8. Co zostalo potwierdzone

- Missing PDD sequence nie wyglada juz jak checked-clean false.
- `Available + detected=false` jest odroznialne od `Unavailable(reason) + detected=false`.
- `NotApplicable` jest odroznialne od `Unavailable`.
- APS nie traktuje unavailable spike jako clean false ani jako high-vol signal.
- JSONL zachowuje stare bool fields i dodaje optional status/reason fields.
- Legacy buy-log deserialization pozostaje kompatybilne.

## 9. Ryzyka resztkowe / czego PR2 jeszcze nie robi

- PR2 nie zmienia globalnego modelu confidence; to zakres PR3.
- PR2 nie usuwa starych bool fields; pozostaja compatibility aliasami.
- PR2 nie rozwiazuje niezaleznego failure w `session_lifecycle_tests` dotyczacego FSC rolling-state degraded reason.
- PR2 nie zmienia config thresholdow ani policy hard-fail semantics.

## 10. Scope out

Poza zakresem pozostaly:
- globalny `Signal<T>` model,
- confidence semantics PR3,
- whale/top3 contract PR4,
- FSC rolling-state/session lifecycle repair,
- Solana execution / trigger / sender path,
- rollout profile rewrites.
