# ADR-0097: Final sybil policy closes with a separate budgeted interference layer

**Date:** 2026-04-14
**Status:** Proposed
**Author:** Ghost Father

## Context

The repository now has all six anti-sybil metrics on the canonical feature path under `MaterializedFeatureSet.sybil_resistance`:

1. FTDI — Fee Topology Diversity Index
2. DBIA — Dev-Buyer Infrastructure Affinity
3. SFD — Spend Fraction Divergence
4. DES — Demand Elasticity Score
5. CPV — Signer Cross-Pool Velocity
6. FSC — Funding Source Concentration

Repository verification also confirms four operational facts:

1. the metrics already appear in buy logs and fingerprint telemetry,
2. `gatekeeper_policy.rs` still evaluates only legacy hard-fails, core checks, and legacy soft signals,
3. the active `ghost_brain_config.toml` freezes legacy soft scoring with `max_soft_points = 255` and `dev_unknown_max_soft_points = 255`,
4. FSC now has the required runtime/data-plane integration and E2E coverage, but remains policy-neutral.

ADR-0095 already established that the six metrics should not be forced into legacy phase thresholds and should not become single-metric hard fails in the first rollout. However, ADR-0095 left one implementation-critical question open:

**how should the sybil layer become policy-effective without distorting the existing legacy soft-scoring model, especially while legacy soft scoring is intentionally frozen in production config?**

If sybil flags are merely appended to the existing `soft_points`, the system risks a false sense of activation:

- telemetry changes,
- JSONL fields populate,
- but BUY/REJECT behavior may remain unchanged because the legacy soft threshold stays operationally neutral.

This means the final closure decision is not only about *which* sybil flags and combos matter, but also about *where* their budget and threshold live.

## Decision

The final sybil policy will be implemented as a **separate budgeted interference layer** with its own diagnostics, point budget, and optional combo-veto stage.

### Accepted final policy shape

1. **Keep the existing Hard Fail layer unchanged.**
   - No single sybil metric becomes a hard fail in the initial activation.
   - Hard fails remain reserved for explicit safety / integrity conditions.

2. **Keep the existing Core Pass logic unchanged.**
   - Sybil metrics do not become ordinary phase gates.
   - Existing viability / safety thresholds stay semantically separate.

3. **Keep the existing Legacy Soft bucket unchanged.**
   - `SoftSignals` and legacy weighted scoring keep their current meaning.
   - Legacy soft scoring may remain frozen in rollout config without blocking sybil activation.

4. **Add a distinct `Sybil Interference` bucket.**
   - Introduce dedicated sybil diagnostics, e.g. `SybilSoftSignals` and `SybilPolicyDiagnostics`.
   - Compute:
     - `sybil_soft_points`
     - `effective_max_sybil_soft_points`
     - `sybil_interference_patterns`
     - optional `sybil_meta_score`

5. **Give the sybil layer its own threshold budget.**
   - Add:
     - `max_sybil_soft_points`
     - `dev_unknown_max_sybil_soft_points`
   - This threshold is independent from legacy `max_soft_points`.

6. **Use canonical sybil thresholds only through the sybil layer.**
   - `min_fee_topology_diversity_index`
   - `max_dev_buyer_infrastructure_affinity`
   - `min_spend_fraction_divergence`
   - `min_demand_elasticity_score`
   - `max_signer_cross_pool_velocity`
   - `max_funding_source_concentration`

7. **Assign final semantic roles to the six metrics as follows.**
   - `DES` is the lead signal.
   - `high_dbia && low_ftdi` is the strongest structural pattern.
   - `SFD` is the main capital-behavior corroborator.
   - `CPV` is a rotation corroborator, never strong enough solo.
   - `FSC` is a funding corroborator, activated last.

8. **Protect against known false-positive patterns.**
   - `high_dbia && high_ftdi` must not be treated like cabal structure.
   - `high_cpv` solo must remain weak.
   - `high_fsc` solo must not become a veto.

