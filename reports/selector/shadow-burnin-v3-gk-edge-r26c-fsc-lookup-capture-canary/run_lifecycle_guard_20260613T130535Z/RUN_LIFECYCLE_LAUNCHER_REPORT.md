# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STATIC_PREFLIGHT_PASS`
- run_state: `DRY_RUN_NOT_STARTED`
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
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-06-13T12:52:50.042582+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
