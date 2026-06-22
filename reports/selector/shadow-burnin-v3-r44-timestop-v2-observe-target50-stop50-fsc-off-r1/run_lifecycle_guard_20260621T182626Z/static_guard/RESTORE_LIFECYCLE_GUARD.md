# Restore Shadow Lifecycle Guard

- status: `FAIL_TESTS`
- claim: `RESTORE_GUARD_FAIL:FAIL_TESTS`
- head: `bbe06d4e2bfb083375c0ffb4f44cda96f1baeddf`
- config: `configs/rollout/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1.toml`
- output_dir: `/root/Gho/reports/selector/shadow-burnin-v3-r44-timestop-v2-observe-target50-stop50-fsc-off-r1/run_lifecycle_guard_20260621T182626Z/static_guard`

## Tests

- status: `FAIL_TESTS`
- commands: `8`

## Config Contract

- status: `PASS`

## Runtime Smoke

- preflight: `SKIPPED`
- runtime: `SKIPPED`
- timeout_seconds: `600`
- exit_code: `None`

## Artifact Deltas


## Legacy Contract Matrix


## Reporter


## Errors

- targeted test failed: cargo test -q -p ghost-launcher components::trigger::shadow_run::tests::p5_precheck_failure_writes_not_dispatched_lifecycle_record

## Non-Claims

- `production_readiness`
- `live_execution`
- `market_recall`
- `Gatekeeper_tuning`
- `FSC_policy`
- `NLN_raw_capture`
