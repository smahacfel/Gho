# RAPORT P3.7-X2S - Working Builder Parity Shadow Simulation Smoke - 2026-05-25

## Status

Decision: PASS-B

Meaning: working builder parity path is wired and coherent, but the final working builder manifest is not account-ready. The next step is a targeted account-source fix on the working builder path, not route discovery and not legacy fallback.

This is not PASS-A because `working_builder_manifest_ready_rows = 0`, `successful_probe_entry_rows = 0`, and `lifecycle_eligible_rows = 0`.

This is not FAIL because:

- `legacy_fallback_attempted_rows = 0`
- `selected_route_handoff_mismatch_rows = 0`
- post-simulation `AccountNotFound` rows = 0
- rpc/sender manifest hashes were emitted for every working-builder probe transport row
- no live Sender initialization or submission was observed

## Runtime

- command: `timeout 30m cargo run -p ghost-launcher --bin ghost-launcher -- --config configs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke.local.toml`
- env source: `GHOST_ENV_FILE=/root/Gho/.env`
- config: `configs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke.local.toml`
- start: `2026-05-25T14:59:51Z`
- end: `2026-05-25T15:29:51Z`
- process status after check: not running

Preflight before runtime passed:

- `execution_mode = Shadow`
- `entry_mode = shadow_only`
- gRPC app probe OK
- trigger RPC probe OK
- trigger balance OK
- no `trigger.live_sender` preflight was run

## Artifacts

- console log: `logs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/x2s_background_console.log`
- system log: `logs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/system.log.2026-05-25`
- oracle log: `logs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/oracle.log.2026-05-25`
- decisions: `logs/rollout/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/decisions/seer_runtime_coverage_audit.jsonl`
- active shadow entries: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/shadow_entries.jsonl`
- active shadow lifecycle: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/shadow_lifecycle.jsonl`
- probe selection: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/probe_selection.jsonl`
- probe transport: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/probe_transport.jsonl`
- probe skips: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/probe_skips.jsonl`
- offline audit JSON: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/v3_p37_x2s_join_key_audit.json`
- offline audit MD: `logs/shadow_run/shadow-burnin-v3-p37-x2s-working-builder-parity-shadow-smoke/v3_p37_x2s_join_key_audit.md`

## Required X2S Fields

| Field | Value |
|---|---:|
| `working_builder_parity_rows` | 55 probe / 75 active shadow |
| `working_builder_request_built_rows` | 55 probe / 75 active shadow |
| `working_builder_buy_variant_counts` | `{"routed_exact_sol_in": 55}` probe |
| `working_builder_rpc_manifest_hash_coverage` | 55/55 probe transport |
| `working_builder_sender_manifest_hash_coverage` | 55/55 probe transport |
| `working_builder_manifest_contains_bcv2_rows` | 55 probe / 75 active shadow |
| `working_builder_manifest_ready_rows` | 0 probe / 0 active shadow |
| `working_builder_manifest_missing_required_rows` | 55 probe / 75 active shadow |
| `legacy_fallback_attempted_rows` | 0 probe / 0 active shadow |
| `selected_route_handoff_mismatch_rows` | 0 probe / 0 active shadow |
| `successful_entry_rows` | 25 active shadow entry artifacts |
| `successful_probe_entry_rows` | 0 |
| `active_shadow_successful_entry_rows` | 25 |
| `lifecycle_eligible_rows` | 0 |
| `post_simulation_account_not_found_rows` | 0 probe / 0 active shadow |

Additional run coverage:

- decision rows: 735
- probe selections: 56
- probe transport rows: 55
- probe entry rows: 0
- probe lifecycle rows: 0
- probe skips: 654
- active shadow entries: 25
- active shadow lifecycle rows: 25

## Working Builder Evidence

Every probe transport row used working builder parity mode:

- `working_builder_parity_mode = "working_builder_parity"`: 55/55
- `working_builder_request_built = true`: 55/55
- `working_builder_buy_variant = "routed_exact_sol_in"`: 55/55
- `working_builder_manifest_contains_bcv2 = true`: 55/55
- rpc manifest hash present: 55/55
- sender manifest hash present: 55/55

Active shadow rows show the same working-builder family:

- `working_builder_parity_mode = "working_builder_parity"`: 25/25 in `shadow_entries.jsonl`
- `working_builder_request_built = true`: 25/25 in `shadow_entries.jsonl`
- `working_builder_buy_variant = "routed_exact_sol_in"`: 25/25 in `shadow_entries.jsonl`
- rpc manifest hash present: 25/25 in `shadow_entries.jsonl`
- sender manifest hash present: 25/25 in `shadow_entries.jsonl`

## Blocker

The remaining blocker is concrete final working builder manifest readiness.

Observed from runtime errors:

- `working_builder_final_manifest_missing_required_account:bonding_curve_v2:*:observed_tx_account_meta`
- `working_builder_final_manifest_missing_required_account:creator_vault:*:route_builder`

Counts from console log:

- total missing required account observations: 80
- `bonding_curve_v2`: 66
- `creator_vault`: 14

Offline audit corroborates:

- probe `bonding_curve_v2_rpc_load_status_counts = {"missing_on_rpc_precheck": 55}`
- active shadow `bonding_curve_v2_rpc_load_status_counts = {"missing_on_rpc_precheck": 50}`
- probe `builder_required_curve_account_ready_counts = {"false": 55}`
- active shadow `builder_required_curve_account_ready_counts = {"false": 50}`
- probe skip `execution_account_readiness_role_counts = {"creator_vault": 514}`

## Safety Checks

No live Sender evidence was found:

- no `LiveTxSender: initialized`
- no `trigger.live_sender`
- no `send_transaction(`
- no `SUBMITTED`
- no live execution mode

The repeated warning `live BUY tip resolution refused legacy fallback because Sender transport is unavailable` confirms the Sender transport was unavailable and no fallback Sender path was used.

## Final Decision

PASS-B - working builder path is coherent, legacy fallback did not return, selected-route handoff mismatch did not return, and post-simulation `AccountNotFound` did not return. Runtime is blocked by concrete final working-builder account readiness:

- `bonding_curve_v2` RPC/precheck load-readiness
- `creator_vault` authoritative account source

Concrete next step:

Targeted account-source fix on the working builder path for `bonding_curve_v2` and `creator_vault`. Do not start R18 before this account-source blocker is fixed or explicitly accepted as the next diagnostic target.
