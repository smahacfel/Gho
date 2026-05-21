# RAPORT P3.7-J4C PROBE ON-CHAIN LIFECYCLE REPORT REPAIR

Date: 2026-05-21
Namespace: `shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1`
Config: `configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml`

## Verdict

```text
P3.7-J4C reporting repair: PASS
Probe on-chain lifecycle report: PASS
Probe lifecycle label generation: PASS
Probe feature availability join: PASS
Selector readiness: NO-GO, insufficient sample size/class balance
Full collection / Phase B / P2 / live / threshold tuning: HOLD / NO-GO
```

J4C fixes the reporting/labeling gap left by J4B. The runtime lifecycle rows
were already resolved in J4B; J4C proves that those rows can be transformed into
probe on-chain lifecycle report rows, labels, and a feature-availability audit.

## Code Changes

- `scripts/shadow_onchain_lifecycle_report.py`
  - added `--artifact-plane {shadow,probe}` and `--probe`;
  - `--probe` reads `[p37_shadow_probe]` transport, entry and lifecycle paths;
  - fixed the undefined `lifecycle` variable by using the current lifecycle
    bundle while coalescing exit-fill join metadata;
  - preserved `probe_id`, `dispatch_source`, `source_ab_record_id`, `run_id`
    and `session_id` additively in output rows.
- `scripts/v3_p37_shadow_lifecycle_labeler.py`
  - preserves the same probe join metadata fields;
  - treats `counterfactual_shadow_probe_simulated` as a valid
    shadow/probe-simulated execution outcome for label classification.

Legacy default behavior is preserved: without `--probe` the on-chain lifecycle
report still uses the active shadow paths.

## Probe On-Chain Lifecycle Report

Command:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml \
  --probe \
  --all-sessions \
  --output /root/Gho/logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/probe_shadow_onchain_lifecycle_report.jsonl
```

Result:

```text
rows_written = 24
scope_candidates = 25
close_truth_coverage = 24/24
failed = 0
truth_status = resolved
truth_source = canonical_account_state_snapshot
ab_record_id coverage = 24/24
probe_id coverage = 24/24
dispatch_source coverage = 24/24
v3_feature_snapshot_hash coverage = 24/24
v3_policy_config_hash coverage = 24/24
```

Drift and truth gap summary:

```text
entry_drift_pct: count=24 mean=0.288165 median=0.000000 p95_abs=0.000000
exit_drift_pct: count=24 mean=0.000002 median=-0.000015 p95_abs=0.000015
entry_truth_gap_ms: count=24 mean=7727.833333 median=8045.000000 p95_abs=8956.000000
exit_truth_gap_ms: count=24 mean=35778.125000 median=39138.500000 p95_abs=39790.000000
```

The report row count is 24 because one of the 25 probe transport rows was a
classified simulation-error row and had no lifecycle close.

## Probe Lifecycle Labels

Command:

```bash
python3 scripts/v3_p37_shadow_lifecycle_labeler.py \
  --shadow-onchain-lifecycle logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/probe_shadow_onchain_lifecycle_report.jsonl \
  --output logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/p3_7_probe_shadow_lifecycle_labels.jsonl \
  --summary-output logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/p3_7_probe_shadow_lifecycle_label_summary.json \
  --summary-md-output PLANS/AUDYT/RAPORT_P3_7_J4C_PROBE_SHADOW_LIFECYCLE_LABEL_SUMMARY_20260521.md
```

Result:

```text
label_rows = 24
analysis_status = ok: 24
truth_status = resolved: 24
execution_verification_class = shadow_onchain_speculative_snapshot_verified: 24
market_outcome_class = market_bad_clean: 23, market_good_clean: 1
buy_quality_class = buy_quality_bad: 23, buy_quality_dirty_good: 1
label_quality = degraded: 24
```

All labels are degraded because the curve finality is speculative and probe
rows intentionally have no active Gatekeeper BUY context.

## Feature Availability

Command:

```bash
python3 scripts/v3_p37_shadow_lifecycle_feature_availability.py \
  --shadow-lifecycle-labels logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/p3_7_probe_shadow_lifecycle_labels.jsonl \
  --shadow-onchain-lifecycle logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1/probe_shadow_onchain_lifecycle_report.jsonl \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml \
  --output-json PLANS/AUDYT/RAPORT_P3_7_J4C_PROBE_FEATURE_AVAILABILITY_20260521.json \
  --output-md PLANS/AUDYT/RAPORT_P3_7_J4C_PROBE_FEATURE_AVAILABILITY_20260521.md
```

Result:

```text
rows_total = 24
raw_shadow_onchain_rows_total = 24
join_quality = matched_by_ab_record_id: 24
unmatched = 0
rows_with_any_decision_time_features = 24
V3/MFS feature group coverage = 24/24 for every audited group
feature_availability_status = insufficient_for_selector
Phase B possible = false
```

The feature join is correct, but the sample is not selector-ready:

```text
buy_quality_bad = 23
buy_quality_dirty_good = 1
min_feature_label_rows = 100
min_temporal_split_class_rows = 20
```

## Validation

Commands passed:

```bash
python3 -m py_compile \
  scripts/shadow_onchain_lifecycle_report.py \
  scripts/v3_p37_shadow_lifecycle_labeler.py \
  scripts/v3_p37_shadow_lifecycle_feature_availability.py \
  scripts/v3_p37_mfs_lifecycle_join_key_audit.py

python3 -m unittest \
  scripts/test_v3_p37_shadow_lifecycle_labeler.py \
  scripts/test_v3_p37_shadow_lifecycle_feature_availability.py \
  scripts/test_v3_p37_mfs_lifecycle_join_key_audit.py -v

python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml \
  --all-sessions \
  --output /tmp/j4c_active_shadow_onchain_report_smoke.jsonl

python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r15-bounded-j4b-r1.toml \
  --probe \
  --all-sessions \
  --output /tmp/j4c_probe_shadow_onchain_report_smoke.jsonl

rustfmt --edition 2021 --check ghost-launcher/src/oracle_runtime.rs

cargo test -p ghost-launcher --lib p37_shadow_probe -- --nocapture
cargo test -p ghost-launcher --lib p37_counterfactual_probe -- --nocapture

git diff --check
```

Observed pass counts:

```text
Python unittest = 19/19 PASS
p37_shadow_probe = 47/47 PASS
p37_counterfactual_probe = 8/8 PASS
```

## Decision

```text
J4B + J4C prove the full counterfactual probe evidence pipeline:
V3/MFS decision row -> probe transport -> probe entry -> probe lifecycle truth
-> on-chain lifecycle report -> labels -> feature availability join.
```

This does not authorize Phase B, P2, live, threshold tuning or active policy
changes. The next controlled step can be a small bounded lifecycle-label
collection only if the operator accepts the degraded/speculative-label boundary
and keeps the run in the counterfactual probe plane.
