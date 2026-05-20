# RAPORT P3.7-J3R Counterfactual Probe Runtime Repair

Date: 2026-05-20

Status: code-level J3R repair PASS, R15-r2 runtime smoke pending

## Verdict

P3.7-J3R repairs the blockers found by the first R15 counterfactual shadow
probe smoke at code and audit-contract level.

The first R15 smoke remains classified as:

```text
R15 smoke: NOT_READY
P0R runtime smoke: PARTIAL / BLOCKED
Full collection: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```

J3R does not claim runtime collection success. It authorizes only a fresh
bounded R15-r2 smoke after the code-level checks remain green.

## Scope

J3R targeted three smoke findings:

1. `AccountNotFound` / `data_problem` on all counterfactual probe simulations.
2. Partial selection-to-transport-to-decision V3 hash continuity.
3. Concurrent probe JSONL append robustness.

It did not change active Gatekeeper policy, IWIM, thresholds, live sender, P2,
or V3 promotion.

## Implemented Changes

### AccountNotFound Diagnostics And Precheck

Probe transport error rows now carry richer execution diagnostics:

- `simulation_error_kind`
- `simulation_error_message`
- `simulation_error_account_pubkey`
- `simulation_error_account_role`
- `prepared_buy_account_set_present`
- `account_overrides_present`
- `bonding_curve`
- `payer_pubkey`
- `token_program`
- `user_ata`
- `curve_account_available`
- `mint_account_available`
- `payer_account_available`
- `quote_age_diagnostic_ms`
- `curve_age_diagnostic_ms`
- `account_features_update_count`
- `curve_data_known`
- `curve_readiness_status`

Known incomplete execution state is converted into a probe skip with:

```text
skip_reason = probe_execution_precheck_failed
precheck_failure_reason = <specific reason>
```

This preserves the invariant that `AccountNotFound` is not success and does not
create fake probe entry rows.

### Decision/V3 Hash Continuity

Probe candidate feature hashes now use the serialized V3 replay payload
boundary used by persisted decision rows. Selection and transport rows propagate
source metadata additively:

- `source_v3_feature_snapshot_hash`
- `source_v3_policy_config_hash`
- `source_decision_log_path`
- `source_decision_row_offset`
- `source_decision_row_sha256`

Transport rows inherit source metadata from the selected probe record. They do
not silently substitute a different decision hash plane.

The join-key audit now reports explicit mismatch reasons, including:

- `decision_row_not_found`
- `multiple_decision_rows_for_ab_record_id`
- `decision_row_missing_v3_payload`
- `feature_hash_missing`
- `feature_hash_mismatch`
- `policy_hash_missing`
- `policy_hash_mismatch`
- `source_plane_mismatch`

Probe readiness requires exact join to persisted decision/V3 rows by
`ab_record_id`, V3 feature hash, V3 policy hash, and source decision plane.

### Probe JSONL Writer Robustness

Probe selection, skip, transport, and entry writes are serialized through a
shared writer lock. This prevents concurrent probe tasks from concatenating JSON
objects into one physical line.

The audit parser is also tolerant of legacy concatenated probe JSONL rows and
will parse multiple JSON objects from one physical line as degraded input rather
than dropping the entire line.

### Fresh R15-r2 Smoke Profile

Added a fresh bounded smoke profile:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2.toml
```

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r15-smoke-r2
```

Bounded probe settings:

```text
max_probes_per_run = 5
max_probes_per_minute = 5
max_concurrent = 1
sample_modulus = 100
sample_threshold = 100
append = false
```

The profile is intentionally separate from the first R15 smoke namespace to
avoid mixing pre-repair and post-repair artifacts.

## Validation

Executed locally:

```bash
cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture
python3 -m py_compile scripts/v3_p37_mfs_lifecycle_join_key_audit.py
python3 -m unittest scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v
```

Observed result:

```text
p37_shadow_probe: 21 passed
p37_counterfactual_probe: 3 passed
Python audit tests: 6 passed
py_compile: PASS
```

The smoke-profile load test now copies the profile and the Ghost Brain config
into a temporary namespace before loading. This keeps `append=false` fail-closed
semantics intact while avoiding test dependence on existing operator runtime
artifacts.

## Remaining Gate

J3R does not complete runtime validation. The next gate is:

```text
R15-r2 runtime smoke
```

Minimal R15-r2 PASS requires:

- `v3_rows > 0`
- strict full replay OK
- `probe_selected_rows > 0`
- `probe_transport_rows > 0`
- exact decision/V3 continuity = `100%`
- `probe_entry_rows > 0`
- probe entry rows carry `ab_record_id` and `probe_id`
- active BUY rows remain unchanged
- no live/P2 path is enabled

If R15-r2 still emits no probe entries, the run can only be classified as
`NOT_READY` unless every failed probe has a precise precheck or simulation
diagnostic explaining why it was not execution-ready.

## Decision

```text
J3R code-level repair: PASS
R15-r2 runtime smoke: NEXT GATE
Bounded collection: HOLD
Full R14: HOLD
Phase B: HOLD
P2/live/tuning: NO-GO
```
