# Raport PR43-D: Shadow V2 EventOrderKey source availability audit

Data: 2026-07-03
Tryb: report-only / audit-only
Final verdict: `EVENTORDERKEY_SOURCE_PARTIAL_PATH_PRESENT`

## Executive summary

PR43-D sprawdza, czy brakujące komponenty `EventOrderKey` blokujące L2 `RESEARCH_CANDIDATE` można realnie pozyskać z obecnego ingest / provider streams / runtime handoff.

Wniosek jest częściowy:

- źródła dla części komponentów istnieją upstream: `GeyserEvent::Transaction` ma `block_time`, `TradeEvent` i `PoolTransaction` niosą `signature`, `tx_index`, `event_ordinal`, `outer_instruction_index`, `inner_group_index`, a NLN Program Streams normalizuje `tx_index`, `block_time` i `instruction_index`;
- obecny Shadow V2 runtime nie przenosi tych pól do wszystkich `EventOrderKey`;
- `ShadowV2EntryBoundaryPayload` ma pola `source_block_time`, `source_tx_signature`, `source_transaction_index`, `source_instruction_index`, `source_inner_instruction_index`, `source_log_index`, ale `TriggerComponent::capture_shadow_v2_entry_boundary()` ustawia je obecnie na `None`;
- `MonitoringEngine` dla exit/path/terminal używa helpera `shadow_v2_event_order_key(...)`, który ustawia `block_time`, `transaction_index`, `instruction_index` i `inner_instruction_index` na explicit `UNKNOWN`;
- Solana nie ma natywnego EVM-style `logIndex`; potencjalny log ordinal może być tylko wewnętrznie wyprowadzony przez enumerację `meta.logMessages` albo custom parser/indexer logic;
- `TERMINAL_TRUTH` i derived after-state są poprawnie klasyfikowane jako `DERIVED`, a nie jako fake chain-observed ordering.

To nie jest L2 PASS. To jest dowód, że ścieżka do częściowej poprawy istnieje, ale pełny research-grade ordering nadal wymaga implementacji propagacji oraz doprecyzowania, czy pole `log_index_or_unknown` oznacza wewnętrzny `LOG_MESSAGE_INDEX_INTERNAL`, a nie natywny Solana log index.

## Zakres

Audyt objął komponenty:

- `block_time`;
- `source_tx_signature`;
- `transaction_index`;
- `instruction_index`;
- `inner_instruction_index`;
- `log_index` rozumiany wyłącznie jako non-native internal log message ordinal, nie jako Solana-native `logIndex`.

Audyt objął event families:

- `ENTRY_ATTEMPT`;
- `ENTRY_POOL_STATE_BEFORE`;
- `ENTRY_FILL`;
- `ENTRY_POOL_STATE_AFTER_DERIVED`;
- `PATH_SAMPLE`;
- `EXIT_ATTEMPT`;
- `EXIT_POOL_STATE_BEFORE`;
- `EXIT_FILL`;
- `EXIT_POOL_STATE_AFTER_DERIVED`;
- `TERMINAL_TRUTH`.

## Evidence z kodu

### EventOrderKey schema

`ghost-brain/src/guardian/post_buy/shadow_v2.rs` definiuje `EventOrderComponent<T>` z wartościami:

- `UNKNOWN`;
- `NOT_APPLICABLE`;
- `DERIVED`;
- `RUNTIME_LOCAL`;
- `Known(T)`.

`EventOrderKey` zawiera slot, block_time, signature, transaction index, instruction index, inner instruction index, log index, `event_seq_in_process` i `observed_at_wall_ms`. Audit labels jawnie oznaczają `EVENT_ORDER_UNKNOWN_BUT_REQUIRED_FOR_RESEARCH`.

### Entry boundary

`ghost-launcher/src/events.rs` ma `ShadowV2EntryBoundaryPayload` z polami source metadata:

- `source_block_time`;
- `source_tx_signature`;
- `source_transaction_index`;
- `source_instruction_index`;
- `source_inner_instruction_index`;
- `source_log_index`.

`ghost-launcher/src/components/trigger/component.rs::capture_shadow_v2_entry_boundary()` tworzy payload z `CanonicalPoolState`, ale wszystkie pola source metadata ustawia na `None`. Oznacza to, że schema i handoff potrafią przenieść dane, ale aktualny capture ich nie pozyskuje.

`ghost-launcher/src/components/post_buy_runtime.rs::shadow_v2_entry_pool_state_from_boundary()` potrafi zbudować `EventOrderKey` z tych pól, jeżeli istnieją. Obecnie dostaje explicit `UNKNOWN`.

### Ingest / provider surface

`off-chain/components/seer/src/types.rs::GeyserEvent::Transaction` ma `signature` i `block_time`.

`off-chain/components/seer/src/types.rs::TradeEvent` ma `signature`, `event_ordinal`, `tx_index`, `provenance`, `timestamp_ms`, `arrival_ts_ms` i `event_time`.

`InstructionProvenance` ma `outer_instruction_index` i `inner_group_index`.

`ghost-launcher/src/events.rs::PoolTransaction` przenosi `event_ordinal`, `tx_index`, `outer_instruction_index` i `inner_group_index`.

