# ADR-0092: Six new Gatekeeper metrics follow the canonical feature path

**Date:** 2026-04-10
**Status:** Accepted
**Author:** Ghost Father

## Context

The repository is preparing to add six new anti-sybil / anti-cabal metrics:

1. FTDI — Fee Topology Diversity Index
2. DBIA — Dev-Buyer Infrastructure Affinity
3. SFD — Spend Fraction Divergence
4. DES — Demand Elasticity Score
5. CPV — Signer Cross-Pool Velocity
6. FSC — Funding Source Concentration

During analysis, three architectural constraints became explicit:

1. the production Gatekeeper decision path is authoritative only when driven from `MaterializedFeatureSet`,
2. `early_fingerprint` and `FINGERPRINT` logging are observability surfaces, not canonical truth for BUY/REJECT,
3. CPV and FSC require bounded rolling state beyond the local per-pool transaction window.

There was a tempting shortcut: compute some or all of these metrics in post-verdict telemetry or attach them ad hoc to the assessment layer. That would violate SSOT, reduce replayability, and create ambiguity about whether a decision came from canonical features or side-channel enrichment.

At the same time, the existing `TxIntelFeatures` structure is primarily local-session oriented, while CPV/FSC depend on global rolling indexes. Mixing all of that into the legacy structure would blur feature provenance and make future audits harder.

## Decision

The accepted implementation path is:

1. introduce a dedicated canonical feature group for the six new metrics under `MaterializedFeatureSet`,
2. keep parser/transport enrichment additive and backward-compatible,
3. compute local metrics (FTDI, DBIA, SFD, DES) from session-local transaction data,
4. compute global metrics (CPV, FSC) from dedicated TTL-bounded rolling indexes,
5. mirror all six metrics to JSONL / `FINGERPRINT` logs for observability,
6. activate policy usage only through the canonical feature path, never through telemetry-only attachment.

Concretely, this means:

- **Do not** treat `assessment.early_fingerprint` as authoritative for the six new metrics.
- **Do** add a dedicated feature bundle (planned as `SybilResistanceFeatures`) to `MaterializedFeatureSet`.
- **Do** add additive raw-data transport fields where required for FTDI / DBIA / SFD.
- **Do** implement CPV and FSC on bounded rolling indexes with explicit TTL, caps, eviction telemetry, and readiness state.
- **Do** keep default thresholds and penalties neutral so that merging the implementation does not silently change production decisions.

## Architectural Impact

This decision establishes a strict separation of concerns:

- parser / transport layers provide raw inputs,
- session and rolling-index layers compute metrics,
- `MaterializedFeatureSet` stores canonical decision inputs,
- Gatekeeper policy consumes canonical features,
- logs mirror outputs but do not define truth.

It also creates a cleaner long-term contract:

- local per-pool metrics and global rolling-state metrics can coexist in one canonical bundle,
- replay remains deterministic,
- future audits can identify exactly which decision inputs were authoritative.

## Risk Assessment

**Rate:** Medium

- **Low** risk if the work stops at additive transport fields and telemetry-only export.
- **Medium** risk when adding canonical feature bundle and policy integration, because serde contracts and replay surfaces expand.
- **High** risk would exist if CPV/FSC were introduced without hard TTL/cap boundaries.
- **Critical** risk would exist if the six metrics were allowed to affect BUY/REJECT outside `MaterializedFeatureSet`.

## Consequences

What becomes easier:

- clear SSOT compliance for new metrics,
- deterministic replay and diffing,
- safe staged rollout from telemetry to policy,
- explicit bounded-memory design for CPV/FSC,
- future extension with cross-metric logic without hiding semantics in logger-only paths.

What becomes harder:

- more up-front contract work is required,
- FTDI/DBIA/SFD need parser/transport enrichment before full computation,
- FSC needs a new funding-transfer event path and operationally maintained neutral-funder list.

## Alternatives Considered

### 1. Attach the six metrics only through `early_fingerprint`

Rejected because `early_fingerprint` is not the canonical decision contract. This would make live behavior depend on a telemetry surface and break the architecture's SSOT boundary.

### 2. Extend only `TxIntelFeatures`

Rejected because CPV and FSC are not purely local-session tx-intelligence fields. Forcing them into the legacy local structure would blur provenance and complicate audits.

### 3. Compute FSC from current pool-only trade events

Rejected because FSC requires funding-flow information from the broader gRPC stream, not just pool trades. Pool-only visibility is insufficient for a trustworthy funding-source metric.

### 4. Activate all six metrics in policy immediately after implementation

Rejected because the false-positive risk is too high without telemetry bake, replay diffing, and bounded-state validation.

## Validation Steps

1. Add additive parser/transport fields required for FTDI, DBIA, and SFD.
2. Add the canonical `MaterializedFeatureSet` feature bundle for the six metrics.
3. Verify neutral-default config produces zero decision drift on replay fixtures.
4. Implement and test bounded TTL/cap behavior for CPV and FSC indexes.
5. Mirror all six metrics and degraded reasons to `gatekeeper_v2_buys.jsonl`.
6. Enable metrics in policy only after telemetry bake and explicit threshold calibration.
7. Verify final BUY/REJECT decisions can be explained entirely from canonical feature snapshots.
