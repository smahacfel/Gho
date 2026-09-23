# ADR-8D — PR1E: alias conflict bez canonical receipt nie zamyka globalnej admission

**Status:** IMPLEMENTED LOCALLY / TARGETED VALIDATION PENDING / DAY 1 NO-GO DO SMOKE PASS
**Data:** 2026-07-29
**Branch:** `agent/ace-core-one-day-kill-test-v3`
**Parent head:** `0641e6b7e8cab98453d4f46f3625d69e14af7c6b`
**Zakres:** wyłącznie rozdzielenie konsekwencji `CandidateAliasConflict` przed i po stagingu canonical receipt.

## D0. Problem i dowód

Kwalifikujący smoke `ace-core-one-day-probe-r1-qualifying-smoke-20260729t110220z` nie wydał canonical events. Pierwszy błąd stagingu wystąpił o `11:04:28.832Z`, lecz o `11:04:28.820Z` log zapisał dwa błędy:

```text
Seer: CandidateIntegrity update failed; new-candidate admission closed
outcome=PrimaryRawCoverageIncomplete
error=candidate identity aliases disagree
```

Nie było wcześniejszego `panic`, `PoisonError`, `ledger_unavailable`, błędu inventory seal, capacity ani coverage gap. Klasyfikacja pierwotnej przyczyny to:

```text
OTHER_EXACTLY_IDENTIFIED
= CandidateAliasConflict podczas zapisu non-canonical PrimaryRawCoverageIncomplete evidence
```

Nie jest to dowód `TRUE_MUTEX_POISON`. Późniejszy tekst `candidate integrity registry mutex is poisoned` pochodził z ogólnego wariantu `RegistryUnavailable` po wtórnym zamknięciu admission, a nie z udowodnionego `Mutex::lock()` -> `PoisonError`.

## D1. Przyczyna kodowa

`ingest_pump_observation()` ma dwie odmienne granice:

```text
non-canonical decision -> nie istnieje CanonicalMutationApplyReceiptV1
canonical decision     -> receipt jest już zestage’owany
```

Obie korzystały z `emit_pump_observation_decision()`. Każdy błąd `record_signal()` — również `CandidateAliasConflict` przed receipt — wywoływał `close_candidate_admission_with_integrity_invalidation()`.

To nadmiernie eskalowało per-kandydatowy konflikt aliasów: registry już oznacza istniejący konfliktujący rekord jako `PrimaryRawCoverageIncomplete`, a bieżąca obserwacja nie dostaje permitu. Bez receipt nie istnieje jednak globalny ownership obligation, którego brak wymagałby zatrzymania niezależnych przyszłych kandydatów.

## D2. Decyzja

Dodano lokalny, jawny scope błędu:

```text
NonCanonicalEvidence
CanonicalLifecycle
```

Przy `CandidateAliasConflict` przed stagingiem obowiązuje:

```text
P/A istnieje
-> P/B non-canonical evidence koliduje
-> P/A pozostaje audytowalnie PrimaryRawCoverageIncomplete
-> P/B jest Blocked(PrimaryRawCoverageIncomplete)
-> zero receiptów, zero permitów, zero Event Bus emission dla P/B
-> global candidate admission pozostaje otwarte
```

Po stagingu receipt oraz dla późniejszego canonical lifecycle zachowano poprzedni fail-closed kontrakt:

```text
receipt staged
-> required integrity signal fails
-> receipt/proof cleanup
-> global admission closed
-> zero CanonicalRuntimePermitV1
```

Nie odzyskiwano mutexu przez `PoisonError::into_inner()`, nie resetowano registry i nie zmieniano CandidateIntegrity lifecycle ani alias validation.

## D3. Regresje

Dodany test `integrity_signal_alias_conflict_before_receipt_blocks_only_conflicting_candidate` potwierdza, że konflikt przed receipt:

1. zwraca `Blocked(PrimaryRawCoverageIncomplete)`;
2. pozostawia globalną admission otwartą;
3. pozostawia `(receipt_count, proof_count) == (0, 0)`;
4. oznacza istniejący alias jako incomplete;
5. nie tworzy canonical mutation ani runtime permitu.

Istniejący test `integrity_signal_alias_conflict_after_receipt_stage_blocks_permit_and_reclaims_fence` pozostaje kontraktem przeciwnej, celowo globalnej ścieżki po stagingu.

## D4. Niezmienione granice

Ta korekta nie zmienia:

- ACE probe, cech, cutoffu, quote proxy ani capacity bounds;
- Gatekeepera, `MaterializedFeatureSet`, Position Managera ani PR2;
- parsera, source authority, locatorów, ordering ani trade tape schema;
- shadow-only execution mode, Triggera ani live execution;
- globalnego fail-close dla `PoisonError`, registry/capacity unavailable, coverage gap, inventory failure lub błędu required signal po receipt.

## D5. Bramka po zmianie

Przed Dniem 1 nadal wymagany jest nowy qualifying smoke z nowym run ID i ścieżkami, `RUST_BACKTRACE=1` oraz captured stderr. PASS wymaga jednocześnie:

```text
pr1_runtime_bypass_attempt_total             = 0
pr1_runtime_candidate_admission_closed_total = 0
pr1_runtime_primary_coverage_gap_total       = 0
non-empty birth/trade tape
health finalize exit                          = 0
verify-probe exit                             = 0
```

Ten ADR nie zezwala na Dzień 1 przed takim wynikiem i nie tworzy PR ani review.
