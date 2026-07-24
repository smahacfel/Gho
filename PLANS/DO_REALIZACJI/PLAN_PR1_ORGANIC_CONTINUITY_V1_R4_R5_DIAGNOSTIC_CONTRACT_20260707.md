# PLAN: PR1 Organic Continuity V1 - R4/R5 Diagnostic Evidence Contract

Status: `READY_FOR_IMPLEMENTATION_DIAGNOSTIC_ONLY`
Data: 2026-07-07
Zakres: PR1 evidence-vector / diagnostic-only contract dla Organic Continuity V1
Canonical source: `MaterializedFeatureSet` / `v3_materialized_feature_snapshot`

## Executive Decision

PR1 Organic Continuity V1 nie moze implementowac directional policy score ani zakladac, ze wyzszy buy pressure jest lepszy.

Po R5 discovery i R4 OOS validation PR1 zostaje ograniczony do:

```text
diagnostic evidence vector
raw component exposure
bucket/reason code surface
no runtime policy
no promotion score
```

Preferowany podzial kontraktu:

```text
organic_continuity_evidence_v1 = raw evidence/vector
organic_continuity_experimental_score_v1 = optional diagnostic only
```

Jezeli jakikolwiek `organic_continuity_score` lub odpowiednik zostanie dodany w przyszlosci, musi byc oznaczony jako:

```text
experimental_diagnostic_score
not_policy_score
not_promotion_candidate
direction_unvalidated
```

## R4/R5 Evidence Update

R4/R5 offline evidence indicates low/limited buy pressure cohorts reduce median loss and left tail.
This contradicts the initial assumption that higher buy ratio should be rewarded.
Therefore Organic Continuity V1 is diagnostic-only and cannot be used for Gatekeeper policy promotion.

Evidence references:

- `reports/selector/r5_organic_continuity_availability_audit.md`
- `reports/selector/r5_edge_candidate_rules.md`
- `reports/selector/r4_oos_validation_of_r5_edge_rules.md`
- `reports/selector/l2_edge_filter_candidates_r4_r5_20260707.md`

Observed OOS_PASS examples:

```text
sol_buy_ratio <= 0.5099 / 0.5173 / 0.5326
organic_broadening.buy_ratio_mean <= 0.25
buy_ratio_max <= 0.6
buy_count <= 4
```

Interpretation boundary:

- These are loss/tail-reduction research signals, not positive absolute PnL proof.
- Thresholds remain diagnostic/research context only.
- No high-buy-ratio positive policy assumption is allowed in PR1.
- No Gatekeeper promotion is allowed from this evidence.

## Required Claim Boundaries

Every PR1 output, evidence object, report, and review summary must preserve:

```text
diagnostic_only=true
shadow_only=true
changes_gatekeeper_decision=false
changes_execution=false
production_promotion_allowed=false
policy_score=false
runtime_filter=false
```

These boundaries are part of the contract, not commentary.

## Explicit Non-Goals

Do not:

```text
modify Gatekeeper BUY/REJECT
modify selector_shadow_score_combined_simple_v1
modify organic_broadening_passes
use lifecycle/outcome/PnL/terminal/exit as input feature
make high buy_ratio a positive policy assumption
```

Also out of scope:

- TX/Jito/live path changes.
- Runtime promotion.
- Live filter, active close, shadow close, or trigger behavior.
- Any use of `shadow_lifecycle` as denominator or feature source.
- Legacy HyperPrediction / old scoring revival.

## Evidence Vector Fields

PR1 evidence vector must expose raw decision-time fields only, derived from `MaterializedFeatureSet`.

Organic raw fields:

