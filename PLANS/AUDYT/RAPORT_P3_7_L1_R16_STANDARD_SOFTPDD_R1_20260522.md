# RAPORT P3.7-L1 R16 Standard / Soft-PDD R1

Status: `DIAGNOSTIC PARTIAL PASS / REPORTING BLOCKED`

## Verdict

R16 runtime run zakonczyl sie kontrolowanie i wyprodukowal pelny lancuch:

```text
V3 decision rows -> probe selection -> probe transport -> probe shadow entries -> probe lifecycle -> on-chain lifecycle report -> lifecycle labels
```

To jest pozytywny sygnal wzgledem J4C: pojawily sie `buy_quality_dirty_good` rows.

R16 nie jest jednak gotowy do ablation ani selector conclusions, bo krytyczna diagnostyka L1 nie spelnia acceptance:

```text
pdd_entry_drift_anchor_coverage_pct = 0.0
pdd_spike_ratio_quality_coverage_pct = 0.0
whale_single_max_pct_coverage_pct = 0.0
r16_artifact_identity_status = FAIL
single_active_hash_status = FAIL
```

## Artifacts

Namespace:

```text
shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1
```

Config:

```text
configs/rollout/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1.toml
```

Key reports:

```text
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v3_shadow_report.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v3_full_replay_report_strict.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/v3_p37_join_key_audit.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_l1_reject_diagnostics_summary.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/probe_shadow_onchain_lifecycle_report.jsonl
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_label_summary.json
logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r16-standard-softpdd-r1/p3_7_shadow_lifecycle_feature_availability.json
```

## Replay / V3

Strict replay:

```text
status = ok
replay_status = full_replay_ok
total_rows = 1972
v3_rows = 1972
bad_rows = 0
```

Decision verdict distribution:

```text
BUY = 6
REJECT_CORE_FAIL = 72
REJECT_HARD_FAIL = 70
TIMEOUT_PHASE1_INSUFFICIENT = 1331
TIMEOUT_PHASE1_NO_DATA = 493
```

## Probe / Lifecycle

Probe artifacts:

```text
probe_selection_rows = 163
probe_transport_rows = 50
probe_entry_rows = 50
probe_lifecycle_rows = 78
probe_skipped_rows = 1922
```

Join-key audit:

```text
probe_readiness.status = ready_for_probe_transport_entry_join
probe_readiness.decision_join_acceptance = pass
probe_readiness.join_key_acceptance = pass
probe_chain_ab_record_id_coverage = 1.0
probe_chain_probe_id_coverage = 1.0
```

Probe entry materialization:

```text
entry_materialized_rows = 39
simulation_error_rows = 11
simulation_error_custom_code_counts = { custom_2006 = 11 }
simulation_account_layout_mismatch:custom_2006 = 11
```

On-chain probe lifecycle report:

```text
rows_written = 39
close_truth_coverage = 39/39
```

Probe lifecycle labels:

```text
rows_total = 39
buy_quality_bad = 37
buy_quality_dirty_good = 2
market_bad_clean = 37
market_good_clean = 2
label_quality = degraded for 39/39
```

Feature availability:

```text
feature_availability_status = insufficient_for_selector
phase_b_possible = false
v3_selector_prototype_possible = false
reason = feature coverage or class balance is below configured minimums
```

## Active BUY Shadow Path

R16 BUY path was exercised:

```text
R16 BUY verdict count = 6
active shadow entries = 5
active shadow on-chain lifecycle rows = 4
active lifecycle labels = 4
```

Active labels:

```text
buy_quality_bad = 2
buy_quality_dirty_good = 2
market_bad_clean = 2
market_good_clean = 2
```

The active shadow lifecycle report also reported:

```text
skipped missing_position_closed = 2
```

## Identity / Hash Contract

Decision/probe artifacts mostly carry one R16 policy/brain hash:

```text
v3_policy_config_hash(decisions) =
  55416d4c7ef23a0aaea0c5b3bb4da0abc6564ce7059049c49a8bf80b07170fdc: 1972

brain_config_hash(decisions) =
  b41923673eacd484bd2178c6c7eb6782c5d90a9755f12ad68f1e625a0b658388: 1972
```

But full artifact identity status is failed:

```text
r16_artifact_identity_status = FAIL
single_active_hash_status = FAIL
```

Cause observed:

```text
active_shadow_lifecycle rows = 14
active_shadow_lifecycle rows with missing run/session/brain/policy hash = 1
```

The missing row is a `record_type=shadow_dispatch` failure:

```text
dispatch_status = failed
classification = data_problem
simulation_outcome = failed
err = Failed to fetch mint account: AccountNotFound
```

This must be fixed before using R16 as a clean identity/hash benchmark.

## L1 Diagnostics Quality

Gate trace coverage exists:

```text
gatekeeper_first_or_terminal_gate_coverage_pct = 100.0
```

But PDD diagnostic fields are not populated sufficiently:

```text
pdd_entry_drift_anchor_coverage_pct = 0.0
pdd_spike_ratio_quality_coverage_pct = 0.0
whale_single_max_pct_coverage_pct = 0.0
diagnostic_quality.status = FAIL
```

Therefore this R16 run can show that the standard/soft-PDD bundle produced some dirty-good lifecycle labels, but it cannot yet answer which exact PDD/whale/gate metrics caused the reject distribution.

## Gate Distribution

Terminal gates:

```text
timeout = 1824
hard_fail = 70
core = 72
```

First-kill gate counts:

```text
pdd = 1531
missing = 421
core3 = 11
hard_fail = 1
market_cap = 1
velocity = 1
```

Reason counts:

```text
TIMEOUT_PHASE1_INSUFFICIENT = 1331
TIMEOUT_PHASE1_NO_DATA = 493
REJECT_CORE_FAIL = 72
HARD_FAIL_MARKET_CAP = 36
HARD_FAIL_PRICE_CHANGE = 13
HARD_FAIL_EXTREME_TOP3 = 12
HARD_FAIL_SLOW_POOL = 7
HARD_FAIL_EXTREME_BUNDLING = 2
```

## Conclusion

R16 is useful and better than J4C because it produced:

```text
2 dirty_good probe lifecycle labels
2 dirty_good active shadow lifecycle labels
strict V3 replay PASS
probe join PASS
active BUY shadow path exercised
```

But R16 is not valid as a final policy diagnostic run because:

```text
PDD drift anchor/current fields are not populated
spike ratio quality is not populated
whale single max pct is not populated
one active shadow lifecycle failure row lacks R16 identity/hash metadata
feature availability remains insufficient_for_selector
custom_2006 still appears in 11 probe rows
```

## Recommended Next Step

Open `P3.7-L1R`:

```text
Diagnostic hydration repair:
1. Fix PDD drift diagnostic propagation from detect_entry_drift into DecisionLogger rows.
2. Fix pdd_spike_ratio_quality propagation.
3. Fix whale_single_max_pct propagation.
4. Stamp run/session/brain/policy identity on shadow_dispatch failure lifecycle rows.
5. Preserve current R16 config; do not change policy thresholds yet.
6. Re-run a small R16-r2 diagnostic run.
```

No Phase B, no P2/live, no threshold tuning until L1 diagnostics quality passes.
