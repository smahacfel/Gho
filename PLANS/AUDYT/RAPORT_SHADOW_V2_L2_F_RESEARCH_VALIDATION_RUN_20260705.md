# Raport Shadow V2 L2-F: research validation run 20260705

## Status

```text
run_id=shadow-v2-l2-f-collection-20260705-r16
main_head=5e6d839984cf38ecb65a0540b0adfabfcead9f23
scope_root=reports/selector/shadow-v2-l2-f-collection-20260705-r16
summary_csv=reports/selector/shadow_v2_l2_f_research_validation_summary.csv
strict_audit=reports/selector/shadow-v2-l2-f-collection-20260705-r16/strict_audit_summary.json
runtime_post_run_manifest=reports/selector/shadow-v2-l2-f-collection-20260705-r16/runtime_post_run_manifest.json
evidence_complete_scope=reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_position_scope_v1.jsonl
final_verdict=L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
l2_research_grade_candidate=true_for_shadow_v2_offline_research_only
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

R16 jest najnowszym audytowanym L2-F collection scope. Finalny wynik jest
pozytywny tylko w sensie offline-only: scope zawiera 750 pozycji, ktore maja
kompletny executable roundtrip, entry/exit `RESEARCH_CANDIDATE`, terminal
truth oraz PASS density/retention dla wszystkich zadeklarowanych L2 baseline
horizons.

Ten verdict nie oznacza runtime approval, live equivalence, active close,
shadow_close_only ani odblokowania strategii produkcyjnej.

## Scope i Artefakty

Audytowany scope:

```text
reports/selector/shadow-v2-l2-f-collection-20260705-r16
```

Wymagane artefakty L2-F sa obecne:

```text
shadow_position_event_v2.jsonl
shadow_replay_v2.jsonl
shadow_lifecycle_v2.jsonl
shadow_path_density_v2.jsonl
candidate_universe_v1.jsonl
candidate_universe_manifest_v1.json
gatekeeper_decision_root_evidence.json
runtime_post_run_manifest.json
strict_audit_summary.json
post_run_manifest.json
shadow_v2_manifest_report.csv
l2_f_evidence_complete_position_scope_v1.jsonl
```

Row counts z `post_run_manifest.json`:

```text
shadow_position_event_v2.jsonl=169474
shadow_replay_v2.jsonl=169474
shadow_lifecycle_v2.jsonl=169474
shadow_path_density_v2.jsonl=1186315
candidate_universe_v1.jsonl=2272
malformed_jsonl_rows=0
total_scope_size_bytes=26174692413
```

## Final L2-F Gate Result

Summary CSV:

```text
reports/selector/shadow_v2_l2_f_research_validation_summary.csv
```

Strict audit:

```text
reports/selector/shadow-v2-l2-f-collection-20260705-r16/strict_audit_summary.json
```

Wynik:

```text
final_verdict=L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
blockers=[]
complete_executable_roundtrip_positions=769
complete_executable_roundtrip_required=500
research_candidate_roundtrip_count=769
entry_execution_label_grade_RESEARCH_CANDIDATE_count=811
exit_execution_label_grade_RESEARCH_CANDIDATE_count=784
sample_size_gate=PASS
malformed_rows=0
unknown_untyped_blockers=0
```

Pozytywny L2-F claim jest ograniczony do pozycji, ktore sa kompletne na
poziomie wszystkich wymaganych dowodow:

```text
position_level_density_retention_verdict=PASS_L2_F_POSITION_LEVEL_DENSITY_RETENTION
l2_research_evidence_complete_roundtrip_positions=750
required_roundtrips=500
density_excluded_roundtrip_positions=19
non_evaluable_positions_excluded_from_positive_claim=true
evidence_complete_position_scope=reports/selector/shadow-v2-l2-f-collection-20260705-r16/l2_f_evidence_complete_position_scope_v1.jsonl
```

Interpretacja: w R16 jest 769 kompletnych research-candidate roundtripow, ale
19 z nich ma typed density/retention exclusions. One nie wspieraja pozytywnego
claimu. Finalne L2-F opiera sie wylacznie na 750 pozycjach z pelnym evidence.

## Gates That Passed

### Temporal source proof

```text
temporal_audit_verdict=PASS_TEMPORAL_NO_LOOKAHEAD_AUDIT
fake_handoff_signature_count=0
event_seq_chain_order_substitute_count=0
terminal_truth_not_derived_count=0
```

### Gatekeeper denominator

```text
gatekeeper_denominator_verdict=GATEKEEPER_DENOMINATOR_COVERAGE_KNOWN
candidate_universe_count=2272
eligible_denominator_count=2272
gatekeeper_decision_count=4535
gatekeeper_decision_joined_to_candidate_count=4535
gatekeeper_decision_unmatched_count=0
checkpoint_reach_count=2268
gatekeeper_buy_count=2041
gatekeeper_reject_count=320
gatekeeper_timeout_count=2174
threshold_starvation_verdict=NO_GATEKEEPER_THRESHOLD_STARVATION_OBSERVED
unknown_reason_count=0
denominator_contract_failures=[]
```

Decision logs nie tworza denominator rows. Candidate universe pozostaje
event-level denominator:

```text
candidate_universe_v1=reports/selector/shadow-v2-l2-f-collection-20260705-r16/candidate_universe_v1.jsonl
candidate_universe_manifest_v1=reports/selector/shadow-v2-l2-f-collection-20260705-r16/candidate_universe_manifest_v1.json
```

### Replay / lifecycle / manifest

```text
manifest_status=PASS
manifest_blockers=[]
replay_lifecycle_verdict=PASS_REPLAY_LIFECYCLE_RECONCILED
malformed_rows=0
unknown_untyped_blockers=0
```

### Account data hash coverage

```text
account_data_hash_coverage_verdict=PASS_ACCOUNT_DATA_HASH_COVERAGE
observed_account_state_boundary_samples=81883
samples_with_account_data_hash=81883
missing_account_data_hash_count=0
```

## Density / Retention Scope

Declared L2 baseline horizons:

```text
2000
3000
10000
30000
120000
```

Unsupported long horizons remain non-blocking and cannot support positive L2
claims:

```text
300000=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
500000=NOT_EVALUABLE_UNDECLARED_FOR_L2_BASELINE
positive_claims_from_undeclared_horizons_allowed=false
```

Broad density audit on the full raw density surface still reports:

```text
density_retention_verdict=BLOCKED_PATH_SAMPLE_COVERAGE_INSUFFICIENT
declared_horizon_present_count=5
declared_horizon_missing_count=0
declared_horizon_path_coverage_blocker_count=5
declared_horizon_retention_blocker_count=0
l2_f_allowed_next=false
```

This broad verdict is intentionally retained as context. It means the full raw
density stream contains non-evaluable positions and should not be described as
100 percent complete. L2-F is granted only over the stricter
`evidence-complete` position scope:

```text
research_candidate_roundtrip_count=769
l2_research_evidence_complete_roundtrip_positions=750
density_excluded_roundtrip_positions=19
sparse_approx_only_position_count=17
retention_gap_position_count=2
missing_declared_horizon_position_count=0
path_sample_coverage_gap_position_count=0
unknown_or_untyped_density_verdict_position_count=0
malformed_density_rows=0
unknown_horizon_rows=0
```

Horizon blocker context for excluded positions:

```text
2000:RETENTION_GAP=2
3000:RETENTION_GAP=2
10000:RETENTION_GAP=2
30000:RETENTION_GAP=2
30000:SPARSE_APPROX_ONLY=5
120000:RETENTION_GAP=2
120000:SPARSE_APPROX_ONLY=17
```

The 19 excluded positions are typed, counted, and excluded from positive claims.
They are not silently promoted to PASS.

## Tooling Memory Hardening

Poprzednia proba wrappera L2-F zostala zatrzymana, bo Python ladowal duze JSONL
do pamieci i doszedl do okolo 15 GB RSS. Tooling zostal utwardzony na tryb
streaming/bounded-memory.

Zmiany:

```text
scripts/shadow_v2_offline_audit_common.py
  - dodano iter_jsonl()
  - dodano iter_canonical_rows(), iter_replay_rows(), iter_lifecycle_rows(), iter_density_rows()

