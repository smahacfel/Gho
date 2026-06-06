# ADR-0145: Selector Lifecycle Run Guard and r7 Incident Closure

Date: 2026-06-06

Status: Accepted as incident closure and new selector-run launch contract.

## Context

Ghost had two separate runtime truths that were accidentally treated too close
to each other:

1. The restored shadow lifecycle path for:

```text
configs/rollout/shadow-burnin.toml
```

2. The selector dataset / R2-only collection profile for:

```text
configs/rollout/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag.toml
```

The restore path was repaired by PR-RESTORE and protected by PR-GUARD. That
guard proved the original restore contract:

```text
legacy_buy + observed remaining accounts count=2
configured payer
DirectBuyBuilder legacy remaining accounts
shadow dispatch
shadow_entries / shadow_lifecycle
shadow_onchain_lifecycle_report rows_written > 0
truth_status=resolved
```

However, the r7 selector profile was not the same operational profile. It was a
feature-rich R2 dataset run, and it retained a sampler-style shadow-run config:

```toml
[trigger.shadow_run]
payer_strategy = "ephemeral"
timeout_ms = 1600
max_concurrent = 8
```

That configuration can emit feature/R2 artifacts, but it is not suitable for
full lifecycle-capable shadow simulation. The restore guard did not catch this
because it guarded `shadow-burnin.toml`, not all selector dataset rollout
profiles.

## Incident

The r7 run:

```text
shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag
```

was originally started to expand the feature-rich R2 denominator after P3F/P3G.
The early r7 checks showed:

```text
feature event source: active
PoolTransaction artifacts: active
DIAG_ACCOUNT_UPDATE_RELAY: active
canonical R2 source: active
R2 denominator: growing
```

Those checks were valid for the R2/data-source objective. They were not
sufficient proof that the same scope was producing valid full shadow lifecycle
simulation.

When the shadow run artifacts were later inspected, the buy log showed broad
simulation pollution:

```text
logs/shadow_run/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag-buys.jsonl
```

The old r7 rows before the repair smoke had:

```text
buys rows      = 1495
entries rows   = 1495
lifecycle rows = 1495
```

Dominant failures included:

```text
AccountNotFound
no_executable_route_account_set:legacy_buy_missing_buyback_remaining_accounts
no_executable_route_account_set:primary_route_bcv2_missing
no_executable_route_account_set:legacy_buy_simulation_load_not_ready
shadow RPC simulate timed out after 1600ms
```

The most damaging class was `AccountNotFound`. It meant the selector profile was
not reliably lifecycle-capable even though it was useful for R2 event and DIAG
collection.

## Root Cause

The root cause was configuration drift between the restored lifecycle path and
the r7 selector path.

The r7 selector profile used:

```toml
payer_strategy = "ephemeral"
timeout_ms = 1600
max_concurrent = 8
```

For lifecycle-capable shadow simulation, this is unsafe:

- an ephemeral payer is not a chain-visible funded account;
- Solana simulation can require the payer account to exist in provider state;
- the result is `AccountNotFound` simulation artifacts rather than valid
  lifecycle rows;
- `timeout_ms = 1600` and `max_concurrent = 8` amplify provider fragility and
  make lifecycle closure less reliable.

The repaired restore config used the safer lifecycle profile:

```toml
payer_strategy = "configured"
timeout_ms = 5000
max_concurrent = 1
```

The earlier PR-RESTORE fixed the OracleRuntime legacy-buy route/account contract.
It did not guarantee that every later selector config would use a lifecycle-safe
shadow-run profile.

The earlier PR-GUARD protected the restore path. It did not enforce lifecycle
config contracts for:

```text
configs/rollout/shadow-burnin-v3-selector-dataset-*.toml
```

That is the guard gap closed by this ADR.

## What Was Stopped and Why

The r7 runtime/finalization flow was stopped after the lifecycle corruption was
identified.

Operational reason:

```text
Do not continue treating the r7 selector scope as lifecycle-capable while its
shadow-run lane is producing AccountNotFound / invalid simulation artifacts.
```

The r7 scope still contains useful evidence for feature/R2-source diagnostics:

```text
PoolTransaction event artifacts
Candidate/NewPoolDetected universe evidence
DIAG/account-state source
canonical R2 labels generated before stop
```

But the original r7 shadow lifecycle lane before the repair smoke must not be
used as a clean full-lifecycle dataset.

The r7 state after incident closure is:

```text
R2/event source proof: usable with normal caveats
old r7 shadow lifecycle artifacts before repair: contaminated by AccountNotFound
full lifecycle claim for old r7 run: not allowed
fixed r7 config short smoke: lifecycle-capable proof observed
next large denominator run: should use a new clean scope, e.g. r8
```

## Repair

The r7 config was updated:

```text
configs/rollout/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag.toml
```

Changed:

```toml
[trigger.shadow_run]
payer_strategy = "configured"
timeout_ms = 5000
max_concurrent = 1
```

