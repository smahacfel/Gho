# ADR-0051: Gatekeeper V2 config fail-closed on parse errors

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context

Runtime forensic analysis of `paper-burnin` startup at `2026-03-28T20:43:31Z` showed that launcher was loading the intended file `ghost-brain/ghost_brain_config.toml`, but both the full Ghost Brain config parse and direct `[gatekeeper_v2]` parse failed.

The decisive runtime evidence was:

- launcher log: full config parse failed at line 64, column 24,
- launcher log: direct `[gatekeeper_v2]` parse failed with `invalid type: floating point '1.0', expected usize`,
- launcher then silently fell back to `GatekeeperV2Config::default()`,
- runtime timeout logs used default Phase-1 thresholds `30/15/15` and `2222ms` instead of values from the edited TOML.

The concrete bad config entry was:

- `ghost-brain/ghost_brain_config.toml`: `min_consecutive_buys = 1.0`

This field is deserialized into `usize`, so the TOML was invalid for Gatekeeper V2.

## Decision

Gatekeeper V2 startup must **fail closed** when the configured `[gatekeeper_v2]` section is missing or invalid and no validated full Ghost Brain fallback is available.

Implemented changes:

1. Fixed the invalid TOML entry in `ghost-brain/ghost_brain_config.toml`:
   - `min_consecutive_buys = 1.0` → `min_consecutive_buys = 1`
2. Changed `ghost-launcher/src/main.rs::load_gatekeeper_v2_config(...)` to return `Result<GatekeeperV2Config>` instead of silently returning defaults.
3. Removed the dangerous startup behavior where parse failure could degrade into built-in Gatekeeper defaults without aborting startup.
4. Added a regression test proving that invalid `[gatekeeper_v2]` input now fails closed.

## Architectural Impact

This hardens the Gatekeeper V2 startup SSOT established previously:

- launcher still prefers direct TOML loading of `[gatekeeper_v2]`,
- validated full Ghost Brain config remains an allowed fallback only if already available and valid,
- built-in defaults are no longer an implicit hidden runtime source of truth for broken production configs.

This affects:

- launcher startup behavior,
- rollout safety,
- operational debugging semantics,
- trustworthiness of Gatekeeper threshold logs and timeout diagnostics.

## Risk Assessment

**Rate:** Medium

### Reduced risks

- silent fallback to incorrect Gatekeeper thresholds,
- misleading startup with runtime behavior that does not match edited TOML,
- operator confusion caused by generic "config loaded" logs masking a broken Gatekeeper section.

### Introduced trade-off

- startup now aborts on invalid/missing `[gatekeeper_v2]` instead of continuing with defaults.

This is intentional. For a production decision gate, fail-fast is safer than silently trading on the wrong thresholds.

## Consequences

What becomes easier:

- diagnosing misconfigured Gatekeeper sessions,
- trusting that runtime thresholds match the configured TOML,
- preventing accidental regression to `30/15/15` defaults during rollout.

What becomes harder:

- partially broken configs no longer limp into runtime,
- operators must correct invalid Gatekeeper TOML before restart succeeds.

## Alternatives Considered

### 1. Keep silent fallback to defaults

Rejected because it directly caused the observed production mismatch and violates SSOT expectations.

### 2. Only fix the TOML and keep fallback behavior

Rejected because the same class of issue would recur on the next invalid edit.

### 3. Fail closed on invalid/missing Gatekeeper config

Accepted because incorrect thresholds in a production gate are more dangerous than a refused startup.

## Validation Steps

1. Confirmed from runtime logs that `20:43` startup used default thresholds after parse failure.
2. Identified the invalid TOML entry at line 64: `min_consecutive_buys = 1.0`.
3. Fixed the TOML type mismatch.
4. Added a launcher unit test ensuring invalid `[gatekeeper_v2]` input now fails closed.
5. Re-run launcher compile checks after the change.