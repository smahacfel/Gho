# ADR-0050: Gatekeeper V2 Startup Config SSOT

**Date:** 2026-03-28
**Status:** Accepted
**Author:** Ghost Father

## Context
Launcher startup logged that `ghost-brain/ghost_brain_config.toml` was loaded, but Gatekeeper behavior could still diverge from the edited `[gatekeeper_v2]` section because startup mixed multiple configuration paths:
- full `GhostBrainConfig::from_toml_file(...)` for general analytical modules,
- legacy alias syncing from deprecated `brain_config.gatekeeper`,
- separate later parsing of `[gatekeeper_v2]`,
- relative config paths that could remain non-canonical in logs and runtime propagation.

This created two operational problems:
1. the startup log did not prove which concrete file path was used at runtime,
2. Gatekeeper-related startup state was not consistently derived from `[gatekeeper_v2]` as the intended SSOT.

## Decision
The launcher now treats `[gatekeeper_v2]` as the single startup source of truth for Gatekeeper runtime thresholds.

Implemented changes:
- canonicalize the loaded launcher config path inside `LauncherConfig::from_file(...)`, so rebased runtime paths (including `ghost_brain_config_path`) become absolute for the current session,
- load Gatekeeper V2 once via a dedicated startup helper using `GhostBrainConfig::gatekeeper_v2_from_toml_file(...)`,
- remove the misleading startup bridge that synchronized launcher gatekeeper aliases from deprecated `brain_config.gatekeeper`,
- sync legacy launcher aliases (`min_tx_to_pass`, `observation_window_ms`) only from the effective `GatekeeperV2Config`,
- reuse the same loaded Gatekeeper V2 config for snapshot TTL derivation and Oracle Runtime startup,
- log the effective Gatekeeper V2 runtime configuration explicitly at Oracle Runtime start.

## Architectural Impact
This makes Gatekeeper startup deterministic:
- one parsed `GatekeeperV2Config` instance now feeds snapshot timing, legacy alias compatibility, and runtime observation tasks,
- non-Gatekeeper Ghost Brain modules still use full config loading, but their failure no longer implies that Gatekeeper V2 fell back to unrelated legacy defaults,
- startup diagnostics now expose the effective Gatekeeper runtime thresholds instead of only a generic file-loaded message.

Components affected:
- `ghost-launcher/src/config.rs`
- `ghost-launcher/src/main.rs`
- `ghost-launcher/src/oracle_runtime.rs`

## Risk Assessment
**Risk:** Low

The change is localized to configuration loading and startup wiring. It does not alter Gatekeeper policy math, verdict evaluation logic, or account/state transitions. Main regression surface is startup behavior and compatibility with existing rollout configs.

## Consequences
Positive:
- edited `[gatekeeper_v2]` values are applied consistently on the next launcher session,
- logs now point to the real effective runtime config,
- legacy launcher aliases cannot silently drift away from Gatekeeper V2 anymore.

Trade-offs:
- startup now makes the Gatekeeper SSOT boundary stricter and more explicit,
- full Ghost Brain config load failures remain possible for unrelated modules, but are now reported separately from Gatekeeper V2 loading.

## Alternatives Considered
1. **Keep the old startup bridge and only improve logs**
   - Rejected because it would preserve the semantic bug: legacy alias values could still be sourced from deprecated config.
2. **Load full `GhostBrainConfig` once and derive Gatekeeper from it only**
   - Rejected because launcher intentionally supports direct `[gatekeeper_v2]` parsing even when unrelated sections are temporarily invalid.
3. **Remove all legacy launcher aliases immediately**
   - Rejected because current runtime still carries compatibility fields and this fix needed to stay within the scoped startup-configuration issue.

## Validation Steps
1. Run `cargo check -p ghost-launcher --lib` and confirm successful compilation.
2. Start a fresh launcher session after editing `[gatekeeper_v2]` values in `ghost-brain/ghost_brain_config.toml`.
3. Verify startup logs show:
   - absolute Ghost Brain config path,
   - explicit Gatekeeper V2 load summary,
   - explicit Oracle Runtime Gatekeeper V2 effective config line.
4. Confirm observed Gatekeeper runtime behavior (for example `max_wait_time_ms`, `min_tx_count`) matches the edited values on the next session.
5. Note: full `cargo test -p ghost-launcher` is currently blocked by pre-existing unrelated test compile errors in `ghost-launcher/src/oracle_runtime.rs` around `apply_trigger_buy_outcome(...)` argument ordering.