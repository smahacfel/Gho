# Selector Lifecycle Run Launcher

- status: `INCONCLUSIVE_ENV_OR_CONFIG`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:INCONCLUSIVE_ENV_OR_CONFIG`
- run_state: `NOT_STARTED`
- scope: `shadow-het-pm-v2-authoritative-20260719-retry6`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-20260719-retry6`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `shadow-het-pm-v2-retry6`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `INCONCLUSIVE_ENV_OR_CONFIG`
- config_contract: `None`
- scope_contract: `None`
- static_guard: `None`
- preflight: `None`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho_dynamic_exit_v1_pr2b/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-19T21:00:44.865471+00:00`
- release_binary_sha256: `f617efea8b446e77dc2f97e19f990c5046dae3ee14480deeba35bccba0b2e2d5`

## Errors

- free_gb=3.85 < min_free_gb=35.00

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
