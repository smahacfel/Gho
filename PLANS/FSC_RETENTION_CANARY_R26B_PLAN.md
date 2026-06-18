# FSC_RETENTION_CANARY_R26B_PLAN

Generated: 2026-06-13

## Purpose

Run `R26B_FSC_RETENTION_CANARY` only after separate approval to verify whether
larger FSC retention/capacity improves real attribution coverage.

This is data collection only. It is not BUY validation.

## Do Not Start Without Approval

Current state:

```text
R26B config prepared
R26B not started
no tmux session started by this plan
```

Required start condition:

```text
explicit user approval
bounded runtime timeout
launcher script only
fresh release build through launcher
disk preflight passes
shadow lifecycle guard passes
```

Manual tmux starts are not accepted.

## Scope

Canary scope:

```text
shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
```

Tmux session:

```text
selector_r26b_fsc_retention_canary
```

Config path:

```text
configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml
```

Brain config path used by the canary:

```text
configs/rollout/ghost_brain_selector_dataset_sampler.toml
```

Funding lane:

```text
funding_lane_mode = "full_chain"
```

Expected ingest shape:

```text
raw Yellowstone gRPC global stream: 1
full-chain funding lane: 1
NLN decoded/program streams: max 2
total expected ingest sources: 4 / 4
```

Policy/execution shape:

```text
entry_mode = "shadow_only"
execution_mode = "shadow"
FSC decision_enabled = false
FSC hard_reject_enabled = false
Sybil combo veto disabled
FSC soft penalties = 0
```

## Retention Delta Under Test

```text
fsc_global_recipient_cap: 13,000 -> 50,000
fsc_per_recipient_cap: 128 -> 256
lookback_window_s: unchanged at 300
warmup_window_s: unchanged at 300
min_abs_store_lamports: unchanged at 1,000,000
min_abs_attribution_lamports: unchanged at 10,000,000
min_rel_to_buy: unchanged at 0.20
same_slot_cross_signature_policy: unchanged at "require_tx_index"
```

## Preflight Gates

Do not start if any gate fails:

- no existing tmux session with the R26B session name
- no stale `ghost-launcher` process for this scope
- no stale launcher process for this scope
- env vars for gRPC/NLN/RPC endpoints are present
- disk free space passes launcher guard
- port `9128` is available or config is adjusted before approval
- current branch/worktree state is intentionally accepted
- launcher dry/static preflight passes

Disk warning:

```text
R26 post-stop disk was about 88% used with about 19 GB free.
scripts/start_selector_lifecycle_run.py defaults to --min-free-gb 35.
R26B should not lower that guard to hide disk pressure.
Clean/archive old logs or explicitly approve a lower bounded guard before start.
```

## Approved-Only Launcher Command

Do not run this command until separately approved.

```bash
python3 scripts/start_selector_lifecycle_run.py \
  --scope shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary \
  --config configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml \
  --tmux-session selector_r26b_fsc_retention_canary \
  --runtime-timeout-seconds 7200
```

Use the launcher defaults for lifecycle guard unless an explicit later decision
changes the runtime or disk budget.

## Live Monitoring During Canary

Scrape metrics periodically:

```text
fsc_authoritative_funding_stream_available
fsc_warmup_ready
fsc_coverage_window_ready
fsc_authoritative_buy_gate_open
fsc_index_entries
fsc_index_global_recipient_cap
fsc_index_per_recipient_cap
fsc_index_lookback_window_ms
fsc_index_configured_transfer_capacity
fsc_index_estimated_memory_bytes
fsc_lookup_hit_rate
fsc_lookup_hits_total
fsc_lookup_misses_total
fsc_lookup_miss_reason_total
fsc_index_global_evictions_total
fsc_index_global_cap_evictions_total
fsc_index_window_prunes_total
fsc_index_lookup_empty_prunes_total
fsc_index_per_recipient_overflows_total
fsc_evidence_status_total
fsc_prune_duration_ms
eventbus_lag_total
```

Also snapshot:

```text
df -h
process RSS / memory
runtime.log errors/reconnects
decision row count
BUY row count
FSC clean/degraded/unavailable counts
funding_source_concentration non-null counts
top degraded reasons
top miss reasons
```

## Stop Conditions

Stop controlled, not brutal, if any occur:

- ResourceExhausted
- reconnect storm
- provider stream-limit rejection
- primary `grpc_global_stream` degradation
- eventbus lag grows materially
- `fsc_prune_duration_ms` p95/p99 becomes unsafe
- process RSS exceeds explicit budget
- disk free space approaches guard floor
- decision logging stalls
- lifecycle proof fails
- runtime timeout reached

## Acceptance Criteria

Minimal PASS:

```text
lookup_hit_rate >= 15%
clean decision FSC coverage >= 10%
clean BUY FSC coverage >= 10%
FSC_NO_RETAINED_RECIPIENT_HISTORY drops materially
primary grpc_global_stream has no degradation
no ResourceExhausted
no reconnect storm
RAM/disk controlled
```

Strong PASS:

```text
lookup_hit_rate >= 25%
clean decision FSC coverage >= 20%
clean BUY FSC coverage >= 20%
evictions/overflows do not explode
```

Fail classifications:

```text
FSC_COVERAGE_NOT_CAP_LIMITED
FSC_COVERAGE_BLOCKED_BY_MEMORY_COST
FSC_ATTRIBUTION_LOGIC_BUG_FOUND
FSC_NEEDS_INDEX_REDESIGN
```

## Required Final Snapshot If R26B Is Later Run

Collect before stop:

```text
df -h
last FSC metrics scrape
decision row count
BUY row count
FSC clean/degraded/unavailable counts
funding_source_concentration non-null counts
lookup hit/miss totals
global cap evictions
window prunes
lookup-empty prunes
per-recipient overflows
top degraded reasons
top miss reasons
runtime.log errors/reconnects
process RSS
```

After stop confirm:

```text
tmux/session/process stopped
no stale python/cargo/launcher process for the scope
final log paths
final disk usage
```

## Interpretation Rules

If coverage improves and resource health is acceptable:

```text
FSC_RETENTION_CANARY_PASS_FOR_COVERAGE
```

If lane works but coverage remains low:

```text
FSC_COVERAGE_NOT_CAP_LIMITED
```

If memory/prune/disk pressure blocks safe operation:

```text
FSC_COVERAGE_BLOCKED_BY_MEMORY_COST
```

Even on pass:

```text
not BUY validation
not policy-ready by default
no policy promotion without separate analysis
```

## Current Plan Verdict

```text
R26B_FSC_RETENTION_CANARY_PREPARED_NOT_STARTED
```
