# ADR-0136: V3 P3.7 Counterfactual Shadow Probe Plane

Date: 2026-05-19

Status: Accepted

## Context

ADR-0134 accepted `shadow_burnin_lifecycle_onchain` as a separate P3.7 truth
dataset kind. ADR-0135 then designed the forward R14 collection lane to combine:

- `MaterializedFeatureSet` / V3 replay payload rows,
- V3 shadow telemetry,
- Gatekeeper V2/V2.5 context,
- shadow transport, entry, and lifecycle artifacts,
- shadow/on-chain lifecycle labels.

J2 and J2b exposed the remaining collection blocker:

- R14/J2 produced V3/MFS replay rows and strict full replay passed.
- Decision-side `ab_record_id` and V3 hash coverage were complete.
- Natural Gatekeeper V2/V2.5 BUY rate was too low: no active BUY and no
  shadow dispatch artifacts were observed in the sentinel window.
- J2b validated join-key propagation only at code/test harness level, not by a
  runtime shadow dispatch row.

The active BUY source remains legacy Gatekeeper V2/V2.5 long-mode plus IWIM.
V3 is still a telemetry/replay sidecar:

```text
gatekeeper_v3.enabled = false
gatekeeper_v3.promotion.enabled = false
```

Waiting for natural BUYs is now an impractical way to collect the dataset that
P3.7 needs. Loosening thresholds or changing IWIM to increase BUY rate is not
allowed because it would contaminate the active decision policy.

## Decision

Introduce a separate counterfactual shadow-only probe plane for P3.7:

```text
p37_shadow_probe
```

The probe plane may sample V3/MFS candidate rows after decision-time evidence
has been materialized and logged, then dispatch a shadow simulation/lifecycle
probe for those sampled rows.

The probe plane is not an active BUY path.

Every probe artifact must carry:

```text
dispatch_source = "counterfactual_shadow_probe"
```

Active verdicts remain unchanged. A normal active `REJECT`, `PENDING`, or
`TIMEOUT` remains exactly that. The counterfactual probe is a separate research
side effect used only to collect lifecycle labels for rows that already have
V3/MFS decision snapshots.

## Required Semantics

The following semantics are mandatory:

- The active verdict remains unchanged.
- The active Gatekeeper V2/V2.5 policy is not modified.
- IWIM policy is not modified.
- No active threshold is changed.
- No live sender path is changed.
- No P2/live path is enabled.
- `no dispatch after reject` remains normal active behavior.
- Probe artifacts must be explicitly marked with
  `dispatch_source=counterfactual_shadow_probe`.
- Probe rows must not be counted as active `BUY` decisions.
- Lifecycle outcomes are post-decision labels, not decision-time features.
- Shadow simulation is not live inclusion.
- Submit is not confirmation.
- Unknown execution status is not success.
- Speculative curve finality remains dirty/degraded, not finalized proof.

## Dataset Interpretation

Counterfactual probe rows are a new collection plane over the existing truth
kind:

```text
truth_dataset_kind = "shadow_burnin_lifecycle_onchain"
collection_plane = "counterfactual_shadow_probe"
```

They must not be merged with active BUY lifecycle rows unless reports segment
the rows by `collection_plane` and `dispatch_source`.

Allowed interpretation:

```text
This row had a V3/MFS decision snapshot and was later shadow-probed
counterfactually. Its lifecycle outcome is a post-decision label for research.
```

Forbidden interpretation:

```text
This row was an active BUY.
This row proves live inclusion.
This row proves V3 selector readiness.
This row authorizes runtime thresholds.
```

## Config Design

The launcher should gain a backward-compatible, disabled-by-default config
surface:

