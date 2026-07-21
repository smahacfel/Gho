# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het_pm_v2_promotion_validation_r2a`
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
- git_head_at_build: `54ab64fa8fa54ff898d3de1a3977c2aeb7e46dea`
- git_head_at_launch: `54ab64fa8fa54ff898d3de1a3977c2aeb7e46dea`
- binary_mtime_utc: `2026-07-17T15:54:01.468190+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