The config now also documents the reason:

```text
Lifecycle-capable shadow simulation must use the configured rollout wallet.
Ephemeral payer accounts are not chain-visible and produce AccountNotFound
simulation artifacts instead of valid lifecycle rows.
```

No Gatekeeper thresholds, FSC policy, R2 labeler, Seer parser, DirectBuyBuilder,
or runtime Rust execution path was changed as part of this config repair.

## Repair Smoke Evidence

After the config repair, a bounded smoke was run against the r7 profile and only
new rows appended after the old baseline were counted.

Old baseline:

```text
shadow buys      = 1495
shadow entries   = 1495
shadow lifecycle = 1495
```

New delta after the repaired config:

```text
buys delta      = 12
entries delta   = 12
lifecycle delta = 17
```

Bad markers in the delta:

```text
AccountNotFound_delta = 0
unsupported_legacy_buy_layout_requires_bcv2_delta = 0
```

Lifecycle proof in the delta:

```text
selected_route_kind=legacy_buy executable rows = 3
shadow_dispatch closed rows = 4
exit_filled rows = 2
position_closed rows = 2
truth_status=resolved lifecycle rows = 4
truth_source=canonical_account_state_snapshot lifecycle rows = 4
close_reason TimeStop = 1
close_reason Target = 1
```

Reporter proof:

```text
python3 scripts/shadow_onchain_lifecycle_report.py \
  --config configs/rollout/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag.toml \
  --output /tmp/r7_lifecycle_repair_report.jsonl \
  --outcome-summary-output /tmp/r7_lifecycle_repair_summary.json
```

Reporter result:

```text
rows_written = 2
close_truth_coverage = 2/2
```

Representative rows:

```text
candidate_id = 57kSv2FeSEM68jRXkNaLBsgLXRCkYusXFcuhypbpump_2ob4vLXXdAEvfVhpmgTB41ZhBCscn5vNTbtpFWtKJL8a_1780710945972
close_reason = TimeStop
truth_source = canonical_account_state_snapshot
final_pnl_pct = -5.797014285714285
fills = 1
```

```text
candidate_id = 4AAbt4Jt89u43SsexqfyG9EnxSSyia9uuxbnqNshpump_CDK7NYySzL88ZKaiHjAPvPPVkAfzSrrHwWKZTNeRB1nb_1780711036741
close_reason = Target
truth_source = canonical_account_state_snapshot
final_pnl_pct = 49.60630000000001
fills = 1
```

This proves the repaired config can produce new full lifecycle rows. It does not
retroactively sanitize the old r7 rows produced under the ephemeral-payer
profile.

## New Guardrail: Static Restore/Selector Config Contract

`scripts/guard_restore_shadow_lifecycle.py` now contains a static shadow-run
config contract.

For any config with:

```toml
[trigger.shadow_run]
enabled = true
```

the guard requires:

```text
trigger.shadow_run.payer_strategy = configured
trigger.shadow_run.timeout_ms >= 5000
trigger.shadow_run.max_concurrent <= 1
```

The guard now treats selector dataset rollout configs as critical files:

```text
configs/rollout/shadow-burnin-v3-selector-dataset-*.toml
```

It also treats the selector lifecycle canary and launcher scripts as critical:

```text
scripts/check_selector_lifecycle_canary.py
scripts/start_selector_lifecycle_run.py
```

Tests added:

```text
test_shadow_run_config_contract_blocks_ephemeral_lifecycle_profile
test_shadow_run_config_contract_allows_configured_lifecycle_profile
test_selector_dataset_config_changed_requires_guard
```

## New Guardrail: Selector Lifecycle Canary

The new script:

```text
scripts/check_selector_lifecycle_canary.py
```

validates event and lifecycle deltas from a baseline snapshot. It has two major
phases.

Event canary requires:

```text
NewPoolDetected_delta > 0
Candidate_delta > 0
PoolTransaction_delta > 0
DIAG_ACCOUNT_UPDATE_RELAY_delta > 0
bad_event_json_delta = 0
```

Lifecycle canary requires:

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

The canary scans the full runtime delta:

```text
shadow buys delta
shadow_entries delta
shadow_lifecycle delta
appended system/oracle logs
```

This is important because the r7 incident was first visible in the buy log, not
only in reporter output.

The canary also runs the canonical lifecycle reporter and requires:

```text
rows_written >= 1
truth_status=resolved rows >= 1
truth_source=canonical_account_state_snapshot rows >= 1
gatekeeper_buy_context_found rows >= 1
final_pnl_pct present rows >= 1
exit_fills_total >= 1
accepted close_reason rows >= 1
```

Tests added:

```text
test_event_canary_requires_feature_events_and_diag
test_event_canary_fails_without_diag
test_lifecycle_canary_passes_full_lifecycle_delta
test_lifecycle_canary_fails_account_not_found_delta
test_lifecycle_canary_fails_account_not_found_from_full_delta_markers
test_event_kind_ignores_non_scalar_type_field
```

