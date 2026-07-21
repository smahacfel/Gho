# Selector Lifecycle Run Launcher

- status: `FAIL_CONFIG_CONTRACT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_CONFIG_CONTRACT`
- run_state: `NOT_STARTED`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry9`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-20260720-retry9-local-dirty-1`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `shadow-het-pm-v2-retry9`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `FAIL_CONFIG_CONTRACT`
- static_guard: `None`
- preflight: `None`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho_dynamic_exit_v1_pr2b/target/release/ghost-launcher`
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-20T13:20:36.034790+00:00`
- release_binary_sha256: `d122f66c25634f4b43e17c8661cb8901324410e8a43634a63332339b73ae0a50`

## Errors

- scope shadow-het-pm-v2-authoritative-20260720-retry9 not found in config text
- shadow_buys path does not contain scope shadow-het-pm-v2-authoritative-20260720-retry9: /root/Gho_dynamic_exit_v1_pr2b/logs/shadow_run/shadow-het-pm-v2-authoritative-20260720-retry8-buys.jsonl
- shadow_entries path does not contain scope shadow-het-pm-v2-authoritative-20260720-retry9: /root/Gho_dynamic_exit_v1_pr2b/logs/shadow_run/shadow-het-pm-v2-authoritative-20260720-retry8/shadow_entries.jsonl
- shadow_lifecycle path does not contain scope shadow-het-pm-v2-authoritative-20260720-retry9: /root/Gho_dynamic_exit_v1_pr2b/logs/shadow_run/shadow-het-pm-v2-authoritative-20260720-retry8/shadow_lifecycle.jsonl
- system_log path does not contain scope shadow-het-pm-v2-authoritative-20260720-retry9: /root/Gho_dynamic_exit_v1_pr2b/logs/rollout/shadow-het-pm-v2-authoritative-20260720-retry8/system.log
- oracle_log path does not contain scope shadow-het-pm-v2-authoritative-20260720-retry9: /root/Gho_dynamic_exit_v1_pr2b/logs/rollout/shadow-het-pm-v2-authoritative-20260720-retry8/oracle.log

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
