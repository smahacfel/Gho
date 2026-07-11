# Plan: Executable Dynamic-Exit Evidence Sidecar V1

## Verdict

```text
OBSERVE_ONLY_STATIC_DYNAMIC_EXIT_QUOTE_EVIDENCE_ADDED
```

This plan adds a research-only sidecar:

```text
executable_dynamic_exit_evidence_v1.jsonl
```

The sidecar answers one bounded question: if a declared dynamic-exit candidate policy wanted to exit at an observed path point, what static quote-equivalent would the current Shadow V2 model have produced?

## Scope

In scope:

- Add `ExecutableDynamicExitEvidenceV1` as a non-canonical JSONL sidecar schema.
- Add a pure `estimate_static_exit_quote_v1(...)` helper based on existing Shadow V2 constant-product static sell semantics.
- Add a forward-only candidate evaluator over observed `ShadowPathSampleV2` and `PoolStateSampleV2`.
- Add additive disabled-by-default config:
  - `executable_dynamic_exit_evidence_enabled`
  - `executable_dynamic_exit_evidence_path`
  - `executable_dynamic_exit_candidate_policies`
- Add a fail-open sidecar writer in `PostBuyRuntime`.
- Add manifest contract support where the sidecar is required only when explicitly enabled.
- Add Mayhem Mode field availability reporting as availability-only, not inference.

Out of scope:

- No Gatekeeper policy change.
- No BUY/REJECT change.
- No selector score change.
- No TX/Jito/live path change.
- No runtime close, active close, or `shadow_close_only`.
- No new canonical `shadow_exit_fill_v2` rows for dynamic-exit candidates.
- No live-fill, live-equivalence, or executable-verified claim.

## Candidate Policies

```text
fixed_exit_2s
fixed_exit_3s
fixed_exit_5s
fixed_exit_10s
tp500_sl500_max30s
tp1000_sl700_max30s
tp2000_sl1000_max60s
trailing_after_1000_trail_500
trailing_after_2000_trail_1000
```

Policy finalization is forward-only:

- Fixed exits use the first observed sample with `age_ms >= fixed_age_ms`.
- TP/SL uses the first observed sample hitting TP or SL before max hold; otherwise it uses max hold.
- Trailing activates only after an observed mark-PnL activation threshold, tracks observed peak, and triggers on observed drawdown.
- No terminal truth, lifecycle outcome, final PnL, future path sample, or post-hoc best sample is allowed for trigger selection.

## Evidence Quality

Allowed values:

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

Forbidden claims:

```text
EXECUTABLE_VERIFIED
LIVE_FILL_READY
EXIT_CONFIRMED
```

Every row must carry:

```text
decision_neutral=true
runtime_close_triggered=false
changes_gatekeeper_decision=false
changes_execution=false
static_model_only=true
not_live_fill=true
not_canonical_exit=true
```

## Manifest Contract

`executable_dynamic_exit_evidence_v1.jsonl` is conditional:

- disabled: `NOT_REQUIRED_DISABLED`;
- enabled: file must exist, row count must be reported, sha must be reported when sha checks are active.

Historical L2-F baseline runs remain valid when the sidecar is disabled.

## Mayhem Availability

Mayhem Mode is availability-only in this PR. It looks only for pre-entry-safe fields such as:

```text
is_mayhem_mode
mayhem_mode
launch_mode
mode
total_supply
mint_supply
initial_curve_token_amount
bot_allocation_amount
```

If unavailable, report:

```text
MAYHEM_MODE_BLOCKED_MISSING_PRE_ENTRY_FIELDS
```

No PnL, dynamic-exit result, lifecycle outcome, or terminal truth may be used to infer Mayhem.

## Acceptance

```text
EXECUTABLE_DYNAMIC_EXIT_EVIDENCE_V1_OBSERVE_ONLY_READY
```

Required proof:

- schema added;
- config added and disabled by default;
- candidate policies supported;
- static quote model reused as pure helper;
- no `shadow_exit_fill_v2` emitted by sidecar;
- no terminal/lifecycle leakage;
- manifest contract conditional;
- write failures are fail-open for runtime;
- Gatekeeper, BUY/REJECT, selector, live TX/Jito, `shadow_close_only`, and active close remain unchanged.
