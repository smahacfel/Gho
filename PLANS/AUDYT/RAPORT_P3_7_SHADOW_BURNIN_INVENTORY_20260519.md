# P3.7 Shadow-Burnin Inventory

Generated: `2026-05-19T10:18:49Z`
Repo root: `/root/Gho`
Git HEAD: `d74b3a564afce074952508b8d7694eada3693886`
Max system log scan bytes: `536870912`

## Scope

This inventory is read-only with respect to Ghost runtime artifacts. It writes only the requested JSON and Markdown reports.

It separates repo code availability, current VPS artifact availability, and external/restored artifact roots.

## Code Availability

- class: `shadow_burnin_code_present`
- missing_required_files: `none`

## Scanned Roots

- `/root/Gho` (current_vps_repo)

## Summary

- detected_runs: `23`
- artifact_availability_class_counts: `{"artifact_complete_for_shadow_runtime_only": 2, "artifact_missing": 2, "artifact_partial_transport_entry_only": 2, "artifact_primary_market_path_only": 17}`
- artifact_root_kind_counts: `{"current_vps_repo": 23}`
- current_vps_truth_report_run_count: `0`
- current_vps_shadow_onchain_lifecycle_reports: `none_found`
- external_restored_roots: `none_provided`

## Run Inventory

### root_config

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `config.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `false`
- `decision_rows`: `0`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `false`
- `oracle_log_exists`: `false`
- `diag_account_update_relay_count`: `0`
- `events_dir_exists`: `false`
- `event_file_count`: `0`
- `session_scope_detected`: `none`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_missing`
- `notes`: `no_shadow_onchain_lifecycle_report_found`

### shadow-burnin

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `121`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `895`
- `session_scope_detected`: `events:shadow-burnin:exec_launcher-1777461502217_20260429_111822_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `diag_account_update_relay_count_skipped_system_log_bytes=16254438385_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-buy-heavy

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `true`
- `entry_rows`: `4226`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `true`
- `transport_rows`: `4232`
- `decision_log_exists`: `false`
- `decision_rows`: `0`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `58`
- `session_scope_detected`: `events:shadow-burnin-buy-heavy:exec_launcher-1778067037117_20260506_113037_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_partial_transport_entry_only`
- `notes`: `diag_account_update_relay_count_skipped_system_log_bytes=1954045461_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-buy-heavy-rerun

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-buy-heavy.local.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `true`
- `entry_rows`: `7782`
- `lifecycle_log_exists`: `true`
- `lifecycle_rows`: `4776`
- `position_closed_count`: `2388`
- `exit_filled_count`: `2388`
- `transport_log_exists`: `true`
- `transport_rows`: `7813`
- `decision_log_exists`: `true`
- `decision_rows`: `5652`
- `buy_log_exists`: `true`
- `buy_rows`: `3410`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `168`
- `session_scope_detected`: `events:shadow-burnin-buy-heavy-rerun:exec_launcher-1778085162461_20260506_163242_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_complete_for_shadow_runtime_only`
- `notes`: `buy_log_files=2; decision_log_files=2; diag_account_update_relay_count_skipped_system_log_bytes=5704462358_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v25-repair

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `true`
- `entry_rows`: `6`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `true`
- `transport_rows`: `6`
- `decision_log_exists`: `true`
- `decision_rows`: `27982`
- `buy_log_exists`: `true`
- `buy_rows`: `1`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `276`
- `session_scope_detected`: `events:shadow-burnin-v25-repair:exec_launcher-1778020562308_20260505_223602_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_partial_transport_entry_only`
- `notes`: `decision_log_files=4; diag_account_update_relay_count_skipped_system_log_bytes=10519426635_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v25-repair-r2

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `629`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `28410`
- `events_dir_exists`: `true`
- `event_file_count`: `7`
- `session_scope_detected`: `events:shadow-burnin-v25-repair-r2:exec_launcher-1778754929050_20260514_103529_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v25-repair-r2.20260513T160100Z.pre-rerun

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `1925`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `74842`
- `events_dir_exists`: `true`
- `event_file_count`: `12`
- `session_scope_detected`: `events:shadow-burnin-v25-repair-r2.20260513T160100Z.pre-rerun:exec_launcher-1778684661190_20260513_150421_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p1

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `349`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `17457`
- `events_dir_exists`: `true`
- `event_file_count`: `6`
- `session_scope_detected`: `events:shadow-burnin-v3-p1:exec_launcher-1778844115667_20260515_112155_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p1.20260515T111441Z.pre-rerun

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `173`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `5272`
- `events_dir_exists`: `true`
- `event_file_count`: `5`
- `session_scope_detected`: `events:shadow-burnin-v3-p1.20260515T111441Z.pre-rerun:exec_launcher-1778770557944_20260514_145557_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `unknown`
- `entry_mode`: `unknown`
- `execution_mode`: `unknown`
- `shadow_run_enabled`: `unknown`
- `emit_event_bus`: `unknown`
- `funding_lane_mode`: `unknown`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `748`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `35488`
- `events_dir_exists`: `true`
- `event_file_count`: `6`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay:exec_launcher-1778875270153_20260515_200110_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r10-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r10-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `1023`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `49887`
- `events_dir_exists`: `true`
- `event_file_count`: `7`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r10-primary-only:exec_launcher-1778959354994_20260516_192234_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r11-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r11-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `2934`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `25`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r11-primary-only:exec_launcher-1778964930229_20260516_205530_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; diag_account_update_relay_count_skipped_system_log_bytes=828991464_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r2

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r2.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `439`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `25896`
- `events_dir_exists`: `true`
- `event_file_count`: `7`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r2:exec_launcher-1778927568090_20260516_103248_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r3

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r3.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `502`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `19642`
- `events_dir_exists`: `true`
- `event_file_count`: `6`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r3:exec_launcher-1778936615305_20260516_130335_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r4

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r4.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `24`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `809`
- `events_dir_exists`: `true`
- `event_file_count`: `2`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r4:exec_launcher-1778943004274_20260516_145004_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r5

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r5.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `259`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `11989`
- `events_dir_exists`: `true`
- `event_file_count`: `3`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r5:exec_launcher-1778948253412_20260516_161733_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r6

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r6.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `162`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `8562`
- `events_dir_exists`: `true`
- `event_file_count`: `3`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r6:exec_launcher-1778949826218_20260516_164346_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r7

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r7.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `123`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `3868`
- `events_dir_exists`: `true`
- `event_file_count`: `2`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r7:exec_launcher-1778951334124_20260516_170854_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r8

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r8.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `full_chain`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `false`
- `decision_rows`: `0`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `0`
- `events_dir_exists`: `true`
- `event_file_count`: `2`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r8:exec_launcher-1778953052110_20260516_173732_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p32-replay-r9-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p32-replay-r9-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `102`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `2933`
- `events_dir_exists`: `true`
- `event_file_count`: `2`
- `session_scope_detected`: `events:shadow-burnin-v3-p32-replay-r9-primary-only:exec_launcher-1778953293264_20260516_174133_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p36-calibrated-r12-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p36-calibrated-r12-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `false`
- `decision_rows`: `0`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `false`
- `oracle_log_exists`: `false`
- `diag_account_update_relay_count`: `0`
- `events_dir_exists`: `false`
- `event_file_count`: `0`
- `session_scope_detected`: `none`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_missing`
- `notes`: `no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p36-sample-r12-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p36-sample-r12-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `false`
- `entry_rows`: `0`
- `lifecycle_log_exists`: `false`
- `lifecycle_rows`: `0`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `false`
- `transport_rows`: `0`
- `decision_log_exists`: `true`
- `decision_rows`: `408`
- `buy_log_exists`: `false`
- `buy_rows`: `0`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `7`
- `session_scope_detected`: `events:shadow-burnin-v3-p36-sample-r12-primary-only:exec_launcher-1779018479298_20260517_114759_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_primary_market_path_only`
- `notes`: `decision_log_files=2; diag_account_update_relay_count_skipped_system_log_bytes=3125223546_limit=536870912; no_shadow_onchain_lifecycle_report_found`

