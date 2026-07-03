# Raport Shadow V2 Exit L1 Diagnostic Burnin PR38-B 2026-07-03

## 1. Executive verdict

Finalny verdict:

```text
PR38_EXIT_L1_DIAGNOSTIC_BURNIN_PASS_ROUNDTRIP_BLOCKED
```

Burnin potwierdził na realnym shadow flow, że po PR33 + PR34-A/B/C + PR37 + PR38-A0 + PR38-B runtime potrafi wygenerować diagnostic executable L1 SELL exit fill z `EXIT_BEFORE` pool-state sample.

Jednocześnie burnin nie potwierdził pełnego executable roundtrip PnL, ponieważ `shadow_terminal_truth_v2.final_pnl_executable_bps` pozostał pusty dla wszystkich terminal truth rows.

To nie jest strategy proof, edge proof, runtime approval, research-grade ani live-equivalence proof.

Najważniejsze wyniki:

| Metryka | Wartość |
|---|---:|
| `accepted_shadow_handoff_count` | 129 |
| `entry_fill_FILLED_count` | 129 |
| `exit_pool_state_before_count` | 129 |
| `exit_token_amount_raw_count` | 129 |
| `exit_fill_FILLED_count` | 129 |
| `exit_fill_BLOCKED_BY_DATA_count` | 0 |
| `exit_execution_simulation_ready_true_count` | 129 |
| `exit_execution_label_grade_DIAGNOSTIC_SIM_count` | 129 |
| `exit_execution_label_grade_RESEARCH_CANDIDATE_count` | 0 |
| `exit_execution_label_grade_LIVE_CONFIRMED_count` | 0 |
| `entry_FILLED_exit_FILLED_same_position_count` | 129 |
| `terminal_truth_with_final_pnl_executable_bps_count` | 0 |
| `complete_executable_roundtrip_positions` | 0 |

Interpretacja verdictu:

- `exit_fill_FILLED_count > 0`: diagnostic SELL L1 fill działa w runtime.
- `complete_executable_roundtrip_positions = 0`: nie ma jeszcze pełnego executable roundtrip proof.
- `terminal_truth_with_final_pnl_executable_bps_count = 0`: terminal truth nie niesie executable PnL mimo tego, że entry i exit fills są `FILLED` dla tych samych pozycji.

## 2. Scope i command

Run:

```text
run_id = shadow-burnin-v2-exit-l1-diagnostic-pr38b-r1
scope = reports/selector/shadow-v2-exit-l1-diagnostic-pr38b-r1
main_head = 1f3db1022c901d20ed145049d1623559a41c8de4
PR39_merge_commit = 1f3db1022c901d20ed145049d1623559a41c8de4
start_utc = 2026-07-03T13:43:39Z
sigint_utc = 2026-07-03T14:33:40Z
end_utc = 2026-07-03T14:34:08Z
elapsed_total_seconds = 3029
configured_run_seconds = 3000
shutdown_method = SIGINT
controller_exit_status = 0
```

Exact runtime command used by controller:

```bash
./target/release/ghost-launcher --config configs/rollout/shadow-v2-exit-l1-diagnostic-pr38b-r1.local.toml
```

Controller settings:

```text
bash controller: launch ghost-launcher, sleep 3000s, send SIGINT, wait up to 300s, SIGTERM only on drain timeout
forced_sigterm = false
clean_shutdown_proven = true
```

## 3. Input evidence / manifest proof

Input/config artifacts were local and not staged:

| Artifact | SHA256 |
|---|---|
| `configs/rollout/shadow-v2-exit-l1-diagnostic-pr38b-r1.local.toml` | `a00ec9309aee4da703648885d1bfd7dee9656b577377a43f3fcc4cc72728df63` |
| `configs/rollout/ghost_brain_shadow_v2_exit_l1_diagnostic_pr38b_r1.local.toml` | `33f3ef3230d296580293c5db06187bf0a68d9fe196a9ad3b017a27991167881c` |
| `target/release/ghost-launcher` | `84a24f5e612ab25d793e22c2880ffff7e0664cc450652b2f5765e049eaf8a601` |
| `pre_run_manifest.json` | `58dd69bfa28df013bd615e6f8aee8ae42a851eb94893620c29742005edf53523` |
| `post_run_manifest.json` | `7775737f02cd8b4181fcb1e716dc3e057b036748a40ea63d66f1c4c4521fccd9` |
| `shadow_v2_manifest_report.csv` | `c81597c28c4d2fe64f8c0b9d5896a5385685372eb4c7febb8948c9b358403270` |
| `shadow_v2_exit_l1_diagnostic_burnin_pr38b_summary.json` | `edbad2dbe992bf00e17e86785e2c33513f94fbdbd7354cc2bcf13ce0a43f2f76` |
| `shadow_v2_exit_l1_diagnostic_burnin_pr38b_summary.csv` | `7be5eb1023d115cca7a87b43512faf89a04e69e7c197aa027005bd527fd25b31` |

Pre-run gates:

