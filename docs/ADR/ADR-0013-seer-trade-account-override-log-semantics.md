# ADR-0013: Seer trade account-override log semantics

**Date:** 2026-03-20  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Log `Emitting PoolTransaction ... has_token_program=... has_global_config=... has_fee_recipient=...` in Seer can be misread as a transaction-validity or parser-health signal. In practice these fields are optional metadata attached to `TradeEvent`, and the binary parser only fills them for specific Pump.fun bonding-curve instruction layouts.

Most trades arriving from `grpc_global_stream` are decoded through parser branches that intentionally set these fields to `None` (notably PumpSwap and CPI/inferred trade variants). This creates frequent `false false false` sequences in logs even when the trade is otherwise valid and successfully forwarded.

## Decision

Treat `has_token_program`, `has_global_config`, and `has_fee_recipient` as **best-effort metadata presence flags**, not correctness flags.

Semantics by parser branch:
- `ParsedEventKind::Trade` for Pump.fun bonding-curve buy/sell: parser reads these accounts from the instruction account list and may populate them.
- `ParsedEventKind::CpiTrade`, `SwapTrade`, `CpiSwapBuy`, `CpiSwapSell`: parser intentionally emits `None` for these fields.
- Seer forwarding does not depend on these fields; they are logged and passed through as optional metadata only.
- Downstream buy-account override derivation in `ghost-launcher` can recover them only from prior successful buy transactions that actually carried the metadata.

## Architectural Impact

This clarifies that:
- Seer ingress telemetry and Shadow Ledger forwarding stay decoupled from optional override metadata.
- Cold-start / PumpSwap / CPI-heavy traffic naturally yields many `false false false` log lines.
- Operational diagnostics should not use these flags as a proxy for event validity, replay health, or parser success rate.

## Risk Assessment

**Rate:** Low

Primary risk is operator confusion, not runtime breakage. Misinterpreting these booleans could lead to incorrect incident diagnosis or unnecessary parser churn.

## Consequences

Benefits:
- Clear distinction between required trade identity fields and optional execution metadata.
- Fewer false positives during log review.
- Better understanding of why downstream shadow-buy overrides may be partially populated.

Trade-offs:
- Existing logs remain terse and do not explicitly name the parser branch that produced the event.
- Investigating a specific `false false false` still requires checking whether the trade came from PumpSwap/CPI/inferred decode paths.

## Alternatives Considered

1. Interpret these flags as parser correctness indicators.  
   Rejected because the code forwards valid trades from multiple branches that intentionally omit these fields.

2. Populate fallback defaults for all parser branches.  
   Rejected because synthetic defaults would blur provenance and could poison downstream override derivation.

3. Block forwarding when these fields are absent.  
   Rejected because this would break valid PumpSwap/CPI traffic and violate current Seer recovery/forwarding contracts.

## Validation Steps

1. Confirm Seer emission log reads the flags from `TradeEvent` optional fields in `off-chain/components/seer/src/lib.rs`.
2. Confirm Pump.fun bonding-curve decode populates the fields in `off-chain/components/seer/src/binary_parser.rs`.
3. Confirm CPI/PumpSwap/inferred branches set them to `None` in the same parser.
4. Confirm downstream `derive_buy_account_overrides()` in `ghost-launcher/src/oracle_runtime.rs` only harvests these values from prior successful buy transactions.
