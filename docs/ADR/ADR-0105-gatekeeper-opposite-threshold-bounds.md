# ADR-0105: Gatekeeper Opposite Threshold Bounds

**Date:** 2026-04-22
**Status:** Accepted
**Author:** Ghost Father

## Context
User requested extending `ghost-brain/ghost_brain_config.toml` with threshold bounds opposite to the ones already present in `[gatekeeper_v2]`, including:
- `mix_interval_cv` / `max_interval_cv`
- `max_volume_cv`
- `max_total_volume_sol`
- `max_timing_entropy`
- `min_dev_tx_ratio`
- `min_sell_buy_ratio`
- `min_compute_unit_cluster_dominance`
- `min_static_fee_profile_ratio`
- `max_avg_inner_ix_count_50tx`
- `min_jito_tip_intensity`

The existing Gatekeeper V2 surface was asymmetric: some metrics exposed only lower bounds, some only upper bounds, and some were logged without a complete bidirectional policy surface. Editing the TOML file alone would have been unsafe because:
- `GatekeeperV2Config` would reject or ignore unknown keys;
- runtime policy paths in `ghost-launcher` would not enforce the new bounds;
- telemetry and buy-log schema would drift from the effective decision surface;
- timing upper bounds would remain inert under the three-layer decision model unless surfaced as soft signals.

Additionally, the requested key `mix_interval_cv` is almost certainly a typo for `max_interval_cv`, but it appeared in a user-facing request and therefore required backward-compatible handling.

## Decision
Extend the Gatekeeper V2 configuration and runtime contract end-to-end instead of performing a TOML-only edit.

Implemented decision:
- add complementary threshold fields to `GatekeeperV2Config` with neutral defaults that preserve current behavior until explicitly tuned;
- use canonical field name `max_interval_cv` and accept `mix_interval_cv` as a serde alias for compatibility;
- enforce the new bounds in both Gatekeeper decision paths:
  - feature/policy path in `ghost-launcher/src/components/gatekeeper_policy.rs`;
  - buffer/local path in `ghost-launcher/src/components/gatekeeper.rs`;
- add new timing soft-signal flags `high_interval_cv` and `high_timing_entropy` so upper timing bounds participate in three-layer mode;
- extend decision/buy log schema and runtime config summaries to expose the new threshold surface;
- update fixtures and add regression tests covering parser aliasing and bidirectional bound enforcement.

Neutral defaults were chosen as follows:
- lower-bound fields default to `0.0`;
- natural capped max-ratio fields keep `1.0` where appropriate;
- effectively unbounded upper checks default to `9999.0`.

## Architectural Impact
This changes the Gatekeeper SSOT across configuration, decisioning, and observability:
- `ghost-brain/src/config/ghost_brain_config.rs`
- `ghost-brain/ghost_brain_config.toml`
- `ghost-launcher/src/components/gatekeeper_policy.rs`
- `ghost-launcher/src/components/gatekeeper.rs`
- `ghost-brain/src/oracle/decision_logger.rs`
- `ghost-launcher/src/oracle_runtime.rs`
- `ghost-launcher/src/tx_intelligence/config.rs`
- `ghost-launcher/src/tx_intelligence/engine.rs`

The buy-log schema version was bumped so emitted telemetry matches the expanded threshold contract. Test fixtures that construct `GatekeeperV2Config` explicitly also required synchronized updates.

## Risk Assessment
**Rate:** Medium

Primary risks:
- expanded policy surface can reject candidates that were previously admissible if operators tighten the new bounds;
- telemetry consumers must tolerate buy-log schema version `13`;
- duplicated Gatekeeper decision paths require continued parity discipline.

Risk was reduced by using neutral defaults and validating both parser compatibility and runtime enforcement with targeted tests.

## Consequences
- Operators now have a symmetric threshold surface for timing, volume, developer activity, and infrastructure fingerprint metrics.
- Config additions in `ghost_brain_config.toml` are now real SSOT knobs instead of dead text.
- Three-layer mode can express timing upper-bound pressure through soft signals.
- The config surface is slightly larger and therefore requires more deliberate tuning discipline.

## Alternatives Considered
1. **Edit only `ghost_brain_config.toml`**
	- Rejected because unknown keys would not be safely represented across schema, runtime, and telemetry.
2. **Add fields only to `GatekeeperV2Config`**
	- Rejected because the new bounds would deserialize but remain behaviorally inert.
3. **Introduce a literal new field `mix_interval_cv`**
	- Rejected because the canonical semantic is clearly an upper bound on interval CV, so `max_interval_cv` must be the SSOT name.

## Validation Steps
1. Run parser/config compatibility validation:
	- `config::ghost_brain_config::tests::test_gatekeeper_v2_from_toml_file_partial_override`
2. Run Gatekeeper policy regression suite:
	- `ghost-launcher --test gatekeeper_policy_tests`
3. Run targeted library checks for local Gatekeeper defaults and buy-log mapping:
	- `test_default_config_values`
	- `test_gatekeeper_buy_log_thresholds_match`
4. Verify edited files report no static diagnostics in the workspace error scan.
5. When broader package tests are revisited, re-run full `ghost-launcher` test coverage after unrelated post-buy integration drift is resolved.