| Gate | Wynik |
|---|---|
| `cargo build -p ghost-launcher --release` | PASS |
| pre-run manifest generation | PASS |
| pre-run strict manifest audit | PASS |
| validation burnin plan audit | PASS / `FIDELITY_ONLY` |
| legacy downgrade audit | PASS |
| launcher preflight | PASS |

Post-run gates:

| Gate | Wynik |
|---|---|
| runtime `post_run_manifest.status` | PASS |
| post-run strict audit | PASS |
| PostBuyRuntime shutdown | PASS |
| Shadow V2 post-run manifest generation before shutdown | PASS |
| `All components shut down successfully` | present |
| `Ghost Launcher shutdown complete` | present |
| forced component abort | false |
| SIGTERM / SIGKILL | false |
| reconnect/disconnect flood after shutdown | false |

## 4. Canonical evidence rows

| Artifact/schema | Rows |
|---|---:|
| `shadow_position_event_v2.jsonl` | 1291 |
| `shadow_replay_v2.jsonl` | 1291 |
| `shadow_lifecycle_v2.jsonl` | 1291 |
| `shadow_path_density_v2.jsonl` | 9037 |

Canonical event kind counts:

| event_kind | Rows |
|---|---:|
| `POSITION_CREATED` | 130 |
| `ENTRY_ATTEMPT` | 129 |
| `POOL_STATE_SAMPLE` | 258 |
| `ENTRY_FILL` | 129 |
| `PATH_SAMPLE` | 258 |
| `EXIT_ATTEMPT` | 129 |
| `EXIT_FILL` | 129 |
| `TERMINAL_TRUTH` | 129 |

Po odjęciu validation smoke marker:

```text
real accepted shadow handoffs = 129
validation_smoke_marker_count = 1
```

## 5. Entry side cross-check

| Counter | Wartość |
|---|---:|
| `entry_boundary_payload_count` | 129 |
| `entry_boundary_payload_missing_count` | 0 |
| `entry_fill_FILLED_count` | 129 |
| `entry_fill_BLOCKED_BY_DATA_count` | 0 |
| `entry_execution_simulation_ready_true_count` | 129 |
| `entry_execution_label_grade_DIAGNOSTIC_SIM_count` | 129 |

Entry side nadal działa zgodnie z wcześniejszym PR34-C burninem: `ENTRY_BEFORE` boundary payload dociera, entry fill idzie przez L1 engine, a grade pozostaje `DIAGNOSTIC_SIM`.

## 6. Exit L1 diagnostic counters

| Counter | Wartość |
|---|---:|
| `exit_attempt_rows` | 129 |
| `exit_pool_state_before_count` | 129 |
| `exit_pool_state_before_missing_count` | 0 |
| `exit_token_amount_raw_count` | 129 |
| `exit_token_amount_raw_missing_count` | 0 |
| `exit_token_amount_raw_persisted_field_count` | 0 |
| `exit_fill_FILLED_count` | 129 |
| `exit_fill_BLOCKED_BY_DATA_count` | 0 |
| `exit_execution_simulation_ready_true_count` | 129 |
| `exit_execution_simulation_ready_false_count` | 0 |
| `exit_execution_label_grade_DIAGNOSTIC_SIM_count` | 129 |
| `exit_execution_label_grade_RESEARCH_CANDIDATE_count` | 0 |
| `exit_execution_label_grade_LIVE_CONFIRMED_count` | 0 |
| `exit_research_provenance_ready_true_count` | 0 |
| `exit_research_provenance_ready_false_count` | 129 |

`exit_token_amount_raw_count` ma basis: `engine_path_precondition_for_shadow_exit_fill_v2_FILLED; raw input amount is not serialized as standalone field in shadow_exit_fill_v2`. Sam raw sell input nie jest osobnym durable fieldem w `shadow_exit_fill_v2`, dlatego raport rozdziela engine-path evidence od persisted-field coverage.

## 7. Exit blocker distribution

| Blocker / limitation | Count |
|---|---:|
| `EXIT_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS` | 0 |
| `EXIT_FILL_POOL_STATE_AFTER_FILL_BOUNDARY` | 0 |
| `EXIT_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY` | 0 |
| `EXIT_FILL_TOKEN_AMOUNT_RAW_UNAVAILABLE` | 0 |
| `EXIT_POOL_STATE_BEFORE_UNAVAILABLE` | 0 |
| `EXIT_FILL_POOL_STATE_SAMPLE_NOT_AVAILABLE_IN_RUNTIME` | 0 |
| `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME` | 129 |
| `unknown_or_untyped_exit_blocker_count` | 0 |

Typed provenance blockers observed for every exit fill:

```text
BLOCKED_ORDERING_AMBIGUITY
FILL_EVENT_EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT
FILL_EVENT_EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK
FILL_EVENT_EVENT_ORDER_UNKNOWN_COMPONENTS=block_time|signature|transaction_index_or_unknown|instruction_index_or_unknown|inner_instruction_index_or_unknown|log_index_or_unknown
POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME
POOL_STATE_EVENT_ORDER_EXPLICIT_UNKNOWN_CHAIN_COMPONENT
POOL_STATE_EVENT_ORDER_INTRA_SLOT_AMBIGUITY_REQUIRES_TIE_BREAK
POOL_STATE_EVENT_ORDER_UNKNOWN_COMPONENTS=block_time|signature|transaction_index_or_unknown|instruction_index_or_unknown|inner_instruction_index_or_unknown|log_index_or_unknown
```

