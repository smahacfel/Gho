# ADR-0007: Seer Early-Discard Audit and WAL Replay Verification

**Date:** 2026-03-19  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

A verification pass was requested for three claimed changes:
- early discard of pre-session pool trades before expensive Borsh deserialization in `seer`
- WAL startup crash hardening for schema-incompatible records in `ghost-core`
- confirmation that the relevant logic is actually present in `binary_parser.rs`, `lib.rs`, and `wal.rs`

The review had to determine not only whether the code exists, but whether execution order and test coverage make the fixes complete and behaviorally correct.

## Decision

The audit concluded:

1. The WAL replay schema-mismatch hardening is present and correctly implemented in `ghost-core/src/wal.rs`.
   - `replay_segment()` catches `bincode::deserialize::<WalRecord>(&buf)` failures
   - incompatible records are skipped with a warning
   - metric `wal_replay_schema_skip_total` is incremented
   - replay continues instead of aborting startup

2. The `seer` early-discard change is only partially correct.
   - `session_pools` tracking exists in `off-chain/components/seer/src/lib.rs`
   - `peek_trade_pool_id()` exists in `off-chain/components/seer/src/binary_parser.rs`
   - the discard guard is present before `parse_trades()`
   - however, `Seer::process_event()` invokes `parse_initialize_pool(&event)?` before that guard
   - `BinaryParser::parse_initialize_pool()` calls `parse_pump_events()` which performs full transaction parsing, including trade-path decoding work

Therefore the stated requirement "discard pre-session pool trades before Borsh deserialization" is not fully satisfied by the current call order.

3. Existing tests are insufficient to prove the intended optimization.
   - `test_session_start_slot_rejects_old_pools` passes, but only validates slot gating for old pools
   - WAL regression tests for roundtrip and truncated tail pass
   - no focused regression test was found for schema-incompatible WAL record skipping with continued replay
   - no focused regression test was found proving that pre-session trades are discarded before expensive parser work

## Architectural Impact

This decision affects the event ingestion hot path in `seer` and the recovery path in `ghost-core`.

- `seer` remains functionally guarded against forwarding some unwanted trade traffic, but it does not yet realize the intended CPU-saving boundary because parsing work is entered too early.
- `ghost-core` WAL replay is more robust against historical/legacy record schemas and should no longer fail startup on that mismatch class.
- Future refactors of `Seer::process_event()` and `BinaryParser::parse_initialize_pool()` must treat parse ordering as a contract, not just the presence of a filter helper.

## Risk Assessment

**Rate:** Medium

- **Seer early-discard risk:** Medium
  - The code creates a false sense of completion: the guard exists, but expensive parser work still occurs earlier than intended.
  - This may preserve unnecessary CPU overhead and parser side effects on pre-session trades.
- **WAL replay risk:** Low to Medium
  - The implementation is correct for the reviewed path, but absence of a schema-skip regression test leaves future regressions possible.

## Consequences

### Positive
- The verification establishes that the WAL fix is real and currently non-regressive against existing replay tests.
- The audit prevents the team from assuming the `seer` optimization is complete when it is not.

### Negative
- Additional refactor work is still required to move discard logic ahead of the full parse path.
- Additional regression tests are required to make this behavior durable.

## Alternatives Considered

1. **Accept the current `seer` implementation as complete because the guard exists**
   - Rejected because execution order proves that the expensive parser path is already entered before the guard.

2. **Treat existing session slot and WAL tests as sufficient coverage**
   - Rejected because they do not directly validate the newly claimed behavior boundaries.

3. **Refactor only `peek_trade_pool_id()` coverage without changing `process_event()` ordering**
   - Rejected because helper correctness alone does not guarantee the intended optimization boundary.

## Validation Steps

The following checks were performed:
- inspected `off-chain/components/seer/src/lib.rs`
- inspected `off-chain/components/seer/src/binary_parser.rs`
- inspected `ghost-core/src/wal.rs`
- confirmed presence of `peek_trade_pool_id`, `session_pools`, and WAL schema-skip logic
- ran:
  - `cargo test -p seer session_start_slot_rejects_old_pools -- --nocapture`
  - `cargo test -p ghost-core wal_append_and_replay_roundtrip -- --nocapture`
  - `cargo test -p ghost-core wal_replay_tolerates_truncated_tail -- --nocapture`
- observed all three tests passing
- confirmed that no dedicated regression test currently proves early discard happens before full parser/Borsh work
- confirmed that no dedicated regression test currently proves schema-incompatible WAL records are skipped while later records still replay
