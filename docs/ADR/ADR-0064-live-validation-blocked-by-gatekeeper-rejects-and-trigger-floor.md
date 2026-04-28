# ADR-0064: Live Validation Blocked by Gatekeeper Rejects and Trigger Floor

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

After implementing and verifying the source-level fixes for:
- bootstrap `market_cap=0.0` false rejects,
- legacy BUY/log-routing mismatch,
- BUY-log routing and shadow metadata hardening,
- readiness ordering around `initial_liquidity_sol` backfill,
- dispatch-error classification for balance/bulkhead failures,

we validated the freshly compiled `ghost-launcher` binary against the active production-like runtime configuration at `/root/Gho/config.toml`.

The live validation established the following facts:

1. The compiled binary starts successfully and ingests live `seer` traffic.
2. The active runtime configuration is `paper + shadow_only`.
3. Preflight fails the trigger capital guard:
   - wallet balance: `0.007327349 SOL`
   - emergency floor: `0.008000000 SOL`
   - buffer: `0.002000000 SOL`
   - max position size: `0.001000000 SOL`
   - required reserve+trade budget: `0.011000000 SOL`
4. During live observation the runtime continued to append fresh decision rows, proving the binary was active.
5. During the same observation window no fresh rows were appended to:
   - `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`
   - `logs/shadow_run/buys.jsonl`
6. Fresh decisions observed in the run window were rejects only (`REJECT_CORE_FAIL`, `REJECT_HARD_FAIL`, `TIMEOUT_PHASE1`) and did not reach a fresh BUY/shadow execution branch.

This means the runtime validation did not produce a new live artifact capable of exercising the newest BUY-path readiness/error-classification fixes end-to-end.

## Decision

We treat the current state as an **operational validation blocker**, not as evidence of a new code regression.

Specifically:
- We will not claim end-to-end live validation of the newest BUY-path fixes unless a fresh candidate reaches the BUY/shadow branch.
- We will use the current run as proof that the binary is alive and decision logging is active, but not as proof that the new BUY-path branch was exercised.
- We will treat the trigger floor/budget shortfall and all-reject market window as the two active blockers for conclusive live validation.
- We will not silently change runtime risk parameters or weaken the trigger guard during validation without explicit authorization.

## Architectural Impact

This decision clarifies a hard operational boundary in the launcher architecture:

- **Source correctness** and **runtime branch reachability** are separate concerns.
- BUY-path fixes in `oracle_runtime.rs` and `components/trigger/shadow_run.rs` cannot be validated live unless the runtime reaches a BUY-capable candidate and the trigger budget gate is satisfiable.
- Decision JSONL, BUY-only JSONL, and shadow-run JSONL must be interpreted as staged evidence:
  1. decision rows prove ingestion and policy evaluation,
  2. BUY-only rows prove BUY-routing,
  3. shadow-run rows prove shadow dispatch and error classification.

## Risk Assessment

**Rate:** Medium

- **Operational risk:** Medium — operators may incorrectly treat lack of fresh BUY/shadow rows as a code failure when the runtime never reached that branch.
- **Regression risk:** Low — source-level tests and inspected code paths support the implemented fixes; the live run simply did not exercise them.
- **Validation risk:** Medium — without a reachable BUY/shadow path, production readiness claims would be overstated.

## Consequences

### Positive
- Prevents false closure based on incomplete live evidence.
- Preserves trigger safety invariants and emergency floor semantics.
- Establishes a clean, auditable explanation for why the current run could not validate the newest branch.

### Negative
- End-to-end live validation remains incomplete.
- Additional operator action is required before the newest BUY-path fixes can be confirmed in a live run.

## Alternatives Considered

### 1. Declare the fix validated from unit tests and source inspection alone
Rejected because the request explicitly required validation against the rebuilt runtime binary in a real run.

### 2. Silently relax trigger budget parameters for the validation run
Rejected because it changes production risk semantics and would violate the requirement to avoid unauthorized scope expansion.

### 3. Treat missing fresh BUY/shadow artifacts as proof that the code fix failed
Rejected because the observed runtime window never produced a fresh BUY-path execution opportunity.

## Validation Steps

To complete end-to-end live validation, perform one of the following under explicit operator approval:

1. **Make the trigger budget satisfiable**
   - fund the configured wallet above `0.011 SOL`, or
   - run against an explicitly authorized temporary config with lower floor/buffer/size.

2. **Observe until a fresh BUY candidate reaches the branch**
   - confirm a new row appears in `logs/decisions.jsonl/gatekeeper_v2_buys.jsonl`, or
   - confirm a new row appears in `logs/shadow_run/buys.jsonl`.

3. **Validate expected post-fix outcomes**
   - if readiness succeeds, the fresh BUY/shadow artifacts should no longer be blocked solely by missing `initial_liquidity_sol` when reserve backfill is available,
   - if dispatch fails due to balance/bulkhead protection, the outcome should resolve to `shadow_insufficient_balance` rather than an unknown bucket,
   - BUY-only rows must remain canonical and include the BUY verdict fields and observation identity fields.
