# Selector Lifecycle Run Launcher

- status: `FAIL_EVENT_CANARY`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_EVENT_CANARY`
- run_state: `RUN_KILLED_AFTER_FAILED_CANARY`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry4`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-refresh-20260719`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het_pm_v2_refresh_retry4`
- allow_zero_buy_lifecycle_proof: `False`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `PASS`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho_dynamic_exit_v1_pr2b/target/release/ghost-launcher`
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `3e2f24c339ad5beb1a102f1d461be00f982066b3`
- binary_mtime_utc: `2026-07-19T20:12:45.228490+00:00`
- release_binary_sha256: `fc489d77469852022df5121ba63a5b6b809776ff68d2d3b48cb5a061c045197d`

## Errors

- event canary failed

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
