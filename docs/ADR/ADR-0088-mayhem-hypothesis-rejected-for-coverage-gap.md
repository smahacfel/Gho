# ADR-0088: Mayhem hypothesis rejected for sample coverage gap

**Date:** 2026-04-09
**Status:** Accepted
**Author:** Ghost Father

## Context

During investigation of the apparent coverage deficit for pool `CKHSsL9dzxJL67PxTYgi2G7iq2Cq3ggnBVAa4wNv1uVZ` / mint `3X3o6wkQA1nqhLRqXyFnDRTnpzo5w9xwpDqPN5r9pump`, an alternative hypothesis emerged:

- the "missing" transactions might be caused by Pump.fun's Pump Mayhem bot,
- those transactions might interact with Pump Mayhem program `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e`,
- and the current on-chain parser/auditor might fail to classify such traffic as trades.

The repository was analyzed at three layers:

1. runtime evidence for the affected pool (`DANE_COVERAGES.md`, full console logs, coverage slice),
2. parser and bridge code in Seer / launcher,
3. direct RPC inspection of suspect signatures.

## Decision

The Mayhem hypothesis is **rejected for this incident/sample**.

Accepted findings:

1. `off-chain/components/seer/src/binary_parser.rs` only treats two program families as trade sources:
   - `PUMP_FUN_PROGRAM_ID`
   - `PUMP_SWAP_PROGRAM_ID`
2. `is_pump_program()` is explicitly:
   - `is_pump_fun_program(p) || p == PUMP_SWAP_PROGRAM_ID`
   - therefore a standalone Mayhem program ID is currently **not** parsed as a trade source.
3. The `TradeEvent.is_mayhem_mode` field is **not** an active classification path:
   - on the on-chain parser path it is populated as `None`,
   - in `types.rs` it is documented as PumpPortal metadata "passed through for future analysis",
   - in launcher bridge code it is only forwarded, not used for gating, routing, or emit decisions.
4. The affected rollout traffic is dominated by `grpc_global_stream` runtime events, not PumpPortal-originated Mayhem flags.
5. The anomalous signature `48aWWJgkh2nnAZXUfEqs4cGURt9pAJaBAeoSteNHGBRGYfP9N28Aj3drug7wFMb9h32efwequjJNoLJXRXYYqh1K` is not a Mayhem-program transaction:
   - RPC inspection shows a standard Pump.fun BUY (`programId=6EF8...`),
   - the tx failed with `Custom 6002` / `TooMuchSolRequired` (slippage),
   - it was enriched by parser but never emitted as a landed trade.
6. A slow RPC sample across the suspect signature set (outside-window + non-emitted set) produced repeated checks with zero observed touches of `MAyhSmzXzV1pTf7LsNkrNwkWKTo4ougAJ1PPg47MD4e` in the inspected subset before public RPC throttling became the limiting factor.

Therefore the sample coverage deficit is currently attributed to:

- audit/window timing mismatch for the majority of the apparent gap,
- one failed standard Pump.fun BUY transaction,
- **not** to unparsed Pump Mayhem traffic in this specific pool sample.

## Architectural Impact

This decision affects the interpretation of the current coverage incident and clarifies the parser boundary:

- Seer parser SSOT currently recognizes Pump.fun + PumpSwap only.
- PumpPortal `isMayhemMode` is telemetry, not logic.
- Coverage/audit scripts that rely on `classify_trade()` also assume Pump.fun-program participation for trade truth.

If Pump Mayhem introduces a distinct executable path in future traffic, support must be added consistently in:

- parser program allowlist,
- instruction/event decoding,
- coverage truth classifier,
- tests covering mixed Pump.fun / PumpSwap / Mayhem activity.

## Risk Assessment

**Rate:** Medium

- **Low** risk to the current root-cause conclusion for this pool, because inspected evidence does not show Mayhem-program participation.
- **Medium** forward-looking risk, because a true standalone Mayhem instruction path would currently be invisible to both parser classification and RPC trade truth logic.
- No account layout or SSOT changes were made in this analysis.

## Consequences

What becomes easier:

- The current incident can be narrowed to timing/audit methodology plus one failed buy.
- Operators should stop treating Mayhem as the leading explanation for this specific pool sample.

What becomes harder:

- Future Mayhem-specific support, if required, must be added in a synchronized way across parser and coverage tooling.
- Existing metrics can falsely reassure operators if a new program family appears without explicit support.

## Alternatives Considered

### 1. Accept the Mayhem hypothesis immediately

Rejected because repository code does not recognize the Mayhem program as a trade source and the inspected suspect signatures did not show Mayhem-program participation.

### 2. Attribute the entire deficit to parser blindness

Rejected because signature-for-signature comparison already showed that most of the apparent deficit is emitted outside the audit window, not absent from runtime output.

### 3. Treat `is_mayhem_mode` as proof of active support

Rejected because code inspection shows the field is only metadata forwarding from PumpPortal and is not used to classify or emit on the on-chain path.

## Validation Steps

1. Verify parser gate:
   - inspect `binary_parser.rs::is_pump_program()`.
2. Verify Mayhem flag semantics:
   - inspect `types.rs` and `pumpportal_connection.rs` handling of `is_mayhem_mode`.
3. Verify launcher bridge behavior:
   - inspect `ghost-launcher/src/components/seer.rs::trade_event_to_pool_transaction()`.
4. Verify coverage classifier boundary:
   - inspect `tools/fetch_pool_trade_counts.py::classify_trade()`.
5. Verify suspect transaction anatomy:
   - inspect RPC transaction data for `48aWW...` and other suspect signatures.
6. If Mayhem support is later required, add explicit regression tests proving:
   - Mayhem-program instructions are recognized,
   - Mayhem-origin trades contribute to both runtime emission and RPC truth denominator.
