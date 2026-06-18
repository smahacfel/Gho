# FSC_RETENTION_COVERAGE_AUTOPSY

Generated: 2026-06-13
Scope: `shadow-burnin-v3-gk-edge-r26-low-solmax-fullchain-validation`

## Goal

Determine why the full-chain funding lane works mechanically while FSC attribution coverage remains low.

This is a coverage/retention autopsy, not an edge proposal.

Do not use this report to enable FSC vetoes, FSC soft score, or FSC policy promotion.

## Observed State

R26 final pre-stop metrics:

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
overall known_coverage avg = 0.047311703434348984
```

BUY-level closed artifact:

```text
BUY rows = 354
FSC clean = 6
FSC degraded = 348
funding_source_concentration non-null = 6
known_coverage avg = 0.05232802646399464
```

Main BUY-level degraded reason:

```text
FSC_INSUFFICIENT_KNOWN_SOURCES = 335
```

Main BUY-level miss reason:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY = 5,677
```

## Current Retention And Capacity Knobs

Current config values:

```text
[fsc_v2]
capture_enabled = true
feature_emit_enabled = true
decision_enabled = false
hard_reject_enabled = false
lookback_window_s = 300
warmup_window_s = 300
min_abs_store_lamports = 1,000,000
min_abs_attribution_lamports = 10,000,000
min_rel_to_buy = 0.20
min_attribution_confidence = 0.60
min_total_buyers = 2
min_known_non_neutral_buyers = 2
min_known_coverage = 0.50
min_non_neutral_known_coverage = 0.30
same_slot_cross_signature_policy = "require_tx_index"
include_wsol = false
include_spl = false

[gatekeeper_v2]
funding_lookback_window_s = 180
funding_dust_threshold_lamports = 1,000,000
fsc_per_recipient_cap = 128
fsc_global_recipient_cap = 13,000
```

Current effective runtime config is built from `[fsc_v2]` where present and falls back to Gatekeeper config for capacity fields.

The important code-level knobs are:

- `FundingSourceConfig.lookback_window_ms`
- `min_abs_store_lamports`
- `min_abs_attribution_lamports`
- `min_rel_to_buy`
- `min_attribution_confidence_bps`
- `per_recipient_cap`
- `global_recipient_cap`
- `min_total_buyers`
- `min_known_non_neutral_buyers`
- `min_known_coverage`
- `min_non_neutral_known_coverage`
- `require_coverage_window_for_actionability`
- neutral funding source set

## What The Code Does

Retention/capacity:

- Each recipient has a transfer history.
- When `history.transfers.len() > per_recipient_cap`, oldest transfers are popped and `fsc_index_per_recipient_overflows_total` increments.
- Global pruning removes recipient histories when outside the lookback window or when `histories.len() > global_recipient_cap`.
- Cap-driven global pruning records evicted recipients and increments `fsc_index_global_evictions_total`.

Lookup:

- A BUY can only receive a concrete funding source if the buyer/recipient has retained pre-buy funding history.
- If the recipient was evicted by global cap, lookup reports `FSC_GLOBAL_RECIPIENT_EVICTED`.
- If no retained recipient history exists, lookup reports `FSC_NO_RETAINED_RECIPIENT_HISTORY`.
- If transfer amount is below `min_abs_attribution_lamports`, lookup reports/accumulates absolute-too-small filtering.
- If transfer amount is below `buy_amount * min_rel_to_buy`, lookup reports/accumulates relative-too-small filtering.
- If ordering within the same slot cannot be established, lookup reports `FSC_SAME_SLOT_ORDERING_UNAVAILABLE`.

Export:

- `funding_source_concentration` is exported only when FSC evidence is decision-time, clean, capture-ready, index-warm, gap-free, and has no excluded reason.
- Degraded/unavailable evidence is intentionally fail-closed and returns no canonical FSC value.

## Primary Autopsy Findings

### 1. Global recipient cap is too tight for full-chain load

Current cap:

```text
fsc_global_recipient_cap = 13,000
```

Observed:

