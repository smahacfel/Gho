# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- run_state: `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`
- scope: `shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h`
- config: `/root/Gho/configs/rollout/shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h.local.toml`
- tmux_session: `shadow-v2-l2-f-research-codex-20260707-r4-compact-headroom-12h`
- allow_zero_buy_lifecycle_proof: `True`

## Gates

- storage: `PASS`
- config_contract: `PASS`
- scope_contract: `PASS`
- static_guard: `PASS`
- preflight: `PASS`
- event_canary: `None`
- lifecycle_canary: `SKIPPED_ZERO_BUY_LIFECYCLE_ALLOWED`

## Runtime Binary

- runtime_binary: `/root/Gho/target/release/ghost-launcher`
- build_release_before_start: `True`
- build_freshness_status: `PASS`
- git_head_at_build: `12176f9af8f041f7a2e0611c4c53330ff363cc71`
- git_head_at_launch: `12176f9af8f041f7a2e0611c4c53330ff363cc71`
- binary_mtime_utc: `2026-07-07T01:13:04.109531+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.

This run used zero-BUY lifecycle allowance.
PASS means event-ingest proof only; it does not claim classic BUY lifecycle proof.