scripts/shadow_v2_path_density_horizon_audit.py
  - usunieto full-list load shadow_path_density_v2.jsonl z glownej sciezki
  - dodano streaming latest snapshot scan per position+horizon

scripts/shadow_v2_temporal_no_lookahead_audit.py
  - canonical/replay/lifecycle czytane streamingowo
  - event_seq monotonicity liczona online przez last_seq_by_position

scripts/shadow_v2_replay_lifecycle_terminal_reconciliation_audit.py
  - replay/lifecycle czytane streamingowo

scripts/shadow_v2_manifest_audit.py
  - single-pass SHA256, line count i JSONL stats

scripts/shadow_v2_l2_f_research_validation_run.py
  - sample gates, malformed rows, account_data_hash coverage i position-level density gate sa streaming/bounded-memory
```

Memory proof z finalnej walidacji R16:

```text
full_l2_f_wrapper_max_rss_kb=164732
full_l2_f_wrapper_elapsed=5:27.01
full_l2_f_wrapper_swaps=0
```

Temporary swap uzyty podczas incydentu zostal usuniety po hardeningu:

```text
swap=0B
/swapfile removed=true
/etc/fstab /swapfile entry removed=true
/etc/fstab backup=/etc/fstab.codex-backup-20260706T0952Z
```

## Final Verdict

```text
L2_RESEARCH_GRADE_CANDIDATE_OFFLINE_ONLY
```

Granted only for Shadow V2 offline research over:

```text
l2_research_evidence_complete_roundtrip_positions=750
```

Not granted:

```text
runtime_approval=false
research_grade=false
live_equivalence=false
strategy_research_unblocked=false
shadow_close_only=false
active_close=false
```

No runtime decision behavior, Gatekeeper policy, BUY/REJECT logic, selector
runtime, TX/Jito/live path, provider stream, threshold, shadow_close_only, or
active close behavior was changed by this validation.
