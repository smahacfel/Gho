# ADR-8D: Shadow V2 L2-F Research Validation Run R16 20260705

## Status

Accepted as Shadow V2 offline-only L2 research-grade candidate.

## Decision

R16 L2-F collection scope otrzymuje pozytywny werdykt L2 wylacznie dla
offline research:

```text
run_id=shadow-v2-l2-f-collection-20260705-r16
main_head=5e6d839984cf38ecb65a0540b0adfabfcead9f23
final_verdict=L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
l2_research_grade_candidate=true_for_shadow_v2_offline_research_only
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

Decyzja jest oparta na pozycyjnym evidence-complete scope:

```text
research_candidate_roundtrip_count=769
l2_research_evidence_complete_roundtrip_positions=750
required_roundtrips=500
density_excluded_roundtrip_positions=19
```

Pozycje z typed density/retention exclusions nie wspieraja pozytywnego claimu.

## Context

L2-F jest validation-only / research-only. Nie wolno w nim zmieniac:

```text
Gatekeeper policy
BUY/REJECT logic
selector runtime
TX/Jito/live path
provider streams
thresholds
shadow_close_only
active_close
```

Maksymalny pozytywny verdict pozostaje:

```text
L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
```

Nadal nie wolno deklarowac:

```text
runtime_approval
live_equivalence
active_close
shadow_close_only
strategy_research_unblocked
```

## Results

Primary artifacts:

```text
reports/selector/shadow-v2-l2-f-collection-20260705-r16/runtime_post_run_manifest.json
reports/selector/shadow-v2-l2-f-collection-20260705-r16/strict_audit_summary.json
reports/selector/shadow_v2_l2_f_research_validation_summary.csv
reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_position_scope_v1.jsonl
reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_density_excluded_positions_v1.jsonl
reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_density_audit_summary.csv
reports/selector/shadow-v2-l2-f-collection-20260705-r16/post_run_manifest.json
reports/selector/shadow-v2-l2-f-collection-20260705-r16/shadow_v2_manifest_report.csv
reports/selector/shadow-v2-l2-f-collection-20260705-r16/candidate_universe_v1.jsonl
reports/selector/shadow-v2-l2-f-collection-20260705-r16/candidate_universe_manifest_v1.json
reports/selector/shadow-v2-l2-f-collection-20260705-r16/gatekeeper_decision_root_evidence.json
```

Sample gates:

```text
complete_executable_roundtrip_positions=769
complete_executable_roundtrip_required=500
research_candidate_roundtrip_count=769
entry_execution_label_grade_RESEARCH_CANDIDATE_count=811
exit_execution_label_grade_RESEARCH_CANDIDATE_count=784
sample_size_gate=PASS
```

Passing gates:

```text
temporal_audit_verdict=PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT
fake_handoff_signature_count=0
event_seq_chain_order_substitute_count=0
terminal_truth_derived_count=801
terminal_truth_not_derived_count=0

gatekeeper_denominator_verdict=GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN
candidate_universe_count=2272
eligible_denominator_count=2272
gatekeeper_decision_count=4535
gatekeeper_decision_joined_to_candidate_count=4535
gatekeeper_decision_unmatched_count=0
checkpoint_reach_count=2268
threshold_starvation_verdict=NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED
unknown_reason_count=0

manifest_status=PASS
manifest_blockers=[]
replay_lifecycle_verdict=PASS_REPLAY_LIFECYCLE_RECONCILED
malformed_rows=0
unknown_untyped_blockers=0

