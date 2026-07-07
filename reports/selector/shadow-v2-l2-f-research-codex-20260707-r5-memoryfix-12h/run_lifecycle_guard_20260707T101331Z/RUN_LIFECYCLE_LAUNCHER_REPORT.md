# Selector Lifecycle Run Launcher

- status: `INCONCLUSIVE_ENV_OR_CONFIG`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:INCONCLUSIVE_ENV_OR_CONFIG`
- run_state: `NOT_STARTED`
- scope: `shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h`
- config: `/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h.local.toml`
- tmux_session: `shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h`
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

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-07T10:04:49.108850+00:00`

## Errors

- free_gb=29.76 < min_free_gb=35.00

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