- `organic_broadening.sequence_available`
- `organic_broadening.total_tx_count`
- `organic_broadening.total_unique_signers`
- `organic_broadening.t0_tx_count`
- `organic_broadening.t1_tx_count`
- `organic_broadening.t2_tx_count`
- `organic_broadening.t0_unique_signers`
- `organic_broadening.t1_unique_signers`
- `organic_broadening.t2_unique_signers`
- `organic_broadening.t1_vs_t0_unique_signer_delta`
- `organic_broadening.t2_vs_t1_unique_signer_delta`
- `organic_broadening.tx_count_growth_ratio`
- `organic_broadening.unique_signer_growth_ratio`
- `organic_broadening.buy_ratio_mean`
- `organic_broadening.buy_ratio_min`
- `organic_broadening.buy_ratio_max`
- `organic_broadening.max_segment_hhi`
- `organic_broadening.min_segment_hhi`
- `organic_broadening.signer_growth_t2_t0`
- `organic_broadening.hhi_delta_t2_t0`
- `organic_broadening.tx_count_growth_vs_signer_growth`
- `organic_broadening.new_signer_ratio_t2`
- `organic_broadening.broadening_score` as existing raw diagnostic field, not policy score
- `organic_broadening.status`
- `organic_broadening.degraded_reasons`

Context fields:

- `tx_intel_features.sol_buy_ratio`
- `tx_intel_features.buy_ratio`
- `tx_intel_features.buy_count`
- `tx_intel_features.burst_ratio`
- `tx_intel_features.same_ms_tx_ratio`
- `alpha_fingerprint.flipper_presence_ratio`
- `alpha_fingerprint.fixed_size_buy_ratio`
- `manipulation_contradictions.contradiction_score`

Availability fields:

- `core_three_finite`
- `raw_organic_vector_finite`
- `sequence_available`
- `status_usable`
- `full_context_available`
- missing/non-finite reason codes

Bucket/reason-code surface:

- Diagnostic bucket codes may represent R4/R5 observed regions.
- Bucket codes must be symmetric/neutral: low and high sides are descriptive, not reward/reject.
- Bucket codes must not trigger BUY, REJECT, selector scoring, live execution, shadow close, or promotion.

## Experimental Score Policy

Default PR1 preference: no Organic Continuity score.

If an optional score container exists, it must have:

- `status = not_implemented` or `disabled` until a separate validation PR approves it.
- `value = null` unless explicitly produced by an offline diagnostic run.
- `experimental_diagnostic_score = true`
- `not_policy_score = true`
- `not_promotion_candidate = true`
- `direction_unvalidated = true`
- stable contract hash that changes when schema or weight seed changes.

No score may be used by Gatekeeper, selector score, TX/Jito/live path, or runtime filters.

## Required Tests

PR1 implementation must add targeted tests proving:

- evidence vector serializes all raw organic fields.
- low buy ratio is represented neutrally, not rejected.
- high buy ratio is represented neutrally, not rewarded as policy.
- claim boundaries forbid runtime promotion.
- no outcome/lifecycle fields are used.
- contract hash changes on schema/weight change if experimental score exists.

Recommended test location:

- `ghost-core/tests/organic_continuity_contract_tests.rs`

Required negative diff checks:

- No edits in `ghost-launcher/src/components/gatekeeper_v3.rs`.
- No edits to `organic_broadening_passes`.
- No edits to `selector_shadow_score_combined_simple_v1` builders/auditors.
- No edits to TX/Jito/live execution path.

## Acceptance Gates

Implementation can be accepted only if all are true:

```text
diagnostic_only=true
shadow_only=true
changes_gatekeeper_decision=false
changes_execution=false
production_promotion_allowed=false
policy_score=false
runtime_filter=false
```

And:

- PR1 derives evidence from `MaterializedFeatureSet`, not mutable live state.
- PR1 does not read lifecycle/outcome/PnL/terminal/exit fields as input.
- PR1 does not change BUY/REJECT/TIMEOUT verdict behavior.
- PR1 does not change selector score or Gatekeeper V3 organic gate behavior.
- PR1 keeps R4/R5 thresholds diagnostic only.
- PR1 has focused unit tests and diff audit.

## Remaining Blockers Before Promotion

Promotion to any policy/runtime use remains blocked by:

- at least one more independent L2 validation dataset;
- separate policy-design review;
- explicit Gatekeeper Policy Auditor review;
- explicit Config Rollout Safety review;
- proof that a policy candidate improves selection without worsening left tail;
- no hidden dependency on outcome/lifecycle fields.

Until then, Organic Continuity V1 is evidence only.
