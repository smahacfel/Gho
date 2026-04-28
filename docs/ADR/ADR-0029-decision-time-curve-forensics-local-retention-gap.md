# ADR-0029: Decision-time curve forensics local retention gap

**Date:** 2026-03-23  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

A forensic request required recovering the full Pump.fun bonding-curve state for a specific BUY decision logged at `2026-03-22T22:42:42.731210800Z` for pool `5iMdy4BkHhyUJP3wfXFZJnmvC8E5BG5JcKkdCSARHyt3` / mint `ExgLHVovgxV3MYs24rBePnr2761D1RnKtRWAE1Uqpump`.

The repository stores decision logs with derived curve metrics (`current_market_cap_sol`, gatekeeper `bonding_progress_pct`, `curve_data_known`, `curve_finality`) but not the full reserve tuple. The authoritative sources for raw state are:
- WAL `ShadowLedgerCurveUpdateRecord`
- ShadowLedger disk snapshots
- selected text logs when they emit curve seed data

Local artifact inspection showed:
- retained WAL segments end at `segment-1773966115610.wal` (`2026-03-20T00:21:55.610Z`)
- the target decision uses `first_seen_ts_ms=1774219353836` (`2026-03-22T22:42:33.836Z`)
- the nearest retained main snapshot is `shadow_ledger_snapshot_1774220848623.bin` (`2026-03-22T23:07:28.623Z`), ~24.91 minutes later
- neither retained snapshots nor retained WAL contain the target mint
- text logs do not contain a matching `CURVE_SEED_RPC_OK` or equivalent raw reserve emission for this mint

## Decision

Treat local recovery of full decision-time curve state as impossible once all three conditions hold:
1. retained WAL does not reach the decision timestamp,
2. retained snapshots do not contain the mint, and
3. text logs do not contain a raw reserve emission for the mint.

In that state, only two categories of output are considered valid:
- a precise statement of the local retention gap with artifact evidence, and
- algebraic reconstruction of fields that are uniquely determined by logged SSOT metrics and canonical genesis constants.

For the cited BUY decision, local evidence supports exact recovery only of:
- record identity and timestamps,
- `curve_data_known=true`,
- `curve_finality=provisional`,
- derived metric formulas,
- reconstructed virtual reserves from logged market cap and gatekeeper progress.

It does **not** support a claim of exact decision-time WAL-backed `real_sol_reserves`, `real_token_reserves`, or `last_update_ts_ms`.

## Architectural Impact

This clarifies the observability contract between:
- decision logging,
- WAL retention,
- ShadowLedger snapshot retention,
- runtime text logging.

The broader system currently allows a decision record to survive after the corresponding authoritative raw curve artifact has already aged out locally.

## Risk Assessment

**Rate: Medium**

Regression risk to runtime behavior is low because this ADR documents an analysis boundary, not a runtime logic change. The operational risk is medium because post-incident investigation can be blocked by retention windows even when decision logs remain available.

## Consequences

- Easier: forensic responses can distinguish cleanly between exact recovery and algebraic reconstruction.
- Harder: some historical BUY decisions cannot be fully reconstructed from local artifacts alone.
- Operationally, future forensic completeness depends on longer WAL/snapshot retention or richer decision-time logging.

## Alternatives Considered

### 1. Infer full raw state from decision log alone
Rejected because only some fields are mathematically determined by the logged metrics.

### 2. Treat nearest later snapshot as exact decision-time truth
Rejected because the nearest retained snapshot is ~24.91 minutes later and does not contain the target mint.

### 3. Claim exact real reserves from protocol intuition
Rejected because no local authoritative artifact backs that claim for this record.

## Validation Steps

- Confirm the target decision record exists in `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` line 366.
- Confirm retained WAL range ends before the decision timestamp.
- Confirm retained snapshots do not contain the target mint.
- Confirm text logs do not contain the target mint or pool with raw reserve emission.
- Use canonical genesis constants plus logged `bonding_progress_pct` and `current_market_cap_sol` only for explicitly labeled algebraic reconstruction.
