# RAPORT P3.7-J3 R15 Counterfactual Shadow Probe Smoke

Date: 2026-05-20  
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke`  
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke.toml`  
Status: `NOT_READY`

## Decision

R15 smoke did not reach minimal PASS.

The run confirmed the V3/MFS replay path and exercised the counterfactual probe selection and transport path, but it did not produce probe entry rows. The join-key audit also failed exact decision/V3 continuity because only 1 of 5 probe selection/transport rows matched the persisted decision row by both `ab_record_id` and `v3_feature_snapshot_hash`.

Current gate:

```text
P3.7-J3 P0R code-level repair: PASS
R15 runtime smoke: NOT_READY
Full R14 / broad collection: HOLD
Phase B V3 selector prototype: HOLD
P2/live/tuning: NO-GO
```

## Runtime

Command:

```bash
timeout 45m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke.toml
```

The process was stopped after `max_probes_per_run=5` had been reached and all five selected probes had produced transport rows. Continuing the process could not produce more probes under the bounded smoke config.

## V3 Replay Result

`scripts/v3_shadow_report.py --json`:

```text
status = ok
v3_rows = 17
full_snapshot_payload_rows = 17
hash_only_rows = 0
bad_rows = 0
stale_against_config = false
rows_missing_policy_hash = 0
rows_missing_snapshot_hash = 0
```

`scripts/v3_full_replay_report.py --strict --json`:

```text
status = ok
replay_status = full_replay_ok
total_rows = 17
v3_rows = 17
bad_rows = 0
full_replay_ok = 17
```

## Artifact Counts

```text
probe_selection_rows = 5
probe_skip_objects = 170
probe_transport_rows = 5
probe_entry_rows = 0
probe_lifecycle_rows = 0
active_shadow_transport_rows = 0
active_shadow_entry_rows = 0
active_shadow_lifecycle_rows = 0
active_buy_rows = 0
```

Decision artifacts:

```text
legacy_live decision rows = 175
v25_shadow decision rows = 17
active buy-like rows = 0
```

Probe skip reasons from the JSONL objects:

```text
probe_rate_limit_exceeded = 94
verdict_type_not_in_sample_scope = 48
probe_concurrency_limit_exceeded = 20
max_probes_per_run_exceeded = 8
```

The `probe_skips.jsonl` file contains three concatenated JSONL lines. The objects are parseable with a streaming decoder, but the line-oriented audit currently counts fewer skip rows than the physical object count. This does not affect the smoke gate result, but it should be fixed before larger collection.

## Probe Transport

All five selected probes produced `probe_transport.jsonl` rows with:

```text
dispatch_source = counterfactual_shadow_probe
collection_plane = counterfactual_shadow_probe
probe_plane = p37_shadow_probe
run_id = shadow-burnin-v3-p37-counterfactual-probe-r15-smoke
session_id = r15-smoke-p0r
amount_lamports = 7000000
probe_amount_source = fixed_lamports
probe_slippage_bps = 2000
v3_policy_config_hash present
v3_feature_snapshot_hash present
ab_record_id present
probe_id present
```

All five transport rows had:

```text
execution_outcome = counterfactual_shadow_probe_simulation_error
err = AccountNotFound
error_class = data_problem
```

No probe entry rows were written. Runtime logs contain:

```text
P37_SHADOW_PROBE_ENTRY_NOT_WRITTEN_NO_EXECUTABLE_ENTRY_PRICE
```

This means the run validated selection and transport metadata, but did not validate probe entry/lifecycle metadata propagation.

## Join-Key Audit

Generated:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke/p3_7_j3_r15_join_key_audit.json
PLANS/AUDYT/RAPORT_P3_7_J3_COUNTERFACTUAL_SHADOW_PROBE_JOIN_KEY_AUDIT_20260520.md
```

Audit summary:

```text
probe_readiness = not_ready
probe_join_key_acceptance = fail
probe_decision_join_acceptance = fail
probe_join_quality = exact_probe_id_and_ab_record_id
probe_selection_rows = 5
probe_transport_rows = 5
probe_entry_rows = 0
probe_lifecycle_rows = 0
```

Readiness reasons:

```text
missing_probe_entry_rows
probe_rows_missing_exact_decision_v3_join
```

Probe decision join:

```text
probe_selection joined_by_ab_record_id = 5 / 5
probe_selection feature_hash_match = 1 / 5
probe_selection policy_hash_match = 5 / 5
probe_transport joined_by_ab_record_id = 5 / 5
probe_transport feature_hash_match = 1 / 5
probe_transport policy_hash_match = 5 / 5
```

The AB join exists, but exact V3 feature snapshot hash continuity is not stable enough for collection.

## Active Boundary

No active BUY was observed or created by probe rows:

```text
active_buy_rows = 0
probe rows are not BUY
probe rows are not live inclusion
probe rows are counterfactual shadow probe artifacts only
```

No P2/live/tuning gate is changed by this smoke.

## Verdict

Minimal R15 PASS criteria were not met:

```text
v3_rows > 0: PASS
strict full replay OK: PASS
probe_selected_rows > 0: PASS
probe_transport_rows > 0: PASS
probe_entry_rows > 0: FAIL
exact ab_record_id continuity selection -> transport: PASS
exact decision/V3 feature hash continuity: FAIL
active BUY unchanged: PASS
no live/P2 path enabled: PASS
```

Overall:

```text
R15 runtime smoke: NOT_READY
Full R14 / broad collection: HOLD
Phase B V3 selector prototype: HOLD
P2/live/tuning: NO-GO
```

## Required Next Step

Implement a narrow P3.7-J3-R15 repair before any bounded collection:

1. Fix probe decision metadata continuity so `probe_selection` and `probe_transport` preserve the exact `v3_feature_snapshot_hash` from the persisted decision row selected by `ab_record_id`.
2. Investigate why counterfactual probe simulation returned `AccountNotFound` for all five selected probes.
3. Decide whether entry rows should be emitted for simulation-error probes as explicitly degraded probe entries, or whether P0 smoke eligibility must skip rows likely to produce `AccountNotFound`.
4. Fix concurrent JSONL appends for `probe_skips.jsonl` so line-oriented readers and streaming decoders agree.
5. Repeat R15 smoke in a fresh namespace or with a documented archive/append policy.

Do not proceed to broad collection until `probe_entry_rows > 0` and exact decision/V3 hash continuity pass.