## New Guardrail: Launcher-Only Run Procedure

The new script:

```text
scripts/start_selector_lifecycle_run.py
```

is the required launcher for lifecycle-capable selector runs.

Manual starts like:

```text
tmux new -d ... ghost-launcher --config ...
```

are no longer accepted for lifecycle-capable selector runs.

The launcher does:

1. Storage gate.
2. Static shadow-run config contract.
3. Scope/path residue check.
4. Static restore/selector guard.
5. `ghost-launcher --preflight`.
6. Baseline snapshot.
7. `tmux` runtime start.
8. Event canary.
9. Lifecycle proof loop.
10. Failure handling.

If event canary fails, the launcher kills the tmux session.

If lifecycle proof does not appear before the configured timeout, the launcher
kills the tmux session.

If lifecycle proof passes, the launcher leaves the runtime in the background and
writes:

```text
claim = SELECTOR_LIFECYCLE_RUN_STARTED_WITH_PROOF
run_state = RUN_LEFT_RUNNING_AFTER_LIFECYCLE_PROOF
```

The proof artifacts are:

```text
reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/RUN_LIFECYCLE_LAUNCHER_REPORT.json
reports/selector/<scope>/run_lifecycle_guard_<UTC_TS>/RUN_LIFECYCLE_LAUNCHER_REPORT.md
```

Without this PASS proof, a selector scope must not be used for:

```text
R2 checkpoints
P3F/P3G baseline claims
feature-rich model decisions
full lifecycle claims
```

Tests added:

```text
test_scope_contract_requires_artifact_paths_to_match_scope
test_scope_contract_blocks_old_scope_residue
```

## Runbook

The operational procedure is documented in:

```text
docs/RUNBOOK_SELECTOR_LIFECYCLE_RUNS.md
```

The runbook states explicitly:

```text
Manual tmux starts are not accepted for lifecycle-capable selector runs.
```

Required example:

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

## Verification

Python compile check:

```text
python3 -m py_compile \
  scripts/guard_restore_shadow_lifecycle.py \
  scripts/test_guard_restore_shadow_lifecycle.py \
  scripts/check_selector_lifecycle_canary.py \
  scripts/start_selector_lifecycle_run.py \
  scripts/test_selector_lifecycle_run_guard.py
```

Unit tests:

```text
python3 -m unittest \
  scripts.test_guard_restore_shadow_lifecycle \
  scripts.test_selector_lifecycle_run_guard \
  -v
```

Result:

```text
Ran 18 tests
OK
```

Diff whitespace check:

```text
git diff --check
```

Dry-run launcher check against repaired r7 config:

```text
python3 scripts/start_selector_lifecycle_run.py \
  --scope shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag \
  --config configs/rollout/shadow-burnin-v3-selector-dataset-r7-feature-rich-r2diag.toml \
  --tmux-session dry_run_selector_dataset_r7_r2diag \
  --output-dir /tmp/selector_lifecycle_launcher_dry_run6 \
  --min-free-gb 1 \
  --skip-static-tests \
  --dry-run
```

Result:

```text
claim = SELECTOR_LIFECYCLE_RUN_STATIC_PREFLIGHT_PASS
run_state = DRY_RUN_NOT_STARTED
config_contract = PASS
scope_contract = PASS
static_guard = PASS
preflight = PASS
```

The dry-run did not start a runtime tmux session.

## Current Operating Position

Current state after the incident:

```text
PR-RESTORE OracleRuntime legacy path: still valid
restore path guard: present
r7 old lifecycle lane: contaminated by sampler shadow-run config
r7 repaired config smoke: full lifecycle proof observed
selector lifecycle launcher/canary: implemented
manual run starts: forbidden for lifecycle-capable selector runs
next clean denominator run: should be r8 or later, started only by launcher
```

The next work should not continue from the old r7 lifecycle rows as a clean
lifecycle dataset. If more feature-rich R2 denominator is needed, start a new
scope with the launcher and require lifecycle proof before leaving the run in
the background.

## Non-Goals

This ADR does not authorize:

```text
live execution
Gatekeeper threshold tuning
FSC policy changes
R2 labeler changes
P3/P3G model promotion
NLN Program Streams as R2 SSOT
treating telemetry-only PoolTransaction as executable
treating shadow simulation as live inclusion
retroactively sanitizing old r7 lifecycle rows
```

## Decision

Accept the selector lifecycle run guard as the required operational contract for
future lifecycle-capable selector runs.

Future selector runs are valid only if:

```text
started through scripts/start_selector_lifecycle_run.py
static config contract passes
event canary passes
lifecycle canary passes
canonical reporter writes resolved lifecycle rows
launcher leaves RUN_LIFECYCLE_LAUNCHER_REPORT with PASS
```

Any run lacking this proof is diagnostic-only and must not be used for lifecycle
claims or downstream baseline decisions that depend on lifecycle validity.