```text
fsc_index_entries = 12,578
global evictions = 476,033
```

The index is operating at or near cap while full-chain input continuously introduces new recipients. The eviction counter is much larger than the retained population, which means recipient history churn is heavy.

This directly explains a large part of:

```text
FSC_NO_RETAINED_RECIPIENT_HISTORY
FSC_GLOBAL_RECIPIENT_EVICTED
```

Conclusion:

```text
global cap is very likely too small for this full-chain lane profile
```

### 2. Per-recipient history may also be too short

Current cap:

```text
fsc_per_recipient_cap = 128
```

Observed:

```text
per-recipient overflows = 553,567
```

This means old transfers are frequently being dropped from individual recipient histories. The current top miss reason is not `FSC_PER_RECIPIENT_HISTORY_OVERFLOW`, but high overflow pressure can still remove the precise pre-buy transfer needed for attribution.

Conclusion:

```text
per-recipient cap is plausibly too low for high-activity recipients under full-chain load
```

### 3. TTL/window may be too short or misaligned with observed buyer timing

Relevant values:

```text
fsc_v2.lookback_window_s = 300
gatekeeper_v2.funding_lookback_window_s = 180
```

The runtime evidence records `ttl_seconds = 300` in FSC evidence, so the active FSC v2 lookback appears to be 300 seconds for this canary.

Observed structural miss reasons include:

```text
FSC_NO_PREBUY_TRANSFER_IN_WINDOW = 5 in BUY diagnostics
FSC_RELATIVE_FUNDING_TOO_SMALL = 88 in BUY diagnostics
FSC_ABS_ATTRIBUTION_TOO_SMALL = 68 in BUY diagnostics
```

`FSC_NO_PREBUY_TRANSFER_IN_WINDOW` is not dominant. That suggests the main issue is not only TTL; retention capacity and recipient-history availability dominate.

Conclusion:

```text
do not change TTL first; test cap expansion first while keeping TTL constant
```

### 4. ABS/REL thresholds filter some candidates but are not the main cause

Current thresholds:

```text
min_abs_store_lamports = 1,000,000
min_abs_attribution_lamports = 10,000,000
min_rel_to_buy = 0.20
```

Observed BUY miss counts:

```text
FSC_RELATIVE_FUNDING_TOO_SMALL = 88
FSC_ABS_ATTRIBUTION_TOO_SMALL = 68
```

These are visible but far below `FSC_NO_RETAINED_RECIPIENT_HISTORY = 5,677`.

Conclusion:

```text
threshold tuning may improve marginal coverage but is not the first-order bottleneck
```

### 5. Same-slot ordering limits some coverage but is not dominant

Current policy:

```text
same_slot_cross_signature_policy = "require_tx_index"
```

Observed:

```text
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 75 in BUY diagnostics
FSC_SAME_SLOT_ORDERING_UNAVAILABLE = 189 in pre-stop Prometheus miss metrics
```

This is real, but much smaller than retained-history misses.

Conclusion:

```text
same-slot ordering is a secondary coverage limiter
```

### 6. Provider health was acceptable for wiring but not sufficient for policy-readiness

Observed provider hiccup:

```text
h2 protocol error
DataLoss: lagged
coverage gate reset to 300000 ms
coverage gate recovered after 300 seconds
```

This validates fail-closed and recovery, but it also shows FSC policy use must account for stream resets/gaps.

Conclusion:

```text
provider hiccup is not the main attribution coverage bottleneck, but it blocks any hard-policy claim
```

## RAM And Disk Cost Of Raising Caps

This was not directly measured in R26, so the numbers below are order-of-magnitude planning estimates.

Memory is the main cost. Disk is mostly affected by logs/artifacts, not by the in-memory index cap itself.

Worst-case retained transfer capacity:

```text
current: 13,000 recipients * 128 transfers = 1,664,000 retained transfer records
50,000 recipients * 128 transfers = 6,400,000 retained transfer records
100,000 recipients * 128 transfers = 12,800,000 retained transfer records
```

Approximate RAM planning envelope:

