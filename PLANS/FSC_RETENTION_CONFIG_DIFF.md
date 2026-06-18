# FSC_RETENTION_CONFIG_DIFF

Generated: 2026-06-13

## Scope

This diff prepares R26B FSC retention/capacity validation only.

It does not change:

- Gatekeeper policy
- execution
- send path
- BUY/REJECT/TIMEOUT decision semantics
- FSC veto
- FSC soft score
- FSC degraded/clean status semantics

## Config Diff

File:

```text
configs/rollout/ghost_brain_selector_dataset_sampler.toml
```

Changed:

```diff
-fsc_per_recipient_cap = 128
-fsc_global_recipient_cap = 13000
+fsc_per_recipient_cap = 256
+fsc_global_recipient_cap = 50000
```

Unchanged FSC thresholds/windows:

```text
[fsc_v2]
lookback_window_s = 300
warmup_window_s = 300
min_abs_store_lamports = 1000000
min_abs_attribution_lamports = 10000000
min_rel_to_buy = 0.20
min_attribution_confidence = 0.60
min_total_buyers = 2
min_known_non_neutral_buyers = 2
min_known_coverage = 0.50
min_non_neutral_known_coverage = 0.30
same_slot_cross_signature_policy = "require_tx_index"
include_wsol = false
include_spl = false
```

Unchanged policy/effect fields:

```text
[fsc_v2]
decision_enabled = false
hard_reject_enabled = false

[gatekeeper_v2]
max_funding_source_concentration = 0.99
soft_penalty_high_fsc = 0
soft_penalty_high_fsc_high_cpv_combo = 0
enable_sybil_interference_layer = false
enable_sybil_combo_veto = false
```

## R26B Launch Config

Added:

```text
configs/rollout/shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary.toml
```

Key properties:

```text
scope/log paths: shadow-burnin-v3-gk-edge-r26b-fsc-retention-canary
ghost_brain_config_path = "../../configs/rollout/ghost_brain_selector_dataset_sampler.toml"
funding_lane_mode = "full_chain"
entry_mode = "shadow_only"
execution_mode = "shadow"
program streams max_streams = 2
grpc global stream shape unchanged
```

The config is prepared only. It has not been started.

## Code Diff

File:

```text
ghost-launcher/src/oracle_metrics.rs
```

Added FSC diagnostic metrics:

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
fsc_evidence_status_total{status}
```

Existing metrics preserved:

```text
fsc_index_global_evictions_total
fsc_index_per_recipient_overflows_total
fsc_lookup_hit_rate
fsc_lookup_miss_reason_total{reason,class}
```

File:

```text
ghost-launcher/src/tx_intelligence/funding_source.rs
```

Changed:

- `prune_global_locked` now returns split `PruneStats`.
- cap-driven global recipient removals increment
  `fsc_index_global_cap_evictions_total`.
- TTL/window recipient removals increment `fsc_index_window_prunes_total`.
- lookup cleanup after an emptied history increments
  `fsc_index_lookup_empty_prunes_total`.
- decision-time FSC evidence status increments
  `fsc_evidence_status_total{status}`.
- retention config and estimated memory gauges are refreshed on FSC paths that
  have `FundingSourceConfig`.

Preserved:

- `fsc_index_global_evictions_total` remains as a compatibility total for
  removed recipient entries.
- `funding_source_concentration` remains non-null only when FSC v2 status is
  `Clean`.
- degraded/unavailable status remains fail-closed.

## Capacity Delta

Configured retained transfer capacity:

```text
R26: 13,000 * 128 = 1,664,000 transfer slots
R26B: 50,000 * 256 = 12,800,000 transfer slots
delta: 7.69x configured transfer-slot capacity
```

The new memory gauge estimates occupancy under current recipients and configured
per-recipient cap. It is a diagnostic upper-envelope estimate, not a substitute
for process RSS.

Approximate worst-case memory planning envelope:

```text
12,800,000 transfer records * 384 bytes ~= 4.9 GB
plus recipient HashMap/string/deque overhead
```

R26B must stop if RSS, prune duration, eventbus lag, provider reconnects, or
primary ingest health deteriorate.

## R26 Baseline For Comparison

```text
fsc_lookup_hit_rate = 5.28%
clean decision FSC coverage = 36 / 2,907 = 1.24%
clean BUY FSC coverage = 6 / 354 = 1.69%
FSC_NO_RETAINED_RECIPIENT_HISTORY = 13,701 Prometheus / 5,677 BUY diagnostics
FSC_GLOBAL_RECIPIENT_EVICTED = 334 Prometheus / 132 BUY diagnostics
global evictions total = 476,033
per-recipient overflows total = 553,567
```

## Prometheus Queries For R26B

Current config/capacity:

```text
fsc_index_global_recipient_cap
fsc_index_per_recipient_cap
fsc_index_lookback_window_ms
fsc_index_configured_transfer_capacity
fsc_index_estimated_memory_bytes
```

Coverage/status:

```text
fsc_lookup_hit_rate
fsc_evidence_status_total
increase(fsc_evidence_status_total[15m])
```

Retention pressure:

```text
rate(fsc_index_global_cap_evictions_total[5m])
rate(fsc_index_window_prunes_total[5m])
rate(fsc_index_lookup_empty_prunes_total[5m])
rate(fsc_index_per_recipient_overflows_total[5m])
```

Miss distribution:

```text
fsc_lookup_miss_reason_total
increase(fsc_lookup_miss_reason_total[15m])
```

## Rollback

Rollback config only:

```text
fsc_per_recipient_cap = 128
fsc_global_recipient_cap = 13000
```

No policy/execution rollback is needed because this change does not alter those
paths.

## Verdict

```text
CONFIG_READY_FOR_R26B_RETENTION_CANARY
POLICY_UNCHANGED
EXECUTION_UNCHANGED
SEND_PATH_UNCHANGED
```
