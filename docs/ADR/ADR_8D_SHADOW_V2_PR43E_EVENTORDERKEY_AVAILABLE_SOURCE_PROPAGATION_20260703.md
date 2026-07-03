# ADR-8D: Shadow V2 PR43-E EventOrderKey available-source propagation

Data: 2026-07-03
Status: Accepted for implementation review
Scope: evidence-only / shadow-only source metadata propagation
Decision: `PR43E_EVENTORDERKEY_PARTIAL_SOURCE_PROPAGATED_STILL_BLOCKED`

## D1 - Problem

After PR43-D, Shadow V2 had a confirmed partial source path for `EventOrderKey`, but available source metadata was not propagated consistently into entry and exit/path evidence records. At the same time, unavailable metadata must not be faked, and partial provider/runtime metadata must not be treated as full chain-order proof.

## D2 - Context

Shadow V2 L1 executable simulation is already present. PR43-E is not a burnin PR and does not target L2 PASS. The work is limited to propagating real source metadata already available in runtime/evidence structures, or optional shadow-only metadata that can be carried without decision consumption.

Allowed source fields are:

- `block_time`;
- `source_tx_signature`;
- `transaction_index` / `tx_index`;
- `instruction_index` / `outer_instruction_index`.

Restricted fields are:

- `inner_instruction_index`, which remains `UNKNOWN` unless exact semantics are proven;
- `log_index_or_unknown`, which is not Solana-native `logIndex` and is internal-only if ever derived from enumerated `meta.logMessages`.

## D3 - Decision

Implement PR43-E as evidence-only propagation:

- entry boundary source metadata is carried into entry pool-state, entry attempt, and entry fill `EventOrderKey` only when an exact source join is present;
- exit/path lifecycle records receive optional shadow-only source fields and carry them into path sample, exit attempt, exit pool-state, and exit fill only when an exact source join is present;
- exact source join requires a non-empty `source_tx_signature`;
- partial source metadata without a source signature remains explicit `UNKNOWN`;
- local handoff signature is not reused as pool-state source signature;
- `inner_group_index` is not mapped to `inner_instruction_index_or_unknown`;
- missing log index is `NOT_APPLICABLE` for Solana-native logIndex and reserved for optional internal log-message ordinal only;
- terminal truth and derived after-state remain `DERIVED`;
- `has_complete_chain_order()` remains unchanged.

No new NLN streams were added.

## D4 - Rejected Alternatives

Rejected:

- adding broad NLN subscriptions in PR43-E;
- treating provider transaction metadata as exact instruction/log ordering proof without exact causal join;
- using `ix_count` or `iix_count` as instruction index;
- mapping `inner_group_index` to exact inner instruction index without accepted contract;
- using `event_seq_in_process` as a chain-order substitute;
- filling `log_index_or_unknown` from `event_ordinal`, `instruction_index`, or `inner_group_index`;
- loosening `has_complete_chain_order()`;
- changing Gatekeeper, BUY/REJECT, selector, live TX/Jito, R51, active close, or shadow close behavior;
- allowing provider source metadata to enter decision features;
- turning terminal truth into chain-observed evidence.

## D5 - Consequences

Positive consequences:

- `EventOrderKey` now has a guarded path for real source `block_time`, source signature, transaction index, and instruction index;
- entry and exit/path evidence families can improve when an exact causal source join exists;
- missing metadata remains auditable through explicit `UNKNOWN` and typed limitations;
- tests now lock in that handoff signatures and event sequence numbers cannot masquerade as chain-order proof.

Remaining limitations:

- current entry capture does not yet prove source join from AccountStateCore into the boundary payload;
- current exit/path lifecycle records do not yet receive source event metadata from monitoring records;
- `inner_instruction_index_or_unknown` remains unresolved;
- Solana-native `logIndex` remains not applicable;
- `account_data_hash`, density, and sample-size are unresolved;
- L2 remains blocked.

## D6 - Invariants

Preserved invariants:

- no BUY/REJECT change;
- no Gatekeeper policy change;
- no selector runtime change;
- no TX/Jito/live path change;
- no R51 touch;
- no `shadow_close_only`;
- no active close;
- no runtime approval;
- no research-grade;
- no live-equivalence;
- no strategy unlock;
- no MaterializedFeatureSet decision input change;
- no hidden decision consumption of Shadow V2 evidence;
- terminal truth remains `DERIVED`.

## D7 - Validation

Executed validation set:

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

The test suite must prove:

- available source `block_time` propagates only with exact source join;
- available source signature propagates only with exact source join;
- available transaction index propagates only with exact source join;
- available instruction index propagates only with exact source join;
- missing source metadata remains explicit `UNKNOWN`;
- handoff signature is not reused as pool-state source signature;
- `inner_group_index` is not silently treated as exact inner instruction index;
- `log_index_or_unknown` is not synthesized;
- `event_seq_in_process` is not accepted as L2 chain-order proof;
- terminal truth remains `DERIVED`;
- Shadow V2 evidence remains non-consumed by Gatekeeper/selector/live decision paths.

## D8 - Final

Final verdict:

`PR43E_EVENTORDERKEY_PARTIAL_SOURCE_PROPAGATED_STILL_BLOCKED`

Approval flags remain false:

- `runtime_approval=false`;
- `shadow_close_only_approval=false`;
- `active_close_approval=false`;
- `research_grade=false`;
- `live_equivalence=false`;
- `strategy_research_unblocked=false`.
