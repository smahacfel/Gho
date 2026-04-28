# ADR-0052: Gatekeeper Phase 2 average interval config self-consistency

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

Launcher startup at `2026-03-28T21:56` loaded the active Gatekeeper V2 configuration from `/root/Gho/ghost-brain/ghost_brain_config.toml` with the expected Phase 1 thresholds (`min_tx=7`, `min_unique=5`, `min_buy=4`, `max_wait_ms=8000`).

However, the actual Oracle Runtime log showed the full effective runtime config still contained `avg_ms=[0,1]`, and live pool decisions were rejected with hard-fail reasons such as:

- `LONG DEADLINE: HARD_FAIL: avg_interval=190ms > 1ms (slow/dead pool)`
- `LONG DEADLINE: HARD_FAIL: avg_interval=167ms > 1ms (slow/dead pool)`

This proved the system was reading the config correctly, but the active Phase 2 value `max_avg_interval_ms = 1.0` made the runtime effectively non-usable for normal pools and created the false operational impression that configuration changes were being ignored.

The same config block already documented the intended policy in comments: `avg_interval_ms < 277`.

## Decision

Restore `gatekeeper_v2.max_avg_interval_ms` in `/root/Gho/ghost-brain/ghost_brain_config.toml` from `1.0` to `277.0` so the active value matches the documented Phase 2 intent and no longer hard-fails normal pools at the 1 ms boundary.

No code-path changes were made for this remediation because the runtime logs already proved the launcher and Oracle Runtime were consuming the config file correctly.

## Architectural Impact

- No account layouts, APIs, or runtime wiring changed.
- The change affects only the effective operator-tunable Gatekeeper behavior in the active TOML.
- Oracle Runtime will continue to use the same SSOT load path; only the configured numeric bound changes.

## Risk Assessment

**Rate:** Low

- Regression risk is limited to Gatekeeper selectivity in Phase 2.
- The change reduces unintended hard-fail pressure and aligns runtime behavior with the documented configuration intent.
- No code, persistence, or protocol contract is altered.

## Consequences

- Normal pools with average inter-transaction spacing above 1 ms are no longer rejected solely because of an accidentally impossible upper bound.
- Operator expectations become aligned with observed runtime behavior.
- Phase 2 remains configurable from TOML without changing code.

## Alternatives Considered

1. **Leave `1.0` in place and treat it as intended.**
   Rejected because current logs showed it was the direct reason for immediate hard-fail rejections and contradicted the config's own documented threshold intent.

2. **Add code-side validation or clamp suspiciously low values.**
   Rejected for this remediation because it would expand scope beyond the immediate production issue. The runtime already exposes the effective config clearly enough once the value is corrected.

3. **Change Gatekeeper hard-fail semantics in code.**
   Rejected because the current failure was caused by the active configuration value, not by a broken config-loading path.

## Validation Steps

1. Restart the launcher with the updated `/root/Gho/ghost-brain/ghost_brain_config.toml`.
2. Confirm Oracle Runtime logs:
   - `Gatekeeper V2 runtime: ... avg_ms=[0,277] ...`
3. Confirm new pool rejections no longer cite:
   - `avg_interval=... > 1ms`
4. If further rejections occur, inspect the exact hard-fail reason from `oracle_decision.log.2026-03-28` before changing any other thresholds.