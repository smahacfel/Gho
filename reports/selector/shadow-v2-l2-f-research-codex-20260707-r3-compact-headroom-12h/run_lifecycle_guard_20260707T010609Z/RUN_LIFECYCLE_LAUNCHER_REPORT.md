# Selector Lifecycle Run Launcher

- status: `FAIL_CONFIG_CONTRACT`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_CONFIG_CONTRACT`
- run_state: `NOT_STARTED`
- scope: `shadow-v2-l2-f-research-codex-20260707-r3-compact-headroom-12h`
- config: `/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r3-compact-headroom-12h.local.toml`
- tmux_session: `shadow-v2-l2-f-research-codex-20260707-r3-compact-headroom-12h`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `PASS`
- config_contract: `FAIL_CONFIG_CONTRACT`
- scope_contract: `None`
- static_guard: `None`
- preflight: `None`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `12176f9af8f041f7a2e0611c4c53330ff363cc71`
- git_head_at_launch: `None`
- binary_mtime_utc: `2026-07-07T01:13:04.109531+00:00`

## Errors

- trigger.shadow_run.max_concurrent must be <= 1 for lifecycle guard configs

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
