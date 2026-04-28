# ADR-0062: Legacy feature terminal policy and BUY log routing consistency

**Date:** 2026-03-30
**Status:** Accepted
**Author:** Ghost Father

## Context

Runtime investigation of Gatekeeper V2 BUY anomalies established a second defect distinct from the earlier bootstrap market-cap issue.

Observed production symptoms were:

- runtime emitted positive `🔓 GATEKEEPER V2: KUPUJ! ...` decisions for obviously invalid cases such as `signers=1 tx=1 phases=1/6`,
- deleting `/root/Gho/logs/decisions.jsonl/gatekeeper_v2_buys.jsonl` did not cause the canonical BUY-only file to be recreated,
- runtime logs still showed `DecisionLogger` starting correctly on `/root/Gho/logs/decisions.jsonl`, and no direct writer failure explained the missing BUY file.

Code-path forensics identified a control-flow split in `ghost-launcher/src/oracle_runtime.rs`:

- when `gatekeeper_v2.use_three_layer_decision = true`, terminal feature evaluation used `GatekeeperBuffer::evaluate_from_features(...)`,
- when `gatekeeper_v2.use_three_layer_decision = false`, terminal feature evaluation used `GatekeeperBuffer::evaluate_from_features_legacy(...)`.

That legacy terminal path had two inconsistent behaviors:

1. it could return `BUY` only from `phases_passed >= min_phases_to_pass`, even when the policy/core-gate decision would not buy,
2. it returned terminal verdicts without populating `assessment.decision`.

This broke two contracts at once:

- **terminal policy consistency:** legacy terminalization could buy on phase count alone and bypass the feature-driven policy verdict,
- **decision-log routing consistency:** `GatekeeperAssessment::to_buy_log(...)` derives `decision_verdict_buy` and `verdict_type` from `assessment.decision`, so legacy BUYs with `decision=None` were written only to `gatekeeper_v2_decisions.jsonl`, never to `gatekeeper_v2_buys.jsonl`.

The active runtime config made the defect visible in practice:

- `use_three_layer_decision = false`
- `min_phases_to_pass = 1`

That combination allowed phase-count-only terminal BUYs such as `1/6` while simultaneously suppressing BUY-only file routing.

## Decision

Legacy feature-terminal evaluation remains backward-compatible only with respect to **pre-deadline wait semantics**, but terminal verdicts must now use the same policy decision contract as the feature-driven path.

Implemented changes:

1. `ghost-launcher/src/components/gatekeeper.rs::evaluate_from_features_legacy(...)` now:
   - builds a policy decision through `evaluate_policy_from_assessment(...)`,
   - stores that decision into `assessment.decision`,
   - keeps `Wait` semantics before deadline,
   - uses policy-driven terminal BUY behavior at deadline instead of phase-count-only BUY,
   - synthesizes a timeout decision through `build_timeout_decision_from_assessment(...)` when Phase 1 was not reached by deadline.

2. `ghost-launcher/src/oracle_runtime.rs::build_timeout_assessment_from_policy_context(...)` now always attaches a timeout decision to the assessment, rather than doing so only when `use_three_layer_decision = true`.

3. Regression coverage was added in `ghost-launcher/src/oracle_runtime.rs` to prove:
   - legacy mode keeps `Wait` semantics before deadline,
   - legacy mode no longer buys at deadline on phase count alone,
   - legacy-mode BUYs populate `decision_verdict_buy` and `verdict_type`, allowing BUY-only logger routing.

## Architectural Impact

This decision does not change the production source of truth for features or canonical account state.

It changes the **terminal verdict contract** for legacy feature mode:

- before: legacy mode preserved wait behavior before deadline but used a weaker, phase-count-only terminal BUY rule and could omit decision metadata,
- after: legacy mode still preserves wait behavior before deadline, but terminal BUY/REJECT/TIMEOUT decisions are policy-consistent and always carry decision metadata.

System-level effects:

- runtime terminal behavior is now consistent with Gatekeeper policy semantics even when `use_three_layer_decision = false`,
- `DecisionLogger` routing contracts are restored because BUY assessments now carry `decision_verdict_buy = true` when they actually represent BUYs,
- the canonical BUY-only file `gatekeeper_v2_buys.jsonl` can be recreated from new BUY traffic again.

## Risk Assessment

**Rate:** Medium

Why medium:

- the change affects terminal Gatekeeper behavior in a compatibility branch that can still be active in production,
- legacy mode will now reject or timeout some cases that previously phase-count-bought,
- timeout assessments now always carry decision metadata, which changes downstream logging shape for that path.

Why not high:

- pre-deadline legacy wait semantics were preserved explicitly,
- the fix narrows legacy behavior toward the already-accepted feature-policy contract rather than introducing a new policy model,
- targeted regression tests passed in-session after the change.

## Consequences

### Positive

- invalid legacy terminal BUYs such as `signers=1 tx=1 phases=1/6` are no longer admitted merely because `min_phases_to_pass = 1`,
- BUY-only log routing is restored because BUY assessments now populate `decision_verdict_buy` and `verdict_type`,
- timeout and reject paths now carry consistent decision metadata for downstream diagnostics.

### Trade-offs

- legacy mode is now less permissive at deadline than the older compatibility branch,
- historical assumptions that legacy mode could BUY purely on passed-phase count are no longer valid,
- some operational comparisons against older legacy runs may show fewer BUYs in deadline-terminalized edge cases.

## Alternatives Considered

### 1. Leave legacy terminal BUY logic unchanged and patch logger routing separately

Rejected.

Reason: it would restore `gatekeeper_v2_buys.jsonl` recreation but would preserve the more serious correctness defect of phase-count-only invalid BUYs.

### 2. Force all legacy mode calls directly into `evaluate_from_features(...)`

Rejected for this fix.

Reason: the narrow requirement was to preserve the existing legacy pre-deadline wait semantics while fixing terminal correctness and decision metadata consistency.

### 3. Fix the issue only in configuration by requiring `use_three_layer_decision = true`

Rejected.

Reason: the bug existed in the runtime compatibility branch itself and could be reintroduced whenever legacy mode was intentionally enabled.

## Validation Steps

Validated in this session with targeted test execution:

1. Legacy feature-terminal regression set:
   - `cargo test -p ghost-launcher feature_driven_legacy_mode -- --nocapture`
   - verified:
     - `feature_driven_legacy_mode_keeps_wait_semantics_before_deadline`
     - `feature_driven_legacy_mode_deadline_does_not_buy_on_phase_count_only`
     - `feature_driven_legacy_mode_buy_populates_log_routing_fields`

2. Existing timeout-path regression:
   - `cargo test -p ghost-launcher evaluate_feature_driven_terminal_verdict_times_out_without_force_check_deadline -- --nocapture`

3. Additional in-session verification:
   - editor diagnostics for touched Rust files were clean after the final patch,
   - runtime RCA confirmed the logger path was correct and the missing BUY-only file was caused by absent decision metadata rather than a writer startup failure.
