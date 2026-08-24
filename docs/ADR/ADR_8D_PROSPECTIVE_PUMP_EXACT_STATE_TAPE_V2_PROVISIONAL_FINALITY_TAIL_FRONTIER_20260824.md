# ADR-8D: Prospective Pump Exact-State Tape V2 — finalized frontier before a provisional tail

**Date:** 2026-08-24

**Status:** IMPLEMENTED / LOCAL VALIDATION PASS / SELF-REVIEW PASS / RAW PRESERVED / NO PROVIDER I/O

**Type:** ADR-8D / standalone prospective V2 offline authority / fail-closed PRXTAPE3 finalized-frontier correction

> The global template `/Gho/docs/ADR/ADR_8D_SZABLON.md` is not available in
> this environment. This document follows the local ADR-8D form used by the
> existing V2 authority changes.

## D0. Confirmed validator stop

After the committed chain-scoped invocation reconciliation, one offline-only
qualification of the immutable complete raw run
`pump-exact-state-v2-1787539185686-2720125` progressed to the next authority
gate and stopped before output publication on:

```text
V2 accepted cohort BlockMeta/full-block slot 441299049 lacks finalized Slot evidence
```

The raw remains unchanged and no exact output or `.partial` output was
created. The prior implementation required a finalized `Slot` parent for every
retained post-readiness BlockMeta/full-block pair. That condition conflated two
different states:

```text
interior missing finalized Slot evidence
  = possible lane loss
  = must fail closed

end-of-capture provisional Slot suffix
  = retained Processed/Confirmed Slot evidence, but no Finalized update yet
  = cannot be rooted or supply availability, but is not by itself loss
```

The old index intentionally stored only `Finalized` Slot parents, so it could
not make this distinction even when the retained protobuf contained an explicit
provisional Slot update.

## D1. Decision

The offline index now retains two separate facts per slot:

```text
finalized_parents
  = parent authority from Finalized Slot only

saw_nonfinalized_slot
  = existence-only evidence from Processed/Confirmed Slot
  = never a parent authority
```

The reconciler still first verifies every post-readiness BlockMeta/full-block
pair and the entire retained parent/blockhash chain. It then derives its
availability frontier from a contiguous finalized prefix.

A suffix may be excluded only if every suffix pair has retained provisional
Slot evidence and no later pair regains finalized authority. The following are
still errors:

- a BlockMeta/full-block pair with no Slot evidence at all;
- a finalized parent that is absent, conflicting or differs from BlockMeta;
- a finalized pair after a provisional suffix, which makes that suffix an
  interior finality-evidence gap;
- any local BlockMeta/full-block mismatch, missing pair, parent-slot break or
  parent-blockhash break anywhere in the retained post-readiness chain.

Thus a provisional tail remains immutable raw evidence but cannot enter:

- `rooted_slots()`;
- canonical account anchors;
- the successful-rooted mutation denominator;
- the reconciled full-block availability frontier; or
- outcome-blind window completeness.

## D2. Regressions

The correction adds:

1. a materializer regression where a fully parent-linked chain ends in a
   retained provisional Slot; it accepts only the preceding finalized pair as
   the frontier;
2. a materializer regression where finalized evidence appears after a
   provisional slot; it fails as an interior gap;
3. a public writer-to-qualifier-to-export PRXTAPE3 fixture with a finalized
   frontier followed by a Processed BlockMeta/full-block tail. It remains
   `Qualified`, exports one complete outcome-blind window from the preceding
   finalized frontier, and leaves no `.partial` artifacts;
4. preservation of the existing regression that a pair with no Slot record at
   all fails raw qualification.

## D3. Excluded scope

This correction does not modify:

- the immutable raw, start manifest, completion receipt, segment chain,
  capture config, provider credentials, operator worktree or previous logs;
- PRXTAPE3 storage/schema, recorder, Yellowstone request, readiness boundary,
  ProgramData authority, account source, GPA/snapshot/backfill/imputation or
  any provider I/O;
