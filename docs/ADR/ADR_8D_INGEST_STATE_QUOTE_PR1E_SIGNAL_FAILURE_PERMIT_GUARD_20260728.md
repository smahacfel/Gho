# ADR-8D — PR1E: błędny zapis integrity evidence nie może wydać canonical runtime permit

**Status:** IMPLEMENTED LOCALLY / TARGETED VALIDATION PASS / DRAFT PR
**Data:** 2026-07-28
**PR:** #86 — PR1E
**Parent review head:** `3453baf319f600ac0776e1d02ba94aa43a3fa9c7`
**Zakres:** wyłącznie canonical admission po zestage’owaniu receiptu

## D0. Problem

`ingest_pump_observation()` zestage’ował `CanonicalMutationApplyReceiptV1`
przed sygnałami non-Ready, co poprawnie chroniło ownership receiptu przed
przedwczesnym terminal retirement. `emit_pump_observation_decision()` jednak
po błędzie `CandidateIntegrityRegistry::record_signal()` tylko logował błąd i
zamykał globalne admission, po czym zwracał `()`.

Canonical caller nie widział więc błędu. Jeżeli canonical mutation nie miała
sygnału `Ready`, ścieżka nie wywoływała również `seal_complete_transaction_inventory()`;
mogła bezwarunkowo skonstruować `CanonicalRuntimePermitV1` mimo zamkniętego
admission.

Przykładem jest alias conflict po stagingu:

```text
existing candidate: pool=P, mint=A
canonical primary mutation: pool=P, mint=B, inventory=None
→ receipt staged for P/B
→ PrimaryRawCoverageIncomplete cannot record: CandidateAliasConflict
→ admission closed
→ previous code could still return Apply(permit)
```

Taki permit byłby niezgodny z PR1E: nieudany wymagany zapis integrity evidence
nie może prowadzić do Event Bus / Oracle / session runtime emission.

## D1. Decyzja

`emit_pump_observation_decision()` zwraca teraz
`Result<(), CandidateIntegrityErrorV1>`.

Jeżeli obowiązkowy signal non-Ready po stagingu receiptu zwróci błąd:

```text
receipt staged
→ record non-Ready signal fails
→ fail_canonical_apply(receipt)
→ attempt receipt/proof cleanup
→ close candidate admission (idempotent)
→ CanonicalRuntimeAdmissionV1::Blocked(PrimaryRawCoverageIncomplete)
→ zero CanonicalRuntimePermitV1
```

`fail_canonical_apply()` pozostaje jedyną ścieżką oznaczającą ten receipt jako
failed; istniejący cleanup reclaimuje resolved receipt/proof, gdy registry jest
dostępny. Jeśli registry jest już niedostępny i cleanup nie jest możliwy,
admission pozostaje zamknięte i permit nie może zostać wydany.

## D2. Ostatnia bramka przed permitem

Bezpośrednio przed konstrukcją `CanonicalRuntimePermitV1` canonical branch
sprawdza jednocześnie:

```text
CandidateIntegrityRegistry::is_available()
&& CandidateIntegrityRegistry::candidate_admission_open()
```

Niespełnienie któregokolwiek warunku odpala ten sam fail/close receipt path i
zwraca typed `Blocked(PrimaryRawCoverageIncomplete)`.

Ta bramka obejmuje również wariant bez sygnałów `Ready`, w którym nie zachodzi
seal inventory i nie istniała wcześniej druga okazja do obserwacji globalnego
zamknięcia admission.

## D3. Niezmienione granice

Ta korekta **nie** zmienia:

- authority Observation Ledgera ani primary raw;
- locatorów, ordering, parsera i transaction inventory;
- `CandidateIntegrity` lifecycle, generation/CAS albo terminal tombstones;
- `MaterializedFeatureSet`, Gatekeeper policy, reason chain lub sender policy;
- quote math, sizing, TP/SL, PnL, execution mode ani PR2;
- AccountObservationArbiter / AccountStateCore authority.

Canonical structural fact może pozostać dowodem w Ledgerze, ale bez permitu
nie może wejść do aktywnego Event Bus / Oracle runtime.

## D4. Regresja

Test Seera konstruuje aktywny candidate `P/A`, a następnie raw-primary
canonical mutation `P/B` z `raw_transaction_mutation_count=None`.
Wymagania testu:

1. receipt zostaje chwilowo zestage’owany;
2. sygnał non-Ready zwraca `CandidateAliasConflict`;
3. wynik admission to `Blocked(PrimaryRawCoverageIncomplete)`;
4. `into_permit()` zwraca `None`, więc IPC bridge nie ma czego emitować;
5. admission jest zamknięte;
6. całkowita liczba receiptów i proofów wynosi `(0, 0)`;
7. istniejący alias pozostaje audytowalnie oznaczony jako incomplete;
8. canonical raw fact pozostaje w Ledgerze wyłącznie jako evidence.

## D5. Status kwalifikacji

Po tej lokalnej korekcie nadal wymagane są na finalnym SHA PR #86:

1. finalna package/workspace/release matrix z klasyfikacją dokładnych failure
   signatures;
2. machine-readable parent-versus-Enforce differential;
3. finalny parent/current performance protocol;
4. credentialed 30-minute / 10,000-successful-primary-mutation closed run przy
   wyłączonym live execution;
5. authority-epoch binary/config receipt;
6. niezależna klasyfikacja czerwonych GitHub Actions.

Ten ADR nie deklaruje żadnej z tych bramek jako PASS i nie zezwala na merge,
live promotion ani legacy/per-event fallback.
