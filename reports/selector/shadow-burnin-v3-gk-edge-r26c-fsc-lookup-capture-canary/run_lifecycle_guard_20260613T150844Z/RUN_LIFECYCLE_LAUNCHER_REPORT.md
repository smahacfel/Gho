# Selector Lifecycle Run Launcher

- status: `FAIL_LIFECYCLE_PROOF`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_LIFECYCLE_PROOF`
- run_state: `RUN_KILLED_AFTER_FAILED_CANARY`
- scope: `shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26c-fsc-lookup-capture-canary.toml`
- tmux_session: `selector_r26c_fsc_lookup_capture_canary`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `PASS`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `4a4e6e44343bd3c0abfae654fea1f2a620aa2d86`
- git_head_at_launch: `4a4e6e44343bd3c0abfae654fea1f2a620aa2d86`
- binary_mtime_utc: `2026-06-13T15:07:37.289833+00:00`

## Errors

- lifecycle proof timeout expired

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
