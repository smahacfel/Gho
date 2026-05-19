# RAPORT P3.7-J3 P0 Counterfactual Shadow Probe Implementation

Date: 2026-05-19

Status: code-level P0 PASS, runtime smoke pending

## Verdict

P3.7-J3 P0 has been implemented as an additive, disabled-by-default
counterfactual shadow probe plane.

This implementation does not authorize P2, live execution, threshold tuning,
active Gatekeeper changes, IWIM changes, live sender changes, or V3 selector
claims.

Full R14 remains HOLD. The next gate is a bounded P3.7-J3 R15 smoke run.

## Implemented Scope

Implemented P0 components:

- `[p37_shadow_probe]` launcher config with fail-closed shadow-only validation.
- Deterministic `deterministic_hash_mod` selection and skip logs.
- Probe join metadata fields:
  - `source_ab_record_id`
  - `probe_id`
  - `dispatch_source`
  - `collection_plane`
  - `probe_plane`
- Isolated probe transport JSONL rows.
- Isolated probe entry JSONL rows.
- Probe-aware join-key audit support.
- R15 smoke rollout profile.

The probe plane writes to dedicated `[p37_shadow_probe]` paths and does not use
the active `trigger.shadow_run.output_path` or active `execution.shadow`
entry/lifecycle paths.

## Runtime Semantics

Preserved invariants:

- Active verdict remains unchanged.
- Probe rows are not active BUY decisions.
- `dispatch_source=counterfactual_shadow_probe` is required.
- P0 `emit_event_bus=false` is fail-closed.
- Probe artifact paths must not collide with active shadow paths.
- Legacy rows without probe metadata still parse.
- Lifecycle labels remain post-decision labels, not decision-time features.

## R15 Smoke Profile

Added:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke
```

Probe artifact paths:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_selection.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_skips.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_transport.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_shadow_entries.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/probe_shadow_lifecycle.jsonl
```

P0 smoke target:

- V3/MFS decision rows exist.
- Probe selected rows exist.
- Probe transport rows exist.
- Probe entry rows exist.
- `ab_record_id` continuity is exact.
- `probe_id` continuity is exact.
- No active BUY mutation.
- No live/P2 path enabled.

Lifecycle close is not required for P0 smoke. Lifecycle/on-chain labels are P1
unless a close happens naturally during smoke.

## Join-Key Audit

`scripts/v3_p37_mfs_lifecycle_join_key_audit.py` now reports canonical lifecycle
readiness and probe P0 readiness separately:

- `readiness`
- `join_key_coverage`
- `probe_readiness`
- `probe_join_key_coverage`
- `probe_artifact_intersections`

For P0, `probe_readiness.status=ready_for_probe_transport_entry_join` is allowed
without lifecycle rows if selected, transport, and entry rows join by exact
`ab_record_id` and `probe_id`.

## Validation

Checks run:

```text
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
rustfmt --edition 2021 --check ghost-launcher/src/config.rs ghost-launcher/src/oracle_runtime.rs
git diff --check
```

Observed result:

- `p37_shadow_probe`: PASS, 12 tests.
- `p37_counterfactual_probe`: PASS, 1 test.
- join-key audit unittest: PASS, 4 tests.
- py_compile: PASS.
- rustfmt check: PASS.
- diff whitespace check: PASS.

Existing repository warnings were observed during cargo tests. No new warning
class was intentionally introduced.

## Open Gates

Still pending:

- Runtime R15 smoke.
- Runtime proof that selected probes create probe transport and entry rows.
- Runtime join-key audit PASS on generated R15 artifacts.
- Lifecycle propagation/on-chain label generation if positions close naturally
  or in P1.

Full R14 remains HOLD until R15 smoke passes.

## Non-Goals Preserved

This work did not:

- enable P2,
- enable live execution,
- change active V2/V2.5 policy,
- change IWIM,
- change the live sender,
- tune thresholds,
- promote V3,
- treat probes as active BUY rows,
- treat lifecycle outcomes as decision-time features.