```text
current cap worst-case: hundreds of MB to low GB
50k global cap worst-case: low single-digit GB
100k global cap worst-case: several GB
```

The exact cost depends on wallet string sharing, HashMap overhead, retained transfers per recipient, and allocator behavior. R26B should measure RSS and prune latency directly instead of assuming the cap is free.

Disk warning from R26:

```text
runtime.log = 1.24 GB
NLN route evidence = 234.7 MB
decision log = 208.9 MB
post-stop disk = 88% used, 19 GB free
```

Increasing retention caps does not by itself write the full index to disk, but a longer canary will continue to grow runtime, decision, and NLN evidence logs quickly.

## Candidate R26B Retention Questions

R26B should answer coverage/capacity questions only:

1. Does increasing `fsc_global_recipient_cap` materially reduce `FSC_NO_RETAINED_RECIPIENT_HISTORY`?
2. Does increasing `fsc_per_recipient_cap` materially reduce overflow pressure and improve clean FSC rate?
3. Does lookup hit rate improve without degrading `grpc_global_stream` or decision throughput?
4. Does prune duration remain bounded?
5. Does process RSS remain within a predefined budget?
6. Does provider reconnect behavior remain comparable to R26?

## Proposed R26B Test Matrix

Do not start this without separate approval.

Recommended first retention-only variant:

```text
fsc_global_recipient_cap: 13,000 -> 50,000
fsc_per_recipient_cap: 128 -> 256
lookback_window_s: keep 300
min_abs_store_lamports: keep 1,000,000
min_abs_attribution_lamports: keep 10,000,000
min_rel_to_buy: keep 0.20
policy: unchanged, telemetry-only
```

Reason:

- Isolates retention/capacity from threshold changes.
- Targets the dominant miss class first.
- Keeps policy inert and avoids converting coverage work into an edge experiment.

Optional second variant only after first result:

```text
fsc_global_recipient_cap: 100,000
fsc_per_recipient_cap: 256 or 512
```

Only run this if R26B-50k is stable in RAM, prune duration, and primary ingest.

## Additional Instrumentation To Add Before Or During R26B

Recommended instrumentation, still telemetry-only:

- process RSS snapshot every N seconds
- FSC average recipient history length
- FSC p50/p95/p99 recipient history length
- accepted funding transfers per second
- global cap prune count by cause: TTL vs cap
- per-recipient overflow count by recipient activity bucket
- lookup misses split by "never seen recipient" vs "seen then evicted"
- prune duration p95/p99/max, not only histogram buckets
- lane gap/reconnect epoch in per-decision FSC evidence

Do not add scoring or veto logic.

## Acceptance Criteria For R26B

R26B can be considered a retention improvement only if all are true:

- primary `grpc_global_stream` remains healthy
- funding lane remains healthy or fails closed and recovers as in R26
- no provider stream-limit rejection
- decision rows continue to emit
- process RSS stays below an explicit budget
- disk stays above explicit free-space floor
- `fsc_lookup_hit_rate` improves materially from 5.28%
- `FSC_NO_RETAINED_RECIPIENT_HISTORY` share drops materially
- BUY-level clean FSC rows increase materially above 6 / 354 equivalent rate
- no Gatekeeper policy, execution, send path, FSC veto, or FSC score is enabled

## Non-Goals

- no BUY validation
- no production promotion
- no FSC hard reject
- no FSC soft score
- no Sybil combo veto
- no Gatekeeper threshold tuning
- no execution/send-path change
- no R26/R27 restart without separate approval

## Autopsy Verdict

```text
FULL_CHAIN_LANE_WORKS
FSC_ATTRIBUTION_COVERAGE_LOW
PRIMARY_BOTTLENECK_RETENTION_CAPACITY
SECONDARY_BOTTLENECKS_THRESHOLDS_AND_SAME_SLOT_ORDERING
POLICY_USE_NOT_READY
```

Next possible stage:

```text
R26B_FSC_RETENTION_CANARY
```

Start condition:

```text
separate user approval + explicit cap/retention delta + RAM/disk budget + no primary ingest degradation criteria
```
