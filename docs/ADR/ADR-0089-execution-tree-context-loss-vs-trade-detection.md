# ADR-0089: Execution-tree context loss is real, but not the root cause of Pump trade loss

**Date:** 2026-04-09
**Status:** Accepted
**Author:** Ghost Father

## Context

A claim was raised against `off-chain/components/seer/src/binary_parser.rs` that the parser has the correct Pump decoder but a fundamentally broken execution-flow model:

- top-level parsing filters by Pump program only,
- inner instruction parsing ignores `InnerInstructionGroup.index`,
- CPI event decoding is gated by `is_pump_program(&prog)`,
- therefore Pump Mayhem flows allegedly "leak sideways" and lose 7-8% coverage.

The specific code under scrutiny was the selected `parse_geyser_transaction(...)` fragment, which is the backfill path used for decoded RPC/Geyser transactions.

## Decision

The claim is assessed as **partially correct architecturally, but incorrect as the primary explanation for missing Pump trade detection in the investigated incident**.

Accepted points:

1. `InnerInstructionGroup.index` explicitly means: "group of inner instructions belonging to one top-level instruction".
2. `binary_parser.rs` currently does **not** use `group.index` inside either live `parse_transaction_raw(...)` or backfill `parse_geyser_transaction(...)` when walking inner instructions.
3. Because of that, the parser loses **wrapper provenance / execution-tree context**:
   - it cannot attribute an inner Pump CPI to a specific outer wrapper program such as a hypothetical Mayhem outer instruction,
   - it cannot classify flow origin (`retail`, `aggregator`, `mayhem`, etc.) from this walker alone.
4. That is a real architectural limitation if the goal is flow attribution.

Rejected / overstated points:

1. The selected snippet is **not the live gRPC hot path** for the investigated rollout. It is the backfill path:
   - `PumpEvent::BackfillTransaction` → `parse_geyser_transaction(...)`
   - live transactions use `parse_transaction_raw(...)`.
2. The statement "outer filter = PUMP ONLY, therefore inner Pump under Mayhem is lost" is incorrect for the current walker design:
   - top-level outer instructions are filtered independently,
   - inner instructions are scanned in a separate pass across all inner groups,
   - if an inner CPI instruction's own program id is Pump.fun or PumpSwap, `decode_ix(...)` still runs even when the outer wrapper is non-Pump.
3. The statement "event parsing depends on program_id == PUMP, so Mayhem events are skipped" is only conditionally true:
   - it is true for wrapper-native non-Pump events,
   - it is **not** true for normal Pump/PumpSwap self-CPI Anchor events emitted from the Pump program itself, because those inner instructions still carry the Pump program id and are decoded by the current code.
4. The claim that `dedup_trade_events(...)` is the likely killer does not apply to the selected backfill snippet because `parse_geyser_transaction(...)` does not call `dedup_trade_events(...)`.
5. In the live path where dedup does run, the current implementation drops ix-level `Trade` / `SwapTrade` only when a matching higher-confidence CPI trade exists, and retains the CPI event. Unit tests explicitly assert this preference.

Therefore the correct interpretation is:

- **real limitation:** missing execution-tree provenance,
- **not proven:** missing Pump trade detection from Mayhem wrapper flows,
- **not supported for the investigated incident:** no evidence that the coverage gap was caused by this parser behavior.

## Architectural Impact

This decision clarifies the difference between two problems that were being conflated:

1. **Trade detection** — whether a Pump/PumpSwap trade is seen at all.
2. **Flow attribution** — whether the parser knows that a detected trade came from a Mayhem / aggregator / wrapper outer instruction.

Current parser behavior is acceptable for the first problem in known Pump self-CPI patterns, but weak for the second.

If future product requirements include wrapper-origin analytics or Mayhem-specific routing/telemetry, the parser should evolve toward an execution-tree-aware walker that:

- threads `group.index` into parsing,
- attaches outer program provenance to inner decoded events,
- optionally tags trades with flow type while preserving current trade detection behavior.

## Risk Assessment

**Rate:** Medium

- **Low** risk that the current conclusion for the investigated sample is wrong because of this exact issue.
- **Medium** risk that future analytics, attribution, or specialized wrapper support will be incomplete without execution-tree context.
- **Medium** risk of false narratives during debugging if provenance loss is misdiagnosed as trade-loss.

## Consequences

What becomes easier:

- Debugging can cleanly distinguish "we saw the trade" from "we know who wrapped the trade".
- The team can reject unsupported claims that outer non-Pump wrappers automatically cause missed Pump CPI trades.

What becomes harder:

- If wrapper-specific behavior must be modeled, the current walker is insufficient and requires a deliberate redesign.
- Future changes must preserve CPI trade detection while adding provenance, not accidentally regress coverage.

## Alternatives Considered

### 1. Accept the claim as-is

Rejected because it overstates the effect of ignoring `group.index` and incorrectly treats outer filtering as proof that inner Pump CPIs are dropped.

### 2. Dismiss the claim entirely

Rejected because the criticism about losing execution-tree provenance is legitimate and important for future flow attribution work.

### 3. Treat dedup as the primary suspect

Rejected because the selected function does not dedup, and the live dedup logic explicitly preserves higher-confidence CPI trade events.

## Validation Steps

1. Verify path selection in `PumpParser::parse(...)`:
   - `Transaction` → `parse_transaction_raw(...)`
   - `BackfillTransaction` → `parse_geyser_transaction(...)`
2. Verify semantic contract of `InnerInstructionGroup.index` in `types.rs`.
3. Verify that both live and backfill walkers scan all inner groups without using `group.index`.
4. Verify that inner Pump CPI instructions still pass `is_pump_program(&prog)` independently of the outer wrapper program.
5. Verify dedup behavior through unit tests showing CPI preference over ix-level duplicates.
6. If provenance becomes a requirement, implement an execution-tree-aware walker and add regression tests for:
   - non-Pump outer wrapper + inner Pump buy,
   - non-Pump outer wrapper + inner Pump self-CPI event,
   - mixed wrapper flows where attribution must survive dedup.
