# Selector Lifecycle Run Launcher

- status: `FAIL_CONFIG_CONTRACT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_CONFIG_CONTRACT`
- run_state: `NOT_STARTED`
- scope: `shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- tmux_session: `r44-timestop-v2-observe`
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

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `bbe06d4e2bfb083375c0ffb4f44cda96f1baeddf`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-06-21T18:33:03.956968+00:00`

## Errors

- static guard failed

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
