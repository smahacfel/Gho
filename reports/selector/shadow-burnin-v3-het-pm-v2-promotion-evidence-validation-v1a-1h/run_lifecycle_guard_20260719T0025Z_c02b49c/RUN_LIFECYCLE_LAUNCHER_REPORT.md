# Selector Lifecycle Run Launcher

- status: `FAIL_PREFLIGHT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_PREFLIGHT`
- run_state: `NOT_STARTED`
- scope: `shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-v1a-1h`
- run_role: `validation`
- launch_cohort_id: `het-pm-v2-validation-v1a-1h-20260719-c02b49c`
- config: `/tmp/het-pm-v2-runtime-c02b49c/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `het-pm-v2-validation-v1a-1h-c02b49c`
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

- runtime_binary: `/tmp/het-pm-v2-runtime-c02b49c/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `c02b49cd4a0a1990e2ee2cb0b78d9ef9879b3889`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-19T00:30:35.138314+00:00`
- release_binary_sha256: `e8413d8d08664192a3f836f598b0202d5b6b392999957b39cc461c492d743adc`

## Errors

- preflight exit_code=1

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