```toml
[p37_shadow_probe]
enabled = false
namespace = "shadow-burnin-v3-p37-counterfactual-probe-r15"
dispatch_source = "counterfactual_shadow_probe"
sample_source = "v3_mfs_decision_rows"
sample_mode = "deterministic_hash_mod"
sample_modulus = 100
sample_threshold = 5
sampling_version = "p37-j3-v1"
max_probes_per_run = 250
max_probes_per_minute = 20
max_concurrent = 2
include_verdict_types = ["REJECT", "PENDING"]
exclude_active_buy_rows = true
enable_eligibility_precheck = true
require_ab_record_id = true
require_materialized_feature_set = true
require_v3_replay_payload = true
require_v3_feature_snapshot_hash = true
require_v3_policy_config_hash = true
require_execution_route_identity = true
require_curve_account_state = true
dedupe_by_probe_id = true
emit_event_bus = false
event_bus_mode = "disabled"
probe_amount_source = "trigger_max_position_size"
probe_amount_lamports = 0
probe_slippage_bps = 2000
probe_quote_age_max_ms = 1500
probe_curve_age_max_ms = 1500
append = false
require_unique_namespace = true

selection_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15/probe_selection.jsonl"
skip_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15/probe_skips.jsonl"
transport_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15/probe_transport.jsonl"
entry_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15/probe_shadow_entries.jsonl"
lifecycle_log_path = "../../logs/shadow_run/shadow-burnin-v3-p37-counterfactual-probe-r15/probe_shadow_lifecycle.jsonl"
```

The config must preserve old config loading through serde defaults.

The probe plane may reuse the existing shadow transport adapter only when the
profile is shadow-capable:

```text
[trigger].entry_mode = "shadow_only"
[execution].execution_mode = "shadow"
[trigger.shadow_run].enabled = true
```

If those conditions are not met and `p37_shadow_probe.enabled=true`, config
validation must fail closed.

## Safety Amendments Required For P0

The probe plane is accepted only with these amendments:

1. Run a probe eligibility precheck before sampling.
2. Assign every sampled row to a `probe_bucket`.
3. Do not emit active BUY or position events for probes.
4. Isolate probe ledger and position state from active shadow/live state.
5. Log probe amount, quote, and slippage parameters.
6. Treat probe backpressure as fail-open for active decisions.
7. Protect probe namespaces and output paths from accidental overwrite.
8. Test that probes do not mutate active BUY logs, verdicts, or metrics.
9. Split minimal P0 from lifecycle-close P1.
10. Use a collision-safe `probe_id`.

## Probe Eligibility Precheck

Eligibility precheck is not active policy. It is a technical guard that prevents
the probe plane from creating meaningless shadow simulations.

Minimum required eligibility:

- valid `pool_id`,
- valid `base_mint` / `mint_id`,
- valid bonding curve or execution route identity,
- `MaterializedFeatureSet` present,
- `v3_feature_snapshot_hash` present,
- `v3_policy_config_hash` present,
- transaction/account identity available,
- curve/account state not critically unavailable,
- protocol state supported by the shadow simulator,
- unique `probe_id`.

Required eligibility skip reasons:

- `invalid_pool_identity`
- `invalid_mint_identity`
- `unsupported_protocol_state`
- `missing_bonding_curve`
- `missing_execution_route_identity`
- `missing_materialized_feature_set`
- `critical_curve_unavailable`
- `critical_account_unavailable`
- `duplicate_probe_id`

## Probe Buckets

Every selected or skipped row must have:

```text
probe_bucket
probe_bucket_reason
probe_bucket_version
```

Initial P0 buckets:

- `v3_reject_manipulation_contradiction`
- `v3_reject_low_opportunity`
- `v3_pending_wait_evidence`
- `v3_pending_wait_sample`
- `active_reject_v3_pending`
- `active_reject_v3_reject`
- `random_eligible_control`

Reports must segment selection, transport, lifecycle, and labels by
`probe_bucket`. Without bucket segmentation, probe labels are not
interpretable enough for P3.7 feature work.

## EventBus Isolation

P0 default:

```text
emit_event_bus = false
event_bus_mode = "disabled"
```

If EventBus emission is later enabled, probes must use dedicated event types
such as:

```text
CounterfactualShadowProbeRequested
CounterfactualShadowProbeCompleted
CounterfactualShadowProbeSkipped
```

Probe events must not be emitted as active BUY, trigger BUY, position-opened,
live BUY, or any event consumed by the active path.

## Probe Ledger And Position Namespace

Probe positions must be isolated from active shadow/live position state.

Requirements:

- `probe_position_id` is distinct from active `position_id`.
- Probe lifecycle is written to the probe lifecycle path.
- Probes must not increment active open position count.
- Probes must not affect `max_concurrent_positions`.
- Probes must not affect active shadow/live ledgers unless the state is
  explicitly namespaced as a probe ledger.

