# RAPORT P3.7-J3R2 R15-r3 Counterfactual Probe Smoke

Date: 2026-05-20

Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r3.toml`

Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r3`

HEAD: `7cd8871` (`Complete P3.7-J3R2 probe repair`)

## Verdict

`R15-r3 smoke: NOT_READY_DIAGNOSED`

The run confirms the J3R2 hash-continuity repair at the probe-selection level:
all selected probe rows exact-join to persisted V3 decision rows by
`ab_record_id`, `v3_feature_snapshot_hash`, and `v3_policy_config_hash`.

The run does not reach minimal smoke PASS because no probe transport or probe
entry rows were emitted. All five selected probes were stopped by the new
required-account precheck before simulation:

```text
probe_execution_precheck_failed:
  missing_required_account:payer_pubkey:HvLVQMA4Uunk7NNJxNbTcLP1NLacgxkaZJpwueZxp7MD = 5
```

Decision:

- `P3.7-J3R2 code-level repair`: remains accepted.
- `R15-r3 runtime smoke`: not accepted as PASS.
- `Full/bounded collection`: HOLD.
- `Phase B V3/MFS lifecycle feature prototype`: HOLD.
- `P2/live/tuning`: NO-GO.

## Runtime

Command:

```bash
timeout 45m env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r3.toml
```

Result:

- Process exit code: `124`
- Interpretation: expected timeout termination.
- Build completed: `2026-05-20T09:38:27Z`
- Runtime active through: `2026-05-20T10:18:45Z`

The timeout includes the release build phase, so the live runtime window was
shorter than 45 minutes.

## Pre-Run State

The R15-r3 namespace was absent before setup. A pre-run join-key audit was
generated and stored at:

- `logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r3/p3_7_j3r2_r15_r3_pre_join_key_audit.json`
- `PLANS/AUDYT/RAPORT_P3_7_J3R2_R15_R3_PRE_JOIN_KEY_AUDIT_20260520.md`

The pre-run audit was `not_ready`, as expected for an empty namespace.

## V3 Replay

`v3_shadow_report.py` output:

- `raw_rows`: 116
- `deduped_rows`: 116
- `v3_rows`: 116
- `bad_rows`: 0
- `no_v3_rows`: 0
- `stale_against_config`: false
- `rows_missing_policy_hash`: 0
- `rows_missing_snapshot_hash`: 0
- `policy_hash_unique_count`: 1
- active vs V3:
  - active `REJECT` + V3 `REJECT`: 72
  - active `REJECT` + V3 `PENDING`: 44

`v3_full_replay_report.py --strict` output:

- `status`: `ok`
- `replay_status`: `full_replay_ok`
- `total_rows`: 116
- `v3_rows`: 116
- `bad_rows`: 0
- `status_counts.full_replay_ok`: 116

Verdict: `V3/MFS replay path PASS`.

## Probe Plane

Probe artifacts:

- `probe_selection_rows`: 5
- `probe_skip_rows`: 419
- `probe_transport_rows`: 0
- `probe_entry_rows`: 0
- `probe_lifecycle_rows`: 0

Skip reasons:

- `probe_execution_precheck_failed`: 5
- `probe_rate_limit_exceeded`: 216
- `max_probes_per_run_exceeded`: 168
- `verdict_type_not_in_sample_scope`: 30

Precheck failure reasons:

- `missing_required_account:payer_pubkey:HvLVQMA4Uunk7NNJxNbTcLP1NLacgxkaZJpwueZxp7MD`: 5

Interpretation:

- J3R2 successfully converted the previously opaque AccountNotFound path into a
  precise required-account precheck diagnosis.
- The actual shadow simulation path was not reached for selected probes.
- The transport and entry join-key gates remain unexercised in this smoke.

## Join-Key Audit

Post-run join-key audit:

- `readiness`: `not_ready`
- `probe_readiness`: `not_ready`
- `probe_selection_rows`: 5
- `probe_transport_rows`: 0
- `probe_entry_rows`: 0
- `probe_selection.exact_decision_v3_join`: 5
- `probe_selection.exact_decision_v3_join_coverage`: 1.0
- `feature_hash_mismatch`: 0
- `policy_hash_mismatch`: 0

The audit still reports `probe_decision_join_acceptance=fail` because required
probe transport and probe entry artifacts are absent. This is correct gate
behavior.

## Active BUY / Live Boundary

No active BUY or shadow entry artifacts were emitted in this namespace:

- `buys.jsonl`: absent
- `shadow_entries.jsonl`: absent
- `probe_transport.jsonl`: absent
- `probe_shadow_entries.jsonl`: absent

Probe rows remain counterfactual diagnostics only. They are not BUY decisions,
not live inclusion, and not Phase B selector evidence.

## Decision

R15-r3 is a useful diagnostic smoke, but not a PASS.

Next recommended stage:

```text
P3.7-J3E Probe Execution Eligibility / Account Resolution
```

Primary question:

```text
Why does the counterfactual probe prepared request use a payer_pubkey that is
missing on-chain in this runtime profile?
```

Candidate investigation points:

- ephemeral payer generation versus account existence for simulation;
- whether probe simulation should use an existing funded simulation payer;
- whether required-account precheck should classify ephemeral payer differently
  when simulation can safely inject or mock payer state;
- whether the shadow simulator needs payer/account override support for
  counterfactual probes.

Do not proceed to bounded collection until a smoke produces probe transport and
probe entry rows, or the probe eligibility/account-resolution design is amended
and re-smoked.
