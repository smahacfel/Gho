# ADR-0016: Shadow BUY Creator Canonicalization Guard

**Date:** 2026-03-22  
**Status:** Accepted  
**Author:** Ghost Father  

## Context

Shadow BUY failures were still appearing as `InstructionError(1, Custom(2006))` with `error_class="semantic"` even when decision telemetry already hinted that the case was unsafe via `shadow_missing_fields:["creator","observed_buy_tx"]`.

`Custom(2006)` in Anchor corresponds to `ConstraintSeeds`, which means the on-chain program rejected an account because its PDA did not match the seeds expected by the program.

The local failure chain was:

1. shadow readiness treated `creator` as present whenever the string was non-empty and not `"unknown"`,
2. invalid values such as `Pubkey::default()` or unparsable strings could therefore pass readiness,
3. the trigger path could still reach BUY transaction construction without a canonical `creator_pubkey`,
4. the direct buy builder would then derive `creator_vault` from the default pubkey,
5. Pump.fun rejected the BUY instruction with a seed constraint failure.

## Decision

The system now enforces creator canonicalization before BUY construction:

1. `oracle_runtime` shadow readiness treats `creator` as present only when it is a valid, non-default Solana pubkey.
2. `execute_gatekeeper_buy_path()` performs an explicit guard and refuses to dispatch BUY when no valid canonical `creator_pubkey` exists.
3. `TriggerComponent` validates `creator_pubkey` before transaction build and fails fast with a deterministic preflight error instead of allowing default-PDA derivation.
4. dispatch error classification maps this failure to `shadow_metadata_missing` so telemetry reports the real root cause rather than a generic semantic simulation failure.

## Architectural Impact

This decision hardens the contract between `OracleRuntime` and `TriggerComponent`:

- `OracleRuntime` remains the authoritative source of canonical shadow BUY metadata.
- `TriggerComponent` no longer tolerates incomplete creator identity and will not derive Pump.fun creator PDAs from fallback zero values.
- The BUY path now fails earlier, deterministically, and with telemetry that reflects metadata incompleteness instead of downstream on-chain rejection.

Affected components:

- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/components/trigger/component.rs`
- downstream shadow telemetry / JSONL classification

## Risk Assessment

**Rate:** Medium

Primary regression risks:

- pools with historically malformed creator metadata may now be skipped earlier instead of reaching simulation,
- any hidden path relying on implicit default creator behavior will now fail fast.

These are acceptable because the old behavior was unsafe and produced invalid PDAs plus misleading semantic failures.

## Consequences

### Positive

- eliminates the unsafe path that derived `creator_vault` from `Pubkey::default()`,
- aligns readiness gating with actual BUY account requirements,
- converts opaque `Custom(2006)` failures into explicit metadata failures,
- reduces false similarity assumptions from Solscan-level pool shape comparisons.

### Negative

- stricter validation may increase early skips for pools whose creator metadata is incomplete or corrupted,
- downstream monitoring must treat `shadow_metadata_missing` as an expected hard stop rather than a transport issue.

## Alternatives Considered

### 1. Keep current behavior and only improve log interpretation

Rejected because it would leave the unsafe PDA derivation path intact and merely rename the crash.

### 2. Recover creator from arbitrary observed trade telemetry

Rejected because creator is an identity field, not a field that should be guessed from unstable observed trade layouts.

### 3. Let the direct buy builder continue defaulting missing creator pubkeys

Rejected because this is the direct cause of seed-derived account mismatch and the resulting `ConstraintSeeds` failure.

## Validation Steps

1. Unit-test that shadow readiness rejects `Pubkey::default()` as creator.
2. Unit-test that Trigger preflight rejects missing canonical `creator_pubkey`.
3. Unit-test that dispatch error classification maps this case to `shadow_metadata_missing`.
4. Unit-test that `shadow_only` continues to skip non-ready cases instead of reaching simulation.
5. In staging / shadow logs, verify that prior `InstructionError(1, Custom(2006))` cases transition to either:
   - `shadow_skipped_not_ready`, or
   - `shadow_metadata_missing`.
