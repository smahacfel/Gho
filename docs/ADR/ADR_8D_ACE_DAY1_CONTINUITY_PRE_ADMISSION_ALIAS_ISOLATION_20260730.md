# ADR-8D: ACE Day 1 — izolacja aliasu ContinuityOnly przed canonical admission

**Data:** 2026-07-30
**Status:** IMPLEMENTED LOCALLY / FOCUSED VALIDATION IN PROGRESS / SMOKE HOLD / DAY 1 NO-GO
**Zakres:** Jedna naprawa pierwotnej przyczyny zamknięcia CandidateIntegrity
admission podczas unieważnionego ACE Day 1. Nie zmienia polityki Gatekeepera,
Brain, ACE probe'a, capacity, backpressure, execution ani PR2.

## D0. Dowód przyczyny

Unieważniony Day 1 zatrzymał canonical admission dla Pump candidate:

```text
pool = 4UWfxMUBoXcsGzpoNxq9GEKTo7Ce8Cec6JGaShkQm3BZ
mint = 9WKCEGBrMjnBcLGVZwLnBjVx73hFVd2rM2y5uxmCPump
```

W tym samym interwale parser PumpSwap zgłosił `ContinuityOnly` dla innego
poola i tego samego mintu. Poprzednia ścieżka Seer wykonywała kolejno:

```text
primary ContinuityOnly wrapper
→ PumpObservationLedger::observe
→ CandidateIntegrity receipt stage
→ fail_canonical_apply
→ terminal alias tombstone dla shared mint
→ valid Pump downstream acknowledgement
→ CandidateAliasConflict
→ global candidate admission close
```

Zatem tekst późniejszego `RegistryUnavailable` o mutexie nie był dowodem
poisoningu. Pierwotną kolizją był lokalny lifecycle receipt wygenerowany dla
obserwacji, która z definicji nie może zostać kandydatem runtime.

## D1. Decyzja

Primary-raw `PoolDetected` o dyspozycji `ContinuityOnly` albo `Suppressed`
jest odrzucany przed wejściem do `PumpObservationLedger` i
`CandidateIntegrityRegistry`:

```text
primary raw PoolDetected
→ disposition pre-admission gate
→ ContinuityOnly / Suppressed: NoApply
→ CandidateAdmission: istniejący Ledger → receipt → Event Bus flow
```

Non-primary observations zachowują istniejącą witness-only drogę. Bramka nie
dotyka canonical trade ingest ani receiptów prawidłowych candidate-admission
mutations.

## D2. Inwarianty

- `ContinuityOnly` nie otrzymuje permitu, nie stage'uje receiptu i nie tworzy
  terminal tombstone ani active CandidateIntegrity record;
- prawidłowy Pump candidate o tym samym mincie może nadal zakończyć canonical
  downstream acknowledgement bez `CandidateAliasConflict`;
- global candidate admission pozostaje otwarte dla lokalnej continuity
  observation;
- każdy rzeczywisty błąd canonical admission poza tą pre-admission dyspozycją
  zachowuje dotychczasowy fail-closed kontrakt;
- obserwacja pozostaje observe-only: nie powstaje trigger, Position Manager,
  live execution ani nowy decision plane.

## D3. Weryfikacja

Focused test reprodukuje interleaving z Day 1:

```text
valid Pump receipt staged
→ shared-mint primary ContinuityOnly detected
→ continuity returns NoApply before Ledger/registry mutation
→ valid Pump receipt acknowledges downstream apply
→ zero terminal alias tombstones and admission remains open
```

Po focused testach wymagane są: `cargo fmt --all --check`, istniejący PR1E
qualification case dla `continuity_only_restored_position`, release build obu
binarek oraz nowy 10-minutowy qualifying smoke. Dzień 1 może wystartować
wyłącznie, jeżeli smoke przejdzie wszystkie ustalone health i offline-probe
bramki.

## D4. Poza zakresem

Nie przebudowano `CandidateAliasConflict`, nie odtruto mutexu, nie usunięto
retirement fence, nie dodano kolejki/retry ani nie zmieniono cadence finalizera
oraz shutdown ownership. To jest punktowa korekta kolejności admission dla
PoolDetected, która nie jest kandydatem.