- actual-Pump invocation reconciliation, full-block pair/hash/parent-chain
  validation, canonical anchors, denominator/coverage/minimum gates,
  semantics/IDL, exact JSONL/window schemas or exporter outcome semantics;
- GO-D/V1, GO-E, Gatekeeper, OracleRuntime, execution, active runtime or
  strategy outcomes.

After a reviewed allowlist-only commit, the only operational action is one new
offline qualification of the already preserved raw with the committed
executable. If it finds absent Slot evidence or any other authority failure,
the run remains fail-closed; no capture retry or repair is implied.

```text
GO_D_SOURCE_AUTHORITY                = VERIFIED
EXTERNAL_GO_E_AUDIT_NOT_USED_AS_GATE = TRUE
```

## D4. Local validation

The complete locked/offline V2 matrix passed on the working diff:

```text
cargo fmt --all -- --check                                      PASS
cargo check --locked --offline -p seer --bin pump-exact-state-tape-v2
                                                                  PASS
cargo test --locked --offline -p ghost-core pump_research_exact_tape_v2
  --lib --no-fail-fast                                            5/5 PASS
cargo test --locked --offline -p seer research_exact_tape_v2
  --lib --no-fail-fast                                           82/82 PASS
cargo test --locked --offline -p seer research_exact_tape_v2_materializer
  --lib --no-fail-fast                                           27/27 PASS
cargo test --locked --offline -p seer research_exact_tape_v2_semantics
  --lib --no-fail-fast                                            9/9 PASS
cargo test --locked --offline -p seer grpc_connection::tests
  --lib --no-fail-fast                                           95/95 PASS
cargo test --locked --offline -p seer --bin pump-exact-state-tape-v2
  --no-fail-fast                                                  1/1 PASS
cargo build --locked --offline --release -p seer
  --bin pump-exact-state-tape-v2                                 PASS
target/release/pump-exact-state-tape-v2 --help                    PASS
git diff --check                                                  PASS
git diff --cached --check                                         PASS
untracked ADR whitespace check                                    PASS
```

The external, mode-`0600` validation log is:

```text
/protected/operator/pump-exact-state-v2-provisional-finality-tail-validation-20260824.log
SHA-256 = 7fa21a804549285ddf7887dd53da1f6027b9d337431526a864009cd4f413f0c9
```

The locally built, unsealed release executable at this stage is mode `0700`,
has `11,670,440` bytes and SHA-256:

```text
52487b887040c157420219185e7750ffe19ba9dacdc2b719a1cb777c34ba2af8
```

## D5. Neutral self-review

The reviewed change accepts no absent Slot evidence.  It accepts a
post-readiness BlockMeta/full-block suffix only after preserving a real
Processed or Confirmed Slot update for each such pair.  That update contributes
only an existence marker; it cannot supply a parent, canonical anchor, rooted
slot, denominator entry or availability timestamp.

The following fail-closed properties were checked against the implementation
and the regressions:

- a finalized parent still must be unique and exactly equal to the
  BlockMeta/full-block parent;
- a pair with no Slot evidence remains an error;
- a finalized pair following any provisional pair remains an error rather than
  allowing an interior finality gap;
- all retained pairs, including the provisional suffix, still undergo local
  BlockMeta/full-block identity comparison and cross-slot parent/blockhash
  chain validation;
- filtered/full-block Pump reconciliation and forward availability stop at the
  last fully finalized parent-linked pair, so the suffix cannot shrink a
  rooted denominator or manufacture an outcome window; and
- no recorder, Yellowstone request, PRXTAPE3 schema, source semantics,
  provider/RPC path, raw byte, active runtime or strategy behavior changed.

No additional concrete defect was found in this bounded correction.  The next
permitted operational action after an allowlist-only commit is one offline
requalification of the already preserved raw.  It will either continue into a
new immutable exact artifact or preserve the next typed fail-closed result;
it implies neither provider I/O nor a capture retry.
