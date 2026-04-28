# ADR-0095: Sybil metrics integrate as a dedicated interference policy layer

**Date:** 2026-04-14
**Status:** Proposed
**Author:** Ghost Father

## Context

The repository now has the canonical contract for six anti-sybil / anti-cabal metrics under `MaterializedFeatureSet.sybil_resistance`:

1. FTDI — Fee Topology Diversity Index
2. DBIA — Dev-Buyer Infrastructure Affinity
3. SFD — Spend Fraction Divergence
4. DES — Demand Elasticity Score
5. CPV — Signer Cross-Pool Velocity
6. FSC — Funding Source Concentration

Current code analysis shows three important facts:

1. the canonical feature path already exists and is the correct SSOT boundary,
2. `gatekeeper_policy.rs` still evaluates only legacy hard-fails, core checks, and legacy soft signals,
3. sybil-specific config thresholds and soft-penalty fields already exist, but they are not yet consumed by the policy engine.

This creates a policy-design decision point. The new metrics can be wired in as:

- plain phase thresholds similar to `min_avg_interval_ms` / `max_avg_interval_ms`,
- new hard-fail kill switches,
- additional soft-scoring flags,
- or a separate multi-level interference gate.

The design must respect the existing rollout contract from ADR-0092:

- decisions must remain canonical and replayable,
- `None` / degraded metrics must not silently become penalties,
- CPV and FSC have warmup / bounded-state semantics that do not fit legacy phase gates,
- v1 should avoid aggressive single-metric hard rejects that create false positives.

There is a second constraint: the existing Gatekeeper three-layer model already has semantic roles:

- **hard fails** for explicit kill conditions,
- **core pass** for viability / safety gates,
- **soft signals** for suspicious but non-terminal evidence.

Forcing the six sybil metrics into legacy phase thresholds would mix local and global metrics, treat missing warmup data as implicit failure pressure, and recreate brittle operator behavior similar to prior threshold misuse in the phase-2 timing path.

## Decision

The six sybil metrics should **not** be added as ordinary legacy phase thresholds and should **not** become standalone hard fails in the first policy activation.

Instead, they should be integrated as a **dedicated Sybil Interference layer** that sits logically on top of the canonical feature snapshot and alongside — not inside — the legacy soft-scoring system.

### Accepted policy shape

1. **Keep the existing hard-fail layer unchanged in v1.**
	- No single sybil metric becomes a `HardFailReason`.
	- Hard-fails remain reserved for explicit safety / integrity violations.

2. **Keep the existing core pass logic unchanged in v1.**
	- The six sybil metrics do not become direct phase gates like `min_avg_interval_ms`.
	- Existing phase gates remain focused on pool viability, safety, and baseline market structure.

3. **Add a separate `SybilSoftSignals` / `SybilPolicyDiagnostics` bucket.**
	- This bucket computes sybil-specific flags, points, interference patterns, and optional meta-score.
	- Final policy becomes:
	  - `legacy_soft_points`
	  - `sybil_soft_points`
	  - `total_soft_points = legacy_soft_points + sybil_soft_points`

4. **Use existing sybil config thresholds as flag boundaries, not as phase gates.**
	- `min_fee_topology_diversity_index`
	- `max_dev_buyer_infrastructure_affinity`
	- `min_spend_fraction_divergence`
	- `min_demand_elasticity_score`
	- `max_signer_cross_pool_velocity`
	- `max_funding_source_concentration`

5. **Define two lead signals, but only inside the interference layer.**
	- `low_des`
	- `high_dbia_low_ftdi_combo`

6. **Use the remaining metrics primarily as amplifiers / corroborators.**
	- `low_ftdi`
	- `high_dbia` (non-authoritative by itself)
	- `low_sfd`
	- `high_cpv`
	- `high_fsc`

7. **DES gets the highest sybil weight, but not a lone veto in v1.**
	- DES is treated as the strongest process-level signal.
	- However, low DES alone does not hard-reject a pool in the initial rollout.
	- Extreme DES can later be promoted to a soft-veto or dedicated reject path only after replay and telemetry bake prove low false-positive risk.

8. **DBIA is authoritative only through interaction with FTDI.**
	- `high_dbia && low_ftdi` is the strongest structural cabal pattern.
	- `high_dbia && high_ftdi` must not be penalized as an equivalent signal, because it can describe shared retail bot infrastructure.

9. **CPV and FSC remain secondary until their rolling-state baselines are validated.**
	- CPV is mainly a rotation corroborator.
	- FSC is mainly a funding concentration corroborator.
	- `high_fsc && high_cpv` may later become a stronger pattern, but only after neutral-funder classification proves reliable.

10. **Continuous meta-score remains telemetry-first.**
	 - A weighted `meta_score` may be computed and logged.
	 - It must not drive production rejection by itself until config gains calibrated normalization anchors beyond a single min/max threshold per metric.

