# Selector Lifecycle Run Launcher

- status: `INCONCLUSIVE_ENV_OR_CONFIG`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:INCONCLUSIVE_ENV_OR_CONFIG`
- run_state: `NOT_STARTED`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry4`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-refresh-20260719`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het_pm_v2_refresh_retry4`
- allow_zero_buy_lifecycle_proof: `False`

## Gates

- storage: `INCONCLUSIVE_ENV_OR_CONFIG`
- config_contract: `None`
- scope_contract: `None`
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
- binary_mtime_utc: `2026-07-19T20:12:45.228490+00:00`
- release_binary_sha256: `fc489d77469852022df5121ba63a5b6b809776ff68d2d3b48cb5a061c045197d`

## Errors

- free_gb=0.00 < min_free_gb=35.00

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
