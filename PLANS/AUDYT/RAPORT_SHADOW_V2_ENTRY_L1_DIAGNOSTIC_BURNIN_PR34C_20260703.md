# Raport Shadow V2 Entry L1 Diagnostic Burnin PR34-C 2026-07-03

## 1. Executive verdict

Finalny verdict:

```text
PR34_ENTRY_L1_DIAGNOSTIC_BURNIN_PASS
```

Burnin potwierdzil na realnym shadow flow, ze po PR33 + PR34-A/B/C runtime potrafi przeniesc upstream `ENTRY_BEFORE` boundary payload do PostBuyRuntime i wygenerowac diagnostic executable L1 entry fill.

To nie jest strategy proof, edge proof, runtime approval, research-grade ani live-equivalence proof.

Najwazniejsze wyniki:

| Metryka | Wartosc |
|---|---:|
| `accepted_shadow_handoff_count` | 81 |
| `entry_boundary_payload_count` | 81 |
| `entry_boundary_payload_missing_count` | 0 |
| `entry_fill_FILLED_count` | 81 |
| `entry_fill_BLOCKED_BY_DATA_count` | 0 |
| `execution_simulation_ready_true_count` | 81 |
| `execution_simulation_ready_false_count` | 0 |
| `research_provenance_ready_true_count` | 0 |
| `research_provenance_ready_false_count` | 81 |
| `execution_label_grade_DIAGNOSTIC_SIM_count` | 81 |
| `execution_label_grade_RESEARCH_CANDIDATE_count` | 0 |
| `execution_label_grade_LIVE_CONFIRMED_count` | 0 |

## 2. Scope i command

Run:

```text
run_id = shadow-burnin-v2-entry-l1-diagnostic-pr34c-r1
scope = reports/selector/shadow-v2-entry-l1-diagnostic-pr34c-r1
main_head = 57c6e313e2b583ea09bde98d0eff5367fd048f88
start_utc = 2026-07-03T09:52:37Z
sigint_utc = 2026-07-03T10:37:37Z
end_utc = 2026-07-03T10:38:09Z
elapsed_to_sigint_seconds = 2700
elapsed_total_seconds = 2732
shutdown_method = SIGINT
launcher_wait_status = 0
```

Exact runtime command used by controller:

```bash
./target/release/ghost-launcher --config configs/rollout/shadow-v2-entry-l1-diagnostic-pr34c-r1.local.toml
```

Controller settings:

```text
duration_seconds = 2700
drain_seconds = 360
controller_exit = SIGINT_CLEAN
forced_kill = false
SIGTERM = false
```

## 3. Input evidence / manifest proof

Input/config artifacts were local and not staged:

| Artifact | SHA256 |
|---|---|
| `configs/rollout/shadow-v2-entry-l1-diagnostic-pr34c-r1.local.toml` | `5dfab2e3dc38d3c6b68c30913e7be52e369a72eb5b2575b2bb807c4cf498875d` |
| `configs/rollout/ghost_brain_shadow_v2_entry_l1_diagnostic_pr34c_r1.local.toml` | `c91d257462c5d607bbd2aea6d775eec2b7a869c7f25dc53e03050fa8ba34399a` |
| `target/release/ghost-launcher` | `ea1c7e79425e700eba9134c61c613ad6b4deaaa032acad32116e1689aa117934` |
| `pre_run_manifest.json` | `4e90226c4279e1fab7b33ed27202b26d0d565f7a36042b595da539f071e49c17` |
| `post_run_manifest.json` | `38f3c30b2a5d427dab152be2547f750195bc7c9f65a62fd1023feaa34d6f977b` |
| `shadow_v2_manifest_report.csv` | `7a0507e0f3e9a4202a507a8d94c378c21cfc9e0da4d1258c765f550ddd26d083` |

Pre-run gates:

| Gate | Wynik |
|---|---|
| pre-run manifest generation | PASS |
| pre-run strict manifest audit | PASS |
| validation burnin plan audit | PASS / `validation_mode=FIDELITY_ONLY` |
| legacy downgrade audit | PASS |
| launcher preflight | PASS, runtime started |

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

## 4. Canonical evidence rows

| Artifact/schema | Rows |
|---|---:|
| `shadow_position_event_v2.jsonl` | 814 |
| `shadow_replay_v2.jsonl` | 814 |
| `shadow_lifecycle_v2.jsonl` | 814 |
| `shadow_path_density_v2.jsonl` | 5698 |

Canonical event kind counts:

| event_kind | Rows |
|---|---:|
| `POSITION_CREATED` | 82 |
| `ENTRY_ATTEMPT` | 81 |
| `POOL_STATE_SAMPLE` | 163 |
| `ENTRY_FILL` | 81 |
| `PATH_SAMPLE` | 162 |
| `EXIT_ATTEMPT` | 82 |
| `EXIT_FILL` | 82 |
| `TERMINAL_TRUTH` | 81 |

Po odjeciu validation smoke marker:

```text
real accepted shadow handoffs = 81
```

## 5. Entry L1 diagnostic counters

