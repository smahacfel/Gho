# ADR-0141: Coordination Risk Phase 0.6 runtime sidecar TIMEOUT join closure

**Date:** 2026-05-31
**Status:** Accepted for Phase 0.6 runtime sidecar join evidence; no selector or policy promotion
**Author:** Codex
**Follows:** `ADR-0140-coordination-risk-phase06-core-substrate-status.md`
**Plan:** `PLANS/PLAN_ANTYCABAL_PHASE_0_6_FINAL_20260531.md`

## Context

`ADR-0140` accepted the corrected `ghost-core::features::coordination` substrate as an
export-only Phase 0.6 core, but kept full Phase 0.6 open because runtime sidecar emission and
artifact-level validation were still pending.

The subsequent runtime sidecar work added:

- decision-time frozen coordination snapshots in `ghost-launcher`;
- additive `CoordinationRiskEvidenceUnit` construction from the frozen snapshot;
- a separate DecisionLogger command and writer for `coordination_risk_evidence.jsonl`;
- targeted no-policy-drift tests around BUY, REJECT and TIMEOUT fixtures.

A runtime smoke then exposed one remaining join gap. `coordination_risk_evidence.jsonl` emitted
TIMEOUT rows, but the corresponding `v25_shadow` terminal decision rows could be dropped by
DecisionLogger plane expansion. The specific failing class was:

```text
active terminal verdict_type = TIMEOUT_*
v25_shadow_verdict_type = None
v25_shadow_confidence = Some(...)
v25_shadow_observation_stage = Some(...)
ab_record_id present
coordination sidecar decision_id == ab_record_id
```

The sidecar row was durable, but the decision-plane row needed for a clean join was not always
durable. This made the runtime sidecar unsafe to call formally closed even though the core evidence
builder was already fail-closed and export-only.

## Decision

Accept a narrow DecisionLogger fix and runtime smoke result as the Phase 0.6 runtime sidecar join
closure for this failure class.

The accepted contract is:

1. `TIMEOUT*` verdicts are terminal outcomes, not BUY and not REJECT.
2. `decision_verdict_buy` must remain absent/null for TIMEOUT rows.
3. A `v25_shadow` row may use the active terminal `TIMEOUT*` verdict as fallback only when:
   - `v25_shadow_verdict_type` is missing;
   - the active terminal `verdict_type` starts with `TIMEOUT`;
   - cached shadow assessment evidence exists through `v25_shadow_confidence` or
     `v25_shadow_observation_stage`.
4. `v25_shadow_reason_chain` by itself is not enough to inherit the terminal TIMEOUT verdict.
5. Main-path `reason_code` fallback is allowed only for the cached-shadow TIMEOUT fallback above.
6. Shadow REJECT rows without a mappable shadow reason code must not inherit main-path
   `reason_code`.
7. Coordination-risk evidence remains export-only and additive.

This ADR does not authorize:

- Gatekeeper scoring changes;
- coordination penalties;
- size multiplier changes;
- threshold tuning;
- `MaterializedFeatureSet` extension;
- CTC/CPCR/ETC revival;
- FSC v2 policy use;
- selector usefulness or promotion claims.

## Implementation

The implementation is intentionally scoped to `ghost-brain/src/oracle/decision_logger.rs`.

### TIMEOUT buy-alias semantics

`gatekeeper_buy_alias_from_verdict()` now treats verdict families explicitly:

```text
BUY*     -> Some(true)
REJECT*  -> Some(false)
TIMEOUT* -> None
unknown -> None
```

This prevents TIMEOUT rows from being represented as false/reject rows in fields that are meant to
carry a buy alias.

### Cached-shadow TIMEOUT fallback

The helper `has_v25_shadow_cached_assessment_evidence()` defines the evidence required for terminal
TIMEOUT fallback:

```text
v25_shadow_confidence.is_some()
OR
v25_shadow_observation_stage.is_some()
```

`v25_shadow_verdict_type_or_terminal_fallback()` may copy active terminal `TIMEOUT*` into the
shadow row only when that cached-shadow evidence exists.

