# RAPORT SHADOW V2 PR43-B EVENTORDERKEY RESEARCH PROVENANCE 20260703

## 1. Werdykt wykonawczy

Final verdict:

```text
PR43B_EVENTORDERKEY_PROVENANCE_IMPROVED
```

PR43-B poprawia i uszczelnia kontrakt `EventOrderKey` dla Shadow V2 L2 research provenance. Zmiana nie przyznaje L2 research-grade, runtime approval, live-equivalence ani strategy unlock.

Najważniejszy wynik:

- `EventOrderKey` rozróżnia teraz jawne `UNKNOWN` od komponentów `NOT_APPLICABLE`, `DERIVED` i `RUNTIME_LOCAL`.
- Dostępne komponenty chain-order z boundary source są propagowane do `PoolStateSampleV2`.
- Brakujące komponenty nie są ukrywane ani wypełniane fake values; pozostają explicit `UNKNOWN` i generują typed limitations/blockers.
- Terminal truth / derived evidence nie udaje observed chain account update.
- `event_seq_in_process` pozostaje runtime-local ordering aid i nie jest traktowany jako substytut chain-order dla L2.

Temporal/no-lookahead blocker został lepiej sklasyfikowany i częściowo zawężony na poziomie schema/testów, ale nie został usunięty. Runtime delta nie został zmierzony w PR43-B, ponieważ zakres PR zabraniał burnina.

Approval flags pozostają:

```text
runtime_approval=false
shadow_close_only_approval=false
active_close_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
```

## 2. PR43-A0 baseline

PR43-A0 zaakceptował:

```text
L2_RESEARCH_GRADE_PATH_PRESENT
```

Interpretacja baseline:

- L1 deterministic diagnostic execution simulation jest domknięty.
- L2 research-grade nie jest przyznane.
- L2 path istnieje bez wymagania L3 live calibration.
- Główne blockery L2: chain-order ambiguity, `account_data_hash`, path density, sample size i `RESEARCH_CANDIDATE=0`.

Temporal baseline z PR43-A0:

```text
temporal_audit_verdict=BLOCKED_TEMPORAL_AMBIGUITY_REMAINS
event_order_key_present_rows=252
event_order_key_missing_required_rows=0
non_monotonic_event_seq_in_process=0
explicit_unknown_chain_order_components:
  block_time=252
  transaction_index_or_unknown=252
  instruction_index_or_unknown=252
  inner_instruction_index_or_unknown=252
  log_index_or_unknown=252
  signature=168
```

PR43-B adresuje tylko klasę blockerów `L2_TEMP_*`. Nie rozwiązuje `account_data_hash`, density, sample size ani L3 calibration.

## 3. Zakres PR43-B

Zakres był implementation + tests + report, bez burnina.

W zakresie:

- propagacja dostępnych chain-order components do `EventOrderKey`;
- utrzymanie explicit `UNKNOWN`, gdy runtime source nie ma danych;
- rozdzielenie `UNKNOWN` od `NOT_APPLICABLE` / `DERIVED` / `RUNTIME_LOCAL`;
- typed limitations i blockers zamiast silent fallback;
- aktualizacja temporal audit counters;
- testy potwierdzające brak fake order i brak konsumpcji Shadow V2 przez decision/live paths.

Poza zakresem:

- `account_data_hash`;
- path density / horizon evaluability;
- sample size;
- live-confirmed fills;
- realized slippage;
- quote/fill divergence;
- landing/failure telemetry;
- jakikolwiek runtime approval.

## 4. Zmiany w kontrakcie EventOrderKey

`EventOrderComponent<T>` zachowuje kompatybilny kształt `Known(T) | Unknown(...)`, ale `Unknown(...)` ma teraz typową klasyfikację:

```text
UNKNOWN
NOT_APPLICABLE
DERIVED
RUNTIME_LOCAL
```

Znaczenie:

- `UNKNOWN`: źródło powinno mieć chain-order dla research-grade, ale go nie dostarczyło.
- `NOT_APPLICABLE`: komponent nie ma zastosowania dla danego event family.
- `DERIVED`: rekord jest deterministycznie wyprowadzony i nie jest observed chain eventem.
- `RUNTIME_LOCAL`: komponent pochodzi tylko z lokalnego runtime ordering, nie z chain order.

Nowy kontrakt nie zamienia braków w sukces. `UNKNOWN` nadal blokuje L2 research provenance. `DERIVED` i `NOT_APPLICABLE` są jawnie widoczne w limitations/audytach i nie udają chain-observed data.

## 5. Poprawiona propagacja source components

Dodano optional source chain-order fields do `ShadowV2EntryBoundaryPayload`:

```text
source_block_time
source_tx_signature
source_transaction_index
source_instruction_index
source_inner_instruction_index
source_log_index
```

Jeżeli te pola są obecne, `PoolStateSampleV2` dostaje je jako `Known(...)` w `EventOrderKey`.

Jeżeli ich nie ma, wpis pozostaje explicit `UNKNOWN` i dostaje limitations:

```text
ENTRY_BOUNDARY_SOURCE_SIGNATURE_UNAVAILABLE
ENTRY_BOUNDARY_SOURCE_TRANSACTION_INDEX_UNAVAILABLE
ENTRY_BOUNDARY_SOURCE_INSTRUCTION_INDEX_UNAVAILABLE
ENTRY_BOUNDARY_SOURCE_INNER_INSTRUCTION_INDEX_UNAVAILABLE
ENTRY_BOUNDARY_SOURCE_LOG_INDEX_UNAVAILABLE
```

