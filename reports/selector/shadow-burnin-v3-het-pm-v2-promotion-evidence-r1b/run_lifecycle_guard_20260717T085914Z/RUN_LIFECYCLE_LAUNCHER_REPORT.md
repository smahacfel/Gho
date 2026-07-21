# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-r1b.toml`
- tmux_session: `het-pm-v2-promotion-r1b`
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
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `4ff01d876fbf41206f6669c836c5f3e38338800b`
- git_head_at_launch: `4ff01d876fbf41206f6669c836c5f3e38338800b`
- binary_mtime_utc: `2026-07-17T07:46:17.121568+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
