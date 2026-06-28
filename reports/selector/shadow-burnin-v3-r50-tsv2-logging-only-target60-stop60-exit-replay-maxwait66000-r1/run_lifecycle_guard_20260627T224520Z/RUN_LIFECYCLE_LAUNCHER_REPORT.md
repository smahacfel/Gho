# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF`
- run_state: `RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF`
- scope: `shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1`
- config: `/root/Gho-tsv2-a1-a2-clean/configs/rollout/shadow-burnin-v3-r50-tsv2-logging-only-target60-stop60-exit-replay-maxwait66000-r1.toml`
- tmux_session: `r50-tsv2-logging-only`
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

- runtime_binary: `/root/Gho-tsv2-a1-a2-clean/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `246eda632a08e35c5b1c1c9573cdfa98e39cd379`
- git_head_at_launch: `246eda632a08e35c5b1c1c9573cdfa98e39cd379`
- binary_mtime_utc: `2026-06-27T18:25:38.363426+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
