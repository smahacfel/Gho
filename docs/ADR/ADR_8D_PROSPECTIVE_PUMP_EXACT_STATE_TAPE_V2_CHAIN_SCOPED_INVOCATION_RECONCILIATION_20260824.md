# ADR-8D: Prospective Pump Exact-State Tape V2 — chain-scoped reconciliation of actual Pump invocations

**Date:** 2026-08-24

**Status:** IMPLEMENTED / LOCAL VALIDATION PASS / SELF-REVIEW PASS / RAW PRESERVED / NO PROVIDER I/O

**Type:** ADR-8D / standalone prospective V2 offline authority / fail-closed PRXTAPE3 full-block reconciliation

> The global template `/Gho/docs/ADR/ADR_8D_SZABLON.md` is not available in
> this environment. This document follows the local ADR-8D form used by the
> existing V2 authority changes.

## D0. Confirmed problem

The preserved complete PRXTAPE3 run
`pump-exact-state-v2-1787539185686-2720125` passed its corrected segment
terminality validation but stopped before exact-output creation on:

```text
V2 filtered Pump transaction lane differs from full-block Pump inventory
```

Read-only inspection established two distinct facts.

First, Yellowstone's `transactions.account_include = [Pump]` predicate is an
account-presence tap, not an instruction-execution predicate. It admits a
non-vote transaction if Pump appears in a static or loaded account vector,
including a transaction that never invokes Pump. The raw contains 119,052
such retained transaction messages, while the full blocks contain 50,320
non-vote actual Pump invocations.

Second, when both lanes are reduced to the same non-vote actual-Pump-invocation
predicate, all 50,242 shared locators have identical retained transaction
protobuf digests. The remaining differences are exclusively outside the
prospective complete chain:

- 78 full-block-only invocations are in slot `441297436`, exactly the
  readiness/warm-up slot, which is excluded by `slot > cohort_slots_strictly_after`;
- 5 filtered-only invocations are in slot `441299080`, immediately after the
  final captured complete full-block slot `441299079`, with no BlockMeta +
  full-block pair and therefore no rooted capability authority.

The existing validator compared all warm-up and trailing ingress records before
deriving the parent-linked full-block frontier. It therefore treated expected
establishment/closure skew as source evidence loss. Conversely, merely
discarding account-include traffic or accepting a global set mismatch would
weaken the proof and is forbidden.

## D1. Decision

The validator keeps two separate, immutable measurements:

```text
filtered_transaction_message_count
  = every retained primary transaction message
  = completion receipt transaction_messages census

comparable invocation inventory
  = non-vote transaction
  + actual Pump outer or inner instruction
  + slot in the complete parent-linked post-readiness BlockMeta/full-block chain
```

The same actual-invocation predicate is applied to the retained filtered
transaction payload and to each retained full-block transaction. The comparison
remains exact inside the authoritative chain:

```text
same slot
+ same transaction index
+ same signature
+ same complete SubscribeUpdateTransactionInfo BLAKE3
```

Any missing, extra, signature-drift, index-drift, or protobuf-digest drift
inside that chain remains a raw-authority error. The full parent chain is
validated before scope selection. Warm-up records and a tail that has not
reached a complete BlockMeta/full-block frontier remain retained raw evidence,
but cannot become rooted mutations or participate in the comparison.

## D2. Regressions

The correction includes:

1. an account-include-only filtered source record increments the immutable
   transaction-lane census but cannot enter the actual-Pump invocation map;
2. loaded-address and inner-instruction Pump calls remain detected, while vote
   transactions remain outside the requested `vote=false` comparison contract;
3. exact in-chain reconciliation still rejects missing, extra, signature,
   transaction-index, and retained-protobuf evidence drift;
4. a unit scope regression allows only a warm-up full-block invocation and an
   unreconciled trailing filtered invocation, while rejecting a missing
   in-chain invocation;
5. a public PRXTAPE3 writer-to-qualifier regression builds that exact
   warm-up/tail shape and reaches `Qualified` with no `.partial` exact output.

## D3. Excluded scope

This change does not modify:

- the preserved raw, start manifest, completion receipt, segments, captures,
  or previous qualification logs;
- PRXTAPE3 codec/schema, recorder, source request, config, ProgramData
  receipt path, provider roles, credentials, RPC, Yellowstone, GPA, snapshot,
  backfill, or imputation;
- the finalized Slot / BlockMeta / full-block parent-chain contract or the
  monotonic availability frontier;
- exact-state semantics, vendored IDL, accounts/anchors, denominator rules,
  coverage/minimum gates, exact JSONL/window schema, or exporter;
- GO-D/V1, GO-E, Gatekeeper, OracleRuntime, execution, active runtime, or
  strategy outcomes.

No provider I/O, new capture, preflight, raw alteration, export, or strategy
operation is part of this correction. After a reviewed allowlist-only commit,
the only permitted operational action is one new offline requalification of
the existing immutable raw using the new qualifier executable.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Verification performed

All verification remains local/offline:

```text
cargo fmt --all -- --check
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2 --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_materializer --lib --no-fail-fast
cargo test --locked --offline -p seer research_exact_tape_v2_semantics --lib --no-fail-fast
cargo test --locked --offline -p seer grpc_connection::tests --lib --no-fail-fast
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2 --no-fail-fast
cargo build --locked --offline --release -p seer --bin pump-exact-state-tape-v2
target/release/pump-exact-state-tape-v2 --help
git diff --check
git diff --cached --check
```

Then perform a neutral self-review of the whole diff. Only a clean
allowlist-only commit may authorize one further offline qualification of the
preserved raw; the result can still be `Blocked` and must then remain
fail-closed.

Completed locally on 2026-08-24:

- formatting and the locked/offline standalone CLI check passed;
- `ghost-core` V2 tests passed `5/5`;
- public Seer V2 tests passed `79/79`, including the real PRXTAPE3
  writer-to-qualifier warm-up/tail regression;
- materializer tests passed `25/25`;
- semantics tests passed `9/9`, gRPC tests `95/95`, and standalone CLI tests
  `1/1`;
- the locked/offline release build and `--help` passed;
- tracked and untracked ADR whitespace checks passed.

The complete local command log is owner-private at
`/protected/operator/pump-exact-state-v2-chain-scope-validation-20260824.log`
with SHA-256
`7d21ad27b0441d873e9615c9f1bcff455184221d78f4fbb0acc3f104a913181c`.

## D5. Neutral self-review

The implemented diff was reviewed against the preserved raw diagnosis and the
existing fail-closed contract.

- The all-message source-lane census remains independently bound to the raw
  completion receipt; dropping account-reference-only ingress from the
  comparable map cannot hide a missing source record.
- The full parent-linked BlockMeta/full-block chain is reconciled before the
  comparison scope is derived. A missing local pair, broken parent link or
  hash mismatch still fails before any scope selection.
- Every non-vote actual Pump outer or inner invocation in that complete chain
  is still compared by slot, transaction index, signature and retained
  protobuf digest. Missing, extra and drift regressions remain fail-closed.
- Warm-up and unreconciled tail records remain durable raw evidence but cannot
  enter rooted slots, anchors or the successful-rooted mutation denominator.
- The correction does not touch recorder behavior, request construction,
  PRXTAPE3 storage/schema, account anchors, semantics, coverage/minimum gates,
  exporter, provider I/O, capture authority or active Ghost runtime code.

No new evidenced local P0/P1/P2 was found. This is a self-review result only;
the next authority step is an allowlist-only commit followed by one offline
qualification of the preserved raw with the committed executable.
