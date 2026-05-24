# RAPORT P3.7 R17 Replay-Ready Diagnostic Smoke - 2026-05-24

## Verdict

R17 replay-ready runtime contract: **PASS jako bounded runtime smoke**.

R17 potwierdza, ze po L2E2a runtime realnie emituje `decision_eval_snapshots` i Gatekeeper V2 replay fields. Strict V3 replay przechodzi, L1 diagnostics oraz identity/hash contract przechodza, a BCV2/AccountNotFound nie wraca jako runtime failure.

R17 **nie odblokowuje jeszcze L2D2 causal axis replay**, bo run nie dostarczyl executable lifecycle-labeled denominatora. To jest teraz blocker:

- temporal snapshot emission: PASS
- strict replay: PASS
- diagnostic quality: PASS
- identity/hash: PASS
- post-simulation AccountNotFound: 0
- route executable rows: 0
- on-chain lifecycle label rows: 0
- buy-quality denominator rows: 0

Final decision: **HOLD_L2D2_EXECUTABLE_LIFECYCLE_DENOMINATOR_MISSING**.

## Code / Config Checkpoint

Committed and pushed before runtime:

- `016ff52` - `Repair P3.7 L2E2a decision eval snapshot emission`
- `f93c41c` - `Add P3.7 R17 replay-ready temporal contract`

R17 config:

- `configs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic.toml`
- brain config: `configs/rollout/ghost_brain_v3_p37_r17_replay_ready_diagnostic.toml`

Temporal diagnostic contract:

- `gatekeeper_v2.mode = "standard"`
- `gatekeeper_v2.max_wait_time_ms = 10000`
- snapshot targets: `2000 / 5000 / 7000 / 10000`
- DOW windows ordered: `2000 <= 5000 <= 7000 <= 10000`
- `execution_mode = shadow`
- `trigger_entry_mode = shadow_only`
- P2/live/promotion disabled

Preflight result:

- `final_decision = GO_R17_REPLAY_READY_DIAGNOSTIC_RUN`
- blockers: `[]`
- runtime support:
  - `gatekeeper_emits_temporal_snapshots = true`
  - `decision_logger_has_v22_fields = true`
  - `gatekeeper_hardcodes_temporal_snapshots_none = false`

## Runtime Scope

Runtime was started from a clean detached worktree:

- worktree: `/root/Gho-r17-clean`
- HEAD: `f93c41c`
- namespace: `shadow-burnin-v3-p37-r17-replay-ready-diagnostic`

The run was bounded and stopped manually after collecting concrete diagnostic data. No runtime process was left running.

Primary artifacts:

- decision log:
  `logs/rollout/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/decisions/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/v2.2/legacy_live/1ddc1a9e03e0010ddc17c51d129bb890c1c79d4a967b9fbb8b31d7bf4dc13b6c/gatekeeper_v2_decisions.jsonl`
- shadow run:
  `logs/shadow_run/shadow-burnin-v3-p37-r17-replay-ready-diagnostic/`

## Replay / Diagnostics

Strict replay:

- `replay_status = full_replay_ok`
- total rows: `44`
- V3 rows: `44`
- bad rows: `0`
- status counts: `full_replay_ok = 44`

Shadow report:

- `status = ok`
- `raw_rows = 44`
- `v3_rows = 44`
- `deduped_rows = 44`
- `bad_rows = 0`
- artifact freshness stale against config: `false`
- full snapshot payload rows: `44`
- hash-only rows: `0`

L1 reject diagnostics:

- `diagnostic_quality.status = PASS`
- `r16_artifact_identity_status = PASS`
- BUY verdict rows: `1`
- active shadow entry rows: `1`
- active shadow lifecycle rows: `1`
- active shadow AccountNotFound rows: `0`
- active shadow AccountNotFound unattributed rows: `0`
- active shadow precheck failures: `2`
- good/dirty_good label rows: `0`

Join-key / execution-feasibility audit note:

- primary decision JSONL has `44` terminal decision rows
- join/feature audits scan decision + buy logs and therefore report `45` raw decision-context rows
- this is a reporting denominator nuance, not a replay/hash failure

## Temporal Snapshot Coverage

Decision log rows:

- rows: `44`
- rows with `decision_eval_snapshots`: `44`
- rows with `gatekeeper_gate_trace`: `44`
- terminal snapshot rows: `44`

Snapshot target coverage:

| Target ms | Rows |
| --- | ---: |
| 2000 | 36 |
| 5000 | 27 |
| 7000 | 37 |
| 10000 / terminal | 44 |

Snapshot count per row:

- min: `2`
- max: `4`

Snapshot drift:

- min: `0 ms`
- max: `9133 ms`
- mean: `1572.39 ms`

Replay readiness:

- `gatekeeper_v2_replay_ready_temporal = true`: `22`
- `gatekeeper_v2_replay_ready_temporal = false`: `22`

