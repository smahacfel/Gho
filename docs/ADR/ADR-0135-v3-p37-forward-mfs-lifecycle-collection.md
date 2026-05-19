# ADR-0135: V3 P3.7 Forward MFS Lifecycle Collection

Date: 2026-05-19

Status: Accepted

## Context

P3.7 recovered two complementary but separate evidence layers:

- R10/R11/R13 have V3/MFS replay payload and market-path truth, but no local
  shadow lifecycle execution proof.
- `shadow-burnin-buy-heavy-rerun` has shadow lifecycle/on-chain truth, but no
  V3/MFS coverage.

ADR-0134 accepted `shadow_burnin_lifecycle_onchain` as a separate truth dataset
kind and explicitly blocked V3 selector claims until rows exist with both
V3/MFS decision snapshots and shadow lifecycle truth.

P3.7-I then found moderate diagnostic signal in historical V2/V2.5 features:

- `joined_feature_rows = 738`
- primary Gatekeeper-context subset: `154` dirty-good vs `417` bad
- `signal_level = moderate_diagnostic_signal`
- `v3_selector_prototype_allowed = false`
- recommendation:
  `design_forward_v3_mfs_lifecycle_collection_run`

The recovered V2/V2.5 signal is enough to justify a new collection run. It is
not enough to claim V3 selector readiness.

## Decision

Design and run a forward-only P3.7-J collection lane that records, in the same
namespace:

- `MaterializedFeatureSet` / V3 replay payload evidence,
- V3 shadow decision telemetry,
- Gatekeeper V2/V2.5 decision context,
- shadow-burnin transport, entry, and lifecycle artifacts,
- shadow/on-chain lifecycle report,
- shadow lifecycle labels,
- feature availability and join-key coverage reports.

The initial rollout namespace is:

```text
shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only
```

A smaller smoke namespace is also defined:

```text
shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke
```

This is a collection run, not a selector candidate.

## Invariants

The collection lane must preserve these invariants:

- `MaterializedFeatureSet` remains the decision snapshot SSOT.
- V3 remains a shadow sidecar: `gatekeeper_v3.enabled=false`.
- V3 replay payload emission is enabled:
  `gatekeeper_v3.replay_payload_enabled=true`.
- V3 promotion is disabled: `gatekeeper_v3.promotion.enabled=false`.
- FSC remains disabled under ADR-0130 single-stream constraints.
- Trigger entry is shadow-only.
- Execution mode is shadow.
- Shadow simulation is not live inclusion.
- Lifecycle outcomes are labels, not decision-time features.
- Speculative curve finality remains dirty/degraded, not finalized proof.

## Runtime Scope

Allowed:

- create isolated collection configs,
- run smoke/preflight before any long collection run,
- run a forward-only shadow-burnin collection namespace,
- generate reports and labels after the run,
- audit join-key coverage.

Not allowed:

- P2 or live promotion,
- runtime threshold tuning,
- active V2/V2.5 policy changes,
- IWIM changes,
- live sender changes,
- MFS schema extension as a policy change,
- FSC active gate,
- treating submit as confirmation,
- treating shadow lifecycle labels as decision-time features.

## Rollout Profiles

The primary profile is:

```text
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-primary-only.toml
```

The smoke profile is:

```text
configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-smoke.toml
```

Both point to:

```text
configs/rollout/ghost_brain_v3_p37_mfs_lifecycle.toml
```

The brain config keeps:

```text
gatekeeper_v3.enabled = false
gatekeeper_v3.shadow_emit_enabled = true
gatekeeper_v3.replay_payload_enabled = true
gatekeeper_v3.promotion.enabled = false
gatekeeper_v3.evidence_requirements.fsc = false
gatekeeper_v3.evidence_requirements.execution = false
```

## Join-Key Contract

The collection run must make join-key coverage measurable across:

- Gatekeeper decision/buy logs,
- V3 replay payload rows,
- shadow transport `buys.jsonl`,
- `shadow_entries.jsonl`,
- `shadow_lifecycle.jsonl`,
- event datasets,
- shadow/on-chain lifecycle report,
- shadow lifecycle labels.

Preferred keys:

```text
ab_record_id
candidate_id
position_id
pool_id
base_mint / mint_id
decision_ts_ms
observation_start_ts_ms
observation_end_ts_ms
v3_feature_snapshot_hash
v3_policy_config_hash
decision_plane
rollout_namespace
```

If `ab_record_id` is missing from shadow transport/entry/lifecycle artifacts,
the run can still proceed, but join quality must be explicitly reported. The
target for a successful forward collection is to avoid relying primarily on
`pool_id + mint + time window` joins.

## Smoke Gate

Run the smoke profile before the primary collection.

Smoke acceptance:

- decision rows are produced,
- V3 rows are produced,
- full V3 replay payload rows equal V3 rows,
- hash-only V3 rows are zero,
- shadow transport path is writable,
- shadow entry path is writable,
- shadow lifecycle path is writable,
- no live transaction requirement appears,
- no P2/promotion/lifecycle-label-as-feature claim is made.

If no BUY/shadow dispatch occurs, smoke may pass only as V3/MFS payload
readiness. Lifecycle readiness remains inconclusive and the collection window
must be extended rather than policy thresholds loosened.

## Collection Gate

The primary collection may unlock only a diagnostic V3/MFS feature prototype,
not P2.

Minimum post-run gate:

- strict V3 replay report passes,
- V3/MFS payload coverage is nonzero,
- shadow lifecycle labels exist,
- at least one positive and one bad lifecycle label exist,
- feature availability confirms V3/MFS coverage,
- join-key audit shows measurable candidate/AB-level coverage,
- shadow/on-chain lifecycle truth is resolved for at least some lifecycle rows.

If the gate fails, reports must classify the failure as:

- no BUY/lifecycle rows,
- no V3/MFS coverage,
- join-key mismatch,
- unresolved truth,
- speculative/dirty-only labels,
- or insufficient class balance.

## Consequences

If P3.7-J succeeds, P3.7 may open a diagnostic V3/MFS lifecycle feature
prototype on the new dataset.

If P3.7-J fails because no lifecycle rows are collected, the project has a data
collection/BY rate problem, not a selector proof.

If P3.7-J fails because V3/MFS payloads are missing, V3 selector work remains
blocked and the telemetry path must be repaired before feature work.

If P3.7-J shows no stable signal after clean lifecycle labels exist, the V3
selector line should move toward feature redesign or closure rather than
threshold tuning.

## Non-Goals

This ADR does not authorize:

- changing active BUY/REJECT/TIMEOUT behavior,
- changing V2/V2.5 thresholds,
- changing IWIM,
- changing live sender,
- enabling P2/live,
- adding FSC as an active gate,
- adding new MFS fields as part of this ADR,
- treating recovered V2/V2.5 diagnostics as runtime thresholds.
