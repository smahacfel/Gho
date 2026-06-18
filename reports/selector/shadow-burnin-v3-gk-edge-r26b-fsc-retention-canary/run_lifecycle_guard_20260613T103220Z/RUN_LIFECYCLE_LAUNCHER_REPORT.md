# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml`
- tmux_session: `selector_r26b_fsc_retention_canary`

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
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `4a4e6e44343bd3c0abfae654fea1f2a620aa2d86`
- binary_mtime_utc: `2026-06-13T10:31:51.321435+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
