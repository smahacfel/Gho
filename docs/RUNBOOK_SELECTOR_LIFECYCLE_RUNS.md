# Selector Lifecycle Run Runbook

This runbook is the operational contract for selector dataset runs that are expected to produce full shadow lifecycle simulation evidence.

Manual `tmux new ... ghost-launcher --config ...` starts are not accepted for lifecycle-capable selector runs.

Use the launcher:

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --scope shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag \
  --config configs/rollout/shadow-burnin-v3-selector-dataset-r8-feature-rich-r2diag.toml \
  --tmux-session selector_dataset_r8_r2diag \
  --min-free-gb 35 \
  --event-canary-seconds 900 \
  --lifecycle-proof-timeout-seconds 3600 \
  --min-reporter-rows 1
```

The run is valid only after the launcher writes:

```text
reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/RUN_LIFECYCLE_LAUNCHER_REPORT.json
reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/RUN_LIFECYCLE_LAUNCHER_REPORT.md
```

with:

```text
status = PASS
claim = SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF
run_state = RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF
```

If this proof does not exist, do not use the scope for R2 checkpoints, P3F/P3G, baseline decisions, or lifecycle claims.

## Static Config Contract

Every lifecycle-capable selector run must pass this config contract before runtime starts:

```text
trigger.shadow_run.enabled = true
trigger.shadow_run.payer_strategy = "configured"
trigger.shadow_run.timeout_ms >= 5000
trigger.shadow_run.max_concurrent <= 1
execution.execution_mode = "Shadow"
execution.entry_mode = "shadow_only"
logging.level = "info"
all shadow/log artifact paths contain the current scope
```

The known invalid profile is:

```text
payer_strategy = "ephemeral"
timeout_ms = 1600
max_concurrent = 8
```

That profile can emit event/R2 artifacts, but it is not lifecycle-capable because simulation can produce `AccountNotFound` artifacts instead of valid shadow lifecycle rows.

## Event Canary

After the initial wait window, the launcher requires:

```text
NewPoolDetected_delta > 0
Candidate_delta > 0
PoolTransaction_delta > 0
DIAG_ACCOUNT_UPDATE_RELAY_delta > 0
bad_event_json_delta = 0
```

Failure stops the run and marks the launcher report as failed.

## Lifecycle Canary

Before the run can be left in the background, the launcher requires full lifecycle proof on rows appended after its baseline snapshot:

```text
shadow_buys_delta > 0
shadow_entries_delta > 0
shadow_lifecycle_delta > 0
AccountNotFound_delta = 0
unsupported_legacy_buy_layout_requires_bcv2_delta = 0
Custom(6062)_delta = 0
0x17ae_delta = 0
legacy_buy executable rows > 0
shadow_dispatch closed rows > 0
position_closed rows > 0
exit_filled rows > 0
truth_status=resolved lifecycle rows > 0
truth_source=canonical_account_state_snapshot lifecycle rows > 0
final_pnl_pct lifecycle rows > 0
close_reason in Target/StopLoss/TimeStop > 0
```

Then the canary runs:

```bash
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config <config> \
  --output <guard-output-dir>/selector_lifecycle_canary_report.jsonl \
  --outcome-summary-output <guard-output-dir>/selector_lifecycle_canary_summary.json
```

Reporter acceptance:

```text
rows_written >= 1
truth_status=resolved rows >= 1
truth_source=canonical_account_state_snapshot rows >= 1
gatekeeper_buy_context_found rows >= 1
final_pnl_pct present rows >= 1
exit_fills_total >= 1
accepted close_reason rows >= 1
```

## Selector Regression Gates

After a selector smoke that validates route materialization or BCV2 handoff
behavior, run the offline regression gate before making a closure claim:

```bash
python3 scripts/ci_assert_selector_regression_gates.py \
  --scope shadow-burnin-v3-selector-dataset-r18c-bcv2-handoff-canonicalization-smoke \
  --root /root/Gho \
  --require-attempted-equals-buy \
  --require-not-executable-zero \
  --min-attempt-coverage 0.95 \
  --json
```

For the repository fixture, use the artifact-only form so CI can validate the
contract without runtime logs:

```bash
python3 scripts/ci_assert_selector_regression_gates.py \
  --scope r18c-bcv2-handoff-regression-fixture \
  --root /root/Gho \
  --audit-json tests/fixtures/selector/r18c_bcv2_handoff_regression/audit_pass.json \
  --jsonl tests/fixtures/selector/r18c_bcv2_handoff_regression/shadow_buys.jsonl \
  --require-attempted-equals-buy \
  --require-not-executable-zero \
  --min-attempt-coverage 0.95 \
  --json
```

The gate must fail closed on:

```text
AccountNotFound > 0
LEGACY_BC_V2_TAIL_RESOLVER_FAILED > 0
missing_on_rpc_precheck for bonding_curve_v2 > 0
selected_route_kind=None for selected_fallback_route_execution_handoff > 0
primary_route_bcv2_missing fatal after final handoff > 0
UNKNOWN_UNCLASSIFIED > 0
can_unlock_execution=true > 0
not_executable_route_rows > 0
attempted_rows < ceil(buy_rows * 0.95)
```

This regression gate is offline/audit-only. It does not start runtime, does not
unlock execution, and does not change Gatekeeper, send path, provider behavior,
slippage, or simulation success tuning.

## Failure Handling

If event canary fails, the launcher kills the `tmux` session and writes `FAIL_EVENT_CANARY`.

If lifecycle proof does not appear before `--lifecycle-proof-timeout-seconds`, the launcher kills the `tmux` session and writes `FAIL_LIFECYCLE_PROOF`.

If the config contract fails, runtime is not started.

Do not repair a failed run by continuing it manually. Fix the config or code, then start a new scope through the launcher.

## Manual Inspection

To inspect a running launcher-owned session:

```bash
tmux ls
tail -n 120 reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/runtime.log
```

To re-run canary validation on an existing launcher baseline:

```bash
python3 scripts/check_selector_lifecycle_canary.py \
  --scope <scope> \
  --config <config> \
  --baseline reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/baseline_before_start.json \
  --output-dir reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/manual_recheck \
  --phase full \
  --json
```

## Non-Claims

A passing launcher proof does not claim:

```text
production readiness
live execution
market recall
Gatekeeper tuning correctness
FSC policy correctness
P3/P3G model quality
```

It claims only that this selector run started from a lifecycle-capable shadow configuration and produced new full shadow lifecycle rows with canonical reporter truth.
