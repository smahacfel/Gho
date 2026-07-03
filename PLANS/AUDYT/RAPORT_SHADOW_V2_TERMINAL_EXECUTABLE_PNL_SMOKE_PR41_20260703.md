# Raport Shadow V2 Terminal Executable PnL Smoke PR41 2026-07-03

## 1. Executive verdict

Finalny verdict smoke po merge PR41:

```text
PR41_TERMINAL_EXECUTABLE_PNL_SMOKE_PASS
```

Smoke potwierdzil w realnym shadow flow, ze runtime generuje `shadow_terminal_truth_v2.final_pnl_executable_bps != null` i exact terminal links do canonical `FILLED` entry oraz `FILLED` exit dla tego samego `position_id`.

To jest validation/fidelity smoke dla terminal executable PnL. To nie jest strategy test, profitability test, alpha test, research-grade, live-equivalence ani runtime approval.

Dodatkowy status L1:

```text
PLAN_PR36_L1_DETERMINISTIC_EXECUTION_SIM_READY_CANDIDATE
```

Ten status oznacza tylko kandydature dla deterministycznej L1 execution simulation. Nie nadaje approval.

## 2. Run metadata

| Pole | Wartosc |
|---|---|
| `run_id` | `shadow-smoke-v2-terminal-executable-pnl-pr41-r1` |
| `scope` | `reports/selector/shadow-v2-terminal-executable-pnl-pr41-r1` |
| `main_head` | `b68ec301e3b17a3a0a81ef005fb1cf37083bc421` |
| `PR41_merge_commit` | `b68ec301e3b17a3a0a81ef005fb1cf37083bc421` |
| `configured_run_seconds` | `900` |
| `duration_seconds` | `932` |
| `shutdown_method` | `SIGINT` |
| `controller_exit_status` | `0` |
| `forced_sigterm` | `false` |
| `clean_shutdown_proven` | `true` |
| `no_forced_component_abort` | `true` |

Exact command/config used:

```bash
./target/release/ghost-launcher --config configs/rollout/shadow-v2-terminal-executable-pnl-pr41-r1.local.toml
```

Controller contract:

```text
launch ghost-launcher
sleep 900s
send SIGINT
wait max 300s for clean drain
SIGTERM only on drain timeout
```

Observed shutdown proof:

```text
launcher_wait_status = All components shut down successfully; Ghost Launcher shutdown complete
runtime_post_run_manifest_status = PASS
post_run_strict_audit_status = PASS
```

## 3. Pre-run / post-run gates

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
| runtime `post_run_manifest.status` | `PASS` |
| post-run strict audit | `PASS` |
| PostBuyRuntime manifest generated before shutdown | PASS |
| `All components shut down successfully` | present |
| `Ghost Launcher shutdown complete` | present |
| forced SIGTERM | `false` |
| forced component abort | `false` |

## 4. Canonical evidence rows

| Metric | Value |
|---|---:|
| `canonical_rows` | 281 |
| `malformed_canonical_rows` | 0 |
| `accepted_shadow_handoff_count` | 28 |
| `validation_smoke_marker_count` | 1 |
| `density_rows` | 1967 |

Canonical event kind counts:

| event_kind | Rows |
|---|---:|
| `ENTRY_ATTEMPT` | 28 |
| `ENTRY_FILL` | 28 |
| `EXIT_ATTEMPT` | 28 |
| `EXIT_FILL` | 28 |
| `PATH_SAMPLE` | 56 |
| `POOL_STATE_SAMPLE` | 56 |
| `POSITION_CREATED` | 29 |
| `TERMINAL_TRUTH` | 28 |


## 5. Entry / exit L1 diagnostic fill counters

| Counter | Value |
|---|---:|
| `entry_fill_FILLED_count` | 28 |
| `entry_fill_BLOCKED_BY_DATA_count` | 0 |
| `entry_execution_simulation_ready_true_count` | 28 |
| `entry_execution_label_grade_DIAGNOSTIC_SIM_count` | 28 |
| `exit_fill_FILLED_count` | 28 |
| `exit_fill_BLOCKED_BY_DATA_count` | 0 |
| `exit_execution_simulation_ready_true_count` | 28 |
| `exit_execution_label_grade_DIAGNOSTIC_SIM_count` | 28 |
| `exit_execution_label_grade_RESEARCH_CANDIDATE_count` | 0 |
| `exit_execution_label_grade_LIVE_CONFIRMED_count` | 0 |
| `entry_FILLED_exit_FILLED_same_position_count` | 28 |

Interpretacja:

- Entry L1 diagnostic fill: PASS.
- Exit L1 diagnostic fill: PASS.
- `LIVE_CONFIRMED`: 0, czyli PR41 nie nadaje live confirmation.
- `RESEARCH_CANDIDATE`: 0, czyli evidence pozostaje diagnostic-only.

## 6. Terminal executable PnL proof

| Counter | Value |
|---|---:|
| `terminal_truth_rows` | 28 |
| `terminal_truth_with_final_pnl_mark_bps_count` | 28 |
| `terminal_truth_with_final_pnl_executable_bps_count` | 28 |
| `terminal_truth_without_final_pnl_executable_bps_count` | 0 |
| `complete_executable_roundtrip_positions` | 28 |
| `terminal_truth_exact_linked_entry_fill_count` | 28 |
| `terminal_truth_exact_linked_exit_fill_count` | 28 |
| `terminal_truth_exact_entry_exit_link_pair_count` | 28 |
| `terminal_truth_linked_entry_fill_missing_count` | 0 |
| `terminal_truth_linked_exit_fill_missing_count` | 0 |
| `unknown_or_untyped_terminal_blocker_count` | 0 |