| Counter | Wartosc |
|---|---:|
| `ENTRY_BOUNDARY_BASE_MINT_MISMATCH_count` | 0 |
| `ENTRY_BOUNDARY_POOL_ID_MISMATCH_count` | 0 |
| `ENTRY_BOUNDARY_HANDOFF_VALIDATION_FAILED_count` | 0 |
| `ENTRY_FILL_POOL_STATE_SAME_SLOT_ORDER_AMBIGUOUS_count` | 0 |
| `ENTRY_FILL_POOL_STATE_AFTER_FILL_BOUNDARY_count` | 0 |
| `ENTRY_FILL_POOL_STATE_NOT_STRICTLY_BEFORE_FILL_BOUNDARY_count` | 0 |
| `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME_count` | 162 |
| `POOL_STATE_ACCOUNT_DATA_HASH_UNAVAILABLE_IN_RUNTIME_affected_entry_fills` | 81 |
| `unknown_or_untyped_blocker_count` | 0 |

Interpretacja:

- `entry_boundary_payload_count = 81`: payload `ENTRY_BEFORE` dociera do PostBuyRuntime dla kazdego realnego entry fill w runie.
- `entry_fill_FILLED_count = 81`: PR33 L1 engine policzyl diagnostic simulated BUY fill dla wszystkich realnych entry fill rows.
- `research_provenance_ready_true_count = 0`: brak pelnej provenance research-grade, glownie przez brak `account_data_hash` oraz explicit UNKNOWN chain-order components.
- `execution_label_grade_RESEARCH_CANDIDATE_count = 0`, `LIVE_CONFIRMED = 0`: wynik pozostaje DIAGNOSTIC_SIM, nie live-equivalent.
- `BLOCKED_ORDERING_AMBIGUITY` wystepuje jako limitation/provenance blocker, ale w tym burninie nie wystepuje jako `fill_status=BLOCKED_BY_DATA`; specyficzne hard blockers `ENTRY_FILL_POOL_STATE_*` maja wartosc 0.

## 6. Sample FILLED diagnostic entry fill

```text
event_id = shadow_v2_entry_fill:G1SATjuHQoqHiRzS4weWB9oGPxN5VPmkrGPVM8rZTEDV:BHVwprZiFjT5jFbGiGqGagmYeULn5TJtjuBYLL8jpump:1783072518338:1783072511805
position_id = G1SATjuHQoqHiRzS4weWB9oGPxN5VPmkrGPVM8rZTEDV:BHVwprZiFjT5jFbGiGqGagmYeULn5TJtjuBYLL8jpump:1783072518338
pool_id = G1SATjuHQoqHiRzS4weWB9oGPxN5VPmkrGPVM8rZTEDV
base_mint = BHVwprZiFjT5jFbGiGqGagmYeULn5TJtjuBYLL8jpump
fill_status = FILLED
execution_simulation_ready = True
research_provenance_ready = False
execution_label_grade = DIAGNOSTIC_SIM
fill_price = 5.836837670714986e-08
fill_amount_tokens = 119927.954055
pool_state_before = pool_state_sample_v2:G1SATjuHQoqHiRzS4weWB9oGPxN5VPmkrGPVM8rZTEDV:BHVwprZiFjT5jFbGiGqGagmYeULn5TJtjuBYLL8jpump:1783072518338:1783072511805:entry_before
quality = L1_BUY_EXECUTION_SIM_DIAGNOSTIC
```

Representative `BLOCKED_BY_DATA` sample IDs:

```text
none observed for shadow_entry_fill_v2 in this burnin
```

## 7. Offline audit cross-checks

| Audit | Verdict / wynik |
|---|---|
| entry reconstruction readiness | `PASS_ENTRY_RECONSTRUCTION_READY` |
| temporal/no-lookahead audit | `BLOCKED_TEMPORAL_AMBIGUITY_REMAINS` |
| malformed canonical rows | 0 |
| missing required event_order_key rows | 0 |
| non_monotonic_event_seq_in_process | 0 |
| post_entry_fields_used_in_pre_decision_context | 0 |
| terminal_truth_used_as_pre_entry_evidence | 0 |
| derived_replay_lifecycle_used_as_canonical_input | 0 |

Temporal ambiguity remains because chain-order components are explicit `UNKNOWN` for many rows. This blocks research-grade interpretation, but it does not invalidate this L1 diagnostic entry-fill wiring burnin.

## 8. Approval flags

Approval flags remain false / not granted:

```text
runtime_approval = false
shadow_close_only_approval = false
active_close_approval = false
research_grade = NOT_GRANTED
live_equivalence = NOT_GRANTED
strategy_research_unblocked = false
```

No changes were made to:

- BUY/REJECT
- Gatekeeper policy
- selector runtime
- TX/Jito/live path
- `shadow_close_only`
- active close
- R51

## 9. Final decision

```text
PR34_ENTRY_L1_DIAGNOSTIC_BURNIN_PASS
```

This proves only that PR33 + PR34-A/B/C can produce diagnostic L1 executable BUY entry labels from an upstream `ENTRY_BEFORE` boundary payload in real shadow flow.

It does not prove live fill, realized slippage, quote/fill divergence, research-grade provenance, executable exit fill, strategy edge, RCE readiness, runtime approval, `shadow_close_only`, active close, or live-equivalence.