This resolves the contract caveat from review: a shadow reason chain alone still proves that a
shadow-plane row exists, but it does not prove a cached assessment verdict/stage state and therefore
must not inherit terminal TIMEOUT.

### Reason-code guard

`expand_gatekeeper_plane_logs()` still derives the shadow row reason code from the shadow verdict
type when possible. It falls back to the main reason code only for the explicit cached-shadow
TIMEOUT fallback. This preserves the existing no-fallback guard for shadow reject rows.

### Regression tests

The patch adds or preserves three logger-level checks:

- `test_logger_persists_v25_timeout_with_cached_confidence_only`
  - proves a cached-confidence/stage TIMEOUT persists to the `v25_shadow` decision file;
  - proves it does not go to the `v25_shadow` BUY file;
  - proves `decision_verdict_buy` remains absent/null.
- `test_expand_timeout_fallback_requires_cached_shadow_assessment_evidence`
  - proves `v25_shadow_reason_chain` alone does not inherit terminal TIMEOUT verdict or reason
    code.
- `test_expand_shadow_plane_does_not_fallback_to_main_reason_code`
  - preserves the prior guard for shadow reject rows.

## Runtime Smoke Evidence

The runtime smoke used the pre-existing shadow-burnin config and was intentionally bounded by a
timeout wrapper:

```text
scope = coordination-risk-phase06-timeout-join-fix-smoke-20260531T203705Z
head_revision = 3f1f9bed39337189ee6b533ff97b917d327948e8
code_state = dirty worktree patch
command = timeout 30m env RUST_LOG=info cargo run --release -p ghost-launcher --bin ghost-launcher -- --config /root/Gho/configs/rollout/shadow-burnin.toml
expected_exit = 124
```

Primary local artifact:

```text
logs/validation/coordination-risk-phase06-timeout-join-fix-smoke-20260531T203705Z/runtime_smoke_validation_report.md
```

Smoke result:

```text
status = PASS
sidecar rows before run = 659
sidecar rows after run = 1107
sidecar rows appended = 448
v25_shadow decision rows scanned = 448
joined rows = 448
missing joins = 0
join coverage = 1.0
```

Decision suffix breakdown:

```text
BUY = 57
TIMEOUT = 216
REJECT = 175
```

The joined suffix breakdown matched the sidecar suffix breakdown exactly:

```text
BUY = 57
TIMEOUT = 216
REJECT = 175
```

Artifact invariants:

```text
snapshot_mode_bad_count = 0
missing_source_snapshot_hash_count = 0
cutoff_after_decision_count = 0
cutoff_slot_after_decision_count = 0
watermark_after_decision_count = 0
penalty_field_count = 0
score_eligible_true_count = 0
fake_zero_fsc_count = 0
skipped_ctc_cpcr_etc_missing_count = 0
gk_reason_code_missing_count = 0
dropping_plane_row_count = 0
panic_signature_count = 0
```

Interpretation:

```text
The positive cached-shadow TIMEOUT fallback path is validated by runtime artifact.
The reason-chain-only caveat is validated by targeted unit test, not by a second runtime smoke.
```

This distinction is intentional. The final contract narrowing does not alter the positive
cached-confidence/stage class proven by the smoke; it only rejects the broader reason-chain-only
case.

## Current Status

```text
Phase 0.6 core substrate: PASS
Repo-truth gap for core substrate: CLOSED
Runtime frozen snapshot contract: PASS at code level
Runtime sidecar writer: PASS at code level
Runtime sidecar TIMEOUT join artifact: PASS for cached-shadow TIMEOUT fallback
Reason-chain-only TIMEOUT fallback: FORBIDDEN by test
Behavioral no-policy-drift: PASS at targeted code level and artifact sidecar level
Full selector readiness: NOT CLAIMED
Metric usefulness: NOT CLAIMED
Promotion readiness: NOT CLAIMED
```

Phase 0.6 may now be treated as closed for the narrow objective of:

```text
decision-time-safe coordination-risk evidence can be emitted as an export-only sidecar
and joined back to terminal v25_shadow decision rows for BUY, REJECT and TIMEOUT.
```

