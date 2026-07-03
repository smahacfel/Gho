# ADR-8D: Shadow V2 PR43-D EventOrderKey source availability audit

Data: 2026-07-03
Status: Accepted for review
Scope: report-only / audit-only
Decision: `EVENTORDERKEY_SOURCE_PARTIAL_PATH_PRESENT`

## D1 — Problem

Po PR43-C Shadow V2 ma poprawną klasyfikację `EventOrderKey`, ale L2 `RESEARCH_CANDIDATE` nadal blokują explicit `UNKNOWN` chain-order components. Trzeba ustalić, czy brakujące komponenty można pozyskać z obecnego provider/ingest/runtime handoff, czy wymagają rozszerzenia upstream evidence surface.

## D2 — Context

L1 diagnostic execution simulation działa dla entry, exit i terminal executable PnL. To nadal nie jest research-grade. L2 wymaga silniejszego temporal/no-lookahead proof, a `event_seq_in_process` nie może zastąpić chain order.

PR43-D jest audytem. Nie implementuje runtime wiring i nie uruchamia burnina.

## D3 — Decision

Przyjmujemy verdict:

`EVENTORDERKEY_SOURCE_PARTIAL_PATH_PRESENT`

Źródła dla części komponentów istnieją:

- `GeyserEvent::Transaction` ma `block_time` i `signature`;
- `TradeEvent` / `PoolTransaction` mają `signature`, `tx_index`, `event_ordinal`, `outer_instruction_index`, `inner_group_index`;
- NLN Program Streams normalizer obsługuje `tx_index`, `block_time` i `instruction_index`;
- `ShadowV2EntryBoundaryPayload` ma pola do przeniesienia source metadata.

Jednocześnie aktualny Shadow V2 runtime nie doprowadza tych pól do pełnego `EventOrderKey` dla wszystkich rodzin eventów. Dla `log_index` obowiązuje osobna korekta semantyczna: Solana nie ma natywnego EVM-style `logIndex`. Wewnętrzny log ordinal może być co najwyżej wyprowadzony przez enumerację `meta.logMessages` albo custom parser/indexer logic.

## D4 — Rejected alternatives

Odrzucono:

- traktowanie `event_seq_in_process` jako substytutu chain order dla L2;
- wpisywanie fake `signature`, `tx_index`, `instruction_index`, `inner_instruction_index` lub provider-native `log_index`;
- traktowanie braku Solana-native `logIndex` jako problemu providera;
- degradację terminal truth z `DERIVED` do rzekomo chain-observed source;
- użycie live/terminal/outcome data jako temporal proof dla pre-entry/pre-exit evidence;
- automatyczne przyznanie research-grade po samym PR43-B/PR43-C.

## D5 — Consequences

PR43-E może być ograniczonym implementation PR dla dostępnych pól:

- propagate `block_time`, `source_tx_signature`, `transaction_index`, `instruction_index` tam, gdzie source istnieje i da się go causalnie powiązać z boundary;
- rozstrzygnąć semantykę `inner_group_index` względem `inner_instruction_index_or_unknown`;
- wybrać jedną ścieżkę dla obecnego pola `log_index_or_unknown`:
  - Option A: rename/clarification do `log_message_index_or_unknown`, gdzie znaczenie to internal index z enumeracji Solana `meta.logMessages`;
  - Option B: zachować nazwę dla backward compatibility, ale udokumentować, że nie jest to Solana-native `logIndex`, tylko opcjonalny internal log message ordinal;
- nie oczekiwać natywnego `logIndex` od NLN/Geyser/Program Streams.

L2 nadal pozostaje zablokowane, dopóki temporal audit, account-data hash, density i sample size nie przejdą swoich bramek.

## D6 — Invariants

Zachowane invariants:

- no BUY/REJECT change;
- no Gatekeeper policy change;
- no selector runtime change;
- no TX/Jito/live path change;
- no R51 touch;
- no shadow_close_only;
- no active close;
- no runtime approval;
- no research-grade;
- no live-equivalence;
- no strategy unlock.

## D7 — Validation

Wymagane i wykonane walidacje dla PR43-D:

- CSV parser check dla `reports/selector/shadow_v2_pr43d_event_order_source_matrix.csv`;
- `git diff --check -- <PR43-D files>`;
- `git diff --cached --check`;
- forbidden staged-file guard.

Cargo nie jest wymagane, ponieważ PR43-D jest report-only i nie dotyka kodu.

## D8 — Final

Final verdict:

`EVENTORDERKEY_SOURCE_PARTIAL_PATH_PRESENT`

Approval flags pozostają false:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `research_grade=false`;
- `live_equivalence=false`;
- `strategy_research_unblocked=false`.
