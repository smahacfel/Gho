# Selector Lifecycle Run Launcher

- status: `FAIL_EVENT_CANARY`
- claim: `SELECTOR_LIFECYCLE_RUN_START_FAIL:FAIL_EVENT_CANARY`
- run_state: `RUN_KILLED_AFTER_FAILED_CANARY`
- scope: `shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h`
- config: `/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h.local.toml`
- tmux_session: `shadow-v2-l2-f-research-codex-20260707-r5-memoryfix-12h`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `PASS`
- event_canary: `None`
- lifecycle_canary: `None`

## Runtime Binary

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `12176f9af8f041f7a2e0611c4c53330ff363cc71`
- git_head_at_launch: `12176f9af8f041f7a2e0611c4c53330ff363cc71`
- binary_mtime_utc: `2026-07-07T10:04:49.108850+00:00`

## Errors

- event canary failed

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.