Interpretation:

The runtime emission contract is working, but target coverage is not yet complete enough to treat this run as causal L2D2 input. Terminal coverage is 100%, but half of rows are still not temporal-replay-ready.

## Verdict Mix

Decision verdict counts:

- `TIMEOUT_PHASE1_INSUFFICIENT`: `31`
- `TIMEOUT_PHASE1_NO_DATA`: `4`
- `REJECT_HARD_FAIL`: `7`
- `REJECT_CORE_FAIL`: `1`
- `BUY`: `1`

Top reason codes:

- `TIMEOUT_PHASE1_INSUFFICIENT`: `31`
- `HARD_FAIL_MARKET_CAP`: `5`
- `TIMEOUT_PHASE1_NO_DATA`: `4`
- `HARD_FAIL_SLOW_POOL`: `2`
- `BUY_EARLY`: `1`
- `REJECT_CORE_FAIL`: `1`

## Route / Execution Feasibility

MFS lifecycle join-key audit:

- `route_executable_rows = 0`
- `route_non_executable_rows = 38`
- `execution_feasibility_rate = 0.0`
- `execution_feasibility_reject_rows = 7`
- `probe_selected_rows = 4`
- `successful_entry_rows = 0`
- `lifecycle_eligible_rows = 0`

Active shadow route diagnostics:

- `active_shadow_route_executable_rows = 0`
- `active_shadow_route_non_executable_rows = 3`
- `active_shadow_no_executable_route_account_set_rows = 3`
- `active_shadow_route_fallback_attempted_rows = 3`
- `active_shadow_route_fallback_success_rows = 0`
- `active_shadow_fallback_repairable = false`
- `active_shadow_recommended_next_path = route_class_exclusion_from_execution_label_universe`

BCV2 readiness:

- `active_shadow_bonding_curve_v2_source = observed_tx_account_meta`
- `active_shadow_observed_bcv2_provenance_status = route_compatible`
- `active_shadow_bonding_curve_v2_rpc_load_status = missing_on_rpc_precheck`
- `active_shadow_builder_required_curve_account_ready = false`
- `active_shadow_bonding_curve_v2_account_not_found_after_simulation_rows = 0`

Interpretation:

R17 keeps the L1R18/L1R16 behavior: unsupported route rows fail closed as execution infeasible and do not become post-simulation AccountNotFound. This protects labels, but it also means R17 did not produce an executable lifecycle denominator.

## Lifecycle / Labels

Shadow artifacts:

- active shadow buys: `1`
- active shadow entries: `1`
- active shadow lifecycle records: `1`
- probe selections: `4`
- probe skips: `44`
- probe transport rows: `0`
- probe entry rows: `0`
- probe lifecycle rows: `0`

On-chain lifecycle recovery:

- scope candidates: `1`
- rows written: `0`
- skipped: `no_closed_positions_in_scope = 1`

Lifecycle labeler:

- lifecycle rows: `0`
- buy-quality denominator rows: `0`
- `phase_f_label_status = not_accepted`

Feature availability:

- `feature_availability_status = lifecycle_only`
- decision rows scanned: `45`
- buy-quality denominator rows: `0`
- temporal split possible: `false`

Interpretation:

The active shadow artifact chain produced an entry/lifecycle artifact, but there was no closed on-chain lifecycle row in scope. Therefore no buy-quality label can be used for L2D2.

## Current Problem

The blocker is no longer snapshot emission, strict replay, identity/hash, AccountNotFound attribution, or BCV2 provenance.

The current blocker is:

1. only `22/44` rows are temporal-replay-ready;
2. route executable universe is still zero in this bounded R17 sample;
3. no closed lifecycle/on-chain label rows were produced.

That blocks causal L2D2 axis replay because L2D2 needs both:

- replay-ready temporal inputs;
- executable lifecycle-labeled denominator.

## Next Step

Do not tune thresholds. Do not start Phase B/P2/live. Do not treat this R17 as policy evidence.

Recommended next path:

1. Add a small R17 replay-readiness audit that explains why `gatekeeper_v2_replay_ready_temporal=false` for 22 rows and breaks down missing target snapshots / drift classes.
2. If the missing readiness is expected for early terminal/no-data rows, define an L2D2 eligibility rule:
   - terminal snapshot required;
   - target snapshots required only when elapsed/deadline made them reachable.
3. Run the next bounded R17 only after that audit can distinguish:
   - valid terminal-only rows;
   - incomplete temporal rows;
   - executable lifecycle-labeled rows.
4. L2D2 can start only after a manifest shows:
   - baseline replay parity possible;
   - route executable rows > 0;
   - lifecycle/buy-quality labels > 0.

Final decision:

`R17_REPLAY_READY_RUNTIME_CONTRACT_PASS_BUT_L2D2_BLOCKED_BY_EXECUTABLE_LIFECYCLE_DENOMINATOR`.
