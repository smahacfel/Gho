# Selector Lifecycle Run Launcher

- status: `FAIL_PREFLIGHT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_PREFLIGHT`
- run_state: `NOT_STARTED`
- scope: `shadow-burnin-v3-r33-maxwait15000-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r33-maxwait15000-fsc-off-r1.toml`
- tmux_session: `gho-r33`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `FAIL`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-06-16T20:15:54.257250+00:00`

## Errors

- preflight exit_code=1

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
