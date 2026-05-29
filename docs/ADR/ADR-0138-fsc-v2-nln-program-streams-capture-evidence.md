# ADR-0138: FSC v2 capture/evidence through NLN Program Streams

**Date:** 2026-05-29
**Status:** Accepted for implementation
**Author:** Codex, based on Ghost Father FSC v2 scope
**Amends:** `ADR-0130-v3-fsc-scope-decision-single-stream.md`
**Plan:** `PLANS/PLAN_FSC_V2_NLN_CAPTURE_EVIDENCE_20260529.md`

## Context

`ADR-0130` de-scoped FSC from the active V3 validation path because the previous provider shape
did not offer independent stream capacity for an authoritative funding lane. The problem was a
data-plane prerequisite, not the absence of local FSC code.

The repository already contains dormant/fail-closed FSC infrastructure:

- `FundingSourceIndex` with rolling transfer history;
- dust threshold;
- neutral funding sources;
- per-recipient and global caps;
- warmup/availability flags;
- materialization into `MaterializedFeatureSet.sybil_resistance`;
- policy surfaces that can read legacy `funding_source_concentration`.

The current implementation is still insufficient for active decision use:

- the legacy score uses `1.0 - distinct_known_sources / known_sources.len()`, not sample-normalized HHI;
- attribution chooses the latest eligible pre-buy transfer, which is vulnerable to small-transfer poisoning;
- same-slot cross-signature ordering can fall back to event/arrival timestamps;
- coverage and lane-health are not decision-time complete;
- neutral-only and unknown funding can be misread as clean if collapsed to `0.0`;
- the legacy `funding_source_concentration` field is already policy-visible and must not silently receive a new meaning.

The new NLN Program Streams provider exposes a semantic event layer with the minimum topics needed
for FSC capture:

- `prod.rpc.solana.pumpfun.create`;
- `prod.rpc.solana.pumpfun.trade`;
- `prod.rpc.solana.system.transfers`.

This satisfies the revisit condition from `ADR-0130` only for capture/evidence qualification. It
does not authorize active policy use and does not turn Program Streams into the R2 canonical market
path source.

## Decision

Ghost may implement **FSC v2 capture/evidence** through NLN Program Streams under the following
contract:

1. FSC v2 is additive and export-only until a later ADR explicitly enables decision use.
2. `fsc_v2.capture_enabled` and `fsc_v2.feature_emit_enabled` may be enabled in a dedicated capture profile.
3. `fsc_v2.decision_enabled` remains `false` in the current implementation phase.
4. `fsc_v2.hard_reject_enabled` remains `false`.
5. FSC v2 must not change active BUY/REJECT, penalty, size-down, combo veto, promotion readiness, IWIM or execution behavior.
6. FSC v2 must not overwrite the semantics of legacy `SybilResistanceFeatures.funding_source_concentration`.
7. FSC v2 must use an additive evidence payload, e.g. `FscV2Evidence`.
8. FSC v2 primary scope is single-hop native SOL funding only.
9. WSOL/SPL funding may be logged as enrichment/audit later, but not included in primary FSC v2.
10. R2 canonical market path remains raw Yellowstone AccountUpdates, DIAG or canonical account-state snapshots.
11. NLN RPC is not a primary audit/replay backend for FSC coverage.
12. Program Streams offset is diagnostic-only unless NLN later documents resume/continuity semantics.

## FSC v2 Semantic Requirements

FSC v2 must compute the primary score as sample-normalized HHI over dominant, meaningful,
high-confidence, non-neutral native-SOL funding source attribution for first successful buy per
unique buyer.

Primary formula:

```text
m = known_non_neutral_funded_buyers
p[source] = count(source) / m
hhi = sum(p[source]^2)
fsc_v2_hhi_norm_count = (hhi - 1/m) / (1 - 1/m)
```

Required behavior:

- `m < 2` returns unavailable/degraded evidence, not `0.0`;
- unknown funding is not clean evidence;
- neutral-only funding is not clean evidence;
- same-slot cross-signature attribution requires `tx_index` or equivalent deterministic ordering;
- arrival timestamps cannot prove chain order for decision-time FSC;
- decision-time FSC and eventual postfill FSC must be separate snapshot modes.

## Required Guardrails

Before any capture profile can be treated as safe:

1. Gatekeeper V2/V2.5 and V3 must ignore FSC v2 while `fsc_v2.decision_enabled=false`.
2. V3 hard-risk contradiction logic must not accidentally hard-fail on newly materialized FSC v2.
3. Legacy `funding_source_concentration` must not be repurposed to carry FSC v2 HHI.
4. Filtered `grpc_global_stream` observations must not be relabeled as full-chain funding coverage.
5. Reconnect/stall without resume must degrade lane health until warmup/lookback coverage is rebuilt.
6. Evidence rows must include provider, topics, config hash, neutral funder set hash/version, cutoff time and cutoff slot.

## Provider Qualification

NLN Program Streams can become the primary live FSC capture feed only after a benchmark package:

- minimum 24h, preferred 72h;
- compare with Chainstack/raw Yellowstone/archive-capable audit source;
- measure missing-on-each-feed, decode errors, reconnects, stalls, queue drops, latency, known coverage, unknown rate, neutral share and same-slot unorderable rate;
- report decision-time vs eventual FSC deltas;
- prove no material backpressure impact on primary trade lane, observation sessions, Gatekeeper or executor.

## Non-Goals

This ADR does not authorize:

- active FSC penalty;
- FSC hard reject;
- FSC size-down;
- FSC combo veto;
- P2/V3 promotion;
- R2 SSOT replacement by Program Streams;
- NLN RPC as coverage proof;
- WSOL/SPL inclusion in primary FSC;
- removing legacy FSC code;
- changing active V3 primary-only rollout semantics.

## Acceptance for P0

P0 is complete when:

1. This ADR exists and explicitly amends `ADR-0130`.
2. The execution plan exists under `PLANS/`.
3. Both documents state capture/evidence ON, active decision OFF.
4. No runtime code, active config profile or Gatekeeper policy has been changed.
5. The next implementation phase can start from a decision-complete contract.

## Future Activation Gate

Future `fsc_v2.decision_enabled=true` requires a separate decision after:

1. NLN funding lane stability benchmark passes.
2. `system.transfers` and `pumpfun.trade` coverage are verified.
3. Decision-time FSC is separated from eventual FSC.
4. Leakage audit passes.
5. Neutral funder set is versioned and hashed.
6. Known non-neutral coverage exceeds configured threshold.
7. `baseline_core + FSC` improves holdout R1/R2 outcomes without shrinking denominator dishonestly.
8. Shadow counterfactuals show no unacceptable false-reject pattern.
9. Rollback is config-only.

Hard reject remains outside this ADR and requires its own ADR.
