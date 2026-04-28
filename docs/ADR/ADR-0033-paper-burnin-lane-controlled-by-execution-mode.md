# ADR-0033: Paper burn-in lane controlled by execution mode

**Date:** 2026-03-27
**Status:** Accepted
**Author:** Ghost Father

## Context

A blocker was raised against `configs/rollout/paper-burnin.toml` claiming that PR-6 still required `oracle.dry_run = true` for the paper lifecycle to start after a successful shadow simulation.

The runtime branch that emits `PostBuySubmitted` is gated by `post_buy_lane == "paper"` in `ghost-launcher/src/oracle_runtime.rs`. That lane is computed from `ctx.dry_run`, so the practical question was whether `ctx.dry_run` still depends on legacy `[oracle].dry_run` in production rollout profiles.

Current production rollout rules in `ghost-launcher/src/config.rs` explicitly reject legacy `dry_run` aliases in production configs. Therefore adding `oracle.dry_run = true` to `paper-burnin.toml` would violate the rollout contract instead of fixing it.

## Decision

Treat `[execution].execution_mode = "paper"` together with `[trigger].entry_mode = "shadow_only"` as the sole supported production control plane for paper burn-in.

`ghost-launcher/src/main.rs` computes the effective Oracle runtime dry-run flag from:

- legacy `config.oracle.dry_run`, or
- `config.execution.execution_mode == ExecutionMode::Paper`

This preserves paper-lane behavior for production paper profiles without requiring or allowing legacy `oracle.dry_run`.

Regression coverage was added to verify:

1. a production paper profile loads successfully without `oracle.dry_run`, and
2. runtime still maps paper execution mode to `dry_run = true`, which yields `post_buy_lane = "paper"`.

## Architectural Impact

This decision keeps rollout SSOT in the explicit execution profile:

- `execution.execution_mode` determines paper/live behavior,
- `trigger.entry_mode` determines shadow/live dispatch style,
- `oracle.dry_run` remains legacy compatibility only and is not part of the production rollout contract.

This removes ambiguity between config parsing, launcher startup, Oracle runtime lane selection, and post-buy lifecycle event emission.

## Risk Assessment

**Risk:** Low

The change is limited to regression coverage and a small helper extraction around an existing effective-flag calculation. No account layouts, program interfaces, or runtime event contracts were changed.

Main regression risk would be a future refactor reintroducing a dependency on legacy `oracle.dry_run` for paper rollouts; the added tests are intended to catch exactly that.

## Consequences

Production `paper-burnin.toml` should not be patched with `oracle.dry_run = true`.

The blocker should be resolved by recognizing it as outdated against current launcher/runtime semantics, not by reintroducing the legacy flag into the tracked rollout profile.

This makes rollout semantics stricter and more reproducible, but it also means operators must reason from `execution_mode`/`entry_mode` rather than from deprecated `dry_run` aliases.

## Alternatives Considered

### Add `oracle.dry_run = true` to `paper-burnin.toml`

Rejected because production config validation explicitly forbids legacy dry-run aliases.

### Revert runtime to depend only on `oracle.dry_run`

Rejected because it would break the current rollout SSOT and recreate ambiguity between production execution profiles and Oracle runtime lane selection.

### Leave behavior undocumented and rely on tribal knowledge

Rejected because this exact mismatch already produced blocker confusion and could recur during future rollout work.

## Validation Steps

1. Run `cargo test -p ghost-launcher --bin ghost-launcher runtime_oracle_dry_run`.
2. Run `cargo test -p ghost-launcher --lib test_production_paper_profile_does_not_require_legacy_oracle_dry_run`.
3. Confirm `configs/rollout/paper-burnin.toml` keeps:
   - `[execution].execution_mode = "paper"`
   - `[trigger].entry_mode = "shadow_only"`
   - no `[oracle].dry_run`
4. Confirm runtime still computes paper lane from effective dry-run logic before calling `start_oracle_runtime_task(...)`.
