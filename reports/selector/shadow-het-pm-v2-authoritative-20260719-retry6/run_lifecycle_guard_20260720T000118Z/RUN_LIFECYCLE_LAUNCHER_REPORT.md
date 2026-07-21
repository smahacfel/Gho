# Selector Lifecycle Run Launcher

- status: `FAIL_CONFIG_CONTRACT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_CONFIG_CONTRACT`
- run_state: `NOT_STARTED`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry6`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-20260719-retry6-local-dirty`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `shadow-het-pm-v2-retry6`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `FAIL`
- preflight: `None`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho_dynamic_exit_v1_pr2b/target/release/ghost-launcher`
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-20T00:00:44.968029+00:00`
- release_binary_sha256: `514dc7b454ff4bd07953e16febd6a8449e6c0c4332188f2a0c3a8b4953632971`

## Errors

- static guard failed

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