Interpretacja: te blockery degradują provenance do `DIAGNOSTIC_SIM`; nie spowodowały `fill_status=BLOCKED_BY_DATA` w tym runie.

## 8. Terminal / executable roundtrip

| Counter | Wartość |
|---|---:|
| `terminal_truth_rows` | 129 |
| `terminal_truth_with_final_pnl_mark_bps_count` | 129 |
| `terminal_truth_with_final_pnl_executable_bps_count` | 0 |
| `terminal_truth_without_final_pnl_executable_bps_count` | 129 |
| `entry_FILLED_exit_FILLED_same_position_count` | 129 |
| `entry_FILLED_exit_BLOCKED_same_position_count` | 0 |
| `entry_BLOCKED_exit_FILLED_same_position_count` | 0 |
| `complete_executable_roundtrip_positions` | 0 |

Wniosek:

```text
Diagnostic exit fill proof = true
Complete executable roundtrip proof = false
```

`entry_FILLED_exit_FILLED_same_position_count = 129`, ale `terminal_truth_with_final_pnl_executable_bps_count = 0`. To oznacza, że kolejnym wąskim zadaniem powinno być terminal executable PnL wiring/audit, a nie edge/research-grade interpretation.

Dedicated exit roundtrip audit script:

```text
MISSING; counters computed from canonical shadow_position_event_v2.jsonl and terminal truth rows
```

## 9. Samples

Sample diagnostic `shadow_exit_fill_v2` with `FILLED`:

```text
event_id = shadow_v2_exit_fill:HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703:1783086278376:exit_filled
position_id = HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703
pool_state_before = shadow_v2_pool_state_exit_before:HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703:1783086278376:exit_filled
fill_price = 3.907507623668629e-08
fill_amount_sol = 0.003322183
fill_amount_tokens = 85020.512305
```

Representative `shadow_exit_fill_v2 BLOCKED_BY_DATA` sample:

```text
none observed for shadow_exit_fill_v2 in this burnin
```

Sample entry+exit filled position without terminal executable PnL:

```text
position_id = HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703
entry_fill_event_id = shadow_v2_entry_fill:HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703:1783086243911
exit_fill_event_id = shadow_v2_exit_fill:HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703:1783086278376:exit_filled
terminal_event_id = shadow_v2_terminal_truth:HVwanvJv9LScajPbEHFxcTpvHVbpqNd8JuTY7gxSckNv:FUiUiCoKM59SwdSrZvE6STvrh77JqjZwYnUMWMkapump:1783086252703:1783086278428:STOP
```

Sample complete executable roundtrip position:

```text
none observed; complete_executable_roundtrip_positions = 0
```

## 10. Offline audit cross-checks

| Audit | Verdict / wynik |
|---|---|
| entry reconstruction readiness | `PASS_ENTRY_RECONSTRUCTION_READY` |
| exit reconstruction readiness | `PASS_EXIT_RECONSTRUCTION_READY` |
| replay/lifecycle reconciliation | `PASS_REPLAY_LIFECYCLE_RECONCILED` |
| manifest/retention audit | `PASS_MANIFEST_RETENTION_AUDIT` |
| path density horizon audit | `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS` |
| temporal/no-lookahead audit | `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` |
| malformed canonical rows | 0 |
| missing required `event_order_key` rows | 0 |
| `non_monotonic_event_seq_in_process` | 0 |
| `post_entry_fields_used_in_pre_decision_context` | 0 |
| `terminal_truth_used_as_pre_entry_evidence` | 0 |
| `derived_replay_lifecycle_used_as_canonical_input` | 0 |

Density remains not evaluable for required horizons:

| Density verdict | Count |
|---|---:|
| `EVALUABLE_EXACT` | 0 |
| `EVALUABLE_APPROX` | 0 |
| `SPARSE_APPROX_ONLY` | 0 |
| `NOT_EVALUABLE_NO_COVERAGE` | 3619 |
| `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY` | 5418 |

Temporal audit remains blocked by explicit UNKNOWN chain-order components, not by missing required ordering or non-monotonic sequence.

## 11. Approval flags

Approval flags remain false / not granted:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
edge_proven = false
```

No changes were made to:

- BUY/REJECT
- Gatekeeper policy
- selector runtime
- TX/Jito/live path
- `shadow_close_only`
- active close
- R51

Raw JSONL, logs, runtime scope and local TOML are local evidence only and are not part of this PR.

## 12. Final decision

```text
PR38_EXIT_L1_DIAGNOSTIC_BURNIN_PASS_ROUNDTRIP_BLOCKED
```

This proves only diagnostic L1 SELL exit fill generation in real shadow flow. It does not prove complete executable roundtrip PnL, research-grade provenance, live fill, live slippage, quote/fill divergence, live-equivalence, runtime approval, `shadow_close_only`, active close, strategy edge, or RCE readiness.