To celowo nie wypełnia chain-order wartościami zastępczymi. Obecny trigger capture ustawia nowe source fields na `None`, bo obecny runtime boundary nie ma tych danych. To oznacza, że w realnym runtime po PR43-B należy spodziewać się lepszej klasyfikacji, ale nie pełnego L2 PASS.

## 6. Usunięcie fałszywego signature source dla entry pool-state

W PR34-B entry pool-state sample mógł użyć post-buy/entry handoff signature jako signature w `EventOrderKey`. PR43-B usuwa tę dwuznaczność dla pool-state source.

Po PR43-B entry pool-state `EventOrderKey.signature` pochodzi wyłącznie z `ShadowV2EntryBoundaryPayload.source_tx_signature`, jeśli runtime rzeczywiście go dostarczy. W przeciwnym razie jest explicit `UNKNOWN`.

To jest ważne dla L2: signature z tx/handoff nie może udawać signature observed pool-state boundary, jeżeli źródło tego nie potwierdza.

## 7. Derived terminal truth i after-state

Terminal truth jest derived evidence. Nie jest observed chain account update.

PR43-B oznacza chain tx components dla derived terminal truth jako `DERIVED`, zamiast pozostawiać je jako nieodróżnialny `UNKNOWN` albo wpisywać fake chain data.

Konsekwencja:

- terminal truth może dalej służyć do reconciliation;
- nie jest używany jako canonical input dla pre-entry/pre-exit boundary;
- temporal audit może odróżnić derived/non-chain components od unknown-but-required-for-research.

## 8. Aktualizacja temporal audit

`scripts/shadow_v2_temporal_no_lookahead_audit.py` rozróżnia teraz:

```text
explicit_unknown_chain_order_components
not_applicable_chain_order_components
derived_chain_order_components
runtime_local_chain_order_components
unknown_but_required_for_research_count
not_applicable_or_derived_chain_components_count
```

Verdict pozostaje konserwatywny:

- `FAIL` dla missing required `event_order_key`, ordering violation, lookahead albo derived-as-canonical violation;
- `BLOCKED` dla explicit `UNKNOWN` chain-order components;
- `PASS` dopiero wtedy, gdy nie ma missing required ordering, non-monotonic seq, lookahead i unknown-but-required blockers.

## 9. Test evidence

Targeted tests potwierdzają:

- dostępne source fields trafiają do `EventOrderKey`;
- brakujące source fields pozostają explicit `UNKNOWN`;
- derived terminal truth jest oznaczony jako `DERIVED`;
- `event_seq_in_process` samodzielnie nie wystarcza do L2 chain order;
- same-slot ambiguity nie jest research-safe bez chain tie-breakerów;
- Shadow V2 nadal nie jest konsumowany przez decision/live paths.

Uruchomione checks są zapisane w ADR PR43-B.

## 10. Co realnie poprawiono

Poprawione:

- lepsza semantyka `EventOrderComponent`;
- source-aware propagation dla boundary chain-order fields;
- brak fake signature dla entry pool-state source;
- derived terminal truth nie udaje chain observed event;
- temporal audit raportuje nową klasyfikację braków;
- `PoolStateSampleV2::research_blockers()` mapuje explicit `UNKNOWN` chain components na typed blockers.

Niepoprawione w tym PR:

- runtime nadal nie ma source tx/ix/log fields w trigger boundary capture;
- `account_data_hash` nadal nie jest obecny w runtime boundary;
- path density nadal wymaga osobnego L2 validation run;
- sample size nadal nie jest research-grade;
- `RESEARCH_CANDIDATE` nadal nie jest przyznawany przez sam PR43-B.

## 11. Wpływ na L2

PR43-B poprawia ścieżkę do L2, ale jej nie domyka.

Oczekiwany stan po PR43-B:

```text
L1_DIAGNOSTIC_SIM remains valid
L2_RESEARCH_CANDIDATE remains blocked
temporal blocker = improved/classified, not fully cleared
runtime_temporal_delta_not_measured_in_PR43B=true
requires_next_15min_smoke_after_merge=true
```

Następny smoke po merge powinien trwać maksymalnie 15 minut i mierzyć:

```text
event_order_key_missing_required_rows
explicit_unknown_chain_order_components
not_applicable_or_derived_chain_components_count
same_slot_ambiguity_count
unknown_but_required_for_research_count
```

Nie może on przyznawać L2 PASS. Jego cel to tylko runtime delta dla PR43-B.

## 12. Następne PR-y

Rekomendowana kolejność:

1. `PR43-C`: 15-min temporal delta smoke po merge PR43-B.
2. `PR44`: account data hash provenance z ingest/reducer boundary.
3. `PR45`: path density / horizon evaluability.
4. `PR46`: L2 research validation run i audit pack.
5. `PR47+`: L3 live-confirmed calibration dataset.

## 13. Final decision

PR43-B może być oceniany jako:

```text
PR43B_EVENTORDERKEY_PROVENANCE_IMPROVED
```

Nie wolno interpretować PR43-B jako:

```text
research_grade=true
live_equivalence=true
runtime_approval=true
shadow_close_only_approval=true
active_close_approval=true
strategy_research_unblocked=true
```
