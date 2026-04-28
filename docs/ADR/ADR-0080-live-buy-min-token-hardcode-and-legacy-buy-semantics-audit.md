# ADR-0080: Live BUY min-token hardcode and legacy buy semantics audit

**Date:** 2026-04-05
**Status:** Accepted
**Author:** Ghost Father

## Context
Production live BUYs were reported as spending the configured SOL budget while receiving effectively zero token quantity. Investigation targeted the authoritative launcher-trigger BUY path from `[trigger]` sizing config through OracleRuntime, trigger request preparation, Pump.fun instruction encoding, and Jito bundle submission.

## Decision
The live BUY path was identified as carrying a hardcoded `min_tokens_out = 1` into authoritative transaction construction. This value is passed unchanged into the Pump.fun instruction encoder and can become the exact token amount for legacy Pump.fun BUY variants. The launcher runtime also ignores the configured `trigger.slippage_tolerance` field entirely, so operator slippage settings never influence production live BUY construction.

## Architectural Impact
- `config.toml` sizing controls authoritative SOL spend only.
- Launcher-trigger BUY construction does not consume any authoritative token quote/min-out source.
- OracleRuntime metadata/buffered-tx enrichment can select `legacy_buy`, which makes the hardcoded `1` materially dangerous instead of merely permissive.
- Post-buy handoff and telemetry inherit the poisoned `min_tokens_out` value.

## Risk Assessment
Critical. The live BUY path can submit economically invalid trades, misreport intended token quantity, and hide the defect because Jito success logs report lamports/tip but not the encoded token amount.

## Consequences
- Easier transaction landing because slippage protection is effectively disabled.
- Harder economic correctness validation because config and UI imply protection that production code does not honor.
- Legacy/routed buy semantics remain unsafe until variant-specific encoding is validated against SSOT on-chain behavior.

## Alternatives Considered
- Treating the issue as only a Jito transport/logging defect was rejected because the malformed quantity originates before submission.
- Treating the issue as only a config-sizing bug was rejected because sizing correctly determines lamports; the quantity guard/encoding path is what collapses.

## Validation Steps
1. Reconstruct a live BUY request and assert the encoded instruction arguments for both `legacy_buy` and `routed_exact_sol_in`.
2. Verify launcher config deserialization exposes and consumes a single authoritative slippage/min-out input.
3. Run staging buys on known legacy and routed pools and compare intended token quantity vs encoded bytes vs on-chain received quantity.
4. Fail CI if authoritative live BUY construction ever emits `min_tokens_out = 1` without an explicit dust-mode flag.