If the current post-buy monitor cannot isolate position state safely, P0 must
stop at selection, transport, and entry. Lifecycle monitoring then moves to P1.

## Probe Sizing And Quote Contract

Probe economics must be explicit because lifecycle PnL and impact depend on
amount and quote age.

Required config:

```text
probe_amount_source = "trigger_max_position_size" | "fixed_lamports"
probe_amount_lamports
probe_slippage_bps
probe_quote_age_max_ms
probe_curve_age_max_ms
```

Required log fields:

```text
probe_amount_lamports
probe_amount_source
probe_slippage_bps
quote_age_ms
curve_age_ms
```

## Backpressure And Path Safety

Probe backpressure must fail open for active decisions.

Queue, rate, or concurrency pressure must skip the probe and must not block the
decision pipeline.

Required skip reasons:

- `probe_queue_full`
- `probe_backpressure`
- `probe_rate_limit_exceeded`
- `probe_concurrency_limit_exceeded`

Probe namespace and output path protection:

- probe namespace must be unique per run,
- if `append=false` and output files already exist, fail closed,
- if `append=true`, every row must log `run_id` and `session_id`,
- probe paths must not collide with active decision, active BUY, shadow entry,
  shadow lifecycle, or historical report paths.

## Sampling Contract

Sampling must be deterministic and replay-auditable.

Preferred selection key:

```text
hash(ab_record_id + v3_policy_config_hash + namespace + sampling_version)
```

The sampler must write one of:

- `probe_selected`
- `probe_skipped`

with an explicit reason. Missing metadata is a skip, not an implicit fallback.

Required skip reasons include:

- `probe_disabled`
- `invalid_pool_identity`
- `invalid_mint_identity`
- `unsupported_protocol_state`
- `missing_bonding_curve`
- `missing_execution_route_identity`
- `missing_materialized_feature_set`
- `critical_curve_unavailable`
- `critical_account_unavailable`
- `missing_ab_record_id`
- `missing_v3_replay_payload`
- `missing_v3_feature_snapshot_hash`
- `missing_v3_policy_config_hash`
- `verdict_type_not_in_sample_scope`
- `active_buy_excluded`
- `probe_queue_full`
- `probe_backpressure`
- `probe_rate_limit_exceeded`
- `probe_concurrency_limit_exceeded`
- `max_probes_per_run_reached`
- `shadow_transport_not_ready`
- `duplicate_probe_id`

## Join-Key Contract

The source decision `ab_record_id` must be preserved as the primary join key
between V3/MFS decision rows and probe artifacts.

The probe plane must also create a separate deterministic probe id:

```text
probe_id = hash(source_ab_record_id + sampling_version + probe_bucket + probe_amount_lamports)
```

Do not mint BUY-shaped `ab_record_id` values for rejected or pending rows.

Required fields on probe selection, transport, entry, lifecycle, on-chain
report, labels, and feature availability rows:

```text
schema_version
collection_plane
dispatch_source
probe_plane
probe_id
probe_sampling_version
probe_bucket
probe_bucket_reason
probe_bucket_version
probe_sample_reason
probe_selected_ts_ms
probe_dispatch_ts_ms
probe_amount_lamports
probe_amount_source
probe_slippage_bps
quote_age_ms
curve_age_ms
source_ab_record_id
ab_record_id
candidate_id
pool_id
base_mint
mint_id
decision_ts_ms
observation_start_ts_ms
observation_end_ts_ms
v3_feature_snapshot_hash
v3_policy_config_hash
source_decision_plane
decision_plane
rollout_namespace
active_verdict_type
active_verdict_buy
active_reason_code
active_reason_chain
v3_shadow_verdict
v3_shadow_reason_code
v3_shadow_confidence
source_decision_log_path
source_decision_row_offset
probe_status
probe_skip_reason
```

For compatibility with existing consumers, probe artifacts may keep top-level
`decision_plane`, but reports must treat `source_decision_plane` as the active
decision source and `dispatch_source` / `collection_plane` as the probe source.

## Runtime Implementation Boundary

Minimal implementation must be additive:

1. Add disabled-by-default probe config and validation.
2. Observe decision rows after `MaterializedFeatureSet` / V3 replay payload
   materialization.
3. Run eligibility precheck.
4. Assign a probe bucket.
5. Deterministically select or skip probe candidates.
6. Build a shadow-only probe request with the source join metadata and explicit
   amount/quote/slippage parameters.
