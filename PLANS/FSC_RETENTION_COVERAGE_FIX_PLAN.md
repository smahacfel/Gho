# FSC_RETENTION_COVERAGE_FIX_PLAN

Generated: 2026-06-13

## Purpose

Fix FSC attribution coverage by increasing real retained funding history and
adding diagnostics that prove whether retention/capacity is the bottleneck.

This is not BUY validation, not policy promotion, and not an edge proposal.
FSC remains telemetry/evidence-only in the current selector sampler profile.

## Non-Goals

- no Gatekeeper policy change
- no execution change
- no send-path change
- no FSC veto
- no FSC soft score
- no treating missing FSC as zero
- no marking degraded FSC evidence as clean
- no removal of degraded or miss reasons by force

## R26 Evidence

R26 proved the full-chain funding lane was mechanically connected and fail-closed
after a provider hiccup, but attribution coverage was too low for policy use.

Observed R26 final state:

```text
fsc_authoritative_funding_stream_available = 1
fsc_warmup_ready = 1
fsc_coverage_window_ready = 1
fsc_authoritative_buy_gate_open = 1

fsc_index_entries = 12,578
fsc_index_global_evictions_total = 476,033
fsc_index_per_recipient_overflows_total = 553,567

fsc_lookup_hit_rate = 0.05283362613052248
fsc_lookup_hits_total = 812
fsc_lookup_misses_total = 14,557
```

Decision-level closed artifact:

```text
decisions = 2,907
FSC clean = 36
FSC degraded = 2,207
FSC unavailable = 664
funding_source_concentration non-null = 36
```

BUY-level closed artifact:

```text
BUY rows = 354
FSC clean = 6
FSC degraded = 348
funding_source_concentration non-null = 6
```

Dominant miss/degraded evidence:

```text
BUY-level degraded: FSC_INSUFFICIENT_KNOWN_SOURCES = 335
BUY-level miss: FSC_NO_RETAINED_RECIPIENT_HISTORY = 5,677
Prometheus miss: FSC_NO_RETAINED_RECIPIENT_HISTORY = 13,701
Prometheus miss: FSC_GLOBAL_RECIPIENT_EVICTED = 334
Prometheus miss: FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 189
Prometheus miss: FSC_RELATIVE_FUNDING_TOO_SMALL = 166
Prometheus miss: FSC_ABS_ATTRIBUTION_TOO_SMALL = 124
```

Interpretation:

```text
R26_FSC_CANARY_WIRING_PASS
R26_FSC_FAIL_CLOSED_PASS
R26_FSC_POLICY_SIGNAL_NOT_READY
R26_FSC_COVERAGE_LOW
```

## Active FSC Knobs

Runtime config is built by `FundingSourceConfig::from_configs`.

FSC v2 TOML fields:

```text
[fsc_v2]
lookback_window_s
warmup_window_s
min_abs_store_lamports
min_abs_attribution_lamports
min_rel_to_buy
min_attribution_confidence
min_total_buyers
min_known_non_neutral_buyers
min_known_coverage
min_non_neutral_known_coverage
same_slot_cross_signature_policy
include_wsol
include_spl
neutral_funder_set_path
neutral_funder_set_version
```

Gatekeeper V2 TOML fields used by FSC capacity/fallback:

```text
[gatekeeper_v2]
funding_lookback_window_s
funding_dust_threshold_lamports
fsc_per_recipient_cap
fsc_global_recipient_cap
fsc_require_coverage_window_for_actionability
neutral_funding_sources
```

Effective R26/R26B capacity fields:

```text
per_recipient_cap = gatekeeper_v2.fsc_per_recipient_cap
global_recipient_cap = gatekeeper_v2.fsc_global_recipient_cap
lookback_window_ms = fsc_v2.lookback_window_s * 1000 when fsc_v2 is present
min_abs_store_lamports = fsc_v2.min_abs_store_lamports when fsc_v2 is present
min_abs_attribution_lamports = fsc_v2.min_abs_attribution_lamports when fsc_v2 is present
min_rel_to_buy = fsc_v2.min_rel_to_buy when fsc_v2 is present
```

## Active Code Paths

Retention/index:

```text
ghost-launcher/src/tx_intelligence/funding_source.rs
FundingSourceIndex::observe_transfer
FundingSourceIndex::compute_for_transactions_at
prune_transfer_history
prune_global_locked
lookup_source_for_buy
lookup_source_for_wallet
```