### shadow-burnin-v3-p36-sample-r13-primary-only

- `artifact_root`: `/root/Gho`
- `artifact_root_kind`: `current_vps_repo`
- `config_path`: `configs/rollout/shadow-burnin-v3-p36-sample-r13-primary-only.toml`
- `entry_mode`: `shadow_only`
- `execution_mode`: `shadow`
- `shadow_run_enabled`: `true`
- `emit_event_bus`: `true`
- `funding_lane_mode`: `disabled`
- `entry_log_exists`: `true`
- `entry_rows`: `1`
- `lifecycle_log_exists`: `true`
- `lifecycle_rows`: `1`
- `position_closed_count`: `0`
- `exit_filled_count`: `0`
- `transport_log_exists`: `true`
- `transport_rows`: `1`
- `decision_log_exists`: `true`
- `decision_rows`: `17133`
- `buy_log_exists`: `true`
- `buy_rows`: `1`
- `system_log_exists`: `true`
- `oracle_log_exists`: `true`
- `diag_account_update_relay_count`: `unknown`
- `events_dir_exists`: `true`
- `event_file_count`: `154`
- `session_scope_detected`: `events:shadow-burnin-v3-p36-sample-r13-primary-only:exec_launcher-1779048005079_20260517_200005_0000`
- `truth_report_exists`: `false`
- `truth_report_rows`: `0`
- `artifact_availability_class`: `artifact_complete_for_shadow_runtime_only`
- `notes`: `decision_log_files=2; diag_account_update_relay_count_skipped_system_log_bytes=4897797694_limit=536870912; no_shadow_onchain_lifecycle_report_found`

## Acceptance Notes

- Current VPS artifacts are reported separately from external/restored roots.
- Missing historical artifacts remain explicit as missing or partial artifact classes.
- Shadow simulation, shadow-onchain report availability, and live inclusion are not conflated.
- No active policy, live sender, IWIM, or FSC behavior is changed by this inventory.
