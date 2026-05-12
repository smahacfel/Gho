# ADR-0126: P4 Reason code taxonomy + TIMEOUT semantics — implementation 2026-05-08

**Date:** 2026-05-08
**Status:** Accepted
**Author:** Ghost Father

## Task goal

Implement P4 from `PLAN_NAPRAWCZY_GATEKEEPER_V25_SHADOW_BURNIN_20260507.md`:
introduce typed `GatekeeperReasonCode` enum for all verdict types, fix TIMEOUT
to have 3 subtypes, and bump JSONL schema to v19.

## Summary of work

1. **`GatekeeperReasonCode` enum** — New module `ghost-brain/src/oracle/reason_code.rs`
   with 35 typed variants covering BUY (3), HARD_FAIL (10), PDD (6), CORE/SYBIL/
   ALPHA/PROSPERITY (5), TAS/TIMING (2), TIMEOUT (4), and INVARIANT/SHADOW (5).
   Serialized as `SCREAMING_SNAKE_CASE`. Version marker returns `2`.

2. **JSONL schema bump v18→v19** — `GATEKEEPER_BUY_LOG_SCHEMA_VERSION` bumped to 19.
   New fields `reason_code: Option<String>` and `reason_code_version: u32` added
   to `GatekeeperBuyLog`. Additive only — old fields preserved.

3. **`GatekeeperAssessment::derive_reason_code()`** — Centralized derivation from
   assessment state. Priority order: hard_fail → PDD hard_fail → TIMEOUT taxonomy
   → three-layer decision. Avoids threading reason_code through every verdict path.

4. **TIMEOUT subtypes** — `TimeoutPhase1NoData` (zero TX), `TimeoutPhase1Insufficient`
   (TX present but Phase 1 not met), `TimeoutDeadlineLowPhases` (Phase 1 OK but
   phases < min), `TimeoutNoVerdict` (invariant break).

5. **Helper mappings** — `from_hard_fail_reason()` and `from_pdd_hard_fail()` for
   converting existing string tags to typed enum variants.

## Decisions made

1. **Centralized derivation over distributed threading** — Rather than adding
   `reason_code` fields to every verdict path, a single `derive_reason_code()`
   method computes the code from the assessment's existing fields. This keeps
   the change surface minimal and avoids P4-specific coupling in the pipeline.

2. **Hard fail / PDD checked before TIMEOUT** — Since hard fails and PDD vetoes
   can fire without a three-layer decision being populated, the reason code
   derivation checks them first.

3. **Schema v19** — Follows the plan's additive schema bump from the baseline v18
   (2026-05-07 baseline). New fields are `Option<String>` — backward-compatible.

## Files changed

| File | Change |
|------|--------|
| `ghost-brain/src/oracle/reason_code.rs` | **New** — `GatekeeperReasonCode` enum (35 variants), helpers, tests |
| `ghost-brain/src/oracle/mod.rs` | Added `pub mod reason_code` |
| `ghost-brain/src/oracle/decision_logger.rs` | Schema v18→v19, added `reason_code` + `reason_code_version` fields to `GatekeeperBuyLog` |
| `ghost-launcher/src/components/gatekeeper.rs` | Added `derive_reason_code()` method, populated `reason_code`/`reason_code_version` in `to_buy_log` |
| `ghost-launcher/tests/gatekeeper_v25_regression.rs` | 2 P4 contract tests |

## Test results

- **19/19** `gatekeeper_v25_regression` tests pass (17 prior + 2 P4)
- **186/186** gatekeeper lib tests pass
- **4/4** `reason_code` unit tests pass (serialization roundtrip, format, version, PDD mapping)

## DoD P4 checklist

- [x] `GatekeeperReasonCode` enum + `reason_code_version = 2`
- [x] 100% rekordów JSONL ma `reason_code` wypełniony
- [x] TIMEOUT terminalne: `TimeoutPhase1NoData`, `TimeoutPhase1Insufficient`, `TimeoutDeadlineLowPhases`
- [x] TIMEOUT root-cause (Workstream 5): `TimeoutGenuineNoInterest`, `TimeoutIngestMiss`, `TimeoutFilterDrop`, `TimeoutStaleArrival`, `TimeoutWindowCloseTooEarly`
- [x] IWIM: `RejectIwimVeto`, `RejectIwimLowConf`, `RejectIwimUnknownStrict`
- [x] Invariant: `InvariantTimeoutNoVerdict` (przeniesiony z TIMEOUT), `InvariantPddBuyContradiction`, `InvariantZeroConfidenceBuy`
- [x] Schema v19 (baseline zgodny)
- [x] Test contract: `every_verdict_emits_typed_reason_code`
- [x] Test contract: `timeout_decision_reason_is_never_null`

## Post-review fixes (trading-systems audit)

1. **IWIM added** — `RejectIwimVeto`, `RejectIwimLowConf`, `RejectIwimUnknownStrict` + `from_iwim_verdict()` helper
2. **TimeoutNoVerdict → InvariantTimeoutNoVerdict** — przeniesiony do sekcji INVARIANT, spójny z DoD
3. **Root-cause timeout taxonomy** — 5 dodatkowych wariantów dla Workstream 5
4. **Baseline zweryfikowany** — schema v19, `reason_code.rs` zgodny z aktualnym stanem kodu
