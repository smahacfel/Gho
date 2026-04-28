# ADR-0087: Live BUY latency timing checkpoints for Gatekeeper, IWIM, submit, and confirm

**Date:** 2026-04-09
**Status:** Accepted
**Author:** Ghost Father

## Context

Dual live evidence showed that the BUY path was arriving on-chain materially later than expected relative to pool detection and Gatekeeper PASS. The operational question was no longer whether BUY sometimes lands, but where latency accumulates inside the hot path:

- when Gatekeeper emits PASS
- how much time is spent between PASS and post-IWIM gating
- when the transaction is actually handed to the Helius Sender submit path
- when confirmation is finally observed on-chain

The user explicitly requested a surgical observability-only change with no logical behavior modifications. The requirement was to add millisecond checkpoints that allow the next live run to localize delay without perturbing BUY decision or execution semantics.

Before this change, the runtime already logged high-level BUY submission and confirmation events, but it did not expose the exact boundary timestamps needed to isolate:

1. Gatekeeper decision latency vs downstream dispatch latency
2. IWIM gating overhead vs disabled/no-op behavior
3. pre-submit preparation latency vs network/landing latency
4. submit-to-confirm delay for the final live BUY signature

## Decision

Add four explicit timing checkpoints in the production BUY path and keep them strictly telemetry-only.

### 1. Gatekeeper PASS checkpoint

In `ghost-launcher/src/oracle_runtime.rs`, capture `gatekeeper_verdict_at` immediately when `GatekeeperVerdict::Buy` is produced.

Emit a dedicated log:

- `Gatekeeper: PASS verdict timing checkpoint`

with at least:

- `pool`
- `base_mint`
- `gatekeeper_verdict_at`
- `iwim_enabled`

### 2. Post-IWIM gating checkpoint

In the same BUY path, capture `post_iwim_gate_at` immediately after IWIM verdict handling finishes or is bypassed.

Emit a dedicated log:

- `Gatekeeper: post-IWIM BUY timing checkpoint`

with at least:

- `gatekeeper_verdict_at`
- `post_iwim_gate_at`
- `iwim_gate_latency_ms`
- `iwim_enabled`
- `iwim_status`
- `iwim_quality`

When IWIM is disabled, `post_iwim_gate_at` is set equal to `gatekeeper_verdict_at`, which forces:

- `iwim_gate_latency_ms = 0`

This encodes the intended no-op contract directly into telemetry.

### 3. Pre-submit BUY checkpoint

In `ghost-launcher/src/components/trigger/component.rs`, capture `buy_submitted_at` immediately before the Helius Sender `send_transaction` call.

Emit a dedicated log:

- `Trigger: live BUY pre-submit timing checkpoint`

with at least:

- `mint`
- `attempt_number`
- `buy_submitted_at`
- tracked signatures
- lamports/tip/priority-fee context
- blockhash metadata

Also enrich the existing log:

- `Trigger: live BUY submitted via Helius Sender`

with `buy_submitted_at` so post-hoc log correlation remains possible even when only submission logs are sampled.

### 4. Confirmed BUY checkpoint

In the sender confirmation telemetry closure, capture `buy_confirmed_at` when confirmation is accepted.

Enrich the existing confirmation telemetry log:

- `Trigger: live BUY sender telemetry`

with:

- `buy_submitted_at`
- `buy_confirmed_at`
- `buy_submit_to_confirm_ms`

This preserves the existing telemetry shape while extending it with the single most important end-to-end latency delta.

## Architectural Impact

This change does not alter BUY routing, Gatekeeper policy, IWIM decision semantics, retry behavior, sender transport, or confirmation rules.

It only strengthens observability by making the BUY path measurable across four runtime boundaries:

1. decision
2. post-policy gating
3. network submission
4. confirmation

The effect is architectural in the sense that future runtime diagnosis can now distinguish:

- slow Gatekeeper/session accumulation
- slow IWIM gating
- slow build/pre-submit path
- slow landing/confirmation path

without introducing alternate execution paths, new feature gates, or new state ownership.

## Risk Assessment

**Risk:** Low

The implementation is logging-only and reuses existing clock helpers:

- `current_time_ms()` in `oracle_runtime.rs`
- `Self::now_ms()` in `trigger/component.rs`

No account layout, BUY instruction encoding, blockhash policy, retry budget, or confirmation policy was changed.

The main residual risk is operational rather than functional:

- higher log volume on active BUY attempts

That trade-off is accepted because the target problem is transient latency diagnosis in a constrained live path.

## Consequences

### Positive

- The next dual/live run can compute exact deltas across the BUY path.
- IWIM-disabled runs now explicitly prove `0ms` gate overhead in logs.
- Submission vs confirmation delay is directly measurable per attempt/signature.
- Existing BUY logs remain intact and become more useful through timestamp enrichment.

### Negative / Trade-offs

- BUY-path logs become slightly noisier.
- Operators must correlate multiple checkpoint lines to build a full waterfall, rather than relying on a single summary line.

## Alternatives Considered

### 1. Add only one aggregate BUY latency log

Rejected because it would not isolate whether latency sits before IWIM, before submit, or after submit.

### 2. Persist structured latency records to a new artifact file

Rejected for now because the user requested a surgical change with no extra execution-side plumbing and no broader artifact contract changes.

### 3. Reuse existing submission/confirmation logs without new checkpoints

Rejected because the existing logs did not expose the exact pre-submit boundary or the Gatekeeper/IWIM boundaries required for diagnosis.

## Validation Steps

1. Build `ghost-launcher` successfully after the telemetry patch.
2. Confirm no diagnostics regressions in:
   - `ghost-launcher/src/oracle_runtime.rs`
   - `ghost-launcher/src/components/trigger/component.rs`
3. During the next dual/live run, extract and compare:
   - `gatekeeper_verdict_at`
   - `post_iwim_gate_at`
   - `buy_submitted_at`
   - `buy_confirmed_at`
4. Compute the derived deltas:
   - `iwim_gate_latency_ms`
   - `buy_submit_to_confirm_ms`
   - `buy_submitted_at - post_iwim_gate_at`
   - `buy_confirmed_at - gatekeeper_verdict_at`
5. Use those deltas to classify the dominant latency bucket before touching any execution logic.