### Recommended interference semantics

The interference layer should classify patterns explicitly, for example:

- `DES_LEAD_SFD`
- `DBIA_FTDI_STRUCTURAL`
- `DBIA_FTDI_PLUS_SFD`
- `FSC_CPV_ROTATION_CLUSTER`
- `DES_LEAD_FSC_CPV`

Each pattern should be explainable from canonical fields and exported to buy logs.

### Recommended rollout policy

#### Stage 1 — production-safe initial activation

- enable sybil flag calculation,
- add `sybil_soft_points`, `sybil_interference_pattern`, and optional `sybil_meta_score` to logs,
- keep verdict effect limited to added soft points,
- allow `None` / degraded metrics to contribute **zero** penalty.

#### Stage 2 — guarded promotion

After telemetry bake and replay diffing:

- permit a dedicated `RejectSybilInterference` verdict for a very small set of proven patterns,
- start with combos only, never with a single metric,
- require corroboration for DES-led rejection,
- keep FSC-based promotion last.

## Architectural Impact

This decision preserves the repository’s current separation of responsibilities:

- parser / session / rolling indexes compute canonical inputs,
- `MaterializedFeatureSet.sybil_resistance` remains the SSOT carrier,
- `gatekeeper_policy.rs` becomes the only place that interprets sybil metrics for BUY/REJECT,
- logs mirror outcomes and interference patterns but do not define truth.

It also implies concrete code-shape changes when implementation begins:

1. add a dedicated `SybilSoftSignals` struct,
2. add a dedicated `SybilPolicyDiagnostics` struct,
3. extend `GatekeeperDecision` / buy-log telemetry with:
	- `sybil_soft_points`
	- `sybil_interference_pattern`
	- `sybil_lead_signal`
	- optional `sybil_meta_score`
4. keep legacy soft scoring separate from sybil soft scoring,
5. optionally add a future verdict type such as `RejectSybilInterference` after calibration.

## Risk Assessment

**Rate:** Medium

- **Low** risk when the layer is telemetry-only or soft-score-only.
- **Medium** risk when sybil points begin to affect final BUY/REJECT via total soft points.
- **High** risk if DES, CPV, or FSC are promoted to standalone kill gates without warmup-quality safeguards.
- **Critical** risk if single-metric hard-fail semantics are introduced before replay calibration.

## Consequences

What becomes easier:

- the six new metrics fit the current architecture without distorting existing phase semantics,
- decisions stay explainable because interference patterns are explicit,
- replay remains deterministic,
- `None` / degraded metrics do not accidentally behave like silent rejects,
- future calibration can promote only the patterns that prove robust in real data.

What becomes harder:

- policy logic becomes one layer more sophisticated,
- telemetry surface grows,
- continuous meta-score activation requires additional calibration anchors,
- FSC remains operationally dependent on funding-stream quality and neutral-funder hygiene.

## Alternatives Considered

### 1. Treat all six metrics as ordinary threshold gates

Rejected because these metrics do not share the same semantics as existing viability-oriented phase thresholds. CPV/FSC also have warmup and bounded-state behavior that is not phase-gate friendly.

### 2. Make DES a standalone hard fail immediately

Rejected for v1 because DES is powerful but still vulnerable to small-sample and curve-readiness edge cases. It should lead scoring first, then only later be considered for stronger action with corroboration.

### 3. Make `high_dbia && low_ftdi` an immediate hard fail in v1

Rejected because even strong structural signals should first survive telemetry bake and replay validation before becoming terminal policy.

### 4. Use only a continuous meta-score

Rejected because the current config surface has threshold fields suitable for boolean flags and penalties, but not yet enough calibrated anchors for robust continuous normalization. A pure meta-score would also be less explainable operationally.

### 5. Keep the six metrics as telemetry-only forever

Rejected because the repository has already invested in canonical feature materialization and config surfaces. Leaving them permanently non-authoritative would waste that architecture and reduce production value.

## Validation Steps

1. Implement `SybilSoftSignals` and `SybilPolicyDiagnostics` in `gatekeeper_policy.rs`.
2. Verify neutral defaults still produce zero decision drift.
3. Add tests proving:
	- `high_dbia && high_ftdi` does not penalize like cabal structure,
	- `high_dbia && low_ftdi` raises the strongest structural signal,
	- `low_des` has the highest soft weight,
	- degraded / `None` metrics contribute zero penalty,
	- CPV/FSC cannot penalize when rolling state is not ready.
4. Export `sybil_interference_pattern` and `sybil_soft_points` to JSONL.
5. Run replay on historical fixtures and inspect false-positive drift.
6. Only after telemetry bake, consider promoting a subset of interference patterns to a dedicated reject verdict.