9. **Treat degraded or unavailable metrics as zero-penalty inputs.**
   - `None` never penalizes.
   - CPV/FSC cannot penalize without readiness.
   - FSC cannot contribute penalty or combo-veto while `FSC_FUNDING_STREAM_UNAVAILABLE` or equivalent readiness-degrading conditions apply.

10. **Add an optional future combo-veto, but only after bake.**
    - Introduce a dedicated verdict such as `RejectSybilInterference`.
    - Enable it only for a small whitelist of validated patterns.
    - Never promote a single metric to veto in the first activation.

### Accepted verdict ordering

Within policy evaluation, the final ordering should be:

1. hard-fail rejection,
2. core-fail rejection,
3. legacy soft rejection,
4. sybil combo-veto rejection (when explicitly enabled and fully ready),
5. sybil soft-budget rejection,
6. BUY.

The system may still export:

- `legacy_soft_points`
- `sybil_soft_points`
- `total_soft_points = legacy_soft_points + sybil_soft_points`

But `total_soft_points` is accepted as a telemetry value first, **not** as the primary gating threshold.

## Architectural Impact

This decision preserves current SSOT boundaries while closing the activation gap:

- `MaterializedFeatureSet.sybil_resistance` remains the only authoritative carrier,
- `gatekeeper_policy.rs` becomes the single interpretation point for sybil verdict logic,
- existing legacy phase/core logic remains stable,
- rollout can activate sybil filtering without unfreezing or recalibrating legacy soft scoring.

Concrete implementation consequences:

1. add dedicated sybil decision types / structs in the Gatekeeper decision layer,
2. add new config fields for a separate sybil threshold budget,
3. extend JSONL / decision logs with separate legacy vs sybil explainability fields,
4. add verdict types such as:
   - `RejectSybilSoftExcess`
   - `RejectSybilInterference`
5. add tests proving readiness / degraded states never become silent penalties.

## Risk Assessment

**Rate:** Medium

- **Low** risk while the layer is telemetry-only or penalty-zero.
- **Medium** risk once `sybil_soft_points` begin to reject pools through their own threshold.
- **High** risk if combo-veto is promoted before replay and telemetry bake.
- **Critical** risk if FSC or CPV are allowed to penalize from degraded or partially ready state.

## Consequences

What becomes easier:

- sybil activation becomes real, not cosmetic,
- legacy soft scoring can remain operationally independent,
- explainability improves because legacy and sybil suspicion are no longer conflated,
- rollout can stage DES/DBIA/FTDI/SFD first and FSC last without semantic hacks.

What becomes harder:

- policy telemetry surface grows,
- the verdict model gains extra states,
- config shape becomes larger,
- combo-veto promotion requires stricter replay governance.

## Alternatives Considered

### 1. Append sybil points directly into legacy `soft_points`

Rejected because the active production config currently freezes legacy soft scoring. This would make sybil activation appear implemented while remaining operationally neutral.

### 2. Unfreeze legacy `max_soft_points` and reuse one shared budget

Rejected because it couples two semantically different buckets and forces recalibration of legacy soft behavior just to activate sybil logic.

### 3. Promote `DES` or `high_dbia && low_ftdi` directly to hard fail

Rejected for first activation because the repo still needs replay bake and explicit false-positive review before terminalizing sybil patterns.

### 4. Keep sybil metrics as telemetry-only indefinitely

Rejected because the canonical feature path, config surface, and runtime investment are already present; leaving them permanently non-authoritative would waste the architecture.

## Validation Steps

1. Add dedicated sybil diagnostics and verdict types to the Gatekeeper decision layer.
2. Add separate config thresholds for the sybil bucket budget.
3. Verify neutral sybil config still produces zero decision drift.
4. Add tests proving:
   - `high_dbia && high_ftdi` is not treated like cabal structure,
   - `low_des` is the strongest single sybil signal,
   - degraded / `None` metrics yield zero penalty,
   - CPV/FSC cannot penalize before readiness,
   - sybil rejection still works while legacy `max_soft_points = 255`.
5. Export separate legacy and sybil explainability fields to JSONL.
6. Run replay and paper-burnin bake before enabling combo-veto.
7. Only after replay validation, allow a narrow whitelist of combo patterns to trigger `RejectSybilInterference`.