account_data_hash_coverage_verdict=PASS_ACCOUNT_DATA_HASH_COVERAGE
observed_account_state_boundary_samples=81883
samples_with_account_data_hash=81883
missing_account_data_hash_count=0
```

## Selector/Gatekeeper Contract Reuse Matrix

Decyzja architektoniczna: L2-F nie tworzy rownoleglego subsystemu
candidate-universe/denominator. L2-F uzywa istniejacego Selector/Gatekeeper
contractu:

```text
selector_gatekeeper_contract_reuse_status=PASS
candidate_universe_builder_source=scripts/build_selector_candidate_universe.py
candidate_universe_adapter_only=True
candidate_universe_parallel_model_detected=False
decision_logs_created_denominator_rows=0
candidate_ids_from_decision_only=0
denominator_invariant_status=PASS
```

R16 korzysta z istniejacych event-level Selector artifacts:

```text
datasets/events/shadow-v2-l2-f-collection-20260705-r16/*.jsonl
```

i odbudowuje `candidate_universe_v1` przez
`scripts/build_selector_candidate_universe.py`. Launcher-log `NewPoolDetected`
path w L2-F jest tylko adapterem do event-level inputu tego samego buildera,
a nie alternatywnym denominator builderem.

| concept | existing Selector/Gatekeeper source/tool/contract | L2-F implementation path | reuse/direct adapter/new code | equivalence proof | tests covering it |
|---|---|---|---|---|---|
| `candidate_universe_v1` | `scripts/build_selector_candidate_universe.py`, `selector_pipeline_common.candidate_universe_row` | `collect_selector_event_artifact_paths(run_id)` -> `build_selector_candidate_universe_from_event_artifacts(...)` -> `candidate_builder.run(...)` | reuse | 2272 rows, required Selector fields present, `status_counts.ok=2272` | `test_launcher_log_adapter_uses_existing_selector_candidate_universe_contract` |
| `candidate_universe_manifest_v1` | `build_selector_candidate_universe.py` manifest contract | `candidate_builder.run(..., manifest_output=...)` | reuse | `status=ok`, `denominator_source=event_artifact_only`, `denominator_invariant_status=PASS` | `test_launcher_log_adapter_uses_existing_selector_candidate_universe_contract` |
| `denominator_invariant_status` | Selector manifest invariant | strict summary/CSV copies value from `candidate_universe_manifest_v1.json` | reuse | `denominator_invariant_status=PASS` | `test_summary_csv_exposes_required_l2_f_metric_names` |
| `decision_logs_created_denominator_rows` | Selector manifest invariant and L2-E denominator audit | decision JSONL is context only and never passed as denominator source | reuse | value `0` | `test_decision_only_rows_do_not_create_l2_f_candidate_universe_denominator` |
| `candidate_ids_from_decision_only` | Selector manifest invariant | L2-F contract gate requires zero | reuse | value `0` | `test_decision_only_rows_do_not_create_l2_f_candidate_universe_denominator` |
| `decision_context_join_key` | `selector_pipeline_common.identity_join_keys`, Gatekeeper audit `match_candidate` | Gatekeeper decisions join by candidate_id, exact join key, or unambiguous `base_mint+pool_id`; decisions remain context only | reuse | `decision_context_join_key_semantics=Selector identity_join_keys mint_pool/base_mint+pool_id; Gatekeeper decisions are context only` | `test_launcher_log_adapter_uses_existing_selector_candidate_universe_contract` |
| Gatekeeper decision join | `scripts/shadow_v2_gatekeeper_coverage_denominator_audit.py` | `run_gatekeeper_audit(...)` over candidate universe + decision root | reuse | `gatekeeper_decision_joined_to_candidate_count=4535`, unmatched `0` | `test_shadow_v2_gatekeeper_coverage_denominator_audit.py` |
| unknown/generic reject reason taxonomy | L2-E Gatekeeper denominator audit reason taxonomy | L2-F consumes `unknown_reason_count` and `unknown_untyped_blockers` as hard gates | reuse | both `0` | `test_shadow_v2_gatekeeper_coverage_denominator_audit.py` |
| threshold starvation verdict | L2-E Gatekeeper denominator audit | L2-F consumes `threshold_starvation_verdict` | reuse | `NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED` | `test_shadow_v2_gatekeeper_coverage_denominator_audit.py` |
| selector schema version | `selector_pipeline_common.SCHEMA_VERSION` | emitted by existing builder and copied into strict summary | reuse | `selector_schema_version=1` | `test_launcher_log_adapter_uses_existing_selector_candidate_universe_contract` |
| manifest/status summary | manifest audit and L2-F strict summary | tracked manifests record path, size, rows, SHA and audit consumption | reuse | `manifest_status=PASS` | `test_summary_csv_exposes_required_l2_f_metric_names` |

Position-level density/retention:

```text
position_level_density_retention_verdict=PASS_L2_F_POSITION_LEVEL_DENSITY_RETENTION
density_retention_verdict_evidence_complete_scope=L2_F_DENSITY_RETENTION_PASS
density_retention_verdict_raw_scope=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
l2_research_evidence_complete_roundtrip_positions=750
density_excluded_roundtrip_positions=19
missing_declared_horizon_position_count=0
path_sample_coverage_gap_position_count=0
sparse_approx_only_position_count=17
retention_gap_position_count=2
unknown_or_untyped_density_verdict_position_count=0
malformed_density_rows=0
unknown_horizon_rows=0
selection_inputs_exclude_pnl=true
selection_inputs_exclude_terminal_outcome_quality=true
```

Excluded positions are listed in:

```text
reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_density_excluded_positions_v1.jsonl
```

Each excluded-position row has predefined typed reasons, horizon-level
blockers, `positive_claim_supported=false`, and explicit flags proving that
PnL/outcome quality is not used as a selection input.

Evidence-complete density audit was rerun with:

```text
--position-scope-jsonl reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_position_scope_v1.jsonl
```

Result:

```text
density_retention_verdict_evidence_complete_scope=L2_F_DENSITY_RETENTION_PASS
position_scope_position_count=750
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_path_coverage_blocker_count=0
declared_horizon_retention_blocker_count=0
```

Broad density context:

```text
density_retention_verdict_raw_scope=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_path_coverage_blocker_count=5
declared_horizon_retention_blocker_count=0
l2_f_allowed_next=false
```

The broad density verdict remains as context for the raw density stream. It is
not used to claim 100 percent density over all observed positions. The positive
L2-F claim is scoped to the 750 positions listed in:

```text
reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_position_scope_v1.jsonl
```

The raw-scope failure remains visible and is not reclassified as PASS. It is
acceptable only because the positive L2-F claim is restricted to the
deterministic evidence-complete scope and that scoped density audit passes.

Declared L2 baseline horizons:

```text
2000
3000
10000
30000
120000
```

Unsupported long horizons remain non-blocking and cannot support positive
research claims:

```text
300000=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
positive_claims_from_undeclared_horizons_allowed=false
```

## Raw Artifact Integrity

Large raw artifacts are intentionally not tracked in git. `post_run_manifest.json`
records their relative path, size, line count, JSONL row count, malformed row
count and SHA256. `runtime_post_run_manifest.json` and
`strict_audit_summary.json` record which audit consumed each raw artifact.

Required raw artifact evidence:

```text
shadow_position_event_v2.jsonl rows=169474 sha256=581f3e40378715be695219737502dc99cc8c27943e3707c796064a4dee955df0
shadow_replay_v2.jsonl rows=169474 sha256=a94e74665246360ab38f122f6bd80a4e750e7a5d81748d774888e260a97a4201
shadow_lifecycle_v2.jsonl rows=169474 sha256=d73ad2f1e1eb253dea5b7111ff6e4674ca16fdf53c3762fdfdc6ea5ee1b08f7a
shadow_path_density_v2.jsonl rows=1186315 sha256=135b3b4130f2726adad9b00e20551ae857b0b1747d3aece70bab1b9d8c69dce0
```

Audit consumption:

```text
shadow_position_event_v2.jsonl -> sample gates, temporal audit, replay/lifecycle audit, account hash coverage, manifest audit
shadow_replay_v2.jsonl -> temporal audit, replay/lifecycle audit, manifest audit
shadow_lifecycle_v2.jsonl -> temporal audit, replay/lifecycle audit, manifest audit
shadow_path_density_v2.jsonl -> raw density audit, evidence-complete density audit, position-level density gate, manifest audit
```

## Why This Is L2 But Not L3

This is enough for Shadow V2 offline research because:

```text
evidence_complete_roundtrips=750 >= required_roundtrips=500
temporal audit PASS
position-level declared density/retention PASS
Gatekeeper denominator known
no threshold starvation
manifest PASS
replay/lifecycle PASS
account_data_hash coverage PASS
malformed rows = 0
unknown/untyped blockers = 0
```

This is not live equivalence because L2 still does not prove:

```text
live fills
realized slippage
quote/fill divergence
live failed/no-fill telemetry
runtime approval safety
active close behavior
```

The remaining L3/live-equivalence work must be handled separately.

## Tooling Decision

Offline L2-F audit tooling was hardened after a RAM incident where the wrapper
path loaded large JSONL files into memory. The decision is to keep the L2-F
audit path streaming/bounded-memory.

Implemented memory hardening:

```text
scripts/shadow_v2_offline_audit_common.py
  - streaming iter_jsonl helpers for canonical/replay/lifecycle/density rows

scripts/shadow_v2_path_density_horizon_audit.py
  - streaming density scan
  - latest position+horizon snapshot aggregation without loading all density rows

scripts/shadow_v2_temporal_no_lookahead_audit.py
  - streaming canonical/replay/lifecycle scans
  - online event_seq monotonicity check

scripts/shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py
  - streaming replay/lifecycle reconciliation

scripts/shadow_v2_manifest_audit.py
  - single-pass SHA256, line count and JSONL stats

scripts/shadow_v2_l2_f_research_validation_run.py
  - streaming sample gates, malformed row counts, account_data_hash coverage
  - position-level evidence-complete density gate
```

Memory proof from final R16 validation:

```text
full_l2_f_wrapper_max_rss_kb=164864
full_l2_f_wrapper_elapsed=6:16.80
full_l2_f_wrapper_swaps=0
```

The temporary `/swapfile` used during the incident was removed after hardening:

```text
swap=0B
/swapfile removed=true
/etc/fstab /swapfile entry removed=true
/etc/fstab backup=/etc/fstab.codex-backup-20260706T0952Z
```

## Rejected Alternatives

### Claim L2 on all 769 roundtrips

Rejected. Nineteen positions have typed density/retention exclusions. They are
excluded from positive claims.

### Treat broad density BLOCKED as fatal despite 750 evidence-complete roundtrips

Rejected. The broad raw density surface is useful context, but the L2-F
research claim is position-level and requires at least 500 evidence-complete
roundtrips. R16 has 750.

### Use undeclared 300000/500000 horizons for positive claims

Rejected. These horizons remain:

```text
NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
```

### Grant runtime approval or live equivalence

Rejected. L2-F is offline-only and does not prove live fills, live failed/no-fill
telemetry, realized slippage, quote/fill divergence, active close, or live path
safety.

## Consequences

```text
L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY=true
runtime_decision_behavior_changes=false
gatekeeper_policy_changes=false
buy_reject_logic_changes=false
selector_runtime_changes=false
tx_jito_live_path_changes=false
provider_stream_changes=false
threshold_changes=false
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

Shadow V2 is now eligible for offline L2 research over the evidence-complete
scope only. It remains blocked from live-equivalence claims and runtime
execution approvals.