Phase 0.6 still does not prove:

```text
precision lift
false-positive reduction
R2 readiness
selector_training_view readiness
threshold readiness
active Gatekeeper policy readiness
```

## Residual Caveats

### No second runtime smoke after contract narrowing

The 30-minute smoke was run before the final reason-chain-only narrowing. It validates the positive
cached-confidence/stage TIMEOUT class. The final narrowing is covered by unit test.

If future reviewers require artifact proof for the negative reason-chain-only case, run a synthetic
or fixture-driven logger smoke that emits `v25_shadow_reason_chain=Some(...)` with no
`v25_shadow_confidence` and no `v25_shadow_observation_stage`.

### `run_id` is nullable

In the smoke, all appended sidecar rows had `run_id = null`. This did not block joinability because
`decision_id == ab_record_id` had full coverage, and `scope_id`/`candidate_id` were present.

For Phase 1 dataset ergonomics, runtime profiles should prefer filling `run_id` when available.

### Pubkey JSON shape

`pool_id` and `mint` are serialized through the current Pubkey representation as byte arrays in the
sidecar. This is valid for structured Rust serde, but less ergonomic for Python/JSONL analytics.

Future Phase 1 export may add base58 mirror fields if needed:

```text
pool_id_base58
mint_base58
```

Do not replace the existing fields destructively without a schema/version decision.

### Funding visibility in this smoke

All appended rows had:

```text
funding_visibility = unavailable
funding_source_concentration = None
```

This is safe for Phase 0.6 because no fake clean `0.0` FSC value was emitted. It does not validate
FSC v2 usefulness.

## Validation

Checks run after the final contract narrowing:

```text
cargo fmt --package ghost-brain
cargo test -p ghost-brain --lib test_logger_persists_v25_timeout_with_cached_confidence_only -- --nocapture
cargo test -p ghost-brain --lib test_expand_timeout_fallback_requires_cached_shadow_assessment_evidence -- --nocapture
cargo test -p ghost-brain --lib test_expand_shadow_plane_does_not_fallback_to_main_reason_code -- --nocapture
```

Additional checks run before the final contract narrowing, on the same DecisionLogger smoke-fix
workstream:

```text
cargo test -p ghost-brain --lib test_logger_splits_legacy_and_shadow_planes -- --nocapture
cargo test -p ghost-brain --lib test_coordination_risk_evidence_sidecar_logging -- --nocapture
cargo test -p ghost-launcher --lib phase06_runtime_sidecar -- --nocapture
cargo fmt --package ghost-brain --check
git diff --check
cargo check -p ghost-brain -p ghost-launcher
```

Warnings observed during validation were pre-existing deprecated/unused warnings outside this
change surface.

## Consequences

Positive:

- TIMEOUT sidecar rows can join back to `v25_shadow` decision rows.
- TIMEOUT is no longer collapsed into a false/reject buy alias.
- Cached-shadow TIMEOUT fallback is explicit and bounded.
- Reason-chain-only shadow evidence cannot silently become a terminal shadow TIMEOUT verdict.
- Coordination-risk sidecar evidence remains additive and export-only.
- Existing no-main-reason fallback guard for shadow reject rows is preserved.

Negative / cost:

- A reason-chain-only shadow row without cached confidence/stage may still be dropped by downstream
  reason-code guards if it lacks a mappable shadow verdict. That is intentional until the runtime
  supplies stronger shadow assessment evidence.
- The smoke artifact is local under `logs/validation/...`; this ADR records the relevant counts so
  GitHub history preserves the closure evidence even if local artifacts are not tracked.

## Next Step

Do not add new coordination-risk metrics or activate scoring.

Return to the selector denominator path:

```text
candidate_universe_v1
accepted_lifecycle_v1
feature_snapshots_v1
R2 canonical market path source
selector_training_view_v1
baseline precision report
```

Any future promotion of coordination-risk evidence into active policy requires a separate ADR,
resolved R1/R2 denominator, leakage audit, holdout ablation and explicit no-regression evidence.