Sample complete executable roundtrip:

| Field | Value |
|---|---|
| `position_id` | `CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594` |
| `terminal_truth_event_id` | `shadow_v2_terminal_truth:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101543956:STOP` |
| `linked_entry_fill_event_id` | `shadow_v2_entry_fill:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101536771` |
| `linked_exit_fill_event_id` | `shadow_v2_exit_fill:CVzzi42CiPoY8L32fTuCyvcrssfx4wTTh5RyMuK7UgGS:FoUkPTEhD8NyaCPkCTWsR74TxiYcpSoNzLwBMGcEpump:1783101543594:1783101543887:exit_filled` |
| `sample_entry_fill_status` | `FILLED` |
| `sample_exit_fill_status` | `FILLED` |
| `sample_final_pnl_executable_bps` | `-5486` |
| `sample_final_pnl_mark_bps` | `-5486` |

Exact link verification:

```text
linked_entry_fill_event_id exists in canonical stream = true
linked_entry_fill.fill_status = FILLED
linked_exit_fill_event_id exists in canonical stream = true
linked_exit_fill.fill_status = FILLED
linked entry/exit position_id == terminal position_id = true
```

Terminal reconciliation distribution:

| Status | Count |
|---|---:|
| `TERMINAL_TRUTH_WITH_DIAGNOSTIC_EXECUTABLE_PNL` | 28 |

Terminal limitations distribution:

| Limitation | Count |
|---|---:|
| `NOT_LIVE_EQUIVALENT` | 28 |
| `SESSION_ID_MISSING_FROM_LIFECYCLE_EXPLICIT_UNKNOWN` | 28 |
| `SHADOW_V2_RECORD_NOT_CONSUMED_BY_DECISIONS` | 28 |
| `TERMINAL_EXECUTABLE_PNL_DIAGNOSTIC_ONLY_NOT_LIVE_CONFIRMED` | 28 |
| `TERMINAL_EXECUTABLE_PNL_FROM_CANONICAL_ENTRY_EXIT_FILLED_EVENTS` | 28 |
| `TERMINAL_TRUTH_DERIVED_FROM_LEGACY_LIFECYCLE_RECORD` | 28 |
| `TERMINAL_TRUTH_MARK_PATH_ONLY_NOT_EXECUTABLE_FILL` | 28 |


## 7. Offline audit verdicts

| Audit | Verdict |
|---|---|
| entry reconstruction readiness | `PASS_ENTRY_RECONSTRUCTION_READY` |
| exit reconstruction readiness | `PASS_EXIT_RECONSTRUCTION_READY` |
| replay/lifecycle reconciliation | `PASS_REPLAY_LIFECYCLE_RECONCILED` |
| temporal/no-lookahead | `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` |
| manifest retention | `PASS_MANIFEST_RETENTION_AUDIT` |
| path density horizon | `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS` |

Dedicated terminal executable PnL audit script:

```text
MISSING
terminal_link_counters_computed_from_canonical_stream = true
```

Important limitations:

- Temporal audit remains `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` because chain-order contains explicit UNKNOWN components.
- Path density remains `BLOCKED_DENSITY_NOT_EVALUABLE_FOR_REQUIRED_HORIZONS`.
- These limitations do not invalidate this narrow terminal executable PnL smoke PASS, but they still block research-grade and live-equivalence claims.

Density verdict distribution:

| Verdict | Count |
|---|---:|
| `NOT_EVALUABLE_HORIZON_EXCEEDS_REPLAY` | 1176 |
| `NOT_EVALUABLE_NO_COVERAGE` | 791 |


## 8. PASS gate evaluation

| PASS gate | Result |
|---|---|
| `accepted_shadow_handoff_count > 0` | PASS (28) |
| `entry_fill_FILLED_count > 0` | PASS (28) |
| `exit_fill_FILLED_count > 0` | PASS (28) |
| `entry_FILLED_exit_FILLED_same_position_count > 0` | PASS (28) |
| `terminal_truth_with_final_pnl_executable_bps_count > 0` | PASS (28) |
| `complete_executable_roundtrip_positions > 0` | PASS (28) |
| `terminal_truth_exact_entry_exit_link_pair_count > 0` | PASS (28) |
| `exit_execution_label_grade_LIVE_CONFIRMED_count = 0` | PASS (0) |
| `runtime_post_run_manifest_status = PASS` | PASS |
| `post_run_strict_audit_status = PASS` | PASS |
| `clean_shutdown_proven = true` | PASS |
| `unknown_or_untyped_terminal_blocker_count = 0` | PASS |

## 9. Approval flags

No approval was granted by this smoke:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = false
live_equivalence = false
strategy_research_unblocked = false
edge_proven = false
```

## 10. Final decision

```text
final_verdict = PR41_TERMINAL_EXECUTABLE_PNL_SMOKE_PASS
PLAN_PR36_L1_DETERMINISTIC_EXECUTION_SIM_READY_CANDIDATE = true
runtime_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
```

What is proven:

- Real shadow flow produced diagnostic `FILLED` entry and diagnostic `FILLED` exit for the same positions.
- Terminal truth wrote `final_pnl_executable_bps` for those same-position FILLED pairs.
- Terminal exact links point to canonical `shadow_entry_fill_v2` and `shadow_exit_fill_v2` records with `fill_status=FILLED`.
- Clean shutdown and post-run manifest strict audit passed.

What is not proven:

- live fills;
- realized slippage;
- quote/fill divergence;
- research-grade temporal ordering;
- evaluable density for required horizons;
- strategy edge or profitability;
- runtime approval.
