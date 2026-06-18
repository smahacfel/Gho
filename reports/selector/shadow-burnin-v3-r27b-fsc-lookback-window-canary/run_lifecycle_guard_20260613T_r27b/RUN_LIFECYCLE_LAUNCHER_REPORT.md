# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-r27b-fsc-lookback-window-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r27b-fsc-lookback-window-canary.toml`
- tmux_session: `r27b-fsc-lookback-window-canary`

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
- binary_mtime_utc: `2026-06-13T20:38:23.604280+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
