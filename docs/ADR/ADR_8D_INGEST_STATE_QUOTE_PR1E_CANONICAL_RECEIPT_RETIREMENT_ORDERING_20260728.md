# ADR-8D: PR1E — ownership canonical receipt przed terminal retirement

Status:
`IMPLEMENTED LOCALLY / TARGETED VALIDATION PASS / FINAL MATRIX PENDING / DRAFT PR`

Data: `2026-07-28`

Repo: `/root/Gho_ingest`

Gałąź: `agent/ingest-state-quote-boundary-pr1e-20260727`

Base PR1E: `103212b16bfc059db367e1ceb3c7d00fd307d6c5`

Parent review head: `0c767d574c82e0adead1d829cbdb7e41c51301c6`

Normatywny plan:
`PLANS/DO_REALIZACJI/PLAN_WYKONAWCZY_NAPRAWY_GRANICY_INGEST_STATE_QUOTE.md`

Powiązany receipt:
`PLANS/DO_REALIZACJI/QUALIFICATION_RECEIPT_PR1E_1EB_20260727.md`

## D0. Problem

Jedna decyzja `PumpObservationLedgerV1` może jednocześnie zawierać canonical
primary mutation oraz non-Ready evidence, przykładowo:

- `raw_transaction_mutation_count=None` →
  `PrimaryRawCoverageIncomplete`;
- NLN-first, primary raw-second o sprzecznych claims →
  `SourceReconciliationConflict`.

Przed tą korektą launcher rejestrował sygnał non-Ready przed
`stage_canonical_mutation()`. Pierwszy non-Ready signal bez widocznego
unresolved receipt mógł więc przenieść candidate do terminal tombstone i
zakolejkować retirement Ledgera. Dopiero potem powstawał canonical apply
receipt. Zakończony receipt mógł pozostać w bounded fence, lecz nie być
widoczny w metryce pending permits.

To naruszało ownership lifecycle:

```text
canonical mutation
  -> receipt ownership
  -> non-Ready evidence
  -> downstream apply / failure resolution
  -> terminal retirement
```

## D1. Decyzja

`ingest_pump_observation()` dla każdej canonical mutation wykonuje teraz
deterministycznie:

```text
Ledger canonical result
  -> stage_canonical_mutation
  -> record non-Ready signals
  -> defer Ready signal
  -> seal transaction inventory
  -> issue canonical runtime permit
```

Niecanonical decisions zachowują poprzednią semantykę: zapisują evidence i
nie otrzymują runtime permitu.

`CanonicalApplyFenceV1` pozostaje jedynym prywatnym, bounded ownership
fence. Gdy receipt kończy się przez `AppliedNewMutation` albo typed failure:

1. resolved receipt i związany proof są usuwane, o ile dla tego dokładnego
   candidate nie istnieje inny unresolved receipt;
2. aktywny non-Ready candidate w `PreMfs` jest przenoszony do istniejącego
   bounded terminal tombstone FIFO;
3. dopiero wtedy jest przekazywany bounded notice do
   `PumpObservationLedgerV1`.

Defensywnie cleanup obsługuje także istniejący terminal tombstone. Nie
odtwarza on candidate ani Ready, lecz nie pozwala, aby historyczne odwrócenie
kolejności zachowało zakończony receipt w fence.

## D2. Rewalidacja cross-owner retirement

`drain_terminal_ledger_retirements()` nie opróżnia już bezwarunkowo kolejki.
Przed wydaniem każdego notice ponownie sprawdza brak unresolved receipt dla
tego samego `PumpCandidateIdentityV1`.

```text
terminal retirement notice + unresolved receipt
  -> notice retained in bounded handoff
  -> zero Ledger retirement

receipt resolved
  -> notice becomes eligible
  -> Ledger terminal tombstone
```

Nie ma cichego usuwania unresolved receipt ani notice. Wyczerpanie bounded
terminal handoff pozostaje typed fail-closed error dla nowych admission.

## D3. Granice i niezmienione authority

Ta korekta nie zmienia:

- primary raw jako jedynego structural authority;
- witness-only NLN oraz secondary raw;
- canonical order i locatorów;
- `MaterializedFeatureSet`, Gatekeeper policy, strategy lub quote math;
- submit/confirmation/protective-exit semantics;
- PR1E external qualification gates.

Candidate z non-Ready integrity outcome nie publikuje `Ready`; canonical raw
state mutation nadal może zostać zastosowana dokładnie raz, jeżeli istnieje
valid runtime permit i downstream zwróci `AppliedNewMutation`.

## D4. Celowane dowody regresyjne

Dodano testy, które sprawdzają:

1. canonical mutation z `raw_transaction_mutation_count=None`:
   receipt istnieje przed non-Ready signal, po downstream apply aktywne
   recordy znikają, tombstone pozostaje, a receipt/proof counts wynoszą zero;
   osobny test wykonuje ten apply przez rzeczywisty
   `PoolObservationSession::ingest_transaction_with_apply_result()` i wymaga
   `AppliedNewMutation`, a nie bezpośredniego acknowledge registry;
2. NLN-first conflict plus późniejszy canonical raw:
   canonical mutation jest zastosowana raz, `Ready=0`, proof i receipt są
   usunięte, a Ledger retirement następuje dopiero po apply resolution;
3. typed failed downstream apply: receipt/proof są usunięte i capacity
   odzyskana;
4. mały limit registry/ledger oraz wiele kolejnych canonical+non-Ready
   mutations: active fence nie rośnie, candidate admission pozostaje otwarte;
5. legacy-style tombstone notice z późno zestage’owanym receipt: ledger
   handoff pozostaje deferred aż do resolution.

Wykonane lokalnie na diffie tej korekty:

```text
cargo check -p ghost-launcher --lib
cargo test -p ghost-launcher --lib candidate_integrity::tests:: -- --nocapture
cargo test -p ghost-launcher --lib \
  components::seer::tests::canonical_missing_inventory_receipt_outlives_non_ready_signal_then_retires_cleanly \
  -- --exact --nocapture
cargo test -p ghost-launcher --lib \
  components::seer::tests::nln_first_conflict_keeps_canonical_receipt_until_apply_then_retires_it \
  -- --exact --nocapture
cargo test -p ghost-launcher --lib \
  components::seer::tests::resolved_non_ready_receipts_do_not_exhaust_small_candidate_capacity \
  -- --exact --nocapture
```

## D5. Nieuzyskane jeszcze bramki

Ten ADR nie oznacza `OFFLINE PASS`, `DIFFERENTIAL PASS`, zielonego CI ani
merge qualification. PR #86 pozostaje Draftem. Nadal wymagane są finalna
package/workspace/release matrix, machine-readable parent-versus-Enforce
differential, parent/current performance protocol, credentialed 30-minute /
10k-primary closed run, authority-epoch receipt oraz pełna klasyfikacja
aktualnych GitHub Actions.
