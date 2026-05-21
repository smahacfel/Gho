# RAPORT P3.7-J4B R15 BOUNDED R1 SMOKE

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml`

## Verdict

```text
P3.7-J4B runtime retention repair: PASS
Probe lifecycle truth resolution: PASS
Probe transport/entry/lifecycle join continuity: PASS
V3/MFS strict replay: PASS
Probe active-BUY mutation: PASS
On-chain lifecycle report script: BLOCKED by script bug
Full collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO
```

The run produced enough evidence for J4B and was stopped manually after the
bounded probe target produced lifecycle truth rows.

## Runtime Stop

The `p37-j4b-r1` tmux session was stopped after the evidence was collected. No
`ghost-launcher` / `cargo run` process for the J4B namespace remained after the
stop.

## V3 Replay

Strict replay:

```text
v3_rows = 151
bad_rows = 0
full_snapshot_payload_rows = 151
hash_only_rows = 0
strict replay = full_replay_ok 151/151
stale_against_config = false
```

V3/MFS replay discipline remained intact.

## Probe Artifact Counts

```text
probe_selection_rows = 102
probe_skip_rows = 1797
probe_transport_rows = 25
probe_shadow_entry_rows = 25
probe_shadow_lifecycle_rows = 48
probe_lifecycle_exit_filled_rows = 24
probe_lifecycle_position_closed_rows = 24
malformed_jsonl_rows = 0
```

Probe lifecycle truth:

```text
truth_status = resolved: 48
truth_source = canonical_account_state_snapshot: 48
truth_detail = null: 48
close_reason = TimeStop: 23
close_reason = Target: 1
```

This is the key J4B change versus J4: the previous `no canonical snapshot
reached guardian` failure disappeared.

## Runtime Retention Evidence

Log evidence:

```text
P37_SHADOW_PROBE_RUNTIME_RETENTION_REQUESTED = 102
POOL_TASK_RETAINED_FOR_POST_BUY_MONITORING = 103
shadow time-stop expired before any canonical snapshot reached guardian = 0
```

Interpretation:

```text
probe-selected pools now retain runtime truth state after terminal decision
probe lifecycle monitor receives canonical snapshots
probe lifecycle can resolve economic truth from canonical account state
```

## Join-Key Audit

Join-key audit output:

```text
PLANS/AUDYT/RAPORT_P3_7_J4B_R15_BOUNDED_R1_JOIN_KEY_AUDIT_20260521.md
PLANS/AUDYT/RAPORT_P3_7_J4B_R15_BOUNDED_R1_JOIN_KEY_AUDIT_20260521.json
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
probe_selection = 102/102
probe_transport = 25/25
probe_entry = 25/25
probe_lifecycle = 48/48
feature_hash_mismatch = 0
policy_hash_mismatch = 0
```

Probe chain continuity:

```text
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
common probe selection/transport/entry/lifecycle ab_record_id = 24
common probe selection/transport/entry/lifecycle probe_id = 24
```

The one transport/entry row without lifecycle was a classified simulation-error
row:

```text
simulation_instruction_error:custom_6042 = 1
```

It did not regress join-key continuity.

## Active BUY / Shadow Separation

The namespace contains natural non-probe shadow artifacts:

```text
buys.jsonl rows = 2
shadow_entries.jsonl rows = 2
shadow_lifecycle.jsonl rows = 4
```

These rows are separate from `dispatch_source=counterfactual_shadow_probe`
probe rows. Probe rows did not mutate active BUY semantics and did not use
P2/live.

## On-Chain Lifecycle Report

Attempted command:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/shadow_onchain_lifecycle_report.jsonl
```

Result:

```text
NameError: name 'lifecycle' is not defined
```

This is a reporting-script bug in `scripts/shadow_onchain_lifecycle_report.py`,
not a J4B runtime blocker. The runtime lifecycle rows themselves already carry
resolved canonical-account-state truth.

## Decision

```text
J4B runtime smoke: PASS
J4B resolved the canonical-snapshot lifecycle truth blocker
Counterfactual probe now reaches transport -> entry -> lifecycle truth labels
No full collection / Phase B / P2 / live escalation yet
```

Recommended next step:

```text
P3.7-J4C shadow_onchain_lifecycle_report.py probe-compatible report repair
```

After J4C, run the lifecycle report and labeler against this same J4B namespace.
If report/label generation passes, the next controlled step can be a small
bounded lifecycle-label collection, still not Phase B and not P2/live.
