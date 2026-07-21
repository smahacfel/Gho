# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- run_role: `validation`
- launch_cohort_id: `het-pm-v2-validation-v1a-20260718-095f9d2`
- config: `/tmp/gho-het-pm-v2-validation-runtime/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het-pm-v2-validation-v1a-095f9d2`
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

- runtime_binary: `/tmp/gho-het-pm-v2-validation-runtime/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `095f9d2996d25cb408d0ff1f6e3faf53d65ae5c1`
- git_head_at_launch: `095f9d2996d25cb408d0ff1f6e3faf53d65ae5c1`
- binary_mtime_utc: `2026-07-18T15:10:27.421000+00:00`
- release_binary_sha256: `89d2dbb58be7cc291d4316698ad13cb6fc83ba22c7ccfc0db370f87515a20f40`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
