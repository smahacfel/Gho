# ADR-0072: Sell readiness audit before live rollout

**Date:** 2026-04-04
**Status:** Accepted
**Author:** Ghost Father

## Context

The repository required a no-live-run audit focused on whether the current system is ready to sell/exit positions before any further live rollout. The team previously observed a dual-micro-live period where buys were believed to occur without matching sell behavior. The audit scope covered live sell dispatch, post-buy exit conditions, Jito fail-closed transport enforcement, paper/closeout guards, rollout configs, retained rollout artifacts, and existing tests.

## Decision

The audit baseline is:

1. Current HEAD enforces live BUY/SELL fail-closed Jito transport at config/startup/runtime boundaries.
2. Current HEAD contains launcher integration tests proving live-lane routing no longer falls back into paper when `LiveSellHandle` is absent and that paper lifecycle remains covered separately.
3. Current HEAD still lacks a local proof of successful end-to-end live SELL landing through real Jito confirmation after a real BUY, because existing tests stop at routing, fail-closed handling, dry-run Jito bundle tests, and synthetic sell-flow coverage.
4. Retained `logs/ssot_run_20260302T125855Z/` and `logs/ssot_run_20260302T125836Z/` are not valid evidence of current live readiness because their archived configs are paper-mode / legacy-dry-run profiles rather than the current rollout SSOT.
5. Historical retained artifacts still explain how the team could perceive “buy/no-sell” or otherwise ambiguous rollout behavior: config/profile drift, missing live signatures in shadow artifacts, and historical architecture that previously routed live exits away from the authoritative launcher live-sell path.

## Architectural Impact

This audit confirms the current authoritative path:

- live BUY dispatch: `ghost-launcher/src/components/trigger/component.rs`
- post-buy live SELL lifecycle: `ghost-launcher/src/components/post_buy_runtime.rs`
- startup/live-sell handle construction: `ghost-launcher/src/main.rs`
- production config/profile enforcement: `ghost-launcher/src/config.rs`

It also confirms that ghost-brain JSONL output is not the SSOT for live exits; live exit evidence is expected in launcher tracing/transport telemetry rather than paper-lifecycle event files.

## Risk Assessment

**Rate:** Medium

Current regression risk is medium because transport/profile guards are present and tested, but there is still no local proof of a real live BUY followed by a landed live SELL over Jito in production-like conditions. Operational ambiguity remains possible if teams rely on retained paper-mode artifacts or shadow-only records to infer live behavior.

## Consequences

- Stronger confidence in current fail-closed SELL architecture.
- Clearer separation between what is code-proven locally and what still requires controlled runtime validation.
- Historical rollout artifacts must be treated carefully because they can misrepresent actual live-lane behavior.
- A future controlled runtime validation remains necessary before declaring full sell readiness for live capital.

## Alternatives Considered

### 1. Treat retained rollout logs as sufficient proof of current readiness

Rejected because the archived run configs are paper/legacy-dry-run profiles and therefore cannot prove current live SELL behavior.

### 2. Treat dry-run Jito and synthetic sell-flow tests as sufficient end-to-end proof

Rejected because they do not prove a real live BUY → live SELL landing sequence through the launcher’s authoritative exit path.

### 3. Require a live run immediately

Rejected because the audit mandate explicitly prohibited live execution before review completion.

## Validation Steps

Locally validated during this audit with:

1. Code inspection of:
   - `ghost-launcher/src/components/post_buy_runtime.rs`
   - `ghost-launcher/src/components/trigger/component.rs`
   - `ghost-launcher/src/main.rs`
   - `ghost-launcher/src/config.rs`
   - `ghost-launcher/src/oracle_runtime.rs`
   - `off-chain/components/trigger/tests/sell_logic_integration.rs`
   - `ghost-launcher/tests/post_buy_runtime_integration.rs`
   - retained rollout configs/log artifacts under `logs/`
2. Targeted Rust tests:
   - `cargo test -p ghost-launcher --test post_buy_runtime_integration -- --nocapture`
   - `cargo test -p ghost-launcher live_dispatch_fails_closed_without_jito_transport -- --nocapture`
   - `cargo test -p ghost-launcher live_transport_guard_rejects_blank_jito_endpoint -- --nocapture`
   - `cargo test -p ghost-launcher runtime_oracle_dry_run_enables_paper_lane_for_paper_execution_mode -- --nocapture`
   - `cargo test -p ghost-launcher runtime_oracle_dry_run_stays_false_for_live_execution_without_legacy_flag -- --nocapture`
   - `cargo test -p ghost-launcher test_validate_execution_profile_accepts_live_transport_with_jito -- --nocapture`
   - `cargo test -p ghost-launcher test_validate_execution_profile_rejects_live_transport_without_jito_endpoint -- --nocapture`
   - `cargo test -p ghost-launcher test_production_rejects_legacy_dry_run_aliases -- --nocapture`
   - `cargo test -p trigger --test sell_logic_integration -- --nocapture`
3. Targeted Python guard test:
   - `python3 -m unittest tools.tests.test_paper_burnin_closeout_guard`
