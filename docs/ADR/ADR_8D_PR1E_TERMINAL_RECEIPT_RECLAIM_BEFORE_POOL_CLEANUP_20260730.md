# ADR-8D — PR1E: reclaim terminalnych canonical receiptów przed cleanupem poola

**Status:** IMPLEMENTED LOCALLY / TARGETED VALIDATION PENDING / DAY 1 NO-GO DO REGRESSION PASS
**Data:** 2026-07-30
**Branch:** `agent/ace-core-one-day-kill-test-v3`
**Parent head:** `068bbf89b5cc090d8b58b782d1eb8eaaf37e837c`
**Zakres:** wyłącznie kolejność `result_rx` → receipt reclaim → existing pool cleanup w `OracleRuntime`.

## D0. Problem i dowód

Diagnostic regression run pokazał kandydat, dla którego canonical `PoolTransaction` miał
już zestage'owany `CanonicalMutationApplyReceiptV1`, lecz dotarł do `OracleRuntime` po
terminalnym wyniku tasku obserwacji.

Dotychczasowa kolejność była:

```text
PoolObservationResult na result_rx
→ join tasku
→ snapshot/pool/session cleanup
→ retire_terminal_candidate
→ TerminalRetirementPending
→ global candidate admission close
→ późny canonical PoolTransaction
→ fail_canonical_apply
```

`retire_terminal_candidate()` poprawnie odmawia retirementu, gdy pozostaje nierozwiązany
receipt. Błędem była kolejność właścicieli obowiązku: po terminalnym wyniku task nie może
już wystawić acknowledgementu `Applied`, więc jego nierozwiązane receipt'y powinny zostać
rozliczone jako `Failed` zanim runtime usunie identity/session/pool.

## D1. Decyzja

W gałęzi `result_rx`, po joinie zakończonego pool tasku i przed istniejącym cleanupem,
runtime:

1. odczytuje identity bieżącego poola;
2. wybiera tylko nierozwiązane receipt'y należące do tego dokładnego kandydata;
3. dla każdego wywołuje istniejące `fail_canonical_apply()`;
4. dopiero po sukcesie kontynuuje dotychczasowe `snapshot_engine.remove_pool()` i
   `remove_pool_with_reason()`.

Nowy registry helper nie usuwa mapy fence, nie ustawia flag `failed` bezpośrednio i nie
omija identity/proof checks. Jest wyłącznie selektorem receiptów; każda rzeczywista
zmiana stanu przechodzi przez istniejącą semantykę `fail_canonical_apply()`.

## D2. Fail-close

Jeżeli reclaim któregokolwiek receiptu zwróci błąd registry albo fence:

```text
terminal receipt reclaim error
→ CANDIDATE_INTEGRITY global admission close
→ istniejący cleanup path
```

Nie ma retry queue, grace-period sleep, force-delete, resetu registry ani reopening
admission. `retire_terminal_candidate()` nie został zmieniony.

## D3. Kontrakt późnego eventu

Po terminalnym cleanupie nie istnieje `pool_task_handle`, więc późny canonical event
pozostaje na istniejącej fail-closed ścieżce i nie może odtworzyć poola ani przejść do
`mark_canonical_apply_succeeded()`. Powtórna próba fail jest ignorowana na granicy
Oracle tak jak wcześniej; nie otwiera candidate admission i nie tworzy nowego poola.

## D4. Test deterministyczny

Test `terminal_result_path_reclaims_staged_receipts_before_cleanup_and_blocks_late_apply`
odtwarza kontrakt:

```text
receipt staged
→ terminal result path reclaims it before cleanup
→ unresolved fence count = 0
→ existing cleanup/retirement succeeds without global close
→ late apply acknowledgement is rejected
→ pool ani receipt nie są odtworzone
```

## D5. Niezmienione granice

Ta zmiana nie modyfikuje:

- `retire_terminal_candidate()` ani jego `TerminalRetirementPending` fence;
- cadence fixu `068bbf89…`, IPC capacity, backpressure, `tokio::select!` priority ani queue;
- CandidateAliasConflict scope;
- ACE probe, configu, Brain, Gatekeepera, MFS, Position Managera, Triggera ani PR2;
- shadow-only / observe-only execution boundary.

## D6. Wymagana walidacja

Przed backportem i qualifying smoke wymagane są:

```text
cargo fmt --all --check
focused terminal receipt test
existing alias/receipt tests
cargo build --release -p ghost-launcher --bin ghost-launcher --bin ace_core_one_day_probe
```

Następnie tylko jeden świeży 10-minutowy regression run na cadence candidate plus ta
korekta. PASS wymaga:

```text
IPC_EGRESS_SATURATED                = 0
primary_coverage_gap                = 0
candidate_admission_closed          = 0
sent == received
final backlog                       = 0
terminal_retirement_failed          = 0
unresolved receipt count at cleanup = 0
```

Ten ADR nie zezwala na Day 1 ani na backport przed tym wynikiem.

## D7. Rollback

Rollbackiem jest usunięcie wyłącznie reclaimu przed cleanupem. Nie należy zastępować go
zmianą retirement fence, ręcznym usuwaniem receiptów ani zwiększaniem capacity.
