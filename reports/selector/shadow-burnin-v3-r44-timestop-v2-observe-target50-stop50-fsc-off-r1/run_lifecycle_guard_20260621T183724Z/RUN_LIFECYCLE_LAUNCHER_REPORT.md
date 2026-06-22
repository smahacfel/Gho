# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- run_state: `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`
- scope: `shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1`
- config: `/root/Gho/configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- tmux_session: `r44-timestop-v2-observe`
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
- git_head_at_build: `bbe06d4e2bfb083375c0ffb4f44cda96f1baeddf`
- git_head_at_launch: `bbe06d4e2bfb083375c0ffb4f44cda96f1baeddf`
- binary_mtime_utc: `2026-06-21T18:33:03.956968+00:00`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.

This run used zero-BUY lifecycle allowance.
PASS means event-ingest proof only; it does not claim classic BUY lifecycle proof.
