# ADR-0077: PumpSwap Routed Coverage Gap Diagnosis

**Date:** 2026-04-03
**Status:** Accepted
**Author:** Ghost Father

## Context
Coverage investigation on Gatekeeper rows for pools `8zBDAaNz5f6Hiop3jEf9FX1ggGvNURTGedauBSwBgko` and `F1ZxU8xTAU81nN1FknK7A1Nis1B8GPzdMw1GiiEpRwy7` showed a large undercount versus exact-window RPC reconstruction. Rust parser / launcher paths needed verification for routed PumpSwap WSOL-base trades, especially transactions carrying PumpSwap CPI swap events alongside weaker ix-level `SwapTrade` candidates.

## Decision
The accepted diagnosis is:

1. `off-chain/components/seer/src/binary_parser.rs` raw-event dedup drops top-level PumpSwap `SwapTrade` too aggressively when a same-pool/same-side CPI event exists but the pool→mint registry entry is still missing.
2. This behavior is implemented in `has_matching_pumpswap_cpi()` and currently returns `true` on `None` registry lookup, which contradicts the nearby intent comment that unresolved CPI trades should not displace ix-level trades.
3. When the surviving CPI trade later fails mint resolution, `ghost-launcher/src/components/seer.rs` drops it before Event Bus forwarding because `trade_has_forwardable_identity()` requires both non-default pool and mint.
4. Gatekeeper itself does not reinterpret token raw units as SOL. It only consumes `PoolTransaction.volume_sol` from the bridge. Any dusting comes from upstream selection of a weak `SwapTrade` candidate with bad SOL extraction, not from Gatekeeper math.

## Architectural Impact
This diagnosis affects the Seer parser SSOT for PumpSwap trade selection, the session bridge forwardability contract, and Gatekeeper coverage observability. The failure mode is concentrated in routed / aggregator PumpSwap flows where mint resolution is delayed or ambiguous at raw dedup time.

## Risk Assessment
High. The system can undercount real trades, overstate dust, and silently drop routed PumpSwap activity before `Gatekeeper::update_tracking()`. This directly impacts coverage metrics and any policy decisions derived from observed tx counts.

## Consequences
The immediate consequence is that exact-window RPC truth can materially exceed Gatekeeper-observed counts on affected WSOL-base pools. The diagnosis narrows the bug to parser dedup plus pre-forward unresolved-trade drops, making remediation targeted. The trade-off is that relaxing raw dedup may temporarily allow more duplicate candidates until later candidate dedup resolves them safely.

## Alternatives Considered
- Treat Gatekeeper dust filtering as the primary bug: rejected because launcher bridge and Gatekeeper only use already-populated SOL lamports.
- Treat OKX routing as a separate unsupported path: rejected because parser tests and code already explicitly support routed / inner-instruction PumpSwap flows.
- Treat top-level PumpSwap decoding as universally wrong: rejected because CPI path is correct and top-level path is only problematic when it is selected or deduped against unresolved CPI state.

## Validation Steps
1. Add regression coverage for routed WSOL-base PumpSwap sells where CPI exists before pool→mint registry population.
2. Verify `parse_trades()` retains either the resolved CPI trade or the top-level `SwapTrade` until candidate dedup can compare fully-resolved trades.
3. Replay affected signatures and confirm:
   - no `mint=111...` trade reaches the session bridge,
   - no spurious unresolved-trade drop occurs,
   - Gatekeeper unique tx count matches exact-window RPC reconstruction,
   - dust count remains zero for the known affected windows.