`off-chain/components/seer/src/nln_program_streams.rs` normalizuje `tx_index`, `block_time` i `instruction_index` dla NLN Program Streams. Dokumentacja NLN dla topiców `solana.pump_fun.buy` i `solana.pump_fun.buy_exact_sol_in` potwierdza decoded payload dla tych program-stream topics, ale publiczna strona nie jest wystarczająca jako dowód pełnych `tx_index` / `instruction_index` dla każdego runtime row; ten dowód pochodzi głównie z aktualnego kodu normalizera.

Ważna korekta semantyczna: Program Streams / enhanced streams mogą dawać decoded event order albo instruction metadata, ale nie należy zakładać natywnego `logIndex`, bo taki model jest EVM-owy. Solana ma `meta.logMessages: Vec<String>`, a ewentualny `LOG_MESSAGE_INDEX_INTERNAL` byłby ordinalem nadanym przez enumerację tej tablicy, nie polem natywnym chain.

### Exit/path/terminal

`ghost-brain/src/guardian/post_buy/engine.rs` używa `shadow_v2_event_order_key(...)` dla `PATH_SAMPLE`, `EXIT_ATTEMPT`, `EXIT_POOL_STATE_BEFORE` i `EXIT_FILL`. Helper przyjmuje slot i optional signature, ale `block_time`, transaction index, instruction index, inner instruction index i log index ustawia na explicit `UNKNOWN`.

`shadow_v2_derived_event_order_key(...)` ustawia terminal/derived chain components jako `DERIVED`. To jest poprawne dla `TERMINAL_TRUTH` i derived after-state, bo te records nie są obserwowanymi transakcjami chain.

## Macierz

Pełna macierz znajduje się w:

`reports/selector/shadow_v2_pr43d_event_order_source_matrix.csv`

Liczba wierszy: 60.

Podsumowanie verdictów wierszy:

- `PARTIAL_SOURCE_PRESENT_NOT_CARRIED`: 35;
- `SOLANA_NATIVE_LOG_INDEX_NOT_APPLICABLE`: 7;
- `NOT_APPLICABLE_DERIVED_COMPONENT`: 18;
- pełne `AVAILABLE` dla L2 bez dodatkowego wiring: 0.

## Co można realnie poprawić w PR43-E

Minimalny PR43-E może być implementation PR, ale tylko dla dostępnych pól:

1. Dodać shadow-only propagation z `PoolTransaction` / `TradeEvent` / NLN Program Streams do entry boundary, jeśli boundary można związać z konkretnym source event bez lookaheadu.
2. Uzupełnić `ShadowV2EntryBoundaryPayload` wartościami:
   - `source_block_time`;
   - `source_tx_signature`;
   - `source_transaction_index`;
   - `source_instruction_index`;
   - ewentualnie `source_inner_instruction_index`, jeżeli semantyka `inner_group_index` zostanie jawnie zaakceptowana jako bounded approximation.
3. Dla exit/path dodać analogiczne pola do lifecycle/monitoring source record albo rozszerzyć source records tak, aby `MonitoringEngine` nie tracił `tx_index` i instruction provenance.
4. Nie wpisywać fake native `log_index`; w PR43-E wybrać jedną z dwóch ścieżek:
   - Option A: późniejsza rename/clarification `log_index_or_unknown` -> `log_message_index_or_unknown`, gdzie znaczenie to internal index z enumeracji Solana `meta.logMessages`;
   - Option B: zachować nazwę dla kompatybilności, ale jawnie udokumentować, że `log_index_or_unknown` nie jest Solana-native `logIndex`, tylko opcjonalny internal log message ordinal.
5. Zachować terminal truth jako `DERIVED`.

## Co nadal blokuje L2

L2 `RESEARCH_CANDIDATE` nadal blokują:

- brak pełnego chain-order dla pool-state boundary i fill boundary;
- brak rozstrzygniętego kontraktu `LOG_MESSAGE_INDEX_INTERNAL` dla pola `log_index_or_unknown`; Solana-native `logIndex` jest `NOT_APPLICABLE`, nie missing provider field;
- brak pełnego transportu `tx_index` / `instruction_index` / `block_time` z ingest do Shadow V2 boundary;
- brak account-data hash provenance; to nie było zakresem PR43-D;
- density/horizon i sample size; to nie było zakresem PR43-D.

## L3 boundary

PR43-D nie rozwiązuje L3. Następujące elementy pozostają L3-only:

- live-confirmed fills;
- realized slippage;
- quote/fill divergence;
- landing/failure/no-fill telemetry;
- Jito/live calibration.

## Final verdict

`EVENTORDERKEY_SOURCE_PARTIAL_PATH_PRESENT`

Uzasadnienie: current runtime/code ma częściowe źródła metadata chain-order i ma częściowo przygotowane pola handoff, ale pełne `EventOrderKey` dla L2 nie jest jeszcze dostępne. `SOLANA_NATIVE_LOG_INDEX` jest formalnie `NOT_APPLICABLE`; jeżeli zostanie użyty log ordinal, musi być oznaczony jako `LOG_MESSAGE_INDEX_INTERNAL` wyprowadzony z `meta.logMessages`, nie jako provider-native field. Pozostałe pola wymagają PR43-E wiring i nadal muszą fail-closed na `UNKNOWN`, gdy source nie istnieje.

## Approval flags

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `research_grade=false`;
- `live_equivalence=false`;
- `strategy_research_unblocked=false`.

## Zakres wykonania

Nie uruchomiono smoke ani burnina. Nie zmieniono runtime code. Nie zmieniono BUY/REJECT, Gatekeeper policy, selector runtime, TX/Jito/live path, R51, shadow_close_only ani active close.