Decision export/fail-closed:

```text
ghost-core/src/features/coordination/metrics.rs
funding_source_concentration_from_fsc_v2
```

Metrics:

```text
ghost-launcher/src/oracle_metrics.rs
```

## Root Cause Hypothesis

R26 coverage is primarily capacity/retention limited:

- `fsc_index_entries` was already near the configured 13,000 global cap.
- `fsc_index_global_evictions_total` reached 476,033.
- `fsc_index_per_recipient_overflows_total` reached 553,567.
- the dominant miss reason was retained-history absence, not threshold filtering.

Secondary bottlenecks exist, but should not be tuned first:

- thresholds: `FSC_RELATIVE_FUNDING_TOO_SMALL` and `FSC_ABS_ATTRIBUTION_TOO_SMALL`
  are visible but far smaller than retained-history misses.
- same-slot ordering: visible, but smaller than retained-history misses.
- provider hiccup: fail-closed/recovery worked, but it prevents policy-readiness.
- TTL/window: possible secondary issue, but R26 did not prove it is first-order.

## Implemented Fix

Config-only retention delta:

```text
fsc_global_recipient_cap: 13,000 -> 50,000
fsc_per_recipient_cap: 128 -> 256
lookback_window_s: unchanged at 300
warmup_window_s: unchanged at 300
min_abs_store_lamports: unchanged at 1,000,000
min_abs_attribution_lamports: unchanged at 10,000,000
min_rel_to_buy: unchanged at 0.20
policy: unchanged, telemetry-only
```

Reason:

- isolate retention/capacity from thresholds and policy
- target the dominant `FSC_NO_RETAINED_RECIPIENT_HISTORY` miss first
- preserve fail-closed behavior and avoid converting coverage work into edge work

## Diagnostics Added

New Prometheus metrics:

```text
fsc_index_global_cap_evictions_total
fsc_index_window_prunes_total
fsc_index_lookup_empty_prunes_total
fsc_index_global_recipient_cap
fsc_index_per_recipient_cap
fsc_index_lookback_window_ms
fsc_index_configured_transfer_capacity
fsc_index_evicted_recipient_entries
fsc_index_estimated_memory_bytes
fsc_evidence_status_total{status="clean|degraded|unavailable"}
```

Existing metrics still used:

```text
fsc_index_entries
fsc_index_global_evictions_total
fsc_index_per_recipient_overflows_total
fsc_lookup_hits_total
fsc_lookup_misses_total
fsc_lookup_hit_rate
fsc_lookup_miss_reason_total{reason,class}
fsc_prune_duration_ms
```

Rate queries for R26B:

```text
rate(fsc_index_global_cap_evictions_total[5m])
rate(fsc_index_window_prunes_total[5m])
rate(fsc_index_lookup_empty_prunes_total[5m])
rate(fsc_index_per_recipient_overflows_total[5m])
rate(fsc_lookup_misses_total[5m])
```

## Memory And Hot-Path Risk

Worst-case configured transfer capacity:

```text
R26: 13,000 * 128 = 1,664,000 retained transfer records
R26B: 50,000 * 256 = 12,800,000 retained transfer records
```

The estimate is intentionally conservative. Actual memory depends on recipient
occupancy, average history length, string allocation, HashMap overhead, and
allocator behavior. The new `fsc_index_estimated_memory_bytes` metric is a
planning gauge, not an allocator-accurate RSS reading.

R26B must measure actual RSS/process memory separately and must not continue if
primary ingest degrades.

## Why Policy Signal Is Still Not Ready

Even after this fix, policy use remains blocked until a canary proves all of:

- lookup hit-rate improves materially
- clean decision coverage improves materially
- clean BUY coverage improves materially
- missing retained history drops materially
- stream hiccups remain fail-closed and recover
- primary ingest remains healthy
- RAM/disk/prune duration stay bounded

The change does not make FSC policy-ready by itself. It only prepares a better
retention experiment.

## Acceptance Gate For R26B

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

Failure classification if coverage does not improve:

```text
FSC_COVERAGE_NOT_CAP_LIMITED
```

Then inspect ordering, thresholds, parsing, TTL/window, or true absence of
pre-buy funding history.

## Final Verdict

```text
FSC_RETENTION_FIX_READY_FOR_CANARY
```
