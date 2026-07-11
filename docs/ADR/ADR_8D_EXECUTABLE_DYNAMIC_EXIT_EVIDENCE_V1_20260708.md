# ADR-8D: Executable Dynamic-Exit Evidence Sidecar V1

## Status

Accepted for implementation as observe-only Shadow V2 research evidence.

## Context

R4/R5 path-based dynamic exit analysis showed that the current path data is mark-only. Fixed timeout labels were sufficient to diagnose that fixed 120s interpretation is misleading, but they were not sufficient to prove that dynamic exits are executable. Promoting dynamic exits would require quote/fill-equivalent evidence at the trigger point.

The required next step is not a runtime close policy. It is a sidecar that records static quote-equivalent evidence for frozen candidate exits while preserving every active runtime boundary.

## Decision

Add a disabled-by-default sidecar:

```text
executable_dynamic_exit_evidence_v1.jsonl
```

The sidecar is not a canonical `ShadowV2Record`, not `shadow_exit_fill_v2`, not terminal truth, not lifecycle truth, and not a live fill.

Implementation boundaries:

- `ghost-brain` owns schema, pure static quote helper, candidate policy definitions, forward-only evaluator, and Mayhem availability check.
- `ghost-launcher` owns optional sidecar file emission from existing Shadow V2 post-buy evidence context.
- Manifest audit treats the sidecar as conditional: required only when explicitly enabled.

## Constraints

Do not change:

```text
Gatekeeper policy
BUY/REJECT logic
selector score
TX/Jito/live path
provider streams
runtime close
shadow_close_only
active_close
```

Do not emit dynamic-exit candidate rows as canonical `shadow_exit_fill_v2`.

Do not use terminal truth, lifecycle outcome, final PnL, future path samples, or post-hoc best samples to select dynamic-exit trigger points.

## Schema Guarantees

Every row must include:

```text
decision_neutral=true
runtime_close_triggered=false
changes_gatekeeper_decision=false
changes_execution=false
static_model_only=true
not_live_fill=true
not_canonical_exit=true
```

Allowed evidence quality values:

```text
STATIC_QUOTE_AVAILABLE
BLOCKED_BY_POOL_STATE_PROVENANCE
BLOCKED_BY_STALE_POOL_STATE
BLOCKED_BY_MISSING_ENTRY_FILL
BLOCKED_BY_MISSING_TOKEN_AMOUNT
BLOCKED_BY_QUOTE_MODEL
MARK_ONLY_NO_EXECUTABLE_QUOTE
NO_TRIGGER_BY_DECLARED_HORIZON
```

## Consequences

Positive:

- Dynamic exit research can collect static executable-quote evidence without changing runtime behavior.
- Future R4/R5/R6 style analyses can separate mark-only path performance from static quote-equivalent feasibility.
- The sidecar can fail independently from canonical evidence emission.

Tradeoffs:

- This still does not prove live fills, live slippage, or live-equivalence.
- Static quotes remain model evidence and must not be marketed as confirmed exits.
- When the sidecar is disabled, existing L2-F manifests remain valid and the artifact is not required.

## Verification

Required checks:

```bash
cargo test -p ghost-brain executable_dynamic_exit -- --nocapture
cargo test -p ghost-launcher executable_dynamic_exit -- --nocapture
cargo test -p ghost-launcher shadow_v2_no_decision_consumption_static_guard -- --nocapture
cargo check -p ghost-brain
cargo check -p ghost-launcher
cargo fmt --check
git diff --check
```

Expected PR verdict:

```text
EXECUTABLE_DYNAMIC_EXIT_EVIDENCE_V1_OBSERVE_ONLY_READY
```
