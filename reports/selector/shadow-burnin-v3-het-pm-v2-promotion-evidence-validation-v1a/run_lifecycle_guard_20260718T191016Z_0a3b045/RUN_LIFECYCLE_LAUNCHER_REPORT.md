# Selector Lifecycle Run Launcher

- status: `FAIL_PREFLIGHT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_PREFLIGHT`
- run_state: `NOT_STARTED`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a`
- run_role: `validation`
- launch_cohort_id: `het-pm-v2-validation-v1a-20260718-0a3b045`
- config: `/tmp/gho-het-pm-v2-validation-runtime-v2/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het-pm-v2-validation-v1a-0a3b045`
- allow_zero_buy_lifecycle_proof: `False`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `FAIL`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/tmp/gho-het-pm-v2-validation-runtime-v2/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `0a3b045c09d5a1fd8e2785668298e0dd147428d0`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-18T19:17:53.712373+00:00`
- release_binary_sha256: `b5c493666e8674eda1e6c781fac05999b1fdaa561da862ea91b4e595bf12ced3`

## Errors

- preflight exit_code=1

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
