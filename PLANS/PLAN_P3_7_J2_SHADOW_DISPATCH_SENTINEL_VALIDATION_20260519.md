# P3.7-J2 Shadow Dispatch Sentinel Validation

Date: 2026-05-19

## Decision

Full R14 remains HOLD.

P3.7-J1 is not yet runtime-validated end-to-end because the J1 smoke produced
V3/MFS decision rows but no shadow transport, shadow entry, or lifecycle rows.

P3.7-J2 validates the repaired join-key path only after a real shadow BUY /
dispatch appears.

## Goal

Run a bounded sentinel runtime profile until one of these happens:

1. A real shadow dispatch produces at least one transport row and one shadow
   entry row.
2. A stronger lifecycle row is observed.
3. The runtime budget is exhausted.

The proof target is the propagation of stable join metadata through:

`decision -> shadow transport -> shadow entry -> shadow lifecycle`

Required join metadata:

- `ab_record_id`
- `candidate_id` where available
- `pool_id`
- `base_mint` / `mint`
- `decision_ts_ms`
- `v3_feature_snapshot_hash`
- `v3_policy_config_hash`
- `decision_plane`
- `rollout_namespace`

## Profile

Use the isolated profile:

`configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml`

Namespace:

`shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel`

This profile is copied from the J1 smoke profile with only namespace/path
changes.

## Non-Goals

- no P2
- no live
- no runtime threshold tuning
- no active V2/V2.5 policy changes
- no IWIM changes
- no live sender changes
- no MFS extension
- no treating shadow simulation as live inclusion
- no treating lifecycle outcome as a decision-time feature

## Preflight

Run the join-key audit before runtime to confirm the namespace is isolated:

```bash
python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/p3_7_mfs_lifecycle_join_key_audit_presmoke.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J2_SENTINEL_JOIN_KEY_AUDIT_PRESMOKE_20260519.md
```

Run direct runtime preflight:

```bash
cargo run --quiet -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --preflight
```

If the formal production preflight wrapper blocks on
`.ghost/baseline_accepted_revision`, record that explicitly in the J2 report.
Do not update the baseline stamp silently.

## Runtime

Recommended runtime budget:

- maximum runtime: 2-4h
- stop early after `shadow_transport_rows >= 1` and `shadow_entry_rows >= 1`
- preferred grace after first shadow dispatch: 10-15 minutes
- preferred stronger stop: at least one lifecycle row or position close

Base command:

```bash
timeout 4h env RUST_LOG=info \
cargo run --release -p ghost-launcher --bin ghost-launcher -- \
  --config /root/Gho/configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml
```

## Post-Run Reports

Always run:

```bash
python3 scripts/v3_shadow_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --json

python3 scripts/v3_full_replay_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --strict \
  --json

python3 scripts/v3_p37_mfs_lifecycle_join_key_audit.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --output-json logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/p3_7_mfs_lifecycle_join_key_audit_after_j2.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J2_SENTINEL_JOIN_KEY_AUDIT_20260519.md
```

If a lifecycle row or closed position exists, run:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel.toml \
  --all-sessions \
  --output logs/shadow_run/shadow-burnin-v3-p37-mfs-lifecycle-r14-j2-sentinel/shadow_onchain_lifecycle_report.jsonl
```

## Acceptance

### PASS Partial

Enough to unblock full R14 collection:

- `v3_full_replay_ok > 0`
- `bad_rows = 0`
- `shadow_transport_rows >= 1`
- `shadow_entry_rows >= 1`
- `shadow_transport_rows_with_ab_record_id == shadow_transport_rows`
- `shadow_entry_rows_with_ab_record_id == shadow_entry_rows`
- `candidate_id` coverage is reported
- `v3_feature_snapshot_hash` coverage on shadow artifacts is nonzero or an
  explicit unavailable reason is reported
- primary join quality is `exact_ab_record_id` or `exact_candidate_id`

Lifecycle/on-chain truth may remain inconclusive if no position closes.

### PASS Full

Stronger proof:

- all PASS Partial criteria
- `shadow_lifecycle_rows >= 1`
- `shadow_lifecycle_rows_with_ab_record_id == shadow_lifecycle_rows`
- `shadow_onchain_lifecycle_report` rows exist
- lifecycle labeler works
- feature availability sees V3/MFS coverage on lifecycle-labeled rows

### INCONCLUSIVE

If no shadow dispatch appears:

- `shadow_transport_rows = 0`
- `shadow_entry_rows = 0`

This is not a J1 repair failure. It means the sentinel did not observe a real
shadow BUY. Full R14 remains HOLD.

### FAIL

If shadow rows appear but the audit falls back to weak joins:

- missing `ab_record_id`
- candidate mismatch
- unexpected missing V3 hash metadata
- primary join quality is `pool_mint_time_window` or weaker

Repair J1/J2 code and repeat sentinel.

## J2 Report

After runtime, write:

`PLANS/AUDYT/RAPORT_P3_7_J2_SHADOW_DISPATCH_SENTINEL_20260519.md`

Required sections:

- runtime duration
- process exit status
- V3 replay status
- shadow transport / entry / lifecycle counts
- `ab_record_id` coverage
- `candidate_id` coverage
- feature/policy hash coverage
- join quality
- lifecycle/on-chain report status if applicable
- final decision: full R14 GO / HOLD / FAIL
