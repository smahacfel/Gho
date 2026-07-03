# Raport: Shadow V2 PR43-E EventOrderKey available-source propagation

Data: 2026-07-03
Status: implementation evidence report
Final verdict: `PR43E_EVENTORDERKEY_PARTIAL_SOURCE_PROPAGATED_STILL_BLOCKED`

## Zakres

PR43-E jest waskim implementation PR dla Shadow V2 evidence-only propagation. Zmiana nie przyznaje L2 PASS i nie zmienia decyzji runtime.

In scope:

- propagate already available source metadata into Shadow V2 `EventOrderKey`;
- keep missing source metadata as explicit `UNKNOWN`;
- document rejected source joins;
- keep terminal truth and derived after-state as `DERIVED`;
- keep `has_complete_chain_order()` unchanged.

Out of scope:

- no broad NLN subscription expansion;
- no new stream added;
- no BUY/REJECT change;
- no Gatekeeper policy change;
- no selector runtime change;
- no TX/Jito/live path change;
- no R51 change;
- no `shadow_close_only`;
- no active close;
- no runtime approval;
- no research-grade;
- no live-equivalence;
- no strategy unlock.

## Fields propagated

The following fields can now be propagated into Shadow V2 `EventOrderKey` when an exact source join is present:

- `block_time`;
- `source_tx_signature`;
- `tx_index` / `transaction_index`;
- `outer_instruction_index` / `instruction_index`.

Entry-side propagation covers:

- `POOL_STATE_SAMPLE` / `ENTRY_POOL_STATE_BEFORE`;
- `ENTRY_ATTEMPT`;
- `ENTRY_FILL`.

Exit/path-side propagation covers:

- `PATH_SAMPLE`;
- `EXIT_ATTEMPT`;
- `EXIT_POOL_STATE_BEFORE`;
- `EXIT_FILL`.

Exit/path source fields were added as optional shadow-only metadata on lifecycle records. They are not decision inputs and are serialized only when present.

## Exact source joins

An exact source join is accepted only when a non-empty `source_tx_signature` is present on the source boundary or lifecycle record. When that condition is met, the code may carry source `block_time`, transaction index, and instruction index into the related `EventOrderKey`.

This PR intentionally does not treat a local handoff signature as chain-observed pool-state source signature.

Test coverage proves that a handoff signature is not reused as the pool-state source signature.

## Source joins not proven

The current entry boundary capture path still does not prove a source event join from AccountStateCore into `ShadowV2EntryBoundaryPayload`. Therefore the active entry capture path emits:

- `ENTRY_BOUNDARY_SOURCE_JOIN_NOT_PROVEN`;
- explicit `UNKNOWN` for missing source fields.

The current exit/path lifecycle records do not yet receive causally tied source event metadata from runtime monitoring. Therefore exit/path records without `source_tx_signature` emit:

- `EXIT_PATH_SOURCE_JOIN_NOT_PROVEN`;
- explicit `UNKNOWN` for missing source fields.

Partial metadata without source signature is not enough. A boundary with `block_time`, `tx_index`, or `instruction_index` but no source signature remains fail-closed as `UNKNOWN`.

## Fields still UNKNOWN

The following remain `UNKNOWN` unless a future exact source join supplies them:

- entry `source_block_time`;
- entry `source_tx_signature`;
- entry `source_transaction_index`;
- entry `source_instruction_index`;
- exit/path `source_block_time`;
- exit/path `source_tx_signature`;
- exit/path `source_transaction_index`;
- exit/path `source_instruction_index`;
- `inner_instruction_index_or_unknown`.

This is intentional. PR43-E wires the evidence surface and guards the semantics, but it does not invent unavailable source fields.

## inner_group_index decision

`inner_group_index` is rejected as an exact `inner_instruction_index_or_unknown` source in PR43-E.

Default limitation:

`INNER_GROUP_INDEX_NOT_EXACT_INNER_INSTRUCTION_INDEX`

Therefore `inner_instruction_index_or_unknown` remains `UNKNOWN` unless a future PR formally proves exact semantics.

## log_index / log_message_index decision

Solana has no native EVM-style `logIndex`.

PR43-E does not synthesize `log_index_or_unknown` from:

- `event_ordinal`;
- `event_seq_in_process`;
- `tx_index`;
- `instruction_index`;
- `inner_group_index`;
- `ix_count`;
- `iix_count`.

The current field name `log_index_or_unknown` is treated as backward-compatible naming only. It is not Solana-native `logIndex`. It is reserved for optional internal `LOG_MESSAGE_INDEX_INTERNAL` if raw `meta.logMessages` are enumerated by our parser or indexer.

When no internal log message ordinal exists, the component is `NOT_APPLICABLE`, and the report emits:

- `SOLANA_NATIVE_LOG_INDEX_NOT_APPLICABLE`;
- `LOG_MESSAGE_INDEX_INTERNAL_UNAVAILABLE`.

## has_complete_chain_order unchanged

`has_complete_chain_order()` was not loosened.

`log_index_or_unknown = NOT_APPLICABLE` does not make chain order complete. L2 chain-order proof still requires stronger components than PR43-E can provide today.

## Terminal truth

Terminal truth and derived after-state remain `DERIVED`.

No terminal truth record is promoted to chain-observed evidence.

## Event families improved

Improved by available-source propagation plumbing and tests:

- `POOL_STATE_SAMPLE` / `ENTRY_POOL_STATE_BEFORE`;
- `ENTRY_ATTEMPT`;
- `ENTRY_FILL`;
- `PATH_SAMPLE`;
- `EXIT_ATTEMPT`;
- `EXIT_POOL_STATE_BEFORE`;
- `EXIT_FILL`.

Still derived:

- terminal truth;
- derived after-state.

## Why L2 is still blocked

PR43-E does not solve:

- `account_data_hash`;
- path density;
- sample size;
- realized slippage;
- quote/fill divergence;
- live failed/no-fill telemetry;
- research-grade temporal ordering;
- live-equivalence.

The expected and accepted verdict is therefore:

`PR43E_EVENTORDERKEY_PARTIAL_SOURCE_PROPAGATED_STILL_BLOCKED`

## Approval flags

All approval flags remain false:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `research_grade=false`;
- `live_equivalence=false`;
- `strategy_research_unblocked=false`.

## Validation

Required validations for PR43-E:

Executed validations:

- `cargo test -p ghost-brain shadow_v2_event_order -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_event_order -- --nocapture` - PASS;
- `cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture` - PASS;
- `cargo check -p ghost-brain` - PASS;
- `cargo check -p ghost-launcher` - PASS;
- `cargo fmt --check` - PASS;
- `python3 -m py_compile scripts/shadow_v2_temporal_no_lookahead_audit.py` - PASS;
- `git diff --check` - PASS;
- `git diff --cached --check` - PASS;
- forbidden staged-file guard - PASS.

## Final

PR43-E propagates available source metadata only when the source join is exact, keeps unavailable components explicit, rejects `inner_group_index` as exact inner instruction index, treats Solana-native `logIndex` as not applicable, preserves terminal truth as derived, and does not grant L2.