7. Dispatch only through the shadow simulation transport.
8. Write probe artifacts into probe-specific paths.
9. Keep EventBus disabled in P0 or use only dedicated probe event types.
10. Keep probe position state isolated from active ledgers.
11. Propagate `ab_record_id`, V3 hashes, probe fields, and dispatch source into
    entry, lifecycle, on-chain reports, labels, and feature availability.

Implementation must not change:

- active Gatekeeper verdict computation,
- IWIM gating,
- live sender behavior,
- trigger live dispatch behavior,
- active BUY log semantics,
- active BUY metrics,
- active open position accounting,
- runtime thresholds.

## Reporting Requirements

J3 reports must segment at least these classes:

- active decision rows,
- active BUY rows,
- counterfactual probe rows,
- probe-selected rows,
- probe-skipped rows,
- probe transport rows,
- probe entry rows,
- probe lifecycle rows,
- probe labels after lifecycle close.
- probe bucket counts,
- probe skip reason counts,
- probe amount/source distributions,
- probe queue/backpressure skips.

Join-key audit must report:

- `probe_rows_with_ab_record_id`
- `probe_rows_with_probe_id`
- `probe_transport_rows_with_ab_record_id`
- `probe_entry_rows_with_ab_record_id`
- `probe_lifecycle_rows_with_ab_record_id`
- `exact_ab_record_id` coverage
- `exact_probe_id` continuity
- fallback join counts
- unmatched rows

## Acceptance Criteria

P3.7-J3 is acceptable only if:

- Probe transport rows are generated with `ab_record_id`.
- Probe entry rows are generated with the same `ab_record_id`.
- P0 probe rows include `probe_bucket`, collision-safe `probe_id`, explicit
  sizing fields, and `dispatch_source=counterfactual_shadow_probe`.
- P0 does not require a closed lifecycle position.
- P1 probe lifecycle rows inherit the same `ab_record_id` and `probe_id` when
  lifecycle rows exist.
- V3/MFS payload is present for probed rows.
- Join-key audit reports `PASS` for exact AB/probe continuity.
- No active policy mutation is observed.
- No live/P2 path is enabled.
- Probe rows are not counted as active BUY decisions.
- Probe rows do not appear in `gatekeeper_v2_buys.jsonl`.
- Probe rows do not set `decision_verdict_buy=true`.
- Probe rows do not change active `reason_code` / `verdict_type`.
- Probe rows do not increment active BUY metrics.
- Probe rows do not increment active open position count.
- Lifecycle labels can be generated after probe lifecycle close in P1.
- Legacy artifacts without probe fields still parse.

## Consequences

This ADR permits a controlled way to collect V3/MFS plus shadow lifecycle labels
when active BUY rate is too low.

It also creates a new interpretation risk: counterfactual probe outcomes are
not active policy outcomes. Reports must keep that segmentation visible.

If J3 succeeds, P3.7 may proceed to a diagnostic V3/MFS lifecycle feature
prototype on counterfactual probe labels. That still does not authorize P2,
live, or runtime thresholds.

If J3 cannot generate probe rows without touching active policy, P3.7 remains
blocked at the dataset collection layer.

## Rejected Alternatives

### Loosen V2/V2.5 thresholds

Rejected. It changes active policy and contaminates the collection target.

### Change IWIM to raise BUY rate

Rejected. IWIM is active behavior and outside P3.7-J3 scope.

### Treat V3 rejected rows as active BUYs

Rejected. Probe dispatch is counterfactual and must not mutate active verdicts.

### Reuse active `buys.jsonl` without a dispatch source

Rejected. It would blur active BUY evidence with probe evidence.

### Wait indefinitely for natural BUYs

Rejected for P3.7 dataset collection. J2 showed this is operationally
impractical under the current gate regime.

### Enable live/P2 for validation

Rejected. Shadow simulation is the only authorized execution plane for J3.

## Non-Goals

This ADR does not authorize:

- P2,
- live execution,
- active Gatekeeper changes,
- IWIM policy changes,
- live sender changes,
- threshold tuning,
- V3 promotion,
- treating probes as BUY decisions,
- treating probe labels as decision-time features,
- treating speculative finality as finalized proof.
