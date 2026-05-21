# RAPORT P3.7-J3K3 Route / Override Propagation

## Status

```text
P3.7-J3K3 code-level repair: PASS
Runtime smoke R15-r10-j3k3: MINIMAL PASS
Collection: CONDITIONAL GO for small bounded collection only
Phase B / P2 / live / threshold tuning: HOLD / NO-GO
```

## Context

P3.7-J3K2 classified the dominant Q6-r2 counterfactual probe blocker as route
override propagation rather than bounded wait:

```text
audited_missing_account_rows = 396
exact_decision_v3_join_rows = 396
mfs_has_account_but_overrides_missing = 385
route_mismatch = 10
builder_required_account_not_in_mfs = 1
recommended_next_fix_path = route_override_propagation
```

The important finding was that the affected rows had decision-time V3/MFS and
DIAG account evidence, but the probe path stopped before request construction
because the strict precheck did not receive a usable legacy bonding-curve route
override.

## Implemented Repair

J3K3 adds a narrow counterfactual-probe-only fallback:

- `P37ShadowProbeCandidate` now derives a legacy `BondingCurve` snapshot from
  `v3_materialized_feature_snapshot.account_features.current_reserves` when
  `curve_data_known=true`;
- `P37ShadowProbeSelectionRecord` carries that decision-time snapshot into the
  selected probe row;
- the P37 probe account override path uses the snapshot only as fallback for
  `legacy_buy_curve` when the normal runtime `AccountStateCore` lookup no
  longer has the curve after session cleanup;
- the fallback is rejected unless the selected probe `base_mint` and `pool_id`
  match the current probe request context.

Active `derive_buy_account_overrides(...)` is unchanged. The repair does not
modify active Gatekeeper policy, IWIM, live sender, thresholds, P2, or active
BUY behavior.

## Safety Contract

```text
Source: decision-time V3/MFS snapshot already persisted with the decision row
Scope: counterfactual_shadow_probe only
Fallback target: legacy_buy_curve override only
Post-hoc guessing: no
Strict precheck: preserved
Exact decision/V3 hash continuity: still required by audit
```

If `current_reserves` are absent, zero, malformed, or not tied to the selected
pool/mint, J3K3 produces no fallback and the existing fail-closed precheck
remains authoritative.

## Tests Added

```text
p37_shadow_probe_candidate_materializes_legacy_curve_snapshot_from_v3_mfs
p37_shadow_probe_uses_selection_curve_snapshot_for_legacy_override_after_runtime_cleanup
```

The first test proves that a candidate can derive the curve snapshot from the
serialized V3/MFS decision payload. The second test simulates runtime cleanup
of the canonical account state and verifies that the probe-only override path
uses the selection snapshot to satisfy the legacy-buy precheck without touching
active override derivation.

## Validation

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture: PASS, 37/37
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture: PASS, 8/8
python py_compile probe/audit scripts: PASS
python unittest probe/audit scripts: PASS, 25/25
rustfmt --check oracle_runtime.rs: PASS
git diff --check: PASS
```

## Next Gate

R15-r10-j3k3 was run after validation and reached the transport/entry stop gate
early:

```text
probe_selection_rows = 19
probe_transport_rows = 5
probe_shadow_entry_rows = 5
probe_lifecycle_rows = 0
simulation_error_rows = 0
probe exact decision/V3 join = 100%
entry materialization = 5/5
active BUY rows = 0
missing_bonding_curve rows = 0
remaining execution_account_not_ready = 1 creator_vault row
```

The next gate is a small bounded collection, not full collection:

```text
max_probe_dispatches_per_run = 25
max_concurrent = 1
stop on hash/join mismatch, simulation-error spike, active BUY mutation, or JSONL corruption
```

Phase B, P2, live sender and runtime threshold tuning remain blocked.
