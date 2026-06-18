# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STATIC_PREFLIGHT_PASS`
- run_state: `DRY_RUN_NOT_STARTED`
- scope: `shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation.toml`
- tmux_session: `selector_dataset_r26_low_solmax_fullchain`

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
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-06-12T11:48:46.853814+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
