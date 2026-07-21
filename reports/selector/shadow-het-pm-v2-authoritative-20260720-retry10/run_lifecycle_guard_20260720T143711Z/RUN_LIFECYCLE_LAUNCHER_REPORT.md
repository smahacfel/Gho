# Selector Lifecycle Run Launcher

- status: `PASS`
- claim: `SELECTOR_EVENT_CANARY_RUN_STARTED_ZERO_BUY_LIFECYCLE_ALLOWED`
- run_state: `RUN_LEFT_RUNNING_AFTER_EVENT_CANARY_ZERO_BUY_LIFECYCLE_ALLOWED`
- scope: `shadow-het-pm-v2-authoritative-20260720-retry10`
- run_role: `calibration`
- launch_cohort_id: `shadow-het-pm-v2-authoritative-20260720-retry10-local-dirty-1`
- config: `/root/Gho_dynamic_exit_v1_pr2b/configs/rollout/shadow-burnin-v3-het-pm-v2-promotion-evidence-validation-r2a.toml`
- tmux_session: `shadow-het-pm-v2-retry10`
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

- runtime_binary: `/root/Gho_dynamic_exit_v1_pr2b/target/release/ghost-launcher`
- build_release_before_start: `False`
- build_freshness_status: `NOT_REQUESTED`
- git_head_at_build: `None`
- git_head_at_launch: `3e2f24c339ad5beb1a102f1d461be00f982066b3`
- binary_mtime_utc: `2026-07-20T14:36:39.399314+00:00`
- release_binary_sha256: `e0b6946cdded0dc5ec1782bedbc3439a9dd951282a5d662c6f68c2757ad08a5b`

## Procedure

A selector lifecycle run is valid only after this launcher writes PASS.
Manual tmux starts are not accepted for lifecycle-capable selector runs.

This run used zero-BUY lifecycle allowance.
PASS means event-ingest proof only; it does not claim classic BUY lifecycle proof.
