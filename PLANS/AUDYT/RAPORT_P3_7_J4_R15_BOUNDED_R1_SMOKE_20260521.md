# RAPORT P3.7-J4 R15 BOUNDED R1 SMOKE

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4-r1`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4-r1.toml`

## Verdict

```text
P3.7-J4 probe lifecycle handoff: PASS
Probe transport/entry/lifecycle join continuity: PASS
V3/MFS strict replay: PASS
Probe active-BUY mutation: PASS
Probe lifecycle economic label quality: NOT_READY
Full collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO
```

The run produced enough evidence for J4. It was stopped after probe dispatch/lifecycle artifacts reached the bounded target and no further waiting was needed for the handoff question.

## Runtime State

The J4 runtime process was no longer active after controlled stop. No `ghost-launcher` / `cargo run` process for the J4 namespace remained.

## V3 Replay

```text
v3_rows = 236
bad_rows = 0
full_snapshot_payload_rows = 236
hash_only_rows = 0
strict replay = full_replay_ok 236/236
stale_against_config = false
```

V3/MFS replay discipline remained intact.

## Probe Artifact Counts

```text
probe_selection_rows = 92
probe_skip_rows = 1724
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_shadow_lifecycle_rows = 50
probe_lifecycle_exit_blocked_rows = 25
probe_lifecycle_position_closed_rows = 25
probe_lifecycle_time_stop_rows = 25
probe_lifecycle_truth_status_failure_rows = 50
malformed_jsonl_rows = 0
```

All probe transport rows had:

```text
dispatch_source = counterfactual_shadow_probe
execution_outcome = counterfactual_shadow_probe_simulated
buy_variant = routed_exact_sol_in
token_param_role = min_tokens_out
```

All probe lifecycle rows had:

```text
dispatch_source = counterfactual_shadow_probe
truth_status = failure
truth_detail = shadow time-stop expired before any canonical snapshot reached guardian
```

## Join-Key Audit

Join-key audit output:

```text
PLANS/AUDYT/RAPORT_P3_7_J4_R15_BOUNDED_R1_JOIN_KEY_AUDIT_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J4_R15_BOUNDED_R1_JOIN_KEY_AUDIT_20260521.json
```

Probe-specific audit result:

```text
probe_readiness.status = ready_for_probe_transport_entry_join
probe_readiness.join_key_acceptance = pass
probe_readiness.join_quality = exact_probe_id_and_ab_record_id
probe_readiness.decision_join_acceptance = pass
probe_readiness.required_exact_decision_v3_join_coverage = 1.0
```

Exact decision/V3 join:

```text
probe_selection = 92/92
probe_transport = 25/25
probe_entry = 25/25
probe_lifecycle = 50/50
feature_hash_mismatch = 0
policy_hash_mismatch = 0
```

Probe chain continuity:

```text
common probe selection/transport/entry/lifecycle ab_record_id = 25
common probe selection/transport/entry/lifecycle probe_id = 25
probe_entry_materialized_rows = 25
transport_without_entry_rows = 0
```

The global lifecycle audit still reports the legacy/common shadow chain as degraded/mint-only because natural non-probe shadow artifacts are present in the same namespace. That does not invalidate the probe-specific chain: the probe chain has exact `probe_id` + `ab_record_id` continuity.

## Active BUY / Shadow Separation

The namespace contains natural non-probe shadow artifacts:

```text
buys.jsonl rows = 2
shadow_lifecycle.jsonl rows = 6
```

These rows do not carry `probe_id` and are separate from `dispatch_source=counterfactual_shadow_probe` probe rows.

Interpretation:

```text
probe rows did not mutate active BUY semantics
probe rows did not use live/P2 path
natural active/shadow rows appeared independently during the run
```

## Lifecycle Handoff Result

J4 answered the handoff question positively:

```text
probe_shadow_entry -> PostBuySubmitted(lane=probe) -> probe monitor -> probe_shadow_lifecycle
```

Evidence:

```text
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_shadow_lifecycle_rows = 50
probe lifecycle rows with ab_record_id/probe_id/V3 hashes = 50/50
```

Therefore the previous J3L blocker, `probe_entry_rows > 0` but `probe_lifecycle_rows = 0`, is resolved at runtime.

## Remaining Blocker

The lifecycle output is not yet an economic truth-label dataset:

```text
probe_lifecycle truth_status = failure for all 50 rows
probe_lifecycle close_reason = TimeStop for 25 position_closed rows
truth_detail = shadow time-stop expired before any canonical snapshot reached guardian
```

This means the probe lifecycle monitor is receiving and closing probe positions, but canonical snapshot / exit-fill evidence is not reaching the guardian in a way that produces resolved economic labels.

## Decision

```text
J4 probe lifecycle handoff: PASS
J4 resolved the missing lifecycle-row blocker
No full/bounded collection escalation yet
Next gate: probe lifecycle truth resolution / canonical snapshot coverage
```

Recommended next stage:

```text
P3.7-J4B Probe Lifecycle Truth Resolution / Canonical Snapshot Coverage
```

J4B should determine why probe positions time-stop without canonical snapshots reaching the guardian, without changing active policy, IWIM, thresholds, P2, or live behavior.